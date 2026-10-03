//! Wizards' Den hub client (docs/protocol.md, sections 1.1-1.2, 1.5).

use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use url::form_urlencoded;

use crate::http::UrlFallbackSet;
use crate::server::{ServerInfo, ServerStatus, STATUS_TIMEOUT};
use crate::{Error, Result};

pub const HUB_URLS: [&str; 2] = [
    "https://hub.spacestation14.com/",
    "https://hub.fallback.spacestation14.com/",
];

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
}
