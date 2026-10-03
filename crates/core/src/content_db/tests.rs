use super::*;
use crate::hash::blake2b256;

/// Builds a manifest (sorted by path, like servers do) from in-memory files.
fn manifest_of(files: &[(&str, &[u8])]) -> Manifest {
    let mut files = files.to_vec();
    files.sort_by_key(|(p, _)| p.as_bytes());
    let mut text = String::from(manifest::HEADER);
    text.push('\n');
    for (path, data) in files {
        text.push_str(&format!("{} {}\n", to_hex(&blake2b256(data)), path));
    }
    Manifest::parse(text.as_bytes()).unwrap()
}

fn store_all(db: &ContentDb, files: &[(&str, &[u8])]) {
    for (_, data) in files {
        db.store_blob(&blake2b256(data), data).unwrap();
    }
}

fn build(
    manifest_hash: Option<&str>,
    zip_hash: Option<&str>,
    fork: &str,
    version: &str,
    engine: &str,
) -> BuildInfo {
    BuildInfo {
        manifest_hash: manifest_hash.map(String::from),
        hash: zip_hash.map(String::from),
        fork_id: fork.into(),
        version: version.into(),
        engine_version: engine.into(),
        ..Default::default()
    }
}

fn compression_of(db: &ContentDb, id: i64) -> i64 {
    db.connection()
        .query_row("SELECT Compression FROM Content WHERE Id = ?1", [id], |r| {
            r.get(0)
        })
        .unwrap()
}

#[test]
fn migrations_create_the_schema() {
    let db = ContentDb::open_in_memory().unwrap();
    let tables: Vec<String> = db
        .connection()
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap();
    assert_eq!(
        tables,
        [
            "Content",
            "ContentEngineDependency",
            "ContentManifest",
            "ContentVersion",
            "InterruptedDownload",
            "InterruptedDownloadContent",
            "RunningClient",
            "SchemaVersions",
        ]
    );

    let scripts: Vec<String> = db
        .connection()
        .prepare("SELECT ScriptName FROM SchemaVersions ORDER BY SchemaVersionID")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap();
    assert_eq!(
        scripts,
        [
            "0000_Initial",
            "0001_Blake2B",
            "0002_ContentIdIndex",
            "0003_InterruptedDownload",
            "0004_RunningClient"
        ]
    );
}

#[test]
fn reopening_a_file_keeps_data_and_uses_wal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub").join("content.db");

    let id = {
        let db = ContentDb::open(&path).unwrap();
        db.store_blob(&blake2b256(b"hello"), b"hello").unwrap().0
    };

    let db = ContentDb::open(&path).unwrap();
    assert_eq!(db.find_content_id(&blake2b256(b"hello")).unwrap(), Some(id));
    let migrations: i64 = db
        .connection()
        .query_row("SELECT COUNT(*) FROM SchemaVersions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(migrations, 5, "migrations are not applied twice");
    let mode: String = db
        .connection()
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
}

#[test]
fn compressible_blob_is_stored_as_zstd_and_roundtrips() {
    let db = ContentDb::open_in_memory().unwrap();
    let data = vec![b'a'; 10_000];
    let hash = blake2b256(&data);

    let (id, created) = db.store_blob(&hash, &data).unwrap();
    assert!(created);
    assert_eq!(compression_of(&db, id), Compression::ZStd as i64);
    let stored: i64 = db
        .connection()
        .query_row(
            "SELECT length(Data) FROM Content WHERE Id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        stored < 1000,
        "10 KB of 'a' must compress well, got {stored}"
    );
    assert_eq!(db.read_blob(id).unwrap(), data);

    let (again, created) = db.store_blob(&hash, &data).unwrap();
    assert_eq!((again, created), (id, false), "same content is stored once");
}

#[test]
fn tiny_blob_is_stored_raw() {
    let db = ContentDb::open_in_memory().unwrap();
    let (id, _) = db.store_blob(&blake2b256(b"hello"), b"hello").unwrap();
    assert_eq!(compression_of(&db, id), Compression::None as i64);
    assert_eq!(db.read_blob(id).unwrap(), b"hello");

    let (empty, _) = db.store_blob(&blake2b256(b""), b"").unwrap();
    assert_eq!(db.read_blob(empty).unwrap(), b"");
}

