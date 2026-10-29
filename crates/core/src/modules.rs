//! Engine modules such as `Robust.Client.WebView` (docs/protocol.md, section 6.3).
//!
//! A build lists the modules it needs in `manifest.yml`. Each module is a signed zip that is
//! unpacked into `modules/<name>/<version>/`; the client finds it through `ROBUST_MODULE_<NAME>`.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use ed25519_dalek::VerifyingKey;
use reqwest::Client;
use serde::Deserialize;

use crate::download::{download_to_file, ProgressFn};
use crate::engine::{preferred_rids, PlatformBuild};
use crate::hash::{parse_hash_hex, to_hex};
use crate::http::UrlFallbackSet;
use crate::paths::DataDirs;
use crate::signature;
use crate::{Error, Result};

/// Written last into a module folder: its presence means the install finished.
const DONE_MARKER: &str = ".installed";

/// The parts of a build's `manifest.yml` the launcher cares about. Other keys are ignored.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ResourceManifest {
    pub modules: Vec<String>,
    pub multi_window: bool,
}

impl ResourceManifest {
    pub fn parse(yaml: &[u8]) -> Result<Self> {
        // An empty or comment-only file is valid and means "nothing special".
        let value: serde_yaml_ng::Value = serde_yaml_ng::from_slice(yaml)
            .map_err(|e| Error::Other(format!("bad manifest.yml: {e}")))?;
        if value.is_null() {
            return Ok(Self::default());
        }
        serde_yaml_ng::from_value(value).map_err(|e| Error::Other(format!("bad manifest.yml: {e}")))
    }
}

/// Dotted version numbers, compared number by number. A suffix such as `-fix-physics` is ignored,
/// so `264.0.2-fix-physics` counts as `264.0.2`.
pub fn parse_version(s: &str) -> Result<Vec<u64>> {
    let mut parts = Vec::new();
    for (i, piece) in s.split('.').enumerate() {
        let digits: String = piece.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            if i == 0 {
                return Err(Error::Other(format!("not a version number: {s:?}")));
            }
            break;
        }
        parts.push(
            digits
                .parse()
                .map_err(|_| Error::Other(format!("version part too large: {s:?}")))?,
        );
        if digits.len() != piece.len() {
            break; // a suffix ends the numeric part
        }
    }
    Ok(parts)
}

