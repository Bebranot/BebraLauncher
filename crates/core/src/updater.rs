//! Brings a build into the content database (docs/protocol.md, sections 2 and 3).

use std::path::Path;

use reqwest::Client;

use ed25519_dalek::VerifyingKey;

use crate::content_db::cull::{CullPolicy, CullReport};
use crate::content_db::ContentDb;
use crate::download::{self, ProgressFn};
use crate::engine::{self, InstalledEngine};
use crate::http::UrlFallbackSet;
use crate::modules::{self, InstalledModule, ModuleIndex, ResourceManifest};
use crate::paths::DataDirs;
use crate::server::BuildInfo;
use crate::{Error, Result};

/// How a build is delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadMode {
    /// File list plus only the missing files (delta updates). Preferred.
    Manifest,
    /// One big zip. Older servers.
    Zip,
}

/// Docs section 2, step 1: manifest mode needs all of `manifest_url`, `manifest_download_url` and
/// `manifest_hash`; otherwise the zip is used if there is one.
pub fn download_mode(build: &BuildInfo) -> Result<DownloadMode> {
    if build.manifest_url().is_some()
        && build.manifest_download_url().is_some()
        && build.manifest_hash().is_some()
    {
        Ok(DownloadMode::Manifest)
    } else if build.download_url().is_some() {
        Ok(DownloadMode::Zip)
    } else {
        Err(Error::Other(
            "server gave no download information at all".into(),
        ))
    }
}

#[derive(Debug, Clone)]
pub struct InstalledContent {
    pub version_id: i64,
    pub mode: DownloadMode,
    /// Files in the build.
    pub files: usize,
    /// Distinct files fetched from the network this time.
    pub downloaded: usize,
    /// The version was already in the database, nothing was done.
    pub reused: bool,
}

/// Makes sure `build` is fully present in the database and returns its version id.
/// `scratch_dir` is where a zip download is kept while it is unpacked.
pub async fn install_content(
    client: &Client,
    db: &ContentDb,
    build: &BuildInfo,
    scratch_dir: &Path,
    progress: Option<ProgressFn<'_>>,
) -> Result<InstalledContent> {
    let mode = download_mode(build)?;

    if let Some(existing) = db.find_version(build)? {
        let files = || -> Result<usize> { Ok(db.manifest_entries(existing.id)?.len()) };
        if is_exact_match(db, build, existing.id, &existing)? {
            db.touch_version(existing.id)?;
            return Ok(InstalledContent {
                version_id: existing.id,
                mode,
                files: files()?,
                downloaded: 0,
                reused: true,
            });
        }
        // Same content (same hash) under another fork/version name or engine: give it a version of
        // its own without downloading anything. If the engine changes and the version has engine
        // modules, their versions would need recalculating, so install normally instead.
        if can_duplicate(db, build, existing.id)? {
            let version_id = db.transaction(|db| {
                db.duplicate_version(
                    existing.id,
                    &build.fork_id,
                    &build.version,
                    &build.engine_version,
                )
            })?;
            return Ok(InstalledContent {
                version_id,
                mode,
                files: files()?,
                downloaded: 0,
                reused: false,
            });
        }
    }

    match mode {
        DownloadMode::Manifest => install_manifest(client, db, build, progress).await,
        DownloadMode::Zip => {
            crate::zipmode::install_zip(client, db, build, scratch_dir, progress).await
        }
    }
}

/// Where engines and modules come from and which key signs them.
#[derive(Clone)]
pub struct Sources {
    pub builds: UrlFallbackSet,
    pub key: VerifyingKey,
}

impl Sources {
    /// The Wizards' Den builds CDN and signing key.
    pub fn official() -> Result<Self> {
        Ok(Self {
            builds: UrlFallbackSet::new(engine::BUILDS_URLS),
            key: crate::signature::official_key()?,
        })
    }
}