#[test]
fn store_blob_rejects_wrong_hash() {
    let db = ContentDb::open_in_memory().unwrap();
    let err = db.store_blob(&blake2b256(b"other"), b"hello").unwrap_err();
    assert!(matches!(err, Error::HashMismatch { .. }));
    assert_eq!(db.find_content_id(&blake2b256(b"hello")).unwrap(), None);
}

#[test]
fn precompressed_blob_roundtrips() {
    let db = ContentDb::open_in_memory().unwrap();
    let data = vec![7u8; 5000];
    let packed = zstd::bulk::compress(&data, 3).unwrap();
    let id = db
        .store_precompressed_blob(&blake2b256(&data), data.len(), &packed)
        .unwrap();
    assert_eq!(compression_of(&db, id), Compression::ZStd as i64);
    assert_eq!(db.read_blob(id).unwrap(), data);
}

#[test]
fn missing_entries_skips_known_and_duplicate_blobs() {
    let db = ContentDb::open_in_memory().unwrap();
    let files: &[(&str, &[u8])] = &[
        ("a.txt", b"same"),
        ("b.txt", b"same"),
        ("c.txt", b"known"),
        ("d.txt", b"new"),
    ];
    let m = manifest_of(files);
    db.store_blob(&blake2b256(b"known"), b"known").unwrap();

    // a.txt and b.txt share a blob: it is wanted once, at the first index.
    assert_eq!(db.missing_entries(&m).unwrap(), [0, 3]);

    store_all(&db, files);
    assert!(db.missing_entries(&m).unwrap().is_empty());
}

#[test]
fn version_manifest_roundtrip_and_hash() {
    let db = ContentDb::open_in_memory().unwrap();
    let files: &[(&str, &[u8])] = &[
        ("Resources/my file.txt", b""),
        ("zeta.bin", b"world"),
        ("Assemblies/Content.Client.dll", b"hello"),
    ];
    let m = manifest_of(files);
    store_all(&db, files);

    let v = db.create_version("fork", "1.0").unwrap();
    db.fill_manifest(v, &m).unwrap();

    let entries = db.manifest_entries(v).unwrap();
    assert_eq!(entries, m.entries, "same entries, same (sorted) order");
    assert_eq!(db.compute_manifest_hash(v).unwrap(), m.hash);

    db.set_version_hash(v, &m.hash).unwrap();
    let got = db.get_version(v).unwrap().unwrap();
    assert_eq!(got.hash, m.hash.to_vec());
    assert_eq!(got.fork_id.as_deref(), Some("fork"));
}

#[test]
fn fill_manifest_fails_when_a_blob_is_missing() {
    let db = ContentDb::open_in_memory().unwrap();
    let m = manifest_of(&[("a.txt", b"never stored")]);
    let v = db.create_version("fork", "1").unwrap();
    assert!(db.fill_manifest(v, &m).is_err());
}

#[test]
fn engine_dependencies_roundtrip() {
    let db = ContentDb::open_in_memory().unwrap();
    let v = db.create_version("fork", "1").unwrap();
    db.add_engine_dependency(v, "Robust", "277.0.0").unwrap();
    db.add_engine_dependency(v, "Robust.Client.WebView", "0.99.0")
        .unwrap();
    assert_eq!(
        db.engine_dependencies(v).unwrap(),
        [
            ("Robust".to_string(), "277.0.0".to_string()),
            ("Robust.Client.WebView".to_string(), "0.99.0".to_string())
        ]
    );
    assert!(
        db.add_engine_dependency(v, "Robust", "1.0").is_err(),
        "one version per module"
    );
}

