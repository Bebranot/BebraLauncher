//! Wizards' Den hub client (docs/protocol.md, sections 1.1-1.2, 1.5).

use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use url::form_urlencoded;

use crate::http::UrlFallbackSet;
use crate::server::{ServerInfo, ServerStatus, STATUS_TIMEOUT};
use crate::{Error, Result};

pub const OFFICIAL_HUB_URL: &str = "https://hub.spacestation14.com/";
pub const OFFICIAL_FALLBACK_HUB_URL: &str = "https://hub.fallback.spacestation14.com/";

pub const HUB_URLS: [&str; 2] = [OFFICIAL_HUB_URL, OFFICIAL_FALLBACK_HUB_URL];

/// The server list request is cancelled after this long.
pub const LIST_TIMEOUT: Duration = Duration::from_secs(10);

/// One entry of `GET {hub}api/servers`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubServerEntry {
    /// `ss14://` or `ss14s://` address; the hub identifies servers by this exact string.
    pub address: String,
    #[serde(rename = "statusData")]
    pub status: ServerStatus,
    #[serde(default, rename = "inferredTags", deserialize_with = "null_to_empty")]
    pub inferred_tags: Vec<String>,
    #[serde(default)]
    pub hub_url: Option<String>,
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
        let set = self.hub.join("api/servers");
        let resp = tokio::time::timeout(LIST_TIMEOUT, set.get(&self.client))
            .await
            .map_err(|_| Error::Other("hub server list request timed out".into()))??;
        check_status(&resp)?;
        let body = tokio::time::timeout(LIST_TIMEOUT, resp.bytes())
            .await
            .map_err(|_| Error::Other("hub server list download timed out".into()))??;
        serde_json::from_slice(&body).map_err(|e| Error::Other(format!("bad hub response: {e}")))
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
        let body = resp.bytes().await?;
        serde_json::from_slice(&body)
            .map_err(|e| Error::Other(format!("bad hub info response: {e}")))
    }
}

/// Checks whether a string is a valid HTTP(S) URL for a server hub.
pub fn is_valid_hub_url(url: &str) -> bool {
    let s = url.trim();
    if s.is_empty() || s.len() > 300 {
        return false;
    }
    match url::Url::parse(s) {
        Ok(u) => {
            (u.scheme() == "http" || u.scheme() == "https")
                && u.has_host()
                && u.query().is_none()
                && u.fragment().is_none()
        }
        Err(_) => false,
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
                    if let Some(ref name) = entry.status.name {
                        for tag in crate::server::infer_tags(name, &entry.status.tags) {
                            if !entry.status.tags.contains(&tag) {
                                entry.status.tags.push(tag);
                            }
                        }
                    }
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
                last_err = Some(Error::Other(format!("hub {hub_url}: {e}")));
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

fn check_status(resp: &reqwest::Response) -> Result<()> {
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(Error::HttpStatus {
            status: resp.status().as_u16(),
            url: resp.url().to_string(),
        })
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

        assert!(!is_valid_hub_url(""));
        assert!(!is_valid_hub_url("ftp://hub.example.com"));
        assert!(!is_valid_hub_url("https://hub.example.com?query=1"));
        assert!(!is_valid_hub_url("https://hub.example.com#fragment"));
        assert!(!is_valid_hub_url("not a url"));

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
}