/// Everything needed to start the game for one installed version.
#[derive(Debug, Clone)]
pub struct PreparedLaunch {
    pub version_id: i64,
    pub engine: InstalledEngine,
    pub modules: Vec<InstalledModule>,
    /// From `manifest.yml`; the official launcher passes this to Steam builds only.
    pub multi_window: bool,
}

/// Makes the engine and the engine modules of an installed version ready to run (docs section 6).
///
/// Module versions are worked out from the build's `manifest.yml` and the engine version, and saved
/// as dependencies of the version. Safe to call again for a version that is already prepared:
/// then it only checks that everything is still on disk.
pub async fn prepare_launch(
    client: &Client,
    db: &ContentDb,
    dirs: &DataDirs,
    sources: &Sources,
    version_id: i64,
    progress: Option<ProgressFn<'_>>,
) -> Result<PreparedLaunch> {
    let engine_requested = robust_version(db, version_id)?.ok_or_else(|| {
        Error::Other(format!(
            "version {version_id} has no engine version recorded"
        ))
    })?;

    let resource = match db.read_file(version_id, "manifest.yml")? {
        Some(yaml) => ResourceManifest::parse(&yaml)?,
        None => ResourceManifest::default(),
    };

    // The module index is only fetched when some module is involved.
    let mut index: Option<ModuleIndex> = None;
    let have: Vec<String> = db
        .engine_dependencies(version_id)?
        .into_iter()
        .map(|(m, _)| m)
        .collect();
    for module in resource.modules.iter().filter(|m| !have.contains(m)) {
        if index.is_none() {
            index = Some(ModuleIndex::fetch(client, &sources.builds).await?);
        }
        let version = index
            .as_ref()
            .expect("fetched above")
            .version_for_engine(module, &engine_requested)?;
        db.add_engine_dependency(version_id, module, &version)?;
    }

    let engine = engine::install_engine(
        client,
        &sources.builds,
        dirs,
        &engine_requested,
        &sources.key,
        progress,
    )
    .await?;

    let mut modules = Vec::new();
    for (name, version) in db.engine_dependencies(version_id)? {
        if name == "Robust" {
            continue;
        }
        if index.is_none() {
            index = Some(ModuleIndex::fetch(client, &sources.builds).await?);
        }
        let installed = modules::install_module(
            client,
            index.as_ref().expect("fetched above"),
            dirs,
            &name,
            &version,
            &sources.key,
            progress,
        )
        .await?;
        modules.push(installed);
    }

    Ok(PreparedLaunch {
        version_id,
        engine,
        modules,
        multi_window: resource.multi_window,
    })
}

/// Housekeeping after an install: drops old versions (the official limits: 15 in total, 3 per
/// fork), never one that a running client is using.
pub fn cleanup(db: &ContentDb) -> Result<CullReport> {
    let in_use = db.running_client_versions(crate::process::is_process_running_from)?;
    db.cull(CullPolicy::default(), &in_use)
}

fn robust_version(db: &ContentDb, version_id: i64) -> Result<Option<String>> {
    Ok(db
        .engine_dependencies(version_id)?
        .into_iter()
        .find(|(m, _)| m == "Robust")
        .map(|(_, ver)| ver))
}

fn can_duplicate(db: &ContentDb, build: &BuildInfo, version_id: i64) -> Result<bool> {
    let has_modules = db
        .engine_dependencies(version_id)?
        .iter()
        .any(|(m, _)| m != "Robust");
    let same_engine =
        robust_version(db, version_id)?.as_deref() == Some(build.engine_version.as_str());
    Ok(!has_modules || same_engine)
}

fn is_exact_match(
    db: &ContentDb,
    build: &BuildInfo,
    version_id: i64,
    v: &crate::content_db::ContentVersion,
) -> Result<bool> {
    let engine = robust_version(db, version_id)?;
    Ok(v.fork_id.as_deref() == Some(build.fork_id.as_str())
        && v.fork_version.as_deref() == Some(build.version.as_str())
        && engine.as_deref() == Some(build.engine_version.as_str()))
}