fn compare_versions(a: &[u64], b: &[u64]) -> std::cmp::Ordering {
    let len = a.len().max(b.len());
    for i in 0..len {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x.cmp(&y);
        }
    }
    std::cmp::Ordering::Equal
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModuleVersionData {
    #[serde(default)]
    pub insecure: bool,
    #[serde(default)]
    pub platforms: HashMap<String, PlatformBuild>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModuleData {
    #[serde(default)]
    pub versions: HashMap<String, ModuleVersionData>,
}

/// `modules.json` of the builds CDN.
#[derive(Debug, Clone, Deserialize)]
pub struct ModuleIndex {
    #[serde(default)]
    pub modules: HashMap<String, ModuleData>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModule {
    pub name: String,
    pub version: String,
    pub rid: String,
    pub url: String,
    pub sha256: String,
    pub sig: String,
}

impl ModuleIndex {
    pub async fn fetch(client: &Client, builds_urls: &UrlFallbackSet) -> Result<Self> {
        let resp = builds_urls.join("modules.json").get(client).await?;
        if !resp.status().is_success() {
            return Err(Error::HttpStatus {
                status: resp.status().as_u16(),
                url: resp.url().to_string(),
            });
        }
        let bytes = resp.bytes().await?;
        serde_json::from_slice(&bytes).map_err(|e| Error::Other(format!("bad module index: {e}")))
    }

    /// Modules are named by the *minimum* engine version they work with, so the right one is the
    /// newest version that is not newer than the engine.
    pub fn version_for_engine(&self, module: &str, engine_version: &str) -> Result<String> {
        let data = self
            .modules
            .get(module)
            .ok_or_else(|| Error::Other(format!("engine module {module} is not in the index")))?;
        let engine = parse_version(engine_version)?;

        data.versions
            .keys()
            .filter_map(|v| parse_version(v).ok().map(|parsed| (parsed, v)))
            .filter(|(parsed, _)| compare_versions(&engine, parsed).is_ge())
            .max_by(|(a, _), (b, _)| compare_versions(a, b))
            .map(|(_, v)| v.clone())
            .ok_or_else(|| {
                Error::Other(format!(
                    "no version of {module} fits engine {engine_version}"
                ))
            })
    }

    /// The download for `module` at exactly `version`: refuses insecure builds, picks the platform.
    pub fn resolve(&self, module: &str, version: &str, rids: &[&str]) -> Result<ResolvedModule> {
        let data = self
            .modules
            .get(module)
            .ok_or_else(|| Error::Other(format!("engine module {module} is not in the index")))?;
        let v = data
            .versions
            .get(version)
            .ok_or_else(|| Error::Other(format!("{module} {version} is not in the index")))?;
        if v.insecure {
            return Err(Error::Other(format!(
                "{module} {version} is marked insecure"
            )));
        }
        for rid in rids {
            if let Some(b) = v.platforms.get(*rid) {
                return Ok(ResolvedModule {
                    name: module.to_string(),
                    version: version.to_string(),
                    rid: (*rid).to_string(),
                    url: b.url.clone(),
                    sha256: b.sha256.clone(),
                    sig: b.sig.clone(),
                });
            }
        }
        Err(Error::Other(format!(
            "{module} {version} has no build for {}",
            rids.join(" or ")
        )))
    }
}

#[derive(Debug, Clone)]
pub struct InstalledModule {
    pub name: String,
    pub version: String,
    /// Folder to hand to the client as `ROBUST_MODULE_<NAME>`.
    pub dir: PathBuf,
    pub downloaded: bool,
}

/// Names and versions become folder names.
fn check_path_part(s: &str) -> Result<()> {
    let ok = !s.is_empty()
        && s.len() <= 100
        && !s.starts_with('.')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'));
    if ok {
        Ok(())
    } else {
        Err(Error::Other(format!("unsafe module name or version {s:?}")))
    }
}

pub fn module_dir(dirs: &DataDirs, name: &str, version: &str) -> Result<PathBuf> {
    check_path_part(name)?;
    check_path_part(version)?;
    Ok(dirs.modules().join(name).join(version))
}

/// Installs `name` at exactly `version` (as found by [`ModuleIndex::version_for_engine`]).
/// Verifies SHA-256 and the Ed25519 signature before anything is unpacked.
///
/// Concurrent installs (another launcher process, the CLI) do not fight over the same files: the
/// download and the unpack go to names with a pid and a timestamp in them, and the finished folder
/// appears in one `rename`.
pub async fn install_module(
    client: &Client,
    index: &ModuleIndex,
    dirs: &DataDirs,
    name: &str,
    version: &str,
    key: &VerifyingKey,
    progress: Option<ProgressFn<'_>>,
) -> Result<InstalledModule> {
    let dir = module_dir(dirs, name, version)?;
    let installed = |downloaded| InstalledModule {
        name: name.to_string(),
        version: version.to_string(),
        dir: dir.clone(),
        downloaded,
    };
    if dir.join(DONE_MARKER).is_file() {
        return Ok(installed(false));
    }

    let build = index.resolve(name, version, preferred_rids())?;
    let want_sha = parse_hash_hex(&build.sha256)?;
    let (pid, nanos) = unique_suffix();

    std::fs::create_dir_all(dirs.scratch())?;
    let part = dirs
        .scratch()
        .join(format!("module-{name}-{version}.zip.{pid}-{nanos}.part"));
    let tmp = dir
        .parent()
        .ok_or_else(|| Error::Other(format!("bad module folder {}", dir.display())))?
        .join(format!("{version}.tmp-{pid}-{nanos}"));
    let result = download_check_unpack(
        client,
        &build,
        &want_sha,
        key,
        &InstallPaths {
            part: &part,
            tmp: &tmp,
            dir: &dir,
        },
        progress,
    )
    .await;
    let _ = std::fs::remove_file(&part);
    if result.is_err() {
        // Only our own temporary folder: the target may belong to a running client.
        let _ = std::fs::remove_dir_all(&tmp);
    }
    result?;
    Ok(installed(true))
}

fn unique_suffix() -> (u32, u128) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    (std::process::id(), nanos)
}

/// Where a module install puts its files: the download, the folder being unpacked and the final
/// folder. All three differ, so two processes never touch the same ones.
struct InstallPaths<'a> {
    part: &'a Path,
    tmp: &'a Path,
    dir: &'a Path,
}

