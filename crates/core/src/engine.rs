//! Robust engine builds: the index, picking a version and installing it
//! (docs/protocol.md, section 6).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ed25519_dalek::VerifyingKey;
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::download::{download_to_file, ProgressFn};
use crate::hash::{parse_hash_hex, to_hex};
use crate::http::UrlFallbackSet;
use crate::paths::DataDirs;
use crate::signature;
use crate::{Error, Result};

pub const BUILDS_URLS: [&str; 2] = [
    "https://robust-builds.cdn.spacestation14.com/",
    "https://robust-builds.fallback.cdn.spacestation14.com/",
];

pub fn engine_urls(engine: &str) -> UrlFallbackSet {
    if engine.eq_ignore_ascii_case("multiverse") {
        UrlFallbackSet::new(["https://cdn.spacestationmultiverse.com/"])
    } else if engine.eq_ignore_ascii_case("supermatter") {
        UrlFallbackSet::new(["https://cdn.simplestation.org/supermatter/"])
    } else {
        UrlFallbackSet::new(BUILDS_URLS)
    }
}

/// Redirect chains in the index are short; this only stops an accidental loop.
const MAX_REDIRECTS: usize = 32;

/// What we ask the index for on this machine, best first.
pub fn preferred_rids() -> &'static [&'static str] {
    if cfg!(target_arch = "aarch64") {
        &["win-arm64", "win-x64"]
    } else {
        &["win-x64"]
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlatformBuild {
    pub url: String,
    pub sha256: String,
    pub sig: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EngineVersionInfo {
    #[serde(default)]
    pub insecure: bool,
    /// This version is an alias: use the one named here instead.
    #[serde(default)]
    pub redirect: Option<String>,
    #[serde(default)]
    pub platforms: HashMap<String, PlatformBuild>,
}

/// `manifest.json` of the builds CDN.
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct EngineIndex(pub HashMap<String, EngineVersionInfo>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEngine {
    /// The real version, after following redirects. May differ from the one asked for.
    pub version: String,
    pub rid: String,
    pub url: String,
    pub sha256: String,
    pub sig: String,
}

async fn fetch_manifest(client: &Client, urls: &UrlFallbackSet, suffix: &str) -> Result<EngineIndex> {
    let resp = urls.join(suffix).get(client).await?;
    if !resp.status().is_success() {
        return Err(Error::HttpStatus {
            status: resp.status().as_u16(),
            url: resp.url().to_string(),
        });
    }
    let bytes = resp.bytes().await?;
    serde_json::from_slice(&bytes).map_err(|e| Error::Other(format!("bad engine index: {e}")))
}

impl EngineIndex {
    pub async fn fetch(client: &Client, urls: &UrlFallbackSet) -> Result<Self> {
        match fetch_manifest(client, urls, "manifest.json").await {
            Ok(idx) => Ok(idx),
            Err(e) => {
                if urls.urls().iter().any(|u| u.contains("spacestationmultiverse")) {
                    fetch_manifest(client, urls, "ssmv-engine-manifest").await
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Follows redirects, refuses insecure builds, picks the first available platform of `rids`.
    pub fn resolve(&self, requested: &str, rids: &[&str]) -> Result<ResolvedEngine> {
        let mut version = requested.to_string();
        let mut info = self.0.get(&version).ok_or_else(|| {
            Error::Other(format!("engine version {requested} is not in the index"))
        })?;

        let mut hops = 0;
        while let Some(next) = &info.redirect {
            hops += 1;
            if hops > MAX_REDIRECTS {
                return Err(Error::Other(format!(
                    "engine version {requested}: too many redirects"
                )));
            }
            version = next.clone();
            info = self.0.get(next).ok_or_else(|| {
                Error::Other(format!("engine redirect target {next} is not in the index"))
            })?;
        }

        if info.insecure {
            return Err(Error::Other(format!(
                "engine version {version} is marked insecure"
            )));
        }

        for rid in rids {
            if let Some(build) = info.platforms.get(*rid) {
                return Ok(ResolvedEngine {
                    version,
                    rid: (*rid).to_string(),
                    url: build.url.clone(),
                    sha256: build.sha256.clone(),
                    sig: build.sig.clone(),
                });
            }
        }
        Err(Error::Other(format!(
            "engine version {version} has no build for {}",
            rids.join(" or ")
        )))
    }
}

#[derive(Debug, Clone)]
pub struct InstalledEngine {
    pub version: String,
    pub zip_path: PathBuf,
    /// Hex Ed25519 signature of the zip, as `SS14.Loader` wants it.
    pub sig_hex: String,
    /// `false` if the files were already there and valid.
    pub downloaded: bool,
}

/// Versions become file names, so only plain characters are allowed.
fn check_version_name(version: &str) -> Result<()> {
    let ok = !version.is_empty()
        && version.len() <= 100
        && !version.starts_with('.')
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'));
    if ok {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "unsafe engine version name {version:?}"
        )))
    }
}

fn zip_path(dirs: &DataDirs, version: &str) -> PathBuf {
    dirs.engines().join(format!("{version}.zip"))
}

fn sig_path(dirs: &DataDirs, version: &str) -> PathBuf {
    dirs.engines().join(format!("{version}.sig"))
}

/// An installed engine, if its files exist (they are not re-verified here).
pub fn installed(dirs: &DataDirs, version: &str) -> Result<Option<InstalledEngine>> {
    check_version_name(version)?;
    let zip = zip_path(dirs, version);
    let sig = sig_path(dirs, version);
    if !zip.is_file() || !sig.is_file() {
        return Ok(None);
    }
    Ok(Some(InstalledEngine {
        version: version.to_string(),
        zip_path: zip,
        sig_hex: std::fs::read_to_string(sig)?.trim().to_string(),
        downloaded: false,
    }))
}

/// Makes sure the engine `requested` (a version from `/info`) is installed and valid.
///
/// Reuses an installed copy when its SHA-256 still matches the index. Otherwise downloads the zip,
/// checks SHA-256 and the Ed25519 signature, and only then moves it into place.
pub async fn install_engine(
    client: &Client,
    index_urls: &UrlFallbackSet,
    dirs: &DataDirs,
    requested: &str,
    key: &VerifyingKey,
    progress: Option<ProgressFn<'_>>,
) -> Result<InstalledEngine> {
    let index = EngineIndex::fetch(client, index_urls).await?;
    let resolved = index.resolve(requested, preferred_rids())?;
    check_version_name(&resolved.version)?;
    let want_sha = parse_hash_hex(&resolved.sha256)?;

    dirs.ensure()?;
    let zip = zip_path(dirs, &resolved.version);
    let sig = sig_path(dirs, &resolved.version);

    if zip.is_file() && sig.is_file() {
        let have = tokio::task::spawn_blocking({
            let zip = zip.clone();
            move || sha256_of_file(&zip)
        })
        .await
        .map_err(|e| Error::Other(format!("hashing task failed: {e}")))??;
        if have == want_sha {
            return Ok(InstalledEngine {
                version: resolved.version,
                zip_path: zip,
                sig_hex: std::fs::read_to_string(&sig)?.trim().to_string(),
                downloaded: false,
            });
        }
    }

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let part = dirs
        .engines()
        .join(format!("{}.zip.{}-{}.part", resolved.version, std::process::id(), nanos));
    let result = download_and_check(client, &resolved, &want_sha, key, &part, progress).await;
    if let Err(e) = result {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }

    // Move into place; the signature file goes last, it marks the install as complete.
    if !zip.is_file() {
        if let Err(e) = std::fs::rename(&part, &zip) {
            if !zip.is_file() {
                let _ = std::fs::remove_file(&part);
                return Err(e.into());
            }
            let _ = std::fs::remove_file(&part);
        }
    } else {
        let _ = std::fs::remove_file(&part);
    }
    std::fs::write(&sig, &resolved.sig)?;
    Ok(InstalledEngine {
        version: resolved.version,
        zip_path: zip,
        sig_hex: resolved.sig,
        downloaded: true,
    })
}

async fn download_and_check(
    client: &Client,
    build: &ResolvedEngine,
    want_sha: &[u8; 32],
    key: &VerifyingKey,
    part: &Path,
    progress: Option<ProgressFn<'_>>,
) -> Result<()> {
    let actual = download_to_file(client, &build.url, part, progress).await?;
    if &actual != want_sha {
        return Err(Error::HashMismatch {
            expected: to_hex(want_sha),
            actual: to_hex(&actual),
        });
    }

    let data = tokio::fs::read(part).await?;
    signature::verify(key, &data, &build.sig)
}

fn sha256_of_file(path: &Path) -> Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = std::io::Read::read(&mut file, &mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::build_client;
    use crate::signature::tests::{sign_hex, test_key};

    fn index(json: &str) -> EngineIndex {
        serde_json::from_str(json).unwrap()
    }

    fn plat(url: &str) -> String {
        format!(
            r#"{{"url":"{url}","sha256":"{}","sig":"AA"}}"#,
            "0".repeat(64)
        )
    }

    #[test]
    fn follows_redirects_and_checks_the_final_version() {
        // Like the real index: an old insecure version redirects to a fixed one.
        let idx = index(&format!(
            r#"{{
              "1.0.0": {{"insecure": true, "redirect": "1.0.7", "platforms": {{}}}},
              "1.0.7": {{"insecure": false, "platforms": {{"win-x64": {}}}}}
            }}"#,
            plat("https://x/1.0.7.zip")
        ));
        let r = idx.resolve("1.0.0", &["win-x64"]).unwrap();
        assert_eq!(r.version, "1.0.7");
        assert_eq!(r.url, "https://x/1.0.7.zip");
        assert_eq!(r.rid, "win-x64");
    }

    #[test]
    fn insecure_final_version_is_refused() {
        let idx = index(&format!(
            r#"{{"1.0.0": {{"insecure": true, "platforms": {{"win-x64": {}}}}}}}"#,
            plat("https://x/a.zip")
        ));
        assert!(idx.resolve("1.0.0", &["win-x64"]).is_err());
    }

    #[test]
    fn unknown_version_missing_platform_and_loops_fail() {
        let idx = index(&format!(
            r#"{{
              "a": {{"redirect": "b"}},
              "b": {{"redirect": "a"}},
              "linux-only": {{"platforms": {{"linux-x64": {}}}}},
              "dangling": {{"redirect": "nowhere"}}
            }}"#,
            plat("https://x/l.zip")
        ));
        assert!(idx.resolve("zzz", &["win-x64"]).is_err());
        assert!(idx.resolve("a", &["win-x64"]).is_err(), "redirect loop");
        assert!(idx.resolve("linux-only", &["win-x64"]).is_err());
        assert!(idx.resolve("dangling", &["win-x64"]).is_err());
    }

    #[test]
    fn platform_preference_order() {
        let idx = index(&format!(
            r#"{{"1": {{"platforms": {{"win-x64": {}, "win-arm64": {}}}}}}}"#,
            plat("https://x/x64.zip"),
            plat("https://x/arm.zip")
        ));
        assert_eq!(
            idx.resolve("1", &["win-arm64", "win-x64"]).unwrap().rid,
            "win-arm64"
        );
        assert_eq!(idx.resolve("1", &["win-x64"]).unwrap().rid, "win-x64");
    }

    #[test]
    fn version_names_are_filename_safe() {
        for ok in ["277.0.0", "264.0.7-fix-physics", "1.2.3+build_4"] {
            assert!(check_version_name(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "..",
            "../x",
            "a/b",
            "a\\b",
            ".hidden",
            "a b",
            "con:",
            &"x".repeat(101),
        ] {
            assert!(check_version_name(bad).is_err(), "{bad:?}");
        }
    }

    // ---- install flow against a local server ----

    use std::sync::Arc;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn index_for(base: &str, version: &str, zip: &[u8], sig: &str) -> String {
        format!(
            r#"{{"{version}": {{"platforms": {{"win-x64": {{"url": "{base}engine.zip", "sha256": "{}", "sig": "{sig}"}}}}}}}}"#,
            to_hex(&crate::hash::sha256(zip))
        )
    }

    #[tokio::test]
    async fn installs_verifies_and_then_reuses() {
        let zip = b"PK fake engine zip contents".repeat(100);
        let sk = test_key();
        let sig = sign_hex(&sk, &zip);

        let base = start_with_index(&zip, "9.9.9", &sig).await;

        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDirs::at(tmp.path());
        let urls = UrlFallbackSet::new([base.clone()]);
        let client = build_client().unwrap();
        let key = sk.verifying_key();

        let first = install_engine(&client, &urls, &dirs, "9.9.9", &key, None)
            .await
            .unwrap();
        assert!(first.downloaded);
        assert_eq!(std::fs::read(&first.zip_path).unwrap(), zip);
        assert_eq!(first.sig_hex, sig);
        assert!(!dirs.engines().join("9.9.9.zip.part").exists());

        let second = install_engine(&client, &urls, &dirs, "9.9.9", &key, None)
            .await
            .unwrap();
        assert!(!second.downloaded, "valid copy is reused");

        let found = installed(&dirs, "9.9.9").unwrap().unwrap();
        assert_eq!(found.sig_hex, sig);
        assert!(installed(&dirs, "1.0.0").unwrap().is_none());
    }

    #[tokio::test]
    async fn bad_signature_or_hash_leaves_nothing_behind() {
        let zip = b"engine bytes".repeat(50);
        let sk = test_key();

        // Signed by the wrong key.
        let wrong_sig = sign_hex(&ed25519_dalek::SigningKey::from_bytes(&[1u8; 32]), &zip);
        let base = start_with_index(&zip, "9.9.9", &wrong_sig).await;
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDirs::at(tmp.path());
        let err = install_engine(
            &build_client().unwrap(),
            &UrlFallbackSet::new([base]),
            &dirs,
            "9.9.9",
            &sk.verifying_key(),
            None,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("signature"), "{err}");
        assert!(installed(&dirs, "9.9.9").unwrap().is_none());
        assert!(!dirs.engines().join("9.9.9.zip.part").exists());
        assert!(!dirs.engines().join("9.9.9.zip").exists());

        // Index promises a different SHA-256 than the file has.
        let good_sig = sign_hex(&sk, &zip);
        let base = start_with_tampered_hash(&zip, "9.9.9", &good_sig).await;
        let err = install_engine(
            &build_client().unwrap(),
            &UrlFallbackSet::new([base]),
            &dirs,
            "9.9.9",
            &sk.verifying_key(),
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, Error::HashMismatch { .. }), "{err}");
        assert!(installed(&dirs, "9.9.9").unwrap().is_none());
    }

    /// Starts a server whose index points back at itself.
    async fn start_with_index(zip: &[u8], version: &str, sig: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/", listener.local_addr().unwrap());
        let index = index_for(&base, version, zip, sig);
        spawn_on(listener, index, zip.to_vec());
        base
    }

    async fn start_with_tampered_hash(zip: &[u8], version: &str, sig: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/", listener.local_addr().unwrap());
        let index = index_for(&base, version, b"some other file", sig);
        spawn_on(listener, index, zip.to_vec());
        base
    }

    fn spawn_on(listener: TcpListener, index: String, zip: Vec<u8>) {
        let index = Arc::new(index);
        let zip = Arc::new(zip);
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let (index, zip) = (index.clone(), zip.clone());
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let head = String::from_utf8_lossy(&buf[..n]).to_string();
                    let body: Vec<u8> = if head.starts_with("GET /manifest.json") {
                        index.as_bytes().to_vec()
                    } else if head.starts_with("GET /engine.zip") {
                        zip.to_vec()
                    } else {
                        let _ = sock
                            .write_all(
                                b"HTTP/1.1 404 X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                            )
                            .await;
                        return;
                    };
                    let h = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = sock.write_all(h.as_bytes()).await;
                    let _ = sock.write_all(&body).await;
                });
            }
        });
    }
}