async fn install_manifest(
    client: &Client,
    db: &ContentDb,
    build: &BuildInfo,
    progress: Option<ProgressFn<'_>>,
) -> Result<InstalledContent> {
    // `download_mode` checked these.
    let manifest_url = build.manifest_url().expect("manifest mode");
    let download_url = build.manifest_download_url().expect("manifest mode");
    let manifest_hash = build.manifest_hash().expect("manifest mode");

    let manifest = download::fetch_manifest(client, manifest_url, manifest_hash).await?;
    let missing = db.missing_entries(&manifest)?;

    let download_id = db.begin_interrupted_download()?;
    if !missing.is_empty() {
        download::check_download_protocol(client, download_url).await?;
        download::download_blobs(
            client,
            db,
            download_url,
            &manifest,
            &missing,
            download_id,
            progress,
        )
        .await?;
    }

    let version_id = db.transaction(|db| {
        let id = db.create_version(&build.fork_id, &build.version)?;
        db.fill_manifest(id, &manifest)?;

        // The stored files must add up to exactly the manifest the server promised.
        let stored = db.compute_manifest_hash(id)?;
        if stored != manifest.hash {
            return Err(Error::HashMismatch {
                expected: crate::hash::to_hex(&manifest.hash),
                actual: crate::hash::to_hex(&stored),
            });
        }
        db.set_version_hash(id, &manifest.hash)?;
        db.add_engine_dependency(id, "Robust", &build.engine_version)?;
        db.finish_download(download_id)?;
        Ok(id)
    })?;

    Ok(InstalledContent {
        version_id,
        mode: DownloadMode::Manifest,
        files: manifest.entries.len(),
        downloaded: missing.len(),
        reused: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(manifest: bool, zip: bool, hash: bool) -> BuildInfo {
        BuildInfo {
            manifest_url: manifest.then(|| "https://x/manifest".to_string()),
            manifest_download_url: manifest.then(|| "https://x/download".to_string()),
            manifest_hash: (manifest && hash).then(|| "0".repeat(64)),
            download_url: zip.then(|| "https://x/c.zip".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn manifest_mode_needs_all_three_fields() {
        assert_eq!(
            download_mode(&build(true, true, true)).unwrap(),
            DownloadMode::Manifest
        );
        assert_eq!(
            download_mode(&build(true, false, true)).unwrap(),
            DownloadMode::Manifest
        );
        // No manifest hash: the manifest could not be trusted, so fall back to the zip.
        assert_eq!(
            download_mode(&build(true, true, false)).unwrap(),
            DownloadMode::Zip
        );
        assert_eq!(
            download_mode(&build(false, true, false)).unwrap(),
            DownloadMode::Zip
        );
        assert!(download_mode(&build(false, false, false)).is_err());
        assert!(download_mode(&build(true, false, false)).is_err());
    }

    #[test]
    fn empty_strings_do_not_count() {
        let b = BuildInfo {
            manifest_url: Some(String::new()),
            manifest_download_url: Some(String::new()),
            manifest_hash: Some(String::new()),
            download_url: Some("https://x/c.zip".into()),
            ..Default::default()
        };
        assert_eq!(download_mode(&b).unwrap(), DownloadMode::Zip);
    }
}

#[cfg(test)]
mod e2e_tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use super::*;
    use crate::hash::{blake2b256, to_hex};
    use crate::http::build_client;
    use crate::manifest::HEADER;
    use crate::test_util::{serve, Req};

    /// Manifest mode never touches the scratch folder.
    fn scratch() -> &'static Path {
        Path::new("scratch-not-used")
    }

    fn files() -> Vec<(&'static str, Vec<u8>)> {
        vec![
            ("a.txt", b"alpha".to_vec()),
            ("b.bin", vec![b'b'; 6000]),
            ("c.txt", b"gamma".to_vec()),
        ]
    }

    fn manifest_text(files: &[(&str, Vec<u8>)]) -> String {
        let mut t = format!("{HEADER}\n");
        for (p, d) in files {
            t.push_str(&format!("{} {p}\n", to_hex(&blake2b256(d))));
        }
        t
    }

    /// Pre-compressed blob stream for the entries asked for in a POST body.
    fn stream_for(files: &[(&str, Vec<u8>)], body: &[u8]) -> Vec<u8> {
        let mut out = 1i32.to_le_bytes().to_vec();
        for chunk in body.chunks(4) {
            let idx = i32::from_le_bytes(chunk.try_into().unwrap()) as usize;
            let data = &files[idx].1;
            out.extend((data.len() as i32).to_le_bytes());
            if data.len() > 100 {
                let packed = zstd::bulk::compress(data, 3).unwrap();
                out.extend((packed.len() as i32).to_le_bytes());
                out.extend(packed);
            } else {
                out.extend(0i32.to_le_bytes());
                out.extend(data);
            }
        }
        out
    }

    struct Fake {
        base: String,
        seen: Arc<std::sync::Mutex<Vec<Req>>>,
        broken: Arc<AtomicBool>,
    }

    async fn fake_cdn() -> Fake {
        let files = files();
        let text = manifest_text(&files);
        let packed_manifest = zstd::bulk::compress(text.as_bytes(), 3).unwrap();
        let broken = Arc::new(AtomicBool::new(false));
        let broken2 = broken.clone();
        let (base, seen) = serve(
            move |req: &Req| match (req.method.as_str(), req.path.as_str()) {
                ("GET", "/manifest") => (
                    200,
                    vec![("Content-Encoding", "zstd".into())],
                    packed_manifest.clone(),
                ),
                ("OPTIONS", "/download") => (
                    204,
                    vec![
                        ("X-Robust-Download-Min-Protocol", "1".into()),
                        ("X-Robust-Download-Max-Protocol", "1".into()),
                    ],
                    vec![],
                ),
                ("POST", "/download") => {
                    let stream = if broken2.load(Ordering::SeqCst) {
                        // The first requested file arrives whole, then the stream dies in the middle
                        // of the next file header.
                        let mut s = stream_for(&files, &req.body[..4]);
                        s.extend([0xff, 0xff]);
                        s
                    } else {
                        stream_for(&files, &req.body)
                    };
                    (
                        200,
                        vec![("Content-Encoding", "zstd".into())],
                        zstd::bulk::compress(&stream, 3).unwrap(),
                    )
                }
                _ => (404, vec![], vec![]),
            },
        )
        .await;
        Fake { base, seen, broken }
    }

    fn build_for(base: &str) -> BuildInfo {
        let text = manifest_text(&files());
        BuildInfo {
            manifest_url: Some(format!("{base}manifest")),
            manifest_download_url: Some(format!("{base}download")),
            manifest_hash: Some(to_hex(&blake2b256(text.as_bytes()))),
            fork_id: "testfork".into(),
            version: "v1".into(),
            engine_version: "100.0.0".into(),
            ..Default::default()
        }
    }

    fn posts(fake: &Fake) -> Vec<Req> {
        fake.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.method == "POST")
            .cloned()
            .collect()
    }

    #[tokio::test]
    async fn installs_then_reuses() {
        let fake = fake_cdn().await;
        let db = ContentDb::open_in_memory().unwrap();
        let client = build_client().unwrap();
        let build = build_for(&fake.base);

        let first = install_content(&client, &db, &build, scratch(), None)
            .await
            .unwrap();
        assert!(!first.reused);
        assert_eq!((first.files, first.downloaded), (3, 3));
        assert_eq!(first.mode, DownloadMode::Manifest);

        // Everything is really there and adds up.
        let entries = db.manifest_entries(first.version_id).unwrap();
        assert_eq!(entries.len(), 3);
        let v = db.get_version(first.version_id).unwrap().unwrap();
        assert_eq!(
            v.hash,
            blake2b256(manifest_text(&files()).as_bytes()).to_vec()
        );
        assert_eq!(
            db.engine_dependencies(first.version_id).unwrap(),
            [("Robust".to_string(), "100.0.0".to_string())]
        );
        for (i, (_, data)) in files().iter().enumerate() {
            let cid = db.find_content_id(&entries[i].hash).unwrap().unwrap();
            assert_eq!(&db.read_blob(cid).unwrap(), data);
        }
        let leftovers: i64 = db
            .connection()
            .query_row("SELECT COUNT(*) FROM InterruptedDownload", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            leftovers, 0,
            "a finished download leaves no interrupted-download row"
        );

        let requests_before = fake.seen.lock().unwrap().len();
        let again = install_content(&client, &db, &build, scratch(), None)
            .await
            .unwrap();
        assert!(again.reused);
        assert_eq!(again.version_id, first.version_id);
        assert_eq!(
            fake.seen.lock().unwrap().len(),
            requests_before,
            "no network when already installed"
        );
    }

    #[tokio::test]
    async fn same_content_under_a_new_version_name_downloads_nothing() {
        let fake = fake_cdn().await;
        let db = ContentDb::open_in_memory().unwrap();
        let client = build_client().unwrap();

        let a = install_content(&client, &db, &build_for(&fake.base), scratch(), None)
            .await
            .unwrap();
        let mut renamed = build_for(&fake.base);
        renamed.version = "v2".into();
        let b = install_content(&client, &db, &renamed, scratch(), None)
            .await
            .unwrap();

        assert_ne!(a.version_id, b.version_id);
        assert_eq!((b.files, b.downloaded, b.reused), (3, 0, false));
        assert_eq!(posts(&fake).len(), 1, "blobs were fetched only once");
    }

    #[tokio::test]
    async fn interrupted_download_resumes_from_stored_blobs() {
        let fake = fake_cdn().await;
        let db = ContentDb::open_in_memory().unwrap();
        let client = build_client().unwrap();
        let build = build_for(&fake.base);

        fake.broken.store(true, Ordering::SeqCst);
        assert!(install_content(&client, &db, &build, scratch(), None)
            .await
            .is_err());
        let kept: i64 = db
            .connection()
            .query_row("SELECT COUNT(*) FROM Content", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kept, 1, "the file that arrived whole is kept");
        let versions: i64 = db
            .connection()
            .query_row("SELECT COUNT(*) FROM ContentVersion", [], |r| r.get(0))
            .unwrap();
        assert_eq!(versions, 0, "no half-built version");

        fake.broken.store(false, Ordering::SeqCst);
        let done = install_content(&client, &db, &build, scratch(), None)
            .await
            .unwrap();
        assert_eq!(
            (done.files, done.downloaded),
            (3, 2),
            "only the two missing files are fetched"
        );
        let last_post = posts(&fake).pop().unwrap();
        assert_eq!(last_post.body.len(), 2 * 4);
    }

    #[tokio::test]
    async fn wrong_manifest_hash_creates_nothing() {
        let fake = fake_cdn().await;
        let db = ContentDb::open_in_memory().unwrap();
        let mut build = build_for(&fake.base);
        build.manifest_hash = Some("0".repeat(64));

        let err = install_content(&build_client().unwrap(), &db, &build, scratch(), None)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::HashMismatch { .. }), "{err}");
        assert!(
            posts(&fake).is_empty(),
            "nothing is downloaded for an untrusted manifest"
        );
        let versions: i64 = db
            .connection()
            .query_row("SELECT COUNT(*) FROM ContentVersion", [], |r| r.get(0))
            .unwrap();
        assert_eq!(versions, 0);
    }

    #[tokio::test]
    async fn zip_build_installs_reuses_and_duplicates_without_downloading() {
        let zip = crate::zipmode::tests::make_zip(&[("a.txt", b"alpha"), ("b/c.txt", b"gamma")]);
        let body = zip.clone();
        let (base, seen) = serve(move |_| (200, vec![], body.clone())).await;
        let db = ContentDb::open_in_memory().unwrap();
        let client = build_client().unwrap();
        let scratch_dir = tempfile::tempdir().unwrap();
        let build = BuildInfo {
            download_url: Some(format!("{base}client.zip")),
            hash: Some(to_hex(&crate::hash::sha256(&zip))),
            fork_id: "zf".into(),
            version: "1".into(),
            engine_version: "100.0.0".into(),
            ..Default::default()
        };

        let first = install_content(&client, &db, &build, scratch_dir.path(), None)
            .await
            .unwrap();
        assert_eq!(
            (first.mode, first.files, first.reused),
            (DownloadMode::Zip, 2, false)
        );
        assert_eq!(seen.lock().unwrap().len(), 1);

        // Same build again: found by the zip hash, nothing is fetched.
        let again = install_content(&client, &db, &build, scratch_dir.path(), None)
            .await
            .unwrap();
        assert!(again.reused);
        assert_eq!(again.version_id, first.version_id);
        assert_eq!(seen.lock().unwrap().len(), 1);

        // Same zip, but the server now names another version and engine: a new version row that
        // shares the files, again without a download.
        let mut renamed = build.clone();
        renamed.version = "2".into();
        renamed.engine_version = "101.0.0".into();
        let dup = install_content(&client, &db, &renamed, scratch_dir.path(), None)
            .await
            .unwrap();
        assert!(!dup.reused);
        assert_ne!(dup.version_id, first.version_id);
        assert_eq!((dup.files, dup.downloaded), (2, 0));
        assert_eq!(seen.lock().unwrap().len(), 1, "no new request");
        assert_eq!(
            db.engine_dependencies(dup.version_id).unwrap(),
            [("Robust".to_string(), "101.0.0".to_string())]
        );
        assert_eq!(
            db.manifest_entries(dup.version_id).unwrap(),
            db.manifest_entries(first.version_id).unwrap()
        );
        let v = db.get_version(dup.version_id).unwrap().unwrap();
        assert_eq!(v.fork_version.as_deref(), Some("2"));
        let blobs: i64 = db
            .connection()
            .query_row("SELECT COUNT(*) FROM Content", [], |r| r.get(0))
            .unwrap();
        assert_eq!(blobs, 2);
    }

    #[tokio::test]
    async fn duplicating_is_skipped_when_the_engine_changes_and_modules_exist() {
        let zip = crate::zipmode::tests::make_zip(&[("a.txt", b"alpha")]);
        let body = zip.clone();
        let (base, seen) = serve(move |_| (200, vec![], body.clone())).await;
        let db = ContentDb::open_in_memory().unwrap();
        let client = build_client().unwrap();
        let scratch_dir = tempfile::tempdir().unwrap();
        let build = BuildInfo {
            download_url: Some(format!("{base}client.zip")),
            hash: Some(to_hex(&crate::hash::sha256(&zip))),
            fork_id: "zf".into(),
            version: "1".into(),
            engine_version: "100.0.0".into(),
            ..Default::default()
        };
        let first = install_content(&client, &db, &build, scratch_dir.path(), None)
            .await
            .unwrap();
        db.add_engine_dependency(first.version_id, "Robust.Client.WebView", "0.1.0")
            .unwrap();

        // Engine changes: the module version might change too, so the version is installed afresh.
        let mut newer = build.clone();
        newer.engine_version = "200.0.0".into();
        let second = install_content(&client, &db, &newer, scratch_dir.path(), None)
            .await
            .unwrap();
        assert_ne!(second.version_id, first.version_id);
        assert_eq!(seen.lock().unwrap().len(), 2, "downloaded again");

        // Engine unchanged, only the name differs: modules can be copied safely.
        let mut renamed = build.clone();
        renamed.version = "9".into();
        let third = install_content(&client, &db, &renamed, scratch_dir.path(), None)
            .await
            .unwrap();
        assert_eq!(third.downloaded, 0);
        assert_eq!(seen.lock().unwrap().len(), 2);
        assert!(db
            .engine_dependencies(third.version_id)
            .unwrap()
            .contains(&("Robust.Client.WebView".to_string(), "0.1.0".to_string())));
    }
}

