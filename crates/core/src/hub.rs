//! Wizards' Den hub client (docs/protocol.md, sections 1.1-1.2, 1.5).

use std::time::Duration;

use reqwest::header::RETRY_AFTER;
use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use url::form_urlencoded;

use crate::http::{read_json_limited, sanitize_url, UrlFallbackSet, MAX_JSON_BYTES};
use crate::server::{ServerInfo, ServerStatus, STATUS_TIMEOUT};
use crate::{Error, Result};

pub const OFFICIAL_HUB_URL: &str = "https://hub.spacestation14.com/";
pub const OFFICIAL_FALLBACK_HUB_URL: &str = "https://hub.fallback.spacestation14.com/";

pub const HUB_URLS: [&str; 2] = [OFFICIAL_HUB_URL, OFFICIAL_FALLBACK_HUB_URL];

/// The server list request is cancelled after this long.
pub const LIST_TIMEOUT: Duration = Duration::from_secs(10);

/// The server list is large, but not unlimited.
pub const MAX_HUB_LIST_BYTES: usize = 8 * 1024 * 1024;
/// Entries past this many are ignored (the busiest servers sort to the top anyway).
pub const MAX_HUB_SERVERS: usize = 5000;

/// Longest we are willing to wait for a `429 Too Many Requests` to pass.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(10);
/// How many times a `429` is retried before the error is returned.
const RETRY_ATTEMPTS: usize = 2;

/// One entry of `GET {hub}api/servers`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubServerEntry {
    /// `ss14://` or `ss14s://` address; the hub identifies servers by this exact string.
    pub address: String,
    /// `statusData` is `null` for a server that has not reported yet; that is not an error.
    #[serde(rename = "statusData", default, deserialize_with = "status_or_default")]
    pub status: ServerStatus,
    #[serde(default, rename = "inferredTags", deserialize_with = "null_to_empty")]
    pub inferred_tags: Vec<String>,
    #[serde(default)]
    pub hub_url: Option<String>,
}

fn status_or_default<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<ServerStatus, D::Error> {
    Ok(Option::<ServerStatus>::deserialize(d)?.unwrap_or_default())
}

fn null_to_empty<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Vec<String>, D::Error> {
    Ok(Option::<Vec<String>>::deserialize(d)?.unwrap_or_default())
}

#[derive(Debug, Clone)]
pub struct HubClient {
    client: Client,
    hub: UrlFallbackSet,
}

impl HubClient {
    /// Client for the official hub.
    pub fn official(client: Client) -> Self {
        Self::new(client, UrlFallbackSet::new(HUB_URLS))
    }

    pub fn new(client: Client, hub: UrlFallbackSet) -> Self {
        Self { client, hub }
    }

    pub async fn list_servers(&self) -> Result<Vec<HubServerEntry>> {
        Ok(self.list_servers_counted().await?.0)
    }

    /// Like [`list_servers`](Self::list_servers), but also reports how many entries were skipped
    /// because they could not be read. One broken entry must not hide the other 300 servers.
    pub async fn list_servers_counted(&self) -> Result<(Vec<HubServerEntry>, usize)> {
        let set = self.hub.join("api/servers");
        let resp = self.list_response(&set).await?;
        check_status(&resp)?;
        let raw: Vec<serde_json::Value> =
            read_json_limited(resp, MAX_HUB_LIST_BYTES, LIST_TIMEOUT).await?;

        let mut entries = Vec::new();
        let mut skipped = 0;
        for (index, value) in raw.into_iter().enumerate() {
            if index >= MAX_HUB_SERVERS {
                skipped += 1;
                continue;
            }
            match serde_json::from_value(value) {
                Ok(entry) => entries.push(entry),
                Err(_) => skipped += 1,
            }
        }
        Ok((entries, skipped))
    }

    /// Asks for the list and honors `Retry-After` on `429 Too Many Requests`.
    async fn list_response(&self, set: &UrlFallbackSet) -> Result<Response> {
        for attempt in 0..=RETRY_ATTEMPTS {
            let resp = tokio::time::timeout(LIST_TIMEOUT, set.get(&self.client))
                .await
                .map_err(|_| Error::Other("hub server list request timed out".into()))??;
            if resp.status() != StatusCode::TOO_MANY_REQUESTS || attempt == RETRY_ATTEMPTS {
                return Ok(resp);
            }
            tokio::time::sleep(retry_delay(&resp)).await;
        }
        unreachable!("the loop always returns")
    }

