//! Game server `/status` and `/info` (docs/protocol.md, sections 1.4-1.5).

use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{Error, Result, ServerAddr};

/// Timeout for `/status` and hub info requests.
pub const STATUS_TIMEOUT: Duration = Duration::from_secs(5);
/// Timeout for the `/info` request made right before connecting.
pub const INFO_TIMEOUT: Duration = Duration::from_secs(15);

fn null_default<'de, D, T>(d: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// Counts are clamped to be non-negative, like the official launcher does.
fn count<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<i32, D::Error> {
    let v = Option::<i64>::deserialize(d)?.unwrap_or(0);
    Ok(v.clamp(0, i32::MAX as i64) as i32)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunLevel {
    PreRoundLobby,
    InRound,
    PostRound,
}

/// `/status` response. Also embedded in hub list entries as `statusData`.
/// Every field is optional on the wire; unknown fields are ignored.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerStatus {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "count")]
    pub players: i32,
    #[serde(default, deserialize_with = "count")]
    pub soft_max_players: i32,
    /// ISO-8601 UTC.
    #[serde(default)]
    pub round_start_time: Option<String>,
    #[serde(default)]
    pub run_level: Option<i32>,
    #[serde(default, deserialize_with = "null_default")]
    pub tags: Vec<String>,
    // Sent by the hub but unknown to the official launcher 0.40.1:
    #[serde(default)]
    pub map: Option<String>,
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub round_id: Option<i64>,
    #[serde(default)]
    pub panic_bunker: Option<bool>,
    #[serde(default)]
    pub auth_methods: Option<Vec<String>>,
}

