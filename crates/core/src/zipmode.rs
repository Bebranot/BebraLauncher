//! Zip-mode install: the whole build as one zip (docs/protocol.md, section 4).
//!
//! Used for servers without delta-update support. The zip is downloaded to a scratch file,
//! checked against the SHA-256 from `/info`, then every file is hashed and stored like in
//! manifest mode, so both modes end up with the same kind of version in the database.

use std::io::Read;
use std::path::{Path, PathBuf};

use reqwest::Client;

use crate::content_db::ContentDb;
use crate::download::{download_to_file, ProgressFn};
use crate::hash::{blake2b256, parse_hash_hex, to_hex};
use crate::server::BuildInfo;
use crate::updater::{DownloadMode, InstalledContent};
use crate::{Error, Result};

/// A single file inside a zip may not unpack to more than this (zip bombs).
const MAX_ENTRY_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Deletes the scratch file when dropped, on every path out of the install.
struct ScratchFile(PathBuf);

impl Drop for ScratchFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
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

    // One transaction: either the whole version exists afterwards or nothing does.
    let (version_id, files) = db.transaction(|db| {
        let id = db.create_version(&build.fork_id, &build.version)?;
        db.set_zip_hash(id, &zip_hash)?;
        let files = ingest_zip(db, id, &scratch.0)?;
        let manifest_hash = db.compute_manifest_hash(id)?;
        db.set_version_hash(id, &manifest_hash)?;
        db.add_engine_dependency(id, "Robust", &build.engine_version)?;
        Ok((id, files))
    })?;

    Ok(InstalledContent {
        version_id,
        mode: DownloadMode::Zip,
        files,
        downloaded: files,
        reused: false,
    })
}

/// Stores every file of the zip and links it to `version_id`. Directory entries are skipped.
/// Returns the number of files. Blocking: call from a thread that may block.
pub fn ingest_zip(db: &ContentDb, version_id: i64, zip_path: &Path) -> Result<usize> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| Error::Other(format!("bad client zip: {e}")))?;

    let mut count = 0;
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| Error::Other(format!("bad client zip entry {i}: {e}")))?;
        let name = entry.name().to_string();
        if name.is_empty() || name.ends_with('/') {
            continue;
        }

        // Don't trust the size in the header for the allocation.
        let mut data = Vec::with_capacity(entry.size().min(16 * 1024 * 1024) as usize);
        let read = entry.take(MAX_ENTRY_BYTES + 1).read_to_end(&mut data)?;
        if read as u64 > MAX_ENTRY_BYTES {
            return Err(Error::Other(format!(
                "{name}: unpacks to an unreasonable size"
            )));
        }

        let hash = blake2b256(&data);
        let (content_id, _) = db.store_blob(&hash, &data)?;
        db.add_manifest_entry(version_id, &name, content_id)?;
        count += 1;
    }
    Ok(count)
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
}
