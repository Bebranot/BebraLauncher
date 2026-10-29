//! Engine modules such as `Robust.Client.WebView` (docs/protocol.md, section 6.3).
//!
//! A build lists the modules it needs in `manifest.yml`. Each module is a signed zip that is
//! unpacked into `modules/<name>/<version>/`; the client finds it through `ROBUST_MODULE_<NAME>`.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use ed25519_dalek::VerifyingKey;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::download::{download_to_file, ProgressFn};
use crate::engine::{preferred_rids, PlatformBuild};
use crate::hash::{parse_hash_hex, to_hex};
use crate::http::UrlFallbackSet;
use crate::paths::DataDirs;
use crate::signature;
use crate::{Error, Result};

/// Written last into a module folder: its presence means the install finished.
const DONE_MARKER: &str = ".installed";

/// What the marker records: which zip was unpacked, and how much of it there is. On reuse the
/// folder is compared with it, so a half-deleted or edited module is installed again instead of
/// being handed to the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstalledMarker {
    sha256: String,
    files: u64,
    bytes: u64,
}

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
    ///
    /// Insecure versions and versions without a build for one of `rids` are skipped here rather
    /// than failing later in [`ModuleIndex::resolve`]. Equal version numbers (`1.2` and `1.2.0` are
    /// the same version) are told apart by the original string, so the answer does not depend on
    /// `HashMap` iteration order.
    pub fn version_for_engine(
        &self,
        module: &str,
        engine_version: &str,
        rids: &[&str],
    ) -> Result<String> {
        let data = self
            .modules
            .get(module)
            .ok_or_else(|| Error::Other(format!("engine module {module} is not in the index")))?;
        let engine = parse_version(engine_version)?;

        data.versions
            .iter()
            .filter(|(_, v)| !v.insecure)
            .filter(|(_, v)| {
                let mut have_platform = false;
                for rid in rids {
                    if v.platforms.contains_key(*rid) {
                        have_platform = true;
                        break;
                    }
                }
                have_platform
            })
            .filter_map(|(v, _)| parse_version(v).ok().map(|parsed| (parsed, v)))
            .filter(|(parsed, _)| compare_versions(&engine, parsed).is_ge())
            .max_by(|(a, a_raw), (b, b_raw)| compare_versions(a, b).then_with(|| a_raw.cmp(b_raw)))
            .map(|(_, v)| v.clone())
            .ok_or_else(|| {
                Error::Other(format!(
                    "no usable version of {module} fits engine {engine_version} on {}",
                    rids.join(" or ")
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

    // The index decides what the folder must contain. If the version has meanwhile disappeared
    // from the index, an intact folder is still usable: refusing to launch would be worse.
    let build = match index.resolve(name, version, preferred_rids()) {
        Ok(build) => build,
        Err(e) => {
            if verified_install(&dir, None).unwrap_or(false) {
                return Ok(installed(false));
            }
            return Err(e);
        }
    };
    if verified_install(&dir, Some(&build.sha256))? {
        return Ok(installed(false));
    }

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

/// Is `dir` a finished install of the module zip with `expected_sha` (if given)?
///
/// The marker alone is not trusted: the number and total size of the files are compared with it,
/// so a folder whose files were deleted, added or edited is installed again.
fn verified_install(dir: &Path, expected_sha: Option<&str>) -> Result<bool> {
    if !dir.is_dir() {
        return Ok(false);
    }
    let Ok(text) = std::fs::read(dir.join(DONE_MARKER)) else {
        return Ok(false);
    };
    let Ok(marker) = serde_json::from_slice::<InstalledMarker>(&text) else {
        return Ok(false);
    };
    if let Some(want) = expected_sha {
        if !marker.sha256.eq_ignore_ascii_case(want.trim()) {
            return Ok(false);
        }
    }
    let (files, bytes) = match dir_totals(dir) {
        Ok(v) => v,
        Err(_) => return Ok(false),
    };
    Ok(files == marker.files && bytes == marker.bytes)
}

/// Writes the finished-install marker with what was unpacked.
fn write_marker(dir: &Path, sha256: &str) -> Result<()> {
    let (files, bytes) = dir_totals(dir)?;
    let marker = InstalledMarker {
        sha256: sha256.to_ascii_lowercase(),
        files,
        bytes,
    };
    let text = serde_json::to_vec(&marker)
        .map_err(|e| Error::Other(format!("cannot write the module marker: {e}")))?;
    std::fs::write(dir.join(DONE_MARKER), text)?;
    Ok(())
}

/// Number of files and their total size under `dir`, not counting the marker itself.
fn dir_totals(dir: &Path) -> Result<(u64, u64)> {
    let mut files = 0u64;
    let mut bytes = 0u64;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_name() == DONE_MARKER {
            continue;
        }
        let meta = entry.metadata()?;
        if meta.is_dir() {
            let (f, b) = dir_totals(&entry.path())?;
            files += f;
            bytes += b;
        } else if meta.is_file() {
            files += 1;
            bytes += meta.len();
        }
    }
    Ok((files, bytes))
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
    let marker_sha = to_hex(want_sha);
    tokio::task::spawn_blocking(move || -> Result<()> {
        let data = std::fs::read(&part)?;
        signature::verify(&key, &data, &sig)?;

        if let Some(parent) = tmp_dir.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir)?;
        unpack_zip(&part, &tmp_dir)?;
        write_marker(&tmp_dir, &marker_sha)?;
        Ok(())
    })
    .await
    .map_err(|e| Error::Other(format!("module unpack task failed: {e}")))??;

    commit_unpack(paths.tmp, paths.dir)
}

/// Moves a finished unpack into place.
///
/// `rename` is atomic, so another process either sees the old state or the complete folder. A
/// folder that passes [`verified_install`] is kept (another process beat us to it); a folder that
/// does not (left over from a crash, or edited by somebody) is replaced. A folder that is in use
/// by a running client cannot be removed and is reported instead of being silently deleted.
fn commit_unpack(tmp: &Path, dir: &Path) -> Result<()> {
    if dir.exists() {
        if verified_install(dir, None)? {
            let _ = std::fs::remove_dir_all(tmp);
            return Ok(());
        }
        std::fs::remove_dir_all(dir).map_err(|e| {
            Error::Other(format!(
                "module folder {} is in use and cannot be replaced: {e}",
                dir.display()
            ))
        })?;
    }
    match std::fs::rename(tmp, dir) {
        Ok(()) => Ok(()),
        Err(_e) if verified_install(dir, None).unwrap_or(false) => {
            // Another process finished the same module first; its folder is as good as ours.
            let _ = std::fs::remove_dir_all(tmp);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(tmp);
            Err(e.into())
        }
    }
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
        let win = &["win-x64"][..];
        assert_eq!(
            idx.version_for_engine("Robust.Client.WebView", "0.89.0", win)
                .unwrap(),
            "0.85.1"
        );
        assert_eq!(
            idx.version_for_engine("Robust.Client.WebView", "0.90.0", win)
                .unwrap(),
            "0.90.0"
        );
        assert_eq!(
            idx.version_for_engine("Robust.Client.WebView", "277.0.0", win)
                .unwrap(),
            "0.90.0",
            "0.120.0 is linux-only and is left for the platform check, not picked and refused later"
        );
        assert_eq!(
            idx.version_for_engine("Robust.Client.WebView", "264.0.2-fix-physics", win)
                .unwrap(),
            "0.90.0"
        );
        assert!(idx
            .version_for_engine("Robust.Client.WebView", "0.1.0", win)
            .is_err());
        assert!(idx.version_for_engine("Nope", "277.0.0", win).is_err());

        // The same index seen from Linux: only the linux build fits.
        assert_eq!(
            idx.version_for_engine("Robust.Client.WebView", "277.0.0", &["linux-x64"])
                .unwrap(),
            "0.120.0"
        );
        assert!(idx
            .version_for_engine("Robust.Client.WebView", "0.89.0", &["linux-x64"])
            .is_err());
    }

    #[test]
    fn insecure_versions_are_never_picked() {
        let idx = index();
        // 0.86.0 has no newer-safe build: 0.85.1 is used, never the insecure 0.8.17.
        assert_eq!(
            idx.version_for_engine("Robust.Client.WebView", "0.86.0", &["win-x64"])
                .unwrap(),
            "0.85.1"
        );
    }

    #[test]
    fn equal_versions_are_picked_deterministically() {
        let idx = ModuleIndex {
            modules: [(
                "M".to_string(),
                ModuleData {
                    versions: ["1.2", "1.2.0"]
                        .iter()
                        .map(|v| {
                            (
                                (*v).to_string(),
                                ModuleVersionData {
                                    insecure: false,
                                    platforms: [(
                                        "win-x64".to_string(),
                                        PlatformBuild {
                                            url: "u".into(),
                                            sha256: "00".into(),
                                            sig: "AA".into(),
                                        },
                                    )]
                                    .into_iter()
                                    .collect(),
                                },
                            )
                        })
                        .collect(),
                },
            )]
            .into_iter()
            .collect(),
        };
        let first = idx.version_for_engine("M", "9.0.0", &["win-x64"]).unwrap();
        for _ in 0..20 {
            assert_eq!(
                idx.version_for_engine("M", "9.0.0", &["win-x64"]).unwrap(),
                first,
                "HashMap order must not decide between 1.2 and 1.2.0"
            );
        }
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

    #[tokio::test]
    async fn a_module_whose_files_were_tampered_with_is_installed_again() {
        let zip = module_zip();
        let f = fixture(
            zip.clone(),
            to_hex(&sha256(&zip)),
            sign_hex(&test_key(), &zip),
        )
        .await;
        let m = install(&f).await.unwrap();
        assert!(m.downloaded);

        // Somebody deleted a file of the installed module: the marker no longer matches.
        std::fs::remove_file(m.dir.join("locales").join("en-US.pak")).unwrap();
        let again = install(&f).await.unwrap();
        assert!(again.downloaded, "a damaged module is downloaded again");
        assert!(again.dir.join("locales").join("en-US.pak").is_file());

        // An edited file is noticed too (the totals no longer match the marker).
        let exe = again.dir.join("Robust.Client.WebView.exe");
        std::fs::write(&exe, b"MZ evil, much longer").unwrap();
        let third = install(&f).await.unwrap();
        assert!(third.downloaded);
        assert_eq!(std::fs::read(&exe).unwrap(), b"MZ fake");
    }

    #[test]
    fn an_install_without_a_marker_or_with_a_broken_one_is_not_trusted() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("0.90.0");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.dll"), b"x").unwrap();
        assert!(!verified_install(&dir, None).unwrap(), "no marker");

        std::fs::write(dir.join(DONE_MARKER), b"not json").unwrap();
        assert!(!verified_install(&dir, None).unwrap(), "garbage marker");

        write_marker(&dir, &"ab".repeat(32)).unwrap();
        assert!(verified_install(&dir, None).unwrap());
        assert!(verified_install(&dir, Some(&"AB".repeat(32))).unwrap());
        assert!(!verified_install(&dir, Some(&"cd".repeat(32))).unwrap());

        std::fs::write(dir.join("extra.dll"), b"x").unwrap();
        assert!(
            !verified_install(&dir, None).unwrap(),
            "an added file changes the totals"
        );
    }

    // ---- P1-9: two processes installing the same module ----

    #[test]
    fn a_module_finished_by_another_process_is_kept() {
        let tmp_root = tempfile::tempdir().unwrap();
        let tmp = tmp_root.path().join("0.90.0.tmp-1-2");
        let dir = tmp_root.path().join("0.90.0");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("ours.exe"), b"ours").unwrap();
        write_marker(&tmp, &"aa".repeat(32)).unwrap();
        // The other process won the race and its folder is complete.
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("theirs.exe"), b"theirs").unwrap();
        write_marker(&dir, &"bb".repeat(32)).unwrap();

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
