//! Zip-mode install: the whole build as one zip (docs/protocol.md, section 4).
//!
//! Used for servers without delta-update support. The zip is downloaded to a scratch file,
//! checked against the SHA-256 from `/info`, then every file is hashed and stored like in
//! manifest mode, so both modes end up with the same kind of version in the database.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use reqwest::Client;

use crate::content_db::{ContentDb, PreparedBlob};
use crate::download::{download_to_file, Progress, ProgressFn, ProgressUnit};
use crate::hash::{blake2b256, parse_hash_hex, to_hex};
use crate::server::BuildInfo;
use crate::updater::{DownloadMode, InstalledContent};
use crate::{Error, Result};

/// A single file inside a zip may not unpack to more than this (zip bombs).
const MAX_ENTRY_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Everything one client zip may unpack to, in total.
const MAX_TOTAL_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// One blocking batch: at most this many entries or this many prepared bytes.
const BATCH_FILES: usize = 64;
const BATCH_BYTES: u64 = 32 * 1024 * 1024;
/// Size of one read from a zip entry.
const READ_CHUNK: usize = 64 * 1024;

/// Deletes the scratch file when dropped, on every path out of the install.
struct ScratchFile(PathBuf);

impl Drop for ScratchFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// One file of the client zip, ready to be linked to a version.
pub struct IngestedFile {
    pub path: String,
    pub content_id: i64,
}

pub async fn install_zip(
    client: &Client,
    db: &ContentDb,
    build: &BuildInfo,
    scratch_dir: &Path,
    progress: Option<ProgressFn<'_>>,
) -> Result<InstalledContent> {
    let url = build
        .download_url()
        .ok_or_else(|| Error::Other("zip mode needs a download url".into()))?;

    std::fs::create_dir_all(scratch_dir)?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let scratch =
        ScratchFile(scratch_dir.join(format!("client-{}-{nanos}.zip.part", std::process::id())));

    let zip_hash = download_to_file(client, url, &scratch.0, progress).await?;
    if let Some(expected) = build.hash() {
        let expected = parse_hash_hex(expected)?;
        if expected != zip_hash {
            return Err(Error::HashMismatch {
                expected: to_hex(&expected),
                actual: to_hex(&zip_hash),
            });
        }
    }

    // Blobs are committed in batches while the zip is read (see `ingest_zip`); only the version row
    // and its file list go in at the end, in one short transaction.
    let files = ingest_zip(db, &scratch.0, progress).await?;
    let file_count = files.len();

    let version_id = db.transaction(|db| {
        let id = db.create_version(&build.fork_id, &build.version)?;
        db.set_zip_hash(id, &zip_hash)?;
        for file in &files {
            db.add_manifest_entry(id, &file.path, file.content_id)?;
        }
        let manifest_hash = db.compute_manifest_hash(id)?;
        db.set_version_hash(id, &manifest_hash)?;
        db.add_engine_dependency(id, "Robust", &build.engine_version)?;
        Ok(id)
    })?;

    Ok(InstalledContent {
        version_id,
        mode: DownloadMode::Zip,
        files: file_count,
        downloaded: file_count,
        reused: false,
    })
}

/// Rejects a path that could escape the virtual file system or upset the Loader. The same rules the
/// Loader's manifest expects: relative, `/`-separated, no drive letters, no NUL or control chars.
fn check_entry_path(name: &str) -> Result<()> {
    let bad = name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains(':')
        || name.contains('\0')
        || name.chars().any(char::is_control);
    let bad_component = name
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..");
    if bad || bad_component {
        return Err(Error::Other(format!(
            "client zip entry {name:?} is not a safe relative path"
        )));
    }
    Ok(())
}