    /// `/info` of a hub-listed server, proxied by the hub (`address` exactly as listed).
    /// Works even when the server itself is unreachable from here. Meant for showing server details;
    /// before connecting, ask the server directly with `server::fetch_info`.
    pub async fn server_info(&self, address: &str) -> Result<ServerInfo> {
        let encoded: String = form_urlencoded::byte_serialize(address.as_bytes()).collect();
        let set = self.hub.join(&format!("api/servers/info?url={encoded}"));
        let resp = tokio::time::timeout(STATUS_TIMEOUT, set.get(&self.client))
            .await
            .map_err(|_| Error::Other("hub info request timed out".into()))??;
        check_status(&resp)?;
        read_json_limited(resp, MAX_JSON_BYTES, STATUS_TIMEOUT).await
    }
}

/// Checks whether a string is a valid hub URL. Plain `http` is only allowed on this machine
/// (a local hub); a remote hub must be `https`, otherwise anyone on the path can replace the
/// server list. Credentials in the address are not accepted: they would be logged.
pub fn is_valid_hub_url(url: &str) -> bool {
    let s = url.trim();
    if s.is_empty() || s.len() > 300 {
        return false;
    }
    let Ok(u) = url::Url::parse(s) else {
        return false;
    };
    if !u.has_host()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
    {
        return false;
    }
    match u.scheme() {
        "https" => true,
        "http" => matches!(
            u.host_str(),
            Some("localhost" | "127.0.0.1" | "::1" | "[::1]")
        ),
        _ => false,
    }
}

/// Trims and normalizes a hub URL (adds trailing slash).
pub fn normalize_hub_url(url: &str) -> Option<String> {
    let s = url.trim();
    if !is_valid_hub_url(s) {
        return None;
    }
    let mut normalized = s.to_string();
    if !normalized.ends_with('/') {
        normalized.push('/');
    }
    Some(normalized)
}

/// Fetches server lists from multiple hubs in parallel, merging and deduplicating servers.
/// If any hub fails but at least one succeeds, the successful results are returned.
pub async fn list_servers_from_hubs(
    client: &Client,
    hub_urls: &[String],
) -> Result<Vec<HubServerEntry>> {
    if hub_urls.is_empty() {
        return Ok(Vec::new());
    }

    let mut join_set = tokio::task::JoinSet::new();

    for url in hub_urls {
        let u = url.clone();
        let c = client.clone();
        join_set.spawn(async move {
            let client_instance = if u == OFFICIAL_HUB_URL
                || u.trim_end_matches('/') == OFFICIAL_HUB_URL.trim_end_matches('/')
            {
                HubClient::official(c)
            } else {
                HubClient::new(c, UrlFallbackSet::new([u.as_str()]))
            };
            let res = client_instance.list_servers().await;
            (u, res)
        });
    }

    let mut all_entries: std::collections::HashMap<String, HubServerEntry> =
        std::collections::HashMap::new();
    let mut last_err = None;
    let mut success_count = 0;

    while let Some(res) = join_set.join_next().await {
        match res {
            Ok((hub_url, Ok(entries))) => {
                success_count += 1;
                for mut entry in entries {
                    entry.hub_url = Some(hub_url.clone());
                    // One place infers tags from the name (`server::all_tags`), not three.
                    entry.status.tags = entry.status.all_tags();
                    match all_entries.entry(entry.address.clone()) {
                        std::collections::hash_map::Entry::Vacant(v) => {
                            v.insert(entry);
                        }
                        std::collections::hash_map::Entry::Occupied(mut occ) => {
                            let existing = occ.get_mut();
                            for tag in entry.inferred_tags {
                                if !existing.inferred_tags.contains(&tag) {
                                    existing.inferred_tags.push(tag);
                                }
                            }
                            for tag in entry.status.tags {
                                if !existing.status.tags.contains(&tag) {
                                    existing.status.tags.push(tag);
                                }
                            }
                            if entry.status.players > existing.status.players {
                                existing.status.players = entry.status.players;
                            }
                            if existing.status.name.is_none() && entry.status.name.is_some() {
                                existing.status.name = entry.status.name;
                            }
                            if existing.status.map.is_none() && entry.status.map.is_some() {
                                existing.status.map = entry.status.map;
                            }
                            if existing.status.preset.is_none() && entry.status.preset.is_some() {
                                existing.status.preset = entry.status.preset;
                            }
                        }
                    }
                }
            }
            Ok((hub_url, Err(e))) => {
                last_err = Some(hub_failure(&hub_url, &e));
            }
            Err(e) => {
                last_err = Some(Error::Other(e.to_string()));
            }
        }
    }

    if success_count == 0 {
        if let Some(err) = last_err {
            return Err(err);
        }
    }

    let mut result: Vec<HubServerEntry> = all_entries.into_values().collect();
    result.sort_by(|a, b| {
        b.status
            .players
            .cmp(&a.status.players)
            .then_with(|| a.address.cmp(&b.address))
    });
    Ok(result)
}