impl ServerStatus {
    pub fn run_level_kind(&self) -> Option<RunLevel> {
        match self.run_level? {
            0 => Some(RunLevel::PreRoundLobby),
            1 => Some(RunLevel::InRound),
            2 => Some(RunLevel::PostRound),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthMode {
    #[serde(alias = "optional")]
    Optional,
    #[serde(alias = "required")]
    Required,
    #[serde(alias = "disabled")]
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthInfo {
    pub mode: AuthMode,
    /// Base64 libsodium sealed-box key, passed through to the client.
    pub public_key: String,
    #[serde(default)]
    pub login_urls: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildInfo {
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub manifest_url: Option<String>,
    #[serde(default)]
    pub manifest_download_url: Option<String>,
    #[serde(default)]
    pub engine_version: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub fork_id: String,
    /// SHA-256 of the whole client zip (hex).
    #[serde(default)]
    pub hash: Option<String>,
    /// BLAKE2b-256 of the manifest text (hex).
    #[serde(default)]
    pub manifest_hash: Option<String>,
    #[serde(default)]
    pub acz: bool,
    #[serde(default)]
    pub engine_type: Option<String>,
}

fn non_empty(s: &Option<String>) -> Option<&str> {
    s.as_deref().filter(|s| !s.is_empty())
}

impl BuildInfo {
    pub fn engine_type(&self) -> &str {
        match self.engine_type.as_deref() {
            Some(s) if !s.trim().is_empty() => s.trim(),
            _ => "Robust",
        }
    }
    pub fn download_url(&self) -> Option<&str> {
        non_empty(&self.download_url)
    }

    pub fn manifest_url(&self) -> Option<&str> {
        non_empty(&self.manifest_url)
    }

    pub fn manifest_download_url(&self) -> Option<&str> {
        non_empty(&self.manifest_download_url)
    }

    pub fn hash(&self) -> Option<&str> {
        non_empty(&self.hash)
    }

    pub fn manifest_hash(&self) -> Option<&str> {
        non_empty(&self.manifest_hash)
    }

    /// Self-hosting rules from the official launcher (docs/protocol.md, section 2, step 0).
    pub fn apply_self_hosting(&mut self, addr: &ServerAddr) {
        if self.acz || self.download_url().is_none() {
            self.download_url = Some(addr.client_zip_url());
            if self.acz {
                let base = addr.api_base();
                self.manifest_url = Some(format!("{base}manifest.txt"));
                self.manifest_download_url = Some(format!("{base}download"));
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfoLink {
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyPolicy {
    pub link: String,
    pub identifier: String,
    pub version: String,
}

/// `/info` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    #[serde(default)]
    pub connect_address: Option<String>,
    #[serde(default)]
    pub build: Option<BuildInfo>,
    pub auth: AuthInfo,
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub links: Vec<InfoLink>,
    #[serde(default)]
    pub privacy_policy: Option<PrivacyPolicy>,
}

impl ServerInfo {
    /// UDP address the game client should connect to.
    pub fn connect_url(&self, addr: &ServerAddr) -> String {
        match self.connect_address.as_deref() {
            Some(c) if !c.is_empty() => c.to_string(),
            _ => addr.default_connect_url(),
        }
    }
}

async fn get_json<T: serde::de::DeserializeOwned>(
    client: &Client,
    url: &str,
    timeout: Duration,
) -> Result<T> {
    let resp = client.get(url).timeout(timeout).send().await?;
    if !resp.status().is_success() {
        return Err(Error::HttpStatus {
            status: resp.status().as_u16(),
            url: url.to_string(),
        });
    }
    Ok(resp.json().await?)
}

/// `GET {api}/status`.
pub async fn fetch_status(client: &Client, addr: &ServerAddr) -> Result<ServerStatus> {
    get_json(client, &addr.status_url(), STATUS_TIMEOUT).await
}

/// Measures round-trip latency in milliseconds to the server's status endpoint.
pub async fn ping_server(client: &Client, addr: &ServerAddr) -> Result<u64> {
    let start = std::time::Instant::now();
    let resp = client
        .get(addr.status_url())
        .timeout(STATUS_TIMEOUT)
        .send()
        .await?;
    if resp.status().is_success() {
        Ok(start.elapsed().as_millis() as u64)
    } else {
        Err(Error::HttpStatus {
            status: resp.status().as_u16(),
            url: addr.status_url(),
        })
    }
}

/// `/info` as requested right before connecting: no query, self-hosting rules applied.
pub async fn fetch_info(client: &Client, addr: &ServerAddr) -> Result<ServerInfo> {
    let mut info: ServerInfo = get_json(client, &addr.info_url(), INFO_TIMEOUT).await?;
    if let Some(build) = info.build.as_mut() {
        build.apply_self_hosting(addr);
    }
    Ok(info)
}

/// `/info?can_skip_build=1`: what the official launcher asks for when it only shows server details.
/// The server may leave the build section out. Self-hosting rules are not applied.
pub async fn fetch_info_preview(client: &Client, addr: &ServerAddr) -> Result<ServerInfo> {
    let url = format!("{}?can_skip_build=1", addr.info_url());
    get_json(client, &url, STATUS_TIMEOUT).await
}

/// Extracts common tags from the server name if they are not already present in `existing_tags`.
pub fn infer_tags(name: &str, existing_tags: &[String]) -> Vec<String> {
    if existing_tags.iter().any(|t| t == "no_tag_infer") {
        return Vec::new();
    }

    let mut added = Vec::new();
    let lower_name = name.to_lowercase();

    let has_lang = existing_tags.iter().any(|t| t.starts_with("lang:"));
    if !has_lang {
        if lower_name.contains("[ru]") || lower_name.contains("[rus]") || lower_name.contains("[ру]") || lower_name.contains(" ru ") {
            added.push("lang:ru".to_string());
        } else if lower_name.contains("[en]") || lower_name.contains("[eng]") || lower_name.contains(" en ") {
            added.push("lang:en".to_string());
        }
    }

    let has_18 = existing_tags.iter().any(|t| t == "18+");
    if !has_18 && (lower_name.contains("18+") || lower_name.contains("+18") || lower_name.contains("[erp]")) {
        added.push("18+".to_string());
    }

    let has_rp = existing_tags.iter().any(|t| t.starts_with("rp:"));
    if !has_rp {
        if lower_name.contains("[hrp]") || lower_name.contains("[hrp+]") || lower_name.contains(" hrp ") {
            added.push("rp:high".to_string());
        } else if lower_name.contains("[mrp]") || lower_name.contains("[mrp+]") || lower_name.contains(" mrp ") {
            added.push("rp:med".to_string());
        } else if lower_name.contains("[lrp]") || lower_name.contains("[lrp+]") || lower_name.contains(" lrp ") {
            added.push("rp:low".to_string());
        } else if lower_name.contains("[nrp]") || lower_name.contains("[nrp+]") || lower_name.contains(" nrp ") {
            added.push("rp:none".to_string());
        }
    }

    added
}

impl ServerStatus {
    pub fn all_tags(&self) -> Vec<String> {
        let mut tags = self.tags.clone();
        if let Some(ref name) = self.name {
            for tag in infer_tags(name, &tags) {
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
            }
        }
        tags
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_inference() {
        let existing = vec!["region:ru".to_string()];
        let inferred = infer_tags("[MRP] [RU] [18+] Station", &existing);
        assert!(inferred.contains(&"rp:med".to_string()));
        assert!(inferred.contains(&"lang:ru".to_string()));
        assert!(inferred.contains(&"18+".to_string()));

        let with_no_infer = vec!["no_tag_infer".to_string()];
        assert!(infer_tags("[MRP] [RU]", &with_no_infer).is_empty());
    }

    #[test]
    fn status_from_live_hub_entry() {
        let json = r#"{
            "map": "Cluster", "name": "[MRP+] Eridani", "tags": ["lang:ru","region:luna"],
            "preset": "Secret", "players": 6, "round_id": 79337, "run_level": 1,
            "panic_bunker": false, "round_start_time": "2026-10-03T08:38:01.2306510Z",
            "soft_max_players": 100
        }"#;
        let s: ServerStatus = serde_json::from_str(json).unwrap();
        assert_eq!(s.players, 6);
        assert_eq!(s.soft_max_players, 100);
        assert_eq!(s.run_level_kind(), Some(RunLevel::InRound));
        assert_eq!(s.tags, ["lang:ru", "region:luna"]);
        assert_eq!(s.map.as_deref(), Some("Cluster"));
    }

    #[test]
    fn status_tolerates_nulls_and_missing_fields() {
        let s: ServerStatus = serde_json::from_str(
            r#"{"name":null,"players":-3,"tags":null,"run_level":null,"map":null}"#,
        )
        .unwrap();
        assert_eq!(s.name, None);
        assert_eq!(s.players, 0, "negative counts are clamped");
        assert!(s.tags.is_empty());
        assert_eq!(s.run_level_kind(), None);

        let empty: ServerStatus = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.players, 0);
    }

    #[test]
    fn unknown_run_level_is_none() {
        let s: ServerStatus = serde_json::from_str(r#"{"run_level": 9}"#).unwrap();
        assert_eq!(s.run_level_kind(), None);
    }

    const INFO_JSON: &str = r#"{
        "auth": {"mode": "Required", "public_key": "5VTsC+byX7j1Z8kaIpioCJET44h1jB00HXUWO2Quujw="},
        "desc": "hello",
        "build": {
            "acz": false, "hash": "8F4A", "fork_id": "eridani", "version": "20260904051054",
            "download_url": "https://x/SS14.Client.zip", "manifest_url": "https://x/manifest",
            "manifest_hash": "EBA9", "engine_version": "277.0.0", "manifest_download_url": "https://x/download"
        },
        "links": [{"url": "https://discord.gg/x", "icon": "discord", "name": "Discord"}],
        "connect_address": "udp://eridani.imperialconnect.space:1214/"
    }"#;

    #[test]
    fn info_from_live_response() {
        let info: ServerInfo = serde_json::from_str(INFO_JSON).unwrap();
        assert_eq!(info.auth.mode, AuthMode::Required);
        let b = info.build.as_ref().unwrap();
        assert_eq!(b.engine_version, "277.0.0");
        assert_eq!(b.manifest_hash(), Some("EBA9"));
        assert!(!b.acz);
        assert_eq!(info.links[0].icon.as_deref(), Some("discord"));
        assert!(info.privacy_policy.is_none());

        let addr = ServerAddr::parse("ss14s://eridani.imperialconnect.space/").unwrap();
        assert_eq!(
            info.connect_url(&addr),
            "udp://eridani.imperialconnect.space:1214/"
        );
    }

    #[test]
    fn info_without_build_or_links() {
        let info: ServerInfo =
            serde_json::from_str(r#"{"auth":{"mode":"disabled","public_key":"k"},"links":null}"#)
                .unwrap();
        assert_eq!(info.auth.mode, AuthMode::Disabled);
        assert!(info.build.is_none());
        assert!(info.links.is_empty());
    }

    #[test]
    fn connect_url_falls_back_to_api_address() {
        let info: ServerInfo =
            serde_json::from_str(r#"{"auth":{"mode":"Optional","public_key":"k"}}"#).unwrap();
        let addr = ServerAddr::parse("ss14://example.com:1213").unwrap();
        assert_eq!(info.connect_url(&addr), "udp://example.com:1213/");
    }

    #[test]
    fn empty_strings_count_as_missing() {
        let b: BuildInfo =
            serde_json::from_str(r#"{"download_url":"","manifest_url":""}"#).unwrap();
        assert_eq!(b.download_url(), None);
        assert_eq!(b.manifest_url(), None);
    }

    #[test]
    fn self_hosting_for_missing_download_url() {
        let addr = ServerAddr::parse("ss14://example.com").unwrap();
        let mut b = BuildInfo::default();
        b.apply_self_hosting(&addr);
        assert_eq!(b.download_url(), Some("http://example.com:1212/client.zip"));
        assert_eq!(
            b.manifest_url(),
            None,
            "manifest endpoints are only inferred for ACZ"
        );
    }

    #[test]
    fn self_hosting_for_acz() {
        let addr = ServerAddr::parse("ss14://game.hardlight.space").unwrap();
        let mut b = BuildInfo {
            acz: true,
            ..Default::default()
        };
        b.apply_self_hosting(&addr);
        assert_eq!(
            b.download_url(),
            Some("http://game.hardlight.space:1212/client.zip")
        );
        assert_eq!(
            b.manifest_url(),
            Some("http://game.hardlight.space:1212/manifest.txt")
        );
        assert_eq!(
            b.manifest_download_url(),
            Some("http://game.hardlight.space:1212/download")
        );
    }

    #[test]
    fn self_hosting_leaves_real_download_url_alone() {
        let addr = ServerAddr::parse("ss14://example.com").unwrap();
        let mut b = BuildInfo {
            download_url: Some("https://cdn/x.zip".into()),
            ..Default::default()
        };
        b.apply_self_hosting(&addr);
        assert_eq!(b.download_url(), Some("https://cdn/x.zip"));
    }
}
