//! Game server `/status` and `/info` (docs/protocol.md, sections 1.4-1.5).

use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Deserializer, Serialize};

use crate::http::{read_json_limited, MAX_JSON_BYTES};
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

/// Reads a number that may come as an integer, a float (`12.0`) or a string (`"12"`).
/// A value of the wrong shape becomes `None` instead of making the whole response unreadable.
fn flexible_i64<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<i64>, D::Error> {
    use serde_json::Value;

    Ok(match Option::<Value>::deserialize(d)? {
        None | Some(Value::Null) | Some(Value::Bool(_)) => None,
        Some(Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Some(Value::String(s)) => {
            let s = s.trim();
            s.parse::<i64>()
                .ok()
                .or_else(|| s.parse::<f64>().ok().map(|f| f as i64))
        }
        Some(Value::Array(_) | Value::Object(_)) => None,
    })
}

/// Counts are clamped to be non-negative, like the official launcher does.
fn count<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<i32, D::Error> {
    let v = flexible_i64(d)?.unwrap_or(0);
    Ok(v.clamp(0, i32::MAX as i64) as i32)
}

/// `run_level` may be a number or a name (`InRound`, `in_round`, ...).
fn run_level<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<i32>, D::Error> {
    use serde_json::Value;

    let raw = Option::<Value>::deserialize(d)?;
    Ok(match raw {
        Some(Value::String(s)) => {
            let text = s.trim().to_ascii_lowercase();
            let named = text.replace(['_', '-'], "");
            match named.as_str() {
                "preroundlobby" | "lobby" => Some(0),
                "inround" => Some(1),
                "postround" => Some(2),
                _ => text.parse::<f64>().ok().map(|f| f as i32),
            }
        }
        Some(Value::Number(n)) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f as i64))
            .map(|v| v as i32),
        _ => None,
    })
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
    #[serde(default, deserialize_with = "run_level")]
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

/// How the server wants accounts to be handled. A mode we don't know yet must not make the whole
/// `/info` unreadable: it becomes [`AuthMode::Unknown`], which behaves like `Optional` (the account
/// is sent if there is one, and a guest is never refused locally) but is visible to callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub enum AuthMode {
    #[default]
    Optional,
    Required,
    Disabled,
    Unknown,
}