#[test]
fn find_version_by_manifest_hash_prefers_exact_match() {
    let db = ContentDb::open_in_memory().unwrap();
    let hash = [9u8; 32];
    let hex = to_hex(&hash);

    let old = db.create_version("fork", "old").unwrap();
    db.set_version_hash(old, &hash).unwrap();
    db.add_engine_dependency(old, "Robust", "1.0.0").unwrap();

    let exact = db.create_version("fork", "new").unwrap();
    db.set_version_hash(exact, &hash).unwrap();
    db.add_engine_dependency(exact, "Robust", "2.0.0").unwrap();

    let found = db
        .find_version(&build(Some(&hex), None, "fork", "new", "2.0.0"))
        .unwrap()
        .unwrap();
    assert_eq!(found.id, exact);

    // Nothing matches fork/version/engine exactly: still found, because the manifest hash is the truth.
    let found = db
        .find_version(&build(Some(&hex), None, "other", "x", "3.0.0"))
        .unwrap()
        .unwrap();
    assert!(found.id == old || found.id == exact);

    assert!(db
        .find_version(&build(
            Some(&to_hex(&[1u8; 32])),
            None,
            "fork",
            "new",
            "2.0.0"
        ))
        .unwrap()
        .is_none());
}

#[test]
fn find_version_by_zip_hash_and_by_fork_version() {
    let db = ContentDb::open_in_memory().unwrap();
    let v = db.create_version("fork", "7").unwrap();
    db.set_zip_hash(v, &[5u8; 32]).unwrap();

    let by_zip = db
        .find_version(&build(None, Some(&to_hex(&[5u8; 32])), "x", "y", "z"))
        .unwrap()
        .unwrap();
    assert_eq!(by_zip.id, v);

    let by_fork = db
        .find_version(&build(None, None, "fork", "7", "z"))
        .unwrap()
        .unwrap();
    assert_eq!(by_fork.id, v);
    assert!(db
        .find_version(&build(None, None, "fork", "8", "z"))
        .unwrap()
        .is_none());
}

#[test]
fn find_version_rejects_malformed_hash() {
    let db = ContentDb::open_in_memory().unwrap();
    assert!(db
        .find_version(&build(Some("nothex"), None, "f", "v", "e"))
        .is_err());
}

#[test]
fn transaction_rolls_back_on_error() {
    let db = ContentDb::open_in_memory().unwrap();
    let result: Result<()> = db.transaction(|db| {
        db.create_version("fork", "1")?;
        Err(Error::Other("boom".into()))
    });
    assert!(result.is_err());
    let count: i64 = db
        .connection()
        .query_row("SELECT COUNT(*) FROM ContentVersion", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);

    db.transaction(|db| db.create_version("fork", "2").map(|_| ()))
        .unwrap();
    let count: i64 = db
        .connection()
        .query_row("SELECT COUNT(*) FROM ContentVersion", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn constraints_protect_the_data() {
    let db = ContentDb::open_in_memory().unwrap();
    let conn = db.connection();
    let v = db.create_version("fork", "1").unwrap();

    // Manifest rows need a real blob.
    assert!(conn
        .execute(
            "INSERT INTO ContentManifest(VersionId, Path, ContentId) VALUES (?1, 'a', 999)",
            [v]
        )
        .is_err());

    // No directory entries.
    let (id, _) = db.store_blob(&blake2b256(b"x"), b"x").unwrap();
    assert!(conn
        .execute(
            "INSERT INTO ContentManifest(VersionId, Path, ContentId) VALUES (?1, 'dir/', ?2)",
            params![v, id]
        )
        .is_err());

    // A blob that is in use cannot be deleted.
    conn.execute(
        "INSERT INTO ContentManifest(VersionId, Path, ContentId) VALUES (?1, 'x', ?2)",
        params![v, id],
    )
    .unwrap();
    assert!(conn
        .execute("DELETE FROM Content WHERE Id = ?1", [id])
        .is_err());

    // Deleting a version removes its manifest rows.
    conn.execute("DELETE FROM ContentVersion WHERE Id = ?1", [v])
        .unwrap();
    let left: i64 = conn
        .query_row("SELECT COUNT(*) FROM ContentManifest", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0);

    // Uncompressed size must match.
    assert!(conn
        .execute(
            "INSERT INTO Content(Hash, Size, Compression, Data) VALUES (zeroblob(32), 10, 0, x'0102')",
            []
        )
        .is_err());
}