/// Stores every file of the zip in the database and returns them. Directory entries are skipped.
///
/// The zip is read, hashed and compressed on blocking threads; the async side only inserts the
/// finished blobs (short transactions), reports progress and yields between batches.
pub async fn ingest_zip(
    db: &ContentDb,
    zip_path: &Path,
    progress: Option<ProgressFn<'_>>,
) -> Result<Vec<IngestedFile>> {
    let archive = std::fs::File::open(zip_path)?;
    let archive =
        zip::ZipArchive::new(archive).map_err(|e| Error::Other(format!("bad client zip: {e}")))?;
    let total = archive.len();
    let archive = Arc::new(Mutex::new(archive));

    let mut seen: HashSet<String> = HashSet::new();
    let mut files: Vec<IngestedFile> = Vec::new();
    let mut batch: Vec<(String, PreparedBlob)> = Vec::new();
    let mut batch_bytes = 0u64;
    let mut unpacked = 0u64;
    let mut next = 0usize;

    while next < total {
        let share = Arc::clone(&archive);
        let remaining_budget = MAX_TOTAL_BYTES.saturating_sub(unpacked);
        let (taken, bytes, items) =
            tokio::task::spawn_blocking(move || read_batch(&share, next, remaining_budget))
                .await
                .map_err(|e| Error::Other(format!("unpack task failed: {e}")))??;
        next = taken;
        unpacked += bytes;

        for (path, blob) in items {
            claim_path(&mut seen, &path)?;
            batch_bytes += blob.size() as u64;
            batch.push((path, blob));

            if let Some(p) = progress {
                p(Progress {
                    done: files.len() as u64 + batch.len() as u64,
                    total: total as u64,
                    unit: ProgressUnit::Items,
                });
            }
        }

        if batch_bytes >= BATCH_BYTES || batch.len() >= BATCH_FILES || next >= total {
            flush_batch(db, &mut batch, &mut files)?;
            batch_bytes = 0;
        }

        tokio::task::yield_now().await;
    }

    Ok(files)
}

/// Claims a path for this version. A second file with the same path would fail on the manifest
/// unique index with a raw SQLite error in the middle of an install, so it is refused here.
fn claim_path(seen: &mut HashSet<String>, path: &str) -> Result<()> {
    if !seen.insert(path.to_string()) {
        return Err(Error::Other(format!(
            "client zip contains the path {path:?} twice"
        )));
    }
    Ok(())
}

/// Inserts the prepared blobs of one batch in a single short transaction.
fn flush_batch(
    db: &ContentDb,
    batch: &mut Vec<(String, PreparedBlob)>,
    files: &mut Vec<IngestedFile>,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    db.transaction(|db| {
        for (path, blob) in batch.iter() {
            let (content_id, _) = db.store_prepared_blob(blob)?;
            files.push(IngestedFile {
                path: path.clone(),
                content_id,
            });
        }
        Ok(())
    })?;
    batch.clear();
    Ok(())
}

/// One blocking batch: the entries read, the bytes they used and the index to continue from.
type ZipBatch = (usize, u64, Vec<(String, PreparedBlob)>);

/// Reads the next batch of non-directory entries. Runs on a blocking thread.
///
/// Returns `(next entry index, bytes read, files)`. Sizes are counted while data really arrives, so
/// a lying header cannot make us allocate or store more than the limits allow.
fn read_batch(
    archive: &Mutex<zip::ZipArchive<std::fs::File>>,
    start: usize,
    budget: u64,
) -> Result<ZipBatch> {
    let mut items = Vec::new();
    let mut bytes = 0u64;
    let mut bytes_in_batch = 0u64;
    let mut i = start;

    {
        let mut guard = archive
            .lock()
            .map_err(|_| Error::Other("client zip is poisoned".into()))?;
        let total = guard.len();

        while i < total && items.len() < BATCH_FILES && bytes_in_batch < BATCH_BYTES {
            let index = i;
            i += 1;

            let mut entry = guard
                .by_index(index)
                .map_err(|e| Error::Other(format!("bad client zip entry {index}: {e}")))?;
            let name = entry.name().to_string();
            if name.is_empty() || name.ends_with('/') {
                continue;
            }
            check_entry_path(&name)?;

            let data = read_entry(
                &name,
                &mut entry,
                MAX_ENTRY_BYTES,
                budget.saturating_sub(bytes),
            )?;
            bytes += data.len() as u64;
            bytes_in_batch += data.len() as u64;

            let hash = blake2b256(&data);
            let blob = PreparedBlob::from_hashed(hash, data)?;
            items.push((name, blob));
        }
    }

    Ok((i, bytes, items))
}