impl<'de> Deserialize<'de> for AuthMode {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        // The server's enum is read case-insensitively by the official launcher; do the same.
        let s = String::deserialize(d)?;
        Ok(match s.trim().to_ascii_lowercase().as_str() {
            "optional" => Self::Optional,
            "required" => Self::Required,
            "disabled" => Self::Disabled,
            _ => Self::Unknown,
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthInfo {
    #[serde(default)]
    pub mode: AuthMode,
    /// Base64 libsodium sealed-box key, passed through to the client. Not needed when auth is
    /// disabled (and some self-hosted servers leave it out).
    #[serde(default)]
    pub public_key: String,
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

impl PrivacyPolicy {
    /// What the player agreed to: the pair the settings file should remember.
    pub fn acceptance(&self) -> (String, String) {
        (self.identifier.clone(), self.version.clone())
    }

    /// Has this exact policy (identifier *and* version) already been accepted?
    ///
    /// The launcher must not connect before the link has been shown and accepted; a changed
    /// version means the player has to agree again. Storing the accepted pairs is the caller's
    /// job (the app keeps them in its settings).
    pub fn is_accepted(&self, accepted: &[(String, String)]) -> bool {
        accepted
            .iter()
            .any(|(id, version)| id == &self.identifier && version == &self.version)
    }
}

/// `/info` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    #[serde(default)]
    pub connect_address: Option<String>,
    #[serde(default)]
    pub build: Option<BuildInfo>,
    #[serde(default)]
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
    ///
    /// A `connect_address` from `/info` is validated first (`udp://host:port`): a server must not
    /// be able to send the client somewhere that is not a game socket. A bad value is ignored.
    pub fn connect_url(&self, addr: &ServerAddr) -> String {
        self.connect_address
            .as_deref()
            .and_then(crate::addr::normalize_connect_address)
            .unwrap_or_else(|| addr.default_connect_url())
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
    read_json_limited(resp, MAX_JSON_BYTES, timeout).await
}

/// `GET {api}/status`.
pub async fn fetch_status(client: &Client, addr: &ServerAddr) -> Result<ServerStatus> {
    get_json(client, &addr.status_url(), STATUS_TIMEOUT).await
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
        if lower_name.contains("[ru]")
            || lower_name.contains("[rus]")
            || lower_name.contains("[ру]")
            || lower_name.contains(" ru ")
        {
            added.push("lang:ru".to_string());
        } else if lower_name.contains("[en]")
            || lower_name.contains("[eng]")
            || lower_name.contains(" en ")
        {
            added.push("lang:en".to_string());
        }
    }

    let has_18 = existing_tags.iter().any(|t| t == "18+");
    if !has_18
        && (lower_name.contains("18+")
            || lower_name.contains("+18")
            || lower_name.contains("[erp]"))
    {
        added.push("18+".to_string());
    }

    let has_rp = existing_tags.iter().any(|t| t.starts_with("rp:"));
    if !has_rp {
        if lower_name.contains("[hrp]")
            || lower_name.contains("[hrp+]")
            || lower_name.contains(" hrp ")
        {
            added.push("rp:high".to_string());
        } else if lower_name.contains("[mrp]")
            || lower_name.contains("[mrp+]")
            || lower_name.contains(" mrp ")
        {
            added.push("rp:med".to_string());
        } else if lower_name.contains("[lrp]")
            || lower_name.contains("[lrp+]")
            || lower_name.contains(" lrp ")
        {
            added.push("rp:low".to_string());
        } else if lower_name.contains("[nrp]")
            || lower_name.contains("[nrp+]")
            || lower_name.contains(" nrp ")
        {
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

    #[test]
    fn unknown_auth_mode_does_not_sink_the_response() {
        let info: ServerInfo =
            serde_json::from_str(r#"{"auth":{"mode":"SomeFutureMode","public_key":"k"}}"#).unwrap();
        assert_eq!(info.auth.mode, AuthMode::Unknown);

        let lower: ServerInfo = serde_json::from_str(r#"{"auth":{"mode":"DISABLED"}}"#).unwrap();
        assert_eq!(lower.auth.mode, AuthMode::Disabled);
        assert_eq!(
            lower.auth.public_key, "",
            "public_key is optional when auth is disabled"
        );

        let no_auth: ServerInfo = serde_json::from_str(r#"{"desc":"x"}"#).unwrap();
        assert_eq!(no_auth.auth.mode, AuthMode::Optional);
    }

    #[test]
    fn numbers_may_be_floats_or_strings() {
        let s: ServerStatus = serde_json::from_str(
            r#"{"players":12.0,"soft_max_players":"100","run_level":"InRound"}"#,
        )
        .unwrap();
        assert_eq!(s.players, 12);
        assert_eq!(s.soft_max_players, 100);
        assert_eq!(s.run_level_kind(), Some(RunLevel::InRound));

        let named: ServerStatus = serde_json::from_str(r#"{"run_level":"post-round"}"#).unwrap();
        assert_eq!(named.run_level_kind(), Some(RunLevel::PostRound));

        // Unusable shapes must not turn the whole response into a parsing error.
        let weird: ServerStatus =
            serde_json::from_str(r#"{"players":"many","run_level":true}"#).unwrap();
        assert_eq!(weird.players, 0);
        assert_eq!(weird.run_level, None);
    }

    #[test]
    fn connect_address_must_be_a_udp_game_address() {
        let addr = ServerAddr::parse("ss14://example.com:1213").unwrap();
        for bad in [
            "https://example.com:1213/",
            "udp://example.com/",
            "udp://user:pass@example.com:1213/",
            "not an address",
        ] {
            let info: ServerInfo = serde_json::from_str(&format!(
                r#"{{"auth":{{"mode":"Optional"}},"connect_address":"{bad}"}}"#
            ))
            .unwrap();
            assert_eq!(
                info.connect_url(&addr),
                "udp://example.com:1213/",
                "{bad} must be ignored"
            );
        }

        let info: ServerInfo = serde_json::from_str(
            r#"{"auth":{"mode":"Optional"},"connect_address":"UDP://GAME.EXAMPLE:1214"}"#,
        )
        .unwrap();
        assert_eq!(
            info.connect_url(&addr),
            "udp://game.example:1214/",
            "a valid address is normalized"
        );
    }

    #[test]
    fn privacy_policy_acceptance_tracks_the_version() {
        let policy = PrivacyPolicy {
            link: "https://example.com/privacy".into(),
            identifier: "example".into(),
            version: "2".into(),
        };
        assert!(!policy.is_accepted(&[]));
        let mut accepted = vec![("example".to_string(), "1".to_string())];
        assert!(
            !policy.is_accepted(&accepted),
            "an older version is not enough"
        );
        accepted.push(policy.acceptance());
        assert!(policy.is_accepted(&accepted));
        assert_eq!(
            policy.acceptance(),
            ("example".to_string(), "2".to_string())
        );
    }

    #[tokio::test]
    async fn status_rejects_an_oversized_body() {
        use crate::http::build_client;
        use crate::test_util::serve;

        // Valid JSON, just far bigger than any real /status response.
        let big = format!("{{\"name\":\"{}\"}}", "a".repeat(MAX_JSON_BYTES)).into_bytes();
        let (base, _) = serve(move |_| (200, vec![], big.clone())).await;
        let addr = ServerAddr::parse(base.trim_end_matches('/')).unwrap();
        let err = fetch_status(&build_client().unwrap(), &addr)
            .await
            .unwrap_err();
        let text = err.to_string();
        assert!(
            text.contains("larger") || text.contains("too large"),
            "{text}"
        );
    }
}