#[cfg(test)]
mod prepare_tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::hash::{sha256, to_hex};
    use crate::http::build_client;
    use crate::signature::tests::{sign_hex, test_key};
    use crate::test_util::{serve_with_base, Req};
    use crate::zipmode::tests::make_zip;

    struct Cdn {
        base: String,
        seen: Arc<Mutex<Vec<Req>>>,
        build: BuildInfo,
        dirs: DataDirs,
        sources: Sources,
        _tmp: tempfile::TempDir,
    }

    impl Cdn {
        fn count(&self, path: &str) -> usize {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.path == path)
                .count()
        }
    }

    /// One server playing the game server's download, the engine index and the module index.
    /// `engine_versions`: what the engine index knows; the build asks for "100.0.0".
    async fn cdn(manifest_yml: &str) -> Cdn {
        let key = test_key();
        let engine_zip = b"PK engine".repeat(40);
        let module_zip = make_zip(&[("webview.exe", b"MZ"), ("data/x.bin", &[1u8; 500])]);
        let client_zip = make_zip(&[
            ("manifest.yml", manifest_yml.as_bytes()),
            ("a.txt", b"alpha"),
        ]);

        let (base, seen) = serve_with_base({
            let (engine_zip, module_zip, client_zip) = (engine_zip.clone(), module_zip.clone(), client_zip.clone());
            let key = key.clone();
            move |base| {
                let engine_index = format!(
                    r#"{{"100.0.0": {{"platforms": {{"win-x64": {{"url": "{base}engine.zip", "sha256": "{}", "sig": "{}"}}}}}}}}"#,
                    to_hex(&sha256(&engine_zip)),
                    sign_hex(&key, &engine_zip)
                );
                let module_index = format!(
                    r#"{{"modules": {{"Robust.Client.WebView": {{"versions": {{
                        "0.5.0": {{"platforms": {{"win-x64": {{"url": "{base}old.zip", "sha256": "{}", "sig": "AA"}}}}}},
                        "50.0.0": {{"platforms": {{"win-x64": {{"url": "{base}module.zip", "sha256": "{}", "sig": "{}"}}}}}},
                        "200.0.0": {{"platforms": {{"win-x64": {{"url": "{base}future.zip", "sha256": "{}", "sig": "AA"}}}}}}
                    }}}}}}}}"#,
                    "00".repeat(32),
                    to_hex(&sha256(&module_zip)),
                    sign_hex(&key, &module_zip),
                    "00".repeat(32)
                );
                move |req: &Req| match req.path.as_str() {
                    "/manifest.json" => (200, vec![], engine_index.clone().into_bytes()),
                    "/modules.json" => (200, vec![], module_index.clone().into_bytes()),
                    "/engine.zip" => (200, vec![], engine_zip.clone()),
                    "/module.zip" => (200, vec![], module_zip.clone()),
                    "/client.zip" => (200, vec![], client_zip.clone()),
                    _ => (404, vec![], vec![]),
                }
            }
        })
        .await;

        let tmp = tempfile::tempdir().unwrap();
        let build = BuildInfo {
            download_url: Some(format!("{base}client.zip")),
            hash: Some(to_hex(&sha256(&client_zip))),
            fork_id: "f".into(),
            version: "1".into(),
            engine_version: "100.0.0".into(),
            ..Default::default()
        };
        Cdn {
            sources: Sources {
                builds: UrlFallbackSet::new([base.clone()]),
                key: key.verifying_key(),
            },
            base,
            seen,
            build,
            dirs: DataDirs::at(tmp.path()),
            _tmp: tmp,
        }
    }

    async fn install_and_prepare(c: &Cdn, db: &ContentDb) -> PreparedLaunch {
        let client = build_client().unwrap();
        let content = install_content(&client, db, &c.build, &c.dirs.scratch(), None)
            .await
            .unwrap();
        prepare_launch(&client, db, &c.dirs, &c.sources, content.version_id, None)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn build_without_modules_only_needs_the_engine() {
        let c = cdn("defaultWindowTitle: Test\n").await;
        let db = ContentDb::open_in_memory().unwrap();
        let p = install_and_prepare(&c, &db).await;

        assert_eq!(p.engine.version, "100.0.0");
        assert!(p.engine.zip_path.is_file());
        assert!(p.modules.is_empty());
        assert!(!p.multi_window);
        assert_eq!(
            c.count("/modules.json"),
            0,
            "the module index is not needed"
        );
        assert_eq!(c.count("/module.zip"), 0);
    }

    #[tokio::test]
    async fn modules_from_manifest_yml_are_resolved_saved_and_installed() {
        let c = cdn("modules:\n  - Robust.Client.WebView\nmultiWindow: true\n").await;
        let db = ContentDb::open_in_memory().unwrap();
        let p = install_and_prepare(&c, &db).await;

        // Engine 100.0.0: the newest module not newer than that is 50.0.0 (not 0.5.0, not 200.0.0).
        assert_eq!(p.modules.len(), 1);
        assert_eq!(p.modules[0].name, "Robust.Client.WebView");
        assert_eq!(p.modules[0].version, "50.0.0");
        assert!(p.modules[0].dir.join("webview.exe").is_file());
        assert!(p.multi_window);
        assert_eq!(
            db.engine_dependencies(p.version_id).unwrap(),
            [
                ("Robust".to_string(), "100.0.0".to_string()),
                ("Robust.Client.WebView".to_string(), "50.0.0".to_string())
            ]
        );
        assert_eq!(c.count("/module.zip"), 1);
        assert_eq!(c.count("/old.zip") + c.count("/future.zip"), 0);
    }

    #[tokio::test]
    async fn preparing_again_downloads_nothing() {
        let c = cdn("modules:\n  - Robust.Client.WebView\n").await;
        let db = ContentDb::open_in_memory().unwrap();
        let first = install_and_prepare(&c, &db).await;
        let again = prepare_launch(
            &build_client().unwrap(),
            &db,
            &c.dirs,
            &c.sources,
            first.version_id,
            None,
        )
        .await
        .unwrap();

        assert!(!again.engine.downloaded);
        assert!(!again.modules[0].downloaded);
        assert_eq!(c.count("/engine.zip"), 1);
        assert_eq!(c.count("/module.zip"), 1);
        assert_eq!(
            db.engine_dependencies(first.version_id).unwrap().len(),
            2,
            "dependencies are not added twice"
        );
    }

    #[tokio::test]
    async fn a_deleted_module_is_fetched_again() {
        let c = cdn("modules:\n  - Robust.Client.WebView\n").await;
        let db = ContentDb::open_in_memory().unwrap();
        let first = install_and_prepare(&c, &db).await;
        std::fs::remove_dir_all(&first.modules[0].dir).unwrap();

        let again = prepare_launch(
            &build_client().unwrap(),
            &db,
            &c.dirs,
            &c.sources,
            first.version_id,
            None,
        )
        .await
        .unwrap();
        assert!(again.modules[0].downloaded);
        assert!(again.modules[0].dir.join("webview.exe").is_file());
    }

    #[tokio::test]
    async fn unknown_module_is_an_error() {
        let c = cdn("modules:\n  - Robust.Client.Nonsense\n").await;
        let db = ContentDb::open_in_memory().unwrap();
        let client = build_client().unwrap();
        let content = install_content(&client, &db, &c.build, &c.dirs.scratch(), None)
            .await
            .unwrap();
        let err = prepare_launch(&client, &db, &c.dirs, &c.sources, content.version_id, None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Nonsense"), "{err}");
        let _ = &c.base;
    }
}