async fn download_check_unpack(
    client: &Client,
    build: &ResolvedModule,
    want_sha: &[u8; 32],
    key: &VerifyingKey,
    paths: &InstallPaths<'_>,
    progress: Option<ProgressFn<'_>>,
) -> Result<()> {
    let actual = download_to_file(client, &build.url, paths.part, progress).await?;
    if &actual != want_sha {
        return Err(Error::HashMismatch {
            expected: to_hex(want_sha),
            actual: to_hex(&actual),
        });
    }

    // Signature check and unpack are CPU and disk work of up to a few hundred MB: keep them off the
    // async runtime so cancellation and progress stay responsive.
    let part = paths.part.to_owned();
    let tmp_dir = paths.tmp.to_owned();
    let sig = build.sig.clone();
    let key = *key;
    let marker = to_hex(want_sha);
    tokio::task::spawn_blocking(move || -> Result<()> {
        let data = std::fs::read(&part)?;
        signature::verify(&key, &data, &sig)?;

        if let Some(parent) = tmp_dir.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir)?;
        unpack_zip(&part, &tmp_dir)?;
        std::fs::write(tmp_dir.join(DONE_MARKER), marker)?;
        Ok(())
    })
    .await
    .map_err(|e| Error::Other(format!("module unpack task failed: {e}")))??;

    commit_unpack(paths.tmp, paths.dir)
}

/// Moves a finished unpack into place.
///
/// `rename` is atomic, so another process either sees the old state or the complete folder. A folder
/// without [`DONE_MARKER`] is a leftover from a crash and may be replaced; a folder that is in use
/// (a running client holds its files) is left alone and reported instead of being deleted.
fn commit_unpack(tmp: &Path, dir: &Path) -> Result<()> {
    if dir.exists() && !dir.join(DONE_MARKER).is_file() {
        std::fs::remove_dir_all(dir).map_err(|e| {
            Error::Other(format!(
                "module folder {} is in use and cannot be replaced: {e}",
                dir.display()
            ))
        })?;
    }
    if !dir.exists() {
        match std::fs::rename(tmp, dir) {
            Ok(()) => return Ok(()),
            Err(_e) if dir.join(DONE_MARKER).is_file() => {
                // Another process finished the same module first; its folder is as good as ours.
                let _ = std::fs::remove_dir_all(tmp);
                return Ok(());
            }
            Err(e) => {
                let _ = std::fs::remove_dir_all(tmp);
                return Err(e.into());
            }
        }
    }
    // Someone else installed the module while we were unpacking.
    let _ = std::fs::remove_dir_all(tmp);
    Ok(())
}