/// Reads one entry in chunks, checking the per-file and total limits as the data arrives.
fn read_entry<R: Read>(name: &str, entry: &mut R, max_entry: u64, budget: u64) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    let mut chunk = [0u8; READ_CHUNK];
    loop {
        let got = entry.read(&mut chunk)?;
        if got == 0 {
            break;
        }
        let grown = data.len() as u64 + got as u64;
        if grown > max_entry {
            return Err(Error::Other(format!(
                "{name}: unpacks to an unreasonable size"
            )));
        }
        if grown > budget {
            return Err(Error::Other(
                "client zip unpacks to an unreasonable size".into(),
            ));
        }
        data.extend_from_slice(&chunk[..got]);
    }
    Ok(data)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;

    use zip::write::SimpleFileOptions;

    use super::*;
    use crate::hash::{sha256, to_hex};
    use crate::http::build_client;
    use crate::manifest;
    use crate::test_util::serve;

    pub(crate) fn make_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            for (name, data) in files {
                if name.ends_with('/') {
                    w.add_directory(*name, opts).unwrap();
                } else {
                    w.start_file(*name, opts).unwrap();
                    w.write_all(data).unwrap();
                }
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    fn files() -> Vec<(&'static str, Vec<u8>)> {
        vec![
            ("Assemblies/", vec![]),
            ("Assemblies/a.dll", b"alpha".to_vec()),
            ("Resources/big.bin", vec![b'z'; 20_000]),
            ("Resources/copy.bin", vec![b'z'; 20_000]),
            ("manifest.yml", b"x: 1\n".to_vec()),
        ]
    }

    fn zip_bytes() -> Vec<u8> {
        let f = files();
        let refs: Vec<(&str, &[u8])> = f.iter().map(|(n, d)| (*n, d.as_slice())).collect();
        make_zip(&refs)
    }

    fn build_for(base: &str, zip: &[u8]) -> BuildInfo {
        BuildInfo {
            download_url: Some(format!("{base}client.zip")),
            hash: Some(to_hex(&sha256(zip))),
            fork_id: "zipfork".into(),
            version: "z1".into(),
            engine_version: "100.0.0".into(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn installs_a_zip_and_dedupes_identical_files() {
        let zip = zip_bytes();
        let body = zip.clone();
        let (base, _) = serve(move |_| (200, vec![], body.clone())).await;
        let db = ContentDb::open_in_memory().unwrap();
        let scratch = tempfile::tempdir().unwrap();

        let got = install_zip(
            &build_client().unwrap(),
            &db,
            &build_for(&base, &zip),
            scratch.path(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(got.mode, DownloadMode::Zip);
        assert_eq!(got.files, 4, "directory entries are not files");

        let entries = db.manifest_entries(got.version_id).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "Assemblies/a.dll",
                "Resources/big.bin",
                "Resources/copy.bin",
                "manifest.yml"
            ]
        );
        let blobs: i64 = db
            .connection()
            .query_row("SELECT COUNT(*) FROM Content", [], |r| r.get(0))
            .unwrap();
        assert_eq!(blobs, 3, "two identical files share one blob");

        // Stored exactly: contents, version hash and zip hash.
        for (e, (_, data)) in entries
            .iter()
            .zip(files().iter().filter(|(n, _)| !n.ends_with('/')))
        {
            let id = db.find_content_id(&e.hash).unwrap().unwrap();
            assert_eq!(&db.read_blob(id).unwrap(), data);
        }
        let v = db.get_version(got.version_id).unwrap().unwrap();
        assert_eq!(v.zip_hash.unwrap(), sha256(&zip).to_vec());
        assert_eq!(
            v.hash,
            manifest::compute_hash(entries.iter().map(|e| (&e.hash, e.path.as_str()))).to_vec()
        );
        assert_eq!(
            db.engine_dependencies(got.version_id).unwrap(),
            [("Robust".to_string(), "100.0.0".to_string())]
        );

        // The scratch file is gone.
        assert_eq!(std::fs::read_dir(scratch.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn wrong_zip_hash_stores_nothing_and_cleans_up() {
        let zip = zip_bytes();
        let body = zip.clone();
        let (base, _) = serve(move |_| (200, vec![], body.clone())).await;
        let db = ContentDb::open_in_memory().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let mut build = build_for(&base, &zip);
        build.hash = Some("00".repeat(32));

        let err = install_zip(&build_client().unwrap(), &db, &build, scratch.path(), None)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::HashMismatch { .. }), "{err}");
        let versions: i64 = db
            .connection()
            .query_row("SELECT COUNT(*) FROM ContentVersion", [], |r| r.get(0))
            .unwrap();
        assert_eq!(versions, 0);
        assert_eq!(std::fs::read_dir(scratch.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn broken_zip_rolls_everything_back() {
        let junk = b"this is not a zip file".to_vec();
        let body = junk.clone();
        let (base, _) = serve(move |_| (200, vec![], body.clone())).await;
        let db = ContentDb::open_in_memory().unwrap();
        let scratch = tempfile::tempdir().unwrap();

        // No hash given by the server: the broken file is only noticed when unpacking.
        let build = BuildInfo {
            download_url: Some(format!("{base}client.zip")),
            fork_id: "f".into(),
            version: "v".into(),
            engine_version: "1".into(),
            ..Default::default()
        };
        assert!(
            install_zip(&build_client().unwrap(), &db, &build, scratch.path(), None)
                .await
                .is_err()
        );
        let versions: i64 = db
            .connection()
            .query_row("SELECT COUNT(*) FROM ContentVersion", [], |r| r.get(0))
            .unwrap();
        assert_eq!(versions, 0, "no half-built version");
        assert_eq!(std::fs::read_dir(scratch.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn http_errors_are_reported() {
        let (base, _) = serve(|_| (404, vec![], vec![])).await;
        let db = ContentDb::open_in_memory().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let build = build_for(&base, b"x");
        let err = install_zip(&build_client().unwrap(), &db, &build, scratch.path(), None)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::HttpStatus { status: 404, .. }));
    }

    // ---- P1-8: path checks, duplicates, unpack budget ----

    #[test]
    fn entry_paths_must_be_plain_relative_paths() {
        for good in ["a.txt", "Resources/my file.txt", "Assemblies/a.dll"] {
            check_entry_path(good).unwrap();
        }
        for bad in [
            "",
            "../evil.txt",
            "a/../../evil.txt",
            "a//b",
            "./a",
            "/abs.txt",
            "C:evil.txt",
            "a\\b.txt",
            "nul\0.txt",
            "bell\u{7}.txt",
        ] {
            assert!(check_entry_path(bad).is_err(), "{bad:?} must be refused");
        }
    }

    async fn install_from_zip(files: &[(&str, &[u8])]) -> Result<InstalledContent> {
        let zip = make_zip(files);
        let body = zip.clone();
        let (base, _) = serve(move |_| (200, vec![], body.clone())).await;
        let db = ContentDb::open_in_memory().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let got = install_zip(
            &build_client().unwrap(),
            &db,
            &build_for(&base, &zip),
            scratch.path(),
            None,
        )
        .await;
        if got.is_err() {
            let versions: i64 = db
                .connection()
                .query_row("SELECT COUNT(*) FROM ContentVersion", [], |r| r.get(0))
                .unwrap();
            assert_eq!(versions, 0, "a refused zip leaves no version");
        }
        got
    }

    #[tokio::test]
    async fn unsafe_paths_are_refused_by_name() {
        for bad in ["../evil.txt", "a\\b.txt", "/abs.txt", "C:evil.txt"] {
            let err = install_from_zip(&[("fine.txt", b"ok"), (bad, b"gotcha")])
                .await
                .unwrap_err();
            assert!(
                err.to_string().contains("safe relative path"),
                "{bad:?}: {err}"
            );
        }
    }

    #[tokio::test]
    async fn duplicate_paths_are_refused_with_a_readable_error() {
        // The zip reader already collapses duplicate names, so the guard is checked directly.
        let mut seen = HashSet::new();
        claim_path(&mut seen, "a/b.txt").unwrap();
        let err = claim_path(&mut seen, "a/b.txt").unwrap_err();
        // Before, this surfaced as a raw "UNIQUE constraint failed" from SQLite.
        assert!(err.to_string().contains("twice"), "{err}");
    }

    #[tokio::test]
    async fn the_unpacked_size_is_capped_per_file_and_in_total() {
        let mut file = std::io::Cursor::new(vec![b'x'; 100]);
        let err = read_entry("a.bin", &mut file, 10, 1000).unwrap_err();
        assert!(err.to_string().contains("unreasonable size"), "{err}");

        let mut file = std::io::Cursor::new(vec![b'x'; 100]);
        let err = read_entry("a.bin", &mut file, 1000, 10).unwrap_err();
        assert!(err.to_string().contains("unreasonable size"), "{err}");

        // A whole zip is stopped once the shared budget is used up.
        let mut file = std::io::Cursor::new(vec![b'x'; 100]);
        assert!(read_entry("a.bin", &mut file, 1000, 100).is_ok());
    }

    // ---- P1-5/P1-11: batching and progress ----

    #[tokio::test]
    async fn files_before_a_bad_entry_are_kept() {
        // More than one batch, so the good files are committed before the failure.
        let mut owned: Vec<(String, Vec<u8>)> = (0..BATCH_FILES + 6)
            .map(|i| (format!("f{i}.txt"), format!("file {i}").into_bytes()))
            .collect();
        owned.push(("../evil.txt".into(), b"gotcha".to_vec()));
        let refs: Vec<(&str, &[u8])> = owned
            .iter()
            .map(|(n, d)| (n.as_str(), d.as_slice()))
            .collect();
        let zip = make_zip(&refs);

        let body = zip.clone();
        let (base, _) = serve(move |_| (200, vec![], body.clone())).await;
        let db = ContentDb::open_in_memory().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let err = install_zip(
            &build_client().unwrap(),
            &db,
            &build_for(&base, &zip),
            scratch.path(),
            None,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("safe relative path"), "{err}");

        let stored: i64 = db
            .connection()
            .query_row("SELECT COUNT(*) FROM Content", [], |r| r.get(0))
            .unwrap();
        assert!(
            stored >= BATCH_FILES as i64,
            "verified blobs of the finished batches stay, got {stored}"
        );
        let versions: i64 = db
            .connection()
            .query_row("SELECT COUNT(*) FROM ContentVersion", [], |r| r.get(0))
            .unwrap();
        assert_eq!(versions, 0, "no half-built version");
    }

    #[tokio::test]
    async fn the_zip_install_reports_item_progress() {
        let zip = zip_bytes();
        let body = zip.clone();
        let (base, _) = serve(move |_| (200, vec![], body.clone())).await;
        let db = ContentDb::open_in_memory().unwrap();
        let scratch = tempfile::tempdir().unwrap();

        let seen = std::sync::Mutex::new(Vec::new());
        let report = |p: Progress| seen.lock().unwrap().push(p);
        let got = install_zip(
            &build_client().unwrap(),
            &db,
            &build_for(&base, &zip),
            scratch.path(),
            Some(&report),
        )
        .await
        .unwrap();

        let seen = seen.into_inner().unwrap();
        let items: Vec<&Progress> = seen
            .iter()
            .filter(|p| p.unit == ProgressUnit::Items)
            .collect();
        assert_eq!(
            items.len(),
            got.files,
            "one item event per stored file, not one event for the whole install"
        );
        assert_eq!(items.last().unwrap().done, got.files as u64);
    }
}