/// `Retry-After` as seconds, capped so a hub cannot stall us for hours. Malformed values are
/// ignored and a short default is used.
fn retry_delay(resp: &Response) -> Duration {
    let asked = resp
        .headers()
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_retry_after);
    asked.unwrap_or(Duration::from_secs(1)).min(MAX_RETRY_WAIT)
}

fn parse_retry_after(value: &str) -> Option<Duration> {
    let seconds: u64 = value.trim().parse().ok()?;
    Some(Duration::from_secs(seconds))
}

fn check_status(resp: &reqwest::Response) -> Result<()> {
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(Error::HttpStatus {
            status: resp.status().as_u16(),
            url: sanitize_url(resp.url().as_str()),
        })
    }
}

/// A hub error that never repeats credentials from the hub address.
fn hub_failure(hub_url: &str, e: &Error) -> Error {
    let safe = sanitize_url(hub_url);
    let has_userinfo =
        url::Url::parse(hub_url).is_ok_and(|u| !u.username().is_empty() || u.password().is_some());
    if has_userinfo {
        Error::Other(format!("hub {safe}: request failed (details hidden)"))
    } else {
        Error::Other(format!("hub {safe}: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_live_list_shape() {
        let json = r#"[
          {"address":"ss14s://eridani.imperialconnect.space/",
           "statusData":{"map":"Cluster","name":"Eridani","tags":["lang:ru"],"preset":"x","players":6,
                         "round_id":79337,"run_level":1,"panic_bunker":false,
                         "round_start_time":"2026-10-03T08:38:01.2306510Z","soft_max_players":100},
           "inferredTags":[]},
          {"address":"ss14://adven.space:1213",
           "statusData":{"map":null,"name":"Adventure","tags":["lang:ru","rp:hrp"],"players":0,"run_level":0,
                         "soft_max_players":50}}
        ]"#;
        let list: Vec<HubServerEntry> = serde_json::from_str(json).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].status.players, 6);
        assert_eq!(list[1].address, "ss14://adven.space:1213");
        assert!(
            list[1].inferred_tags.is_empty(),
            "missing inferredTags is fine"
        );
    }

    #[test]
    fn info_url_is_percent_encoded() {
        let set = UrlFallbackSet::new(["https://hub.example/"]);
        let encoded: String = form_urlencoded::byte_serialize("ss14s://a.b/c".as_bytes()).collect();
        let j = set.join(&format!("api/servers/info?url={encoded}"));
        assert_eq!(
            j.urls()[0],
            "https://hub.example/api/servers/info?url=ss14s%3A%2F%2Fa.b%2Fc"
        );
    }

    #[test]
    fn hub_url_validation_and_normalization() {
        assert!(is_valid_hub_url("https://hub.spacestation14.com/"));
        assert!(is_valid_hub_url("https://hub.corvax.fun"));
        assert!(is_valid_hub_url("http://localhost:8080/hub"));
        assert!(is_valid_hub_url("http://127.0.0.1:8080/"));
        assert!(is_valid_hub_url("http://[::1]:8080/"));

        assert!(!is_valid_hub_url(""));
        assert!(!is_valid_hub_url("ftp://hub.example.com"));
        assert!(!is_valid_hub_url("https://hub.example.com?query=1"));
        assert!(!is_valid_hub_url("https://hub.example.com#fragment"));
        assert!(!is_valid_hub_url("not a url"));
        // Plain http is only for a hub on this machine, and credentials are never allowed.
        assert!(!is_valid_hub_url("http://hub.example.com/"));
        assert!(!is_valid_hub_url("https://user:pass@hub.example.com/"));
        assert!(!is_valid_hub_url("https://user@hub.example.com/"));

        assert_eq!(
            normalize_hub_url("https://hub.corvax.fun"),
            Some("https://hub.corvax.fun/".into())
        );
        assert_eq!(
            normalize_hub_url("  https://hub.corvax.fun/  "),
            Some("https://hub.corvax.fun/".into())
        );
        assert_eq!(normalize_hub_url("invalid"), None);
    }

    #[tokio::test]
    async fn one_broken_entry_does_not_hide_the_list() {
        use crate::http::build_client;
        use crate::test_util::serve;

        let body = br#"[
            {"address":"ss14://a.example","statusData":{"name":"A","players":3}},
            {"address":"ss14://b.example","statusData":null},
            {"statusData":{"name":"no address"}},
            "not an object"
        ]"#
        .to_vec();
        let (base, _) = serve(move |_| (200, vec![], body.clone())).await;
        let hub = HubClient::new(
            build_client().unwrap(),
            UrlFallbackSet::try_new([base]).unwrap(),
        );
        let (list, skipped) = hub.list_servers_counted().await.unwrap();
        assert_eq!(list.len(), 2, "a null statusData still makes a usable row");
        assert_eq!(list[1].address, "ss14://b.example");
        assert_eq!(list[1].status.players, 0);
        assert_eq!(skipped, 2);
    }

    #[tokio::test]
    async fn a_429_is_retried_after_retry_after() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        use crate::http::build_client;
        use crate::test_util::serve;

        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let (base, _) = serve(move |_| {
            if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                (429, vec![("Retry-After", "0".to_string())], vec![])
            } else {
                (200, vec![], b"[]".to_vec())
            }
        })
        .await;
        let hub = HubClient::new(
            build_client().unwrap(),
            UrlFallbackSet::try_new([base]).unwrap(),
        );
        assert!(hub.list_servers().await.unwrap().is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn retry_after_is_parsed_and_capped() {
        assert_eq!(parse_retry_after("3"), Some(Duration::from_secs(3)));
        assert_eq!(parse_retry_after("  0 "), Some(Duration::ZERO));
        assert_eq!(parse_retry_after("Wed, 21 Oct 2015 07:28:00 GMT"), None);
        assert_eq!(parse_retry_after("soon"), None);
        assert!(MAX_RETRY_WAIT <= Duration::from_secs(10));
    }

    #[tokio::test]
    async fn hub_info_rejects_an_oversized_body() {
        use crate::http::build_client;
        use crate::test_util::serve;

        let big = format!(
            "{{\"auth\":{{\"mode\":\"Disabled\",\"public_key\":\"k\"}},\"desc\":\"{}\"}}",
            "a".repeat(MAX_JSON_BYTES)
        )
        .into_bytes();
        let (base, _) = serve(move |_| (200, vec![], big.clone())).await;
        let hub = HubClient::new(build_client().unwrap(), UrlFallbackSet::new([base]));
        let err = hub.server_info("ss14://example.com").await.unwrap_err();
        let text = err.to_string();
        assert!(
            text.contains("larger") || text.contains("too large"),
            "{text}"
        );
    }

    #[tokio::test]
    async fn hub_errors_do_not_repeat_credentials() {
        use crate::http::build_client;

        // Nothing listens on port 1, so the request fails at the transport level.
        let hub = "http://user:secret@127.0.0.1:1/".to_string();
        let err = list_servers_from_hubs(&build_client().unwrap(), &[hub])
            .await
            .unwrap_err()
            .to_string();
        assert!(!err.contains("secret"), "{err}");
        assert!(err.contains("127.0.0.1"), "{err}");
    }
}