/// Unpacks `zip_path` into `dest`. Entries that would land outside `dest` make the whole unpack
/// fail: a signed module has no reason to contain them.
pub fn unpack_zip(zip_path: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| Error::Other(format!("bad module zip: {e}")))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| Error::Other(format!("bad module zip entry {i}: {e}")))?;
        let name = entry.name().to_string();
        let relative = entry
            .enclosed_name()
            .filter(|p| p.components().all(|c| matches!(c, Component::Normal(_))))
            .ok_or_else(|| {
                Error::Other(format!(
                    "module zip entry {name:?} points outside the module folder"
                ))
            })?;
        let target = dest.join(relative);

        if entry.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&target)?;
        std::io::copy(&mut entry, &mut out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::build_client;
    use crate::signature::tests::{sign_hex, test_key};
    use crate::zipmode::tests::make_zip;

    fn v(s: &str) -> Vec<u64> {
        parse_version(s).unwrap()
    }

    #[test]
    fn manifest_yml_parsing() {
        let m = ResourceManifest::parse(
            b"modules:\n  - Robust.Client.WebView\nmultiWindow: true\nwindowIconSet: /x\n",
        )
        .unwrap();
        assert_eq!(m.modules, ["Robust.Client.WebView"]);
        assert!(m.multi_window);

        // The real manifest.yml of a build: other keys and comments only.
        let real = b"defaultWindowTitle: Space Station 14\nwindowIconSet: /Textures/Logo/icon\n\n# comment\n";
        assert_eq!(
            ResourceManifest::parse(real).unwrap(),
            ResourceManifest::default()
        );

        assert_eq!(
            ResourceManifest::parse(b"").unwrap(),
            ResourceManifest::default()
        );
        assert_eq!(
            ResourceManifest::parse(b"# nothing\n").unwrap(),
            ResourceManifest::default()
        );
        assert!(ResourceManifest::parse(b"modules: [unclosed").is_err());
        assert!(ResourceManifest::parse(b"modules: 5").is_err());
    }

    #[test]
    fn version_parsing_and_order() {
        assert_eq!(v("0.85.1.1"), [0, 85, 1, 1]);
        assert_eq!(v("264.0.2-fix-physics"), [264, 0, 2]);
        assert_eq!(v("277"), [277]);
        assert!(parse_version("abc").is_err());
        assert!(parse_version("").is_err());

        use std::cmp::Ordering::*;
        assert_eq!(
            compare_versions(&v("1.2"), &v("1.2.0")),
            Equal,
            "missing parts count as zero"
        );
        assert_eq!(
            compare_versions(&v("1.10"), &v("1.9")),
            Greater,
            "numbers, not text"
        );
        assert_eq!(compare_versions(&v("0.99.0"), &v("277.0.0")), Less);
    }

    fn index() -> ModuleIndex {
        serde_json::from_str(
            r#"{"modules": {"Robust.Client.WebView": {"versions": {
                "0.8.17":  {"insecure": true,  "platforms": {"win-x64": {"url": "u1", "sha256": "00", "sig": "AA"}}},
                "0.85.1":  {"insecure": false, "platforms": {"win-x64": {"url": "u2", "sha256": "00", "sig": "AA"}}},
                "0.90.0":  {"platforms": {"win-x64": {"url": "u3", "sha256": "00", "sig": "AA"}}},
                "0.120.0": {"platforms": {"linux-x64": {"url": "u4", "sha256": "00", "sig": "AA"}}}
            }}}}"#,
        )
        .unwrap()
    }

    #[test]
    fn picks_the_newest_module_not_newer_than_the_engine() {
        let idx = index();
        assert_eq!(
            idx.version_for_engine("Robust.Client.WebView", "0.89.0")
                .unwrap(),
            "0.85.1"
        );
        assert_eq!(
            idx.version_for_engine("Robust.Client.WebView", "0.90.0")
                .unwrap(),
            "0.90.0"
        );
        assert_eq!(
            idx.version_for_engine("Robust.Client.WebView", "277.0.0")
                .unwrap(),
            "0.120.0"
        );
        assert_eq!(
            idx.version_for_engine("Robust.Client.WebView", "264.0.2-fix-physics")
                .unwrap(),
            "0.120.0"
        );
        assert!(idx
            .version_for_engine("Robust.Client.WebView", "0.1.0")
            .is_err());
        assert!(idx.version_for_engine("Nope", "277.0.0").is_err());
    }

    #[test]
    fn resolve_refuses_insecure_and_missing_platforms() {
        let idx = index();
        assert!(idx
            .resolve("Robust.Client.WebView", "0.8.17", &["win-x64"])
            .is_err());
        assert_eq!(
            idx.resolve("Robust.Client.WebView", "0.90.0", &["win-x64"])
                .unwrap()
                .url,
            "u3"
        );
        assert!(
            idx.resolve("Robust.Client.WebView", "0.120.0", &["win-x64"])
                .is_err(),
            "linux only"
        );
        assert!(idx
            .resolve("Robust.Client.WebView", "9.9.9", &["win-x64"])
            .is_err());
    }

    #[test]
    fn folder_names_are_checked() {
        let dirs = DataDirs::at("C:/d");
        assert!(module_dir(&dirs, "Robust.Client.WebView", "0.90.0").is_ok());
        for bad in ["..", "a/b", "a\\b", "", ".x"] {
            assert!(module_dir(&dirs, bad, "1").is_err(), "{bad:?}");
            assert!(module_dir(&dirs, "ok", bad).is_err(), "{bad:?}");
        }
    }

    use std::sync::{Arc, Mutex};

    use crate::hash::sha256;
    use crate::test_util::{serve_with_base, Req};

    struct Fixture {
        base: String,
        seen: Arc<Mutex<Vec<Req>>>,
        dirs: DataDirs,
        _tmp: tempfile::TempDir,
    }

    /// A server with `modules.json` (pointing at itself) and one module zip.
    async fn fixture(zip: Vec<u8>, sha_hex: String, sig_hex: String) -> Fixture {
        let (base, seen) = serve_with_base(move |base| {
            let index = format!(
                r#"{{"modules": {{"Robust.Client.WebView": {{"versions": {{"0.90.0": {{"platforms": {{"win-x64": {{
                    "url": "{base}module.zip", "sha256": "{sha_hex}", "sig": "{sig_hex}"}}}}}}}}}}}}}}"#
            );
            move |req: &Req| match req.path.as_str() {
                "/modules.json" => (200, vec![], index.clone().into_bytes()),
                "/module.zip" => (200, vec![], zip.clone()),
                _ => (404, vec![], vec![]),
            }
        })
        .await;
        let tmp = tempfile::tempdir().unwrap();
        Fixture {
            base,
            seen,
            dirs: DataDirs::at(tmp.path()),
            _tmp: tmp,
        }
    }

    async fn install(f: &Fixture) -> Result<InstalledModule> {
        let client = build_client().unwrap();
        let urls = UrlFallbackSet::new([f.base.clone()]);
        let index = ModuleIndex::fetch(&client, &urls).await?;
        install_module(
            &client,
            &index,
            &f.dirs,
            "Robust.Client.WebView",
            "0.90.0",
            &test_key().verifying_key(),
            None,
        )
        .await
    }

    fn module_zip() -> Vec<u8> {
        make_zip(&[
            ("Robust.Client.WebView.exe", b"MZ fake"),
            ("locales/en-US.pak", &vec![5u8; 3000]),
            ("cef/", b""),
        ])
    }

    #[tokio::test]
    async fn installs_a_module_and_then_reuses_it() {
        let zip = module_zip();
        let f = fixture(
            zip.clone(),
            to_hex(&sha256(&zip)),
            sign_hex(&test_key(), &zip),
        )
        .await;

        let m = install(&f).await.unwrap();
        assert!(m.downloaded);
        assert_eq!(
            m.dir,
            f.dirs
                .modules()
                .join("Robust.Client.WebView")
                .join("0.90.0")
        );
        assert_eq!(
            std::fs::read(m.dir.join("Robust.Client.WebView.exe")).unwrap(),
            b"MZ fake"
        );
        assert_eq!(
            std::fs::read(m.dir.join("locales").join("en-US.pak")).unwrap(),
            vec![5u8; 3000]
        );
        assert!(m.dir.join("cef").is_dir());
        assert!(m.dir.join(DONE_MARKER).is_file());
        assert_eq!(
            std::fs::read_dir(f.dirs.scratch()).unwrap().count(),
            0,
            "scratch file removed"
        );

        let requests = f.seen.lock().unwrap().len();
        let again = install(&f).await.unwrap();
        assert!(!again.downloaded);
        // Only the index is fetched again, not the module.
        assert_eq!(f.seen.lock().unwrap().len(), requests + 1);
        assert!(
            f.seen
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.path == "/module.zip")
                .count()
                == 1
        );
    }

    #[tokio::test]
    async fn bad_hash_or_signature_leaves_nothing() {
        let zip = module_zip();

        let f = fixture(zip.clone(), "00".repeat(32), sign_hex(&test_key(), &zip)).await;
        let err = install(&f).await.unwrap_err();
        assert!(matches!(err, Error::HashMismatch { .. }), "{err}");
        assert!(!module_dir(&f.dirs, "Robust.Client.WebView", "0.90.0")
            .unwrap()
            .exists());

        let other_key = ed25519_dalek::SigningKey::from_bytes(&[3u8; 32]);
        let f = fixture(
            zip.clone(),
            to_hex(&sha256(&zip)),
            sign_hex(&other_key, &zip),
        )
        .await;
        let err = install(&f).await.unwrap_err();
        assert!(err.to_string().contains("signature"), "{err}");
        assert!(!module_dir(&f.dirs, "Robust.Client.WebView", "0.90.0")
            .unwrap()
            .exists());
        assert_eq!(std::fs::read_dir(f.dirs.scratch()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn entries_escaping_the_folder_are_refused() {
        let zip = make_zip(&[("fine.txt", b"ok"), ("../evil.txt", b"gotcha")]);
        let f = fixture(
            zip.clone(),
            to_hex(&sha256(&zip)),
            sign_hex(&test_key(), &zip),
        )
        .await;

        let err = install(&f).await.unwrap_err();
        assert!(err.to_string().contains("outside"), "{err}");
        let dir = module_dir(&f.dirs, "Robust.Client.WebView", "0.90.0").unwrap();
        assert!(!dir.exists(), "the half-unpacked module is removed");
        assert!(!dir.parent().unwrap().join("evil.txt").exists());
    }

    #[tokio::test]
    async fn an_unfinished_folder_is_replaced() {
        let zip = module_zip();
        let f = fixture(
            zip.clone(),
            to_hex(&sha256(&zip)),
            sign_hex(&test_key(), &zip),
        )
        .await;
        let dir = module_dir(&f.dirs, "Robust.Client.WebView", "0.90.0").unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("stale.bin"), b"left over from a crash").unwrap();

        let m = install(&f).await.unwrap();
        assert!(m.downloaded);
        assert!(!dir.join("stale.bin").exists());
        assert!(dir.join("Robust.Client.WebView.exe").is_file());
    }

    // ---- P1-9: two processes installing the same module ----

    #[test]
    fn a_module_finished_by_another_process_is_kept() {
        let tmp_root = tempfile::tempdir().unwrap();
        let tmp = tmp_root.path().join("0.90.0.tmp-1-2");
        let dir = tmp_root.path().join("0.90.0");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("ours.exe"), b"ours").unwrap();
        std::fs::write(tmp.join(DONE_MARKER), "aa").unwrap();
        // The other process won the race and its folder is complete.
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("theirs.exe"), b"theirs").unwrap();
        std::fs::write(dir.join(DONE_MARKER), "bb").unwrap();

        commit_unpack(&tmp, &dir).unwrap();
        assert!(!tmp.exists(), "our temporary folder is gone");
        assert_eq!(std::fs::read(dir.join("theirs.exe")).unwrap(), b"theirs");
        assert!(!dir.join("ours.exe").exists());
    }

    #[tokio::test]
    async fn two_installs_of_one_module_both_succeed() {
        let zip = module_zip();
        let f = fixture(
            zip.clone(),
            to_hex(&sha256(&zip)),
            sign_hex(&test_key(), &zip),
        )
        .await;
        let client = build_client().unwrap();
        let urls = UrlFallbackSet::new([f.base.clone()]);
        let index = ModuleIndex::fetch(&client, &urls).await.unwrap();
        let key = test_key().verifying_key();

        let (a, b) = tokio::join!(
            install_module(
                &client,
                &index,
                &f.dirs,
                "Robust.Client.WebView",
                "0.90.0",
                &key,
                None
            ),
            install_module(
                &client,
                &index,
                &f.dirs,
                "Robust.Client.WebView",
                "0.90.0",
                &key,
                None
            ),
        );
        assert!(a.is_ok(), "{:?}", a.err());
        assert!(b.is_ok(), "{:?}", b.err());

        let dir = module_dir(&f.dirs, "Robust.Client.WebView", "0.90.0").unwrap();
        assert!(dir.join(DONE_MARKER).is_file());
        assert!(dir.join("Robust.Client.WebView.exe").is_file());

        // No temporary folders and no scratch files survive either install.
        let leftovers: Vec<String> = std::fs::read_dir(dir.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        assert_eq!(std::fs::read_dir(f.dirs.scratch()).unwrap().count(), 0);
    }
}
