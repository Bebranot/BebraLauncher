//! The content database: downloaded game files, deduplicated (docs/protocol.md, section 5).
//!
//! The schema is the one the official launcher uses, because `SS14.Loader` reads this file directly.
//! Do not change tables or their meaning without checking the Loader.

pub mod cull;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::hash::{blake2b256, parse_hash_hex, to_hex, Hash256};
use crate::manifest::{self, Manifest, ManifestEntry};
use crate::server::BuildInfo;
use crate::{Error, Result};

/// Applied in order; names match the official `SchemaVersions` table.
const MIGRATIONS: &[(&str, &str)] = &[
    ("0000_Initial", include_str!("migrations/0000_Initial.sql")),
    ("0001_Blake2B", include_str!("migrations/0001_Blake2B.sql")),
    (
        "0002_ContentIdIndex",
        include_str!("migrations/0002_ContentIdIndex.sql"),
    ),
    (
        "0003_InterruptedDownload",
        include_str!("migrations/0003_InterruptedDownload.sql"),
    ),
    (
        "0004_RunningClient",
        include_str!("migrations/0004_RunningClient.sql"),
    ),
];

/// A compressed blob must save more than this many bytes to be stored compressed.
const COMPRESSION_SAVINGS_THRESHOLD: usize = 10;
const ZSTD_LEVEL: i32 = 3;

/// How many hashes go into one `WHERE Hash IN (...)` query. Keeps the number of SQL variables
/// well under SQLite's limit and the memory needed for a 700k-blob database low.
const HASH_BATCH: usize = 500;

/// `PRAGMA user_version` of a database created by this launcher. A database without it that
/// already holds data was made by something else and must not be wiped by a destructive migration.
const OUR_USER_VERSION: i64 = 1;

/// How `Content.Data` is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None = 0,
    Deflate = 1,
    ZStd = 2,
}

impl Compression {
    fn from_db(v: i64) -> Result<Self> {
        match v {
            0 => Ok(Self::None),
            1 => Ok(Self::Deflate),
            2 => Ok(Self::ZStd),
            other => Err(Error::Other(format!(
                "unknown compression scheme {other} in content db"
            ))),
        }
    }
}

/// A blob that is ready for [`ContentDb::store_prepared_blob`]: hash known, compression decided.
///
/// Building one does the CPU work (hashing, zstd) outside any transaction, so a slow network read
/// never holds the write lock and the work can be moved to a blocking thread.
pub struct PreparedBlob {
    hash: Hash256,
    size: usize,
    compression: Compression,
    data: Vec<u8>,
}

impl PreparedBlob {
    /// Hashes `data` and rejects it when the hash is not `hash`.
    pub fn verify(hash: &Hash256, data: Vec<u8>) -> Result<Self> {
        let actual = blake2b256(&data);
        if &actual != hash {
            return Err(Error::HashMismatch {
                expected: to_hex(hash),
                actual: to_hex(&actual),
            });
        }
        Self::from_hashed(*hash, data)
    }

    /// Like [`PreparedBlob::verify`], for callers that already computed `hash` over exactly `data`.
    pub fn from_hashed(hash: Hash256, data: Vec<u8>) -> Result<Self> {
        let size = data.len();
        if !data.is_empty() {
            let compressed = zstd::bulk::compress(&data, ZSTD_LEVEL)?;
            if compressed.len() + COMPRESSION_SAVINGS_THRESHOLD < data.len() {
                return Ok(Self {
                    hash,
                    size,
                    compression: Compression::ZStd,
                    data: compressed,
                });
            }
        }
        Ok(Self {
            hash,
            size,
            compression: Compression::None,
            data,
        })
    }

    /// Bytes as delivered by the server; the caller has checked the hash and the size.
    pub fn precompressed(hash: Hash256, size: usize, zstd_data: Vec<u8>) -> Self {
        Self {
            hash,
            size,
            compression: Compression::ZStd,
            data: zstd_data,
        }
    }

    /// Uncompressed size.
    pub fn size(&self) -> usize {
        self.size
    }
}

#[derive(Debug, Clone)]
pub struct ContentVersion {
    pub id: i64,
    pub hash: Vec<u8>,
    pub fork_id: Option<String>,
    pub fork_version: Option<String>,
    pub last_used: String,
    pub zip_hash: Option<Vec<u8>>,
}

pub struct ContentDb {
    conn: Connection,
}

impl ContentDb {
    /// Opens (creating if needed) the database in WAL mode and applies migrations.
    /// WAL lets us keep downloading while a game that reads the same file is running.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "cache_size", -64000)?;
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        conn.pragma_update(None, "mmap_size", 268435456i64)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let mut db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS SchemaVersions(
                SchemaVersionID INTEGER PRIMARY KEY,
                ScriptName TEXT NOT NULL,
                Applied DATETIME NOT NULL
            );",
        )?;

        let applied: HashSet<String> = tx
            .prepare("SELECT ScriptName FROM SchemaVersions")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;

        for (name, sql) in MIGRATIONS {
            if applied.contains(*name) {
                continue;
            }
            if *name == "0001_Blake2B" && !Self::can_wipe(&tx)? {
                return Err(Error::Other(
                    "content.db was not created by this launcher and cannot be upgraded without \
                     losing its data: close the launcher, move that file away and start again \
                     (the game content is downloaded anew)"
                        .into(),
                ));
            }
            tx.execute_batch(sql)?;
            tx.execute(
                "INSERT INTO SchemaVersions(ScriptName, Applied) VALUES (?1, datetime('now'))",
                [name],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// May a destructive migration run? Only on a database we created (marked with
    /// [`OUR_USER_VERSION`]) or on an empty one: a non-empty database without our mark was
    /// restored from somewhere or belongs to another launcher, and wiping it would lose data.
    fn can_wipe(tx: &rusqlite::Transaction<'_>) -> Result<bool> {
        let user_version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if user_version >= OUR_USER_VERSION {
            return Ok(true);
        }
        let versions: i64 =
            tx.query_row("SELECT COUNT(*) FROM ContentVersion", [], |r| r.get(0))?;
        let blobs: i64 = tx.query_row("SELECT COUNT(*) FROM Content", [], |r| r.get(0))?;
        Ok(versions == 0 && blobs == 0)
    }

    /// Runs `f` in one transaction: commit on `Ok`, roll back on `Err` or on a panic. Must not be
    /// nested.
    pub fn transaction<T>(&self, f: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        self.begin()?;
        // Without RAII a panic inside `f` would leave `BEGIN IMMEDIATE` open on this connection,
        // poisoning every later write in the process.
        let guard = TxGuard {
            conn: &self.conn,
            finished: false,
        };
        let value = f(self)?;
        self.commit()?;
        guard.finish();
        Ok(value)
    }

    /// Flushes the WAL into the database file without blocking readers. Best effort: a running
    /// game or another launcher may hold the file, and then SQLite reports "busy" instead.
    pub fn checkpoint_passive(&self) -> Result<()> {
        self.conn
            .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |_| Ok(()))?;
        Ok(())
    }

    /// Manual transaction control. Never keep one open across a network await: collect the blobs
    /// first and insert them in a short transaction (see [`crate::content_db::PreparedBlob`]).
    /// Never nest.
    pub fn begin(&self) -> Result<()> {
        Ok(self.conn.execute_batch("BEGIN IMMEDIATE")?)
    }

    pub fn commit(&self) -> Result<()> {
        Ok(self.conn.execute_batch("COMMIT")?)
    }

    pub fn rollback(&self) -> Result<()> {
        Ok(self.conn.execute_batch("ROLLBACK")?)
    }

    // ---- interrupted downloads ----
    //
    // Blobs are committed while a download runs. Until the version that uses them exists,
    // they are tied to an InterruptedDownload row so that cleanup keeps them and a retry can
    // continue from them. Finishing the download removes the row.

    pub fn begin_interrupted_download(&self) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO InterruptedDownload(Added) VALUES (datetime('now'))",
            [],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn link_interrupted_blob(&self, download_id: i64, content_id: i64) -> Result<()> {
        // A cleanup in another process may have removed the parent row while we were downloading.
        // That is not an error: the blob is stored and verified, the next attempt just may not
        // find it protected and downloads it again.
        self.conn.execute(
            "INSERT OR IGNORE INTO InterruptedDownloadContent(InterruptedDownloadId, ContentId)
             SELECT ?1, ?2 WHERE EXISTS (SELECT 1 FROM InterruptedDownload WHERE Id = ?1)",
            [download_id, content_id],
        )?;
        Ok(())
    }

    /// Marks the download as still active. Cleanup only drops rows whose heartbeat is old enough,
    /// so a second launcher downloading at the same time keeps its blobs. Returns whether the row
    /// still exists (a cleanup may have removed it meanwhile).
    pub fn touch_interrupted_download(&self, download_id: i64) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE InterruptedDownload SET Added = datetime('now') WHERE Id = ?1",
            [download_id],
        )?;
        Ok(changed > 0)
    }

    pub fn finish_download(&self, download_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM InterruptedDownload WHERE Id = ?1",
            [download_id],
        )?;
        Ok(())
    }

    // ---- blobs ----

    pub fn find_content_id(&self, hash: &Hash256) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT Id FROM Content WHERE Hash = ?1",
                [hash.as_slice()],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// The id and stored size of a blob, if we have it.
    fn find_content(&self, hash: &Hash256) -> Result<Option<(i64, i64)>> {
        Ok(self
            .conn
            .query_row(
                "SELECT Id, Size FROM Content WHERE Hash = ?1",
                [hash.as_slice()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    /// Indices of manifest entries whose blob we don't have yet. A blob that appears under several
    /// paths is listed once (at its first index).
    ///
    /// Hashes are looked up in batches (see [`HASH_BATCH`]) instead of loading the whole, possibly
    /// enormous `Content` table into memory.
    pub fn missing_entries(&self, manifest: &Manifest) -> Result<Vec<usize>> {
        // Distinct hashes in the order they first appear.
        let mut wanted: Vec<Hash256> = Vec::new();
        let mut seen: HashSet<Hash256> = HashSet::with_capacity(manifest.entries.len());
        for entry in &manifest.entries {
            if seen.insert(entry.hash) {
                wanted.push(entry.hash);
            }
        }

        let existing = self.existing_hashes(&wanted)?;

        let mut missing = Vec::new();
        let mut queued: HashSet<Hash256> = HashSet::new();
        for (i, entry) in manifest.entries.iter().enumerate() {
            if queued.contains(&entry.hash) {
                continue;
            }
            if !existing.contains(&entry.hash) {
                queued.insert(entry.hash);
                missing.push(i);
            }
        }
        Ok(missing)
    }

    /// Which of `hashes` are in the database, asked in batches of [`HASH_BATCH`].
    fn existing_hashes(&self, hashes: &[Hash256]) -> Result<HashSet<Hash256>> {
        let mut found = HashSet::with_capacity(hashes.len().min(4096));
        for chunk in hashes.chunks(HASH_BATCH) {
            let placeholders = std::iter::repeat_n("?", chunk.len())
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!("SELECT Hash FROM Content WHERE Hash IN ({placeholders})");
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map(
                rusqlite::params_from_iter(chunk.iter().map(|h| h.as_slice())),
                |r| r.get::<_, Vec<u8>>(0),
            )?;
            for row in rows {
                if let Ok(hash) = row?.try_into() {
                    found.insert(hash);
                }
            }
        }
        Ok(found)
    }

    /// Stores a file after checking that it really has `hash`. Compresses with zstd when that saves
    /// enough space. Returns the content id and whether a new row was created.
    pub fn store_blob(&self, hash: &Hash256, data: &[u8]) -> Result<(i64, bool)> {
        let prepared = PreparedBlob::verify(hash, data.to_vec())?;
        self.store_prepared_blob(&prepared)
    }

    /// Inserts an already verified and compressed blob. Short and CPU-free, so it is safe to call
    /// inside a short write transaction.
    pub fn store_prepared_blob(&self, blob: &PreparedBlob) -> Result<(i64, bool)> {
        if let Some((id, size)) = self.find_content(&blob.hash)? {
            // The hash covers the bytes, so a row with another size is corrupt rather than stale.
            if size != blob.size as i64 {
                return Err(Error::Other(format!(
                    "content db row {id} for this hash has size {size}, expected {}",
                    blob.size
                )));
            }
            return Ok((id, false));
        }
        let id = self.insert_content(&blob.hash, blob.size, blob.compression, &blob.data)?;
        Ok((id, true))
    }

    /// Stores a zstd-compressed file as delivered by the server.
    ///
    /// **Unchecked:** `hash` and `size` are taken on trust — nothing is decompressed or hashed
    /// here, because the caller has already done both (see [`crate::download`]). Use
    /// [`ContentDb::store_blob`] or [`ContentDb::store_prepared_blob`] when that is not the case.
    pub fn store_precompressed_blob_unchecked(
        &self,
        hash: &Hash256,
        size: usize,
        zstd_data: &[u8],
    ) -> Result<i64> {
        let blob = PreparedBlob::precompressed(*hash, size, zstd_data.to_vec());
        Ok(self.store_prepared_blob(&blob)?.0)
    }

    fn insert_content(
        &self,
        hash: &Hash256,
        size: usize,
        compression: Compression,
        data: &[u8],
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO Content(Hash, Size, Compression, Data) VALUES (?1, ?2, ?3, ?4)",
            params![hash.as_slice(), size as i64, compression as i64, data],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// The uncompressed bytes of a blob.
    pub fn read_blob(&self, content_id: i64) -> Result<Vec<u8>> {
        let (size, compression, data): (i64, i64, Vec<u8>) = self.conn.query_row(
            "SELECT Size, Compression, Data FROM Content WHERE Id = ?1",
            [content_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        match Compression::from_db(compression)? {
            Compression::None => Ok(data),
            Compression::ZStd => {
                let out = zstd::bulk::decompress(&data, size as usize)?;
                if out.len() as i64 != size {
                    return Err(Error::Other(format!(
                        "blob {content_id} decompressed to the wrong size"
                    )));
                }
                Ok(out)
            }
            Compression::Deflate => Err(Error::Other(
                "deflate-compressed blobs are not supported".into(),
            )),
        }
    }

    // ---- versions ----

    /// New version row with a placeholder hash; set the real one with [`ContentDb::set_version_hash`].
    pub fn create_version(&self, fork_id: &str, fork_version: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO ContentVersion(Hash, ForkId, ForkVersion, LastUsed, ZipHash)
             VALUES (zeroblob(32), ?1, ?2, datetime('now'), NULL)",
            params![fork_id, fork_version],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn set_version_hash(&self, version_id: i64, hash: &Hash256) -> Result<()> {
        self.conn.execute(
            "UPDATE ContentVersion SET Hash = ?1 WHERE Id = ?2",
            params![hash.as_slice(), version_id],
        )?;
        Ok(())
    }

    pub fn set_zip_hash(&self, version_id: i64, hash: &Hash256) -> Result<()> {
        self.conn.execute(
            "UPDATE ContentVersion SET ZipHash = ?1 WHERE Id = ?2",
            params![hash.as_slice(), version_id],
        )?;
        Ok(())
    }

    pub fn touch_version(&self, version_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE ContentVersion SET LastUsed = datetime('now') WHERE Id = ?1",
            [version_id],
        )?;
        Ok(())
    }

    /// Every stored version, newest first.
    pub fn list_versions(&self) -> Result<Vec<ContentVersion>> {
        let mut stmt = self.conn.prepare(
            "SELECT Id, Hash, ForkId, ForkVersion, LastUsed, ZipHash FROM ContentVersion ORDER BY Id DESC",
        )?;
        let rows = stmt.query_map([], row_to_version)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn get_version(&self, version_id: i64) -> Result<Option<ContentVersion>> {
        Ok(self
            .conn
            .query_row(
                "SELECT Id, Hash, ForkId, ForkVersion, LastUsed, ZipHash FROM ContentVersion WHERE Id = ?1",
                [version_id],
                row_to_version,
            )
            .optional()?)
    }

    /// Links every file of the manifest to the version. All blobs must already be stored.
    ///
    /// Only the hashes this manifest needs are looked up, in batches, so a large `Content` table
    /// never has to be read whole.
    pub fn fill_manifest(&self, version_id: i64, manifest: &Manifest) -> Result<()> {
        let mut wanted: Vec<Hash256> = Vec::new();
        let mut seen: HashSet<Hash256> = HashSet::with_capacity(manifest.entries.len());
        for entry in &manifest.entries {
            if seen.insert(entry.hash) {
                wanted.push(entry.hash);
            }
        }
        let hash_to_id = self.ids_for_hashes(&wanted)?;

        let mut insert = self.conn.prepare(
            "INSERT INTO ContentManifest(VersionId, Path, ContentId) VALUES (?1, ?2, ?3)",
        )?;
        for entry in &manifest.entries {
            let content_id = hash_to_id
                .get(&entry.hash)
                .copied()
                .ok_or_else(|| Error::Other(format!("missing blob for {}", entry.path)))?;
            insert.execute(params![version_id, entry.path, content_id])?;
        }
        Ok(())
    }

    /// `hash -> Content.Id` for the hashes we have, asked in batches of [`HASH_BATCH`].
    fn ids_for_hashes(&self, hashes: &[Hash256]) -> Result<HashMap<Hash256, i64>> {
        let mut found = HashMap::with_capacity(hashes.len().min(4096));
        for chunk in hashes.chunks(HASH_BATCH) {
            let placeholders = std::iter::repeat_n("?", chunk.len())
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!("SELECT Hash, Id FROM Content WHERE Hash IN ({placeholders})");
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map(
                rusqlite::params_from_iter(chunk.iter().map(|h| h.as_slice())),
                |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)?)),
            )?;
            for row in rows {
                let (hash_bytes, id) = row?;
                if let Ok(hash) = hash_bytes.try_into() {
                    found.insert(hash, id);
                }
            }
        }
        Ok(found)
    }

    /// Count of files in a version without deserializing full paths and hashes.
    pub fn count_manifest_entries(&self, version_id: i64) -> Result<usize> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM ContentManifest WHERE VersionId = ?1",
            [version_id],
            |r| r.get(0),
        )?;
        Ok(count as usize)
    }

    /// Adds one file to a version. The blob must already be stored.
    pub fn add_manifest_entry(&self, version_id: i64, path: &str, content_id: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO ContentManifest(VersionId, Path, ContentId) VALUES (?1, ?2, ?3)",
            params![version_id, path, content_id],
        )?;
        Ok(())
    }

    /// A new version with the same files as `from` but another name and engine version, for when
    /// the server reports different fork/version/engine for content we already have (docs section 2).
    /// Copies manifest rows, not blobs. The `Robust` dependency gets `engine_version`; other modules
    /// are copied as they are, so callers must not use this when the engine changes and modules exist.
    pub fn duplicate_version(
        &self,
        from: i64,
        fork_id: &str,
        fork_version: &str,
        engine_version: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO ContentVersion(Hash, ForkId, ForkVersion, LastUsed, ZipHash)
             SELECT Hash, ?2, ?3, datetime('now'), ZipHash FROM ContentVersion WHERE Id = ?1",
            params![from, fork_id, fork_version],
        )?;
        let new_id = self.conn.last_insert_rowid();
        self.conn.execute(
            "INSERT INTO ContentManifest(VersionId, Path, ContentId)
             SELECT ?2, Path, ContentId FROM ContentManifest WHERE VersionId = ?1",
            [from, new_id],
        )?;
        for (module, version) in self.engine_dependencies(from)? {
            let version = if module == "Robust" {
                engine_version.to_string()
            } else {
                version
            };
            self.add_engine_dependency(new_id, &module, &version)?;
        }
        Ok(new_id)
    }

    /// The contents of one file of a version, if the version has it.
    pub fn read_file(&self, version_id: i64, path: &str) -> Result<Option<Vec<u8>>> {
        let id: Option<i64> = self
            .conn
            .query_row(
                "SELECT ContentId FROM ContentManifest WHERE VersionId = ?1 AND Path = ?2",
                params![version_id, path],
                |r| r.get(0),
            )
            .optional()?;
        id.map(|id| self.read_blob(id)).transpose()
    }

    /// The files of a version, sorted by path (bytewise on UTF-8).
    pub fn manifest_entries(&self, version_id: i64) -> Result<Vec<ManifestEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT cm.Path, c.Hash FROM ContentManifest cm
             JOIN Content c ON c.Id = cm.ContentId
             WHERE cm.VersionId = ?1 ORDER BY cm.Path",
        )?;
        let rows = stmt.query_map([version_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (path, hash) = row?;
            let hash: Hash256 = hash
                .try_into()
                .map_err(|_| Error::Other(format!("content hash of {path} is not 32 bytes")))?;
            out.push(ManifestEntry { hash, path });
        }
        Ok(out)
    }

    /// Manifest hash recomputed from what is stored; must equal the hash the server gave.
    pub fn compute_manifest_hash(&self, version_id: i64) -> Result<Hash256> {
        let entries = self.manifest_entries(version_id)?;
        Ok(manifest::compute_hash(
            entries.iter().map(|e| (&e.hash, e.path.as_str())),
        ))
    }

    /// Adds the version of one module. Repeating the same call is harmless (another launcher
    /// process or a second preparation may do it too): the first recorded version wins.
    pub fn add_engine_dependency(
        &self,
        version_id: i64,
        module: &str,
        module_version: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO ContentEngineDependency(VersionId, ModuleName, ModuleVersion)
             VALUES (?1, ?2, ?3)",
            params![version_id, module, module_version],
        )?;
        Ok(())
    }

    /// `(module, version)` pairs; the engine itself is the module `Robust`.
    pub fn engine_dependencies(&self, version_id: i64) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT ModuleName, ModuleVersion FROM ContentEngineDependency WHERE VersionId = ?1 ORDER BY ModuleName",
        )?;
        let rows = stmt.query_map([version_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// `(module, version)` pairs of every stored version; the engine itself is `Robust`.
    pub fn all_engine_dependencies(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT ModuleName, ModuleVersion FROM ContentEngineDependency")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Do we already have this build? (docs/protocol.md, section 2, step 2.)
    /// The manifest hash is the source of truth, then the zip hash, then fork and version.
    /// With several candidates, one that matches fork, version and engine exactly comes first;
    /// ties are broken by `LastUsed` and `Id`, so the answer never depends on SQLite's mood.
    ///
    /// `IS` rather than `=`: a row with `ForkId`/`ForkVersion` NULL would make `=` return NULL,
    /// and SQLite sorts NULL first — the "not matching" row would come out ahead.
    pub fn find_version(&self, build: &BuildInfo) -> Result<Option<ContentVersion>> {
        const ENGINE_OF: &str = "(SELECT ModuleVersion FROM ContentEngineDependency ced \
                                 WHERE ced.VersionId = cv.Id AND ModuleName = 'Robust')";
        const TO_END: &str = "cv.LastUsed DESC, cv.Id DESC";
        let cols = "cv.Id, cv.Hash, cv.ForkId, cv.ForkVersion, cv.LastUsed, cv.ZipHash";

        let found = if let Some(h) = build.manifest_hash() {
            let hash = parse_hash_hex(h)?;
            self.conn
                .query_row(
                    &format!(
                        "SELECT {cols} FROM ContentVersion cv WHERE cv.Hash = ?1
                         ORDER BY (cv.ForkVersion IS ?2 AND cv.ForkId IS ?3) DESC,
                                  ({ENGINE_OF} IS ?4) DESC, {TO_END}"
                    ),
                    params![
                        hash.as_slice(),
                        build.version,
                        build.fork_id,
                        build.engine_version
                    ],
                    row_to_version,
                )
                .optional()?
        } else if let Some(h) = build.hash() {
            let hash = parse_hash_hex(h)?;
            self.conn
                .query_row(
                    &format!(
                        "SELECT {cols} FROM ContentVersion cv WHERE cv.ZipHash = ?1
                         ORDER BY (cv.ForkVersion IS ?2 AND cv.ForkId IS ?3) DESC,
                                  ({ENGINE_OF} IS ?4) DESC, {TO_END}"
                    ),
                    params![
                        hash.as_slice(),
                        build.version,
                        build.fork_id,
                        build.engine_version
                    ],
                    row_to_version,
                )
                .optional()?
        } else {
            self.conn
                .query_row(
                    &format!(
                        "SELECT {cols} FROM ContentVersion cv WHERE cv.ForkId IS ?1 AND cv.ForkVersion IS ?2
                         ORDER BY ({ENGINE_OF} IS ?3) DESC, {TO_END}"
                    ),
                    params![build.fork_id, build.version, build.engine_version],
                    row_to_version,
                )
                .optional()?
        };
        Ok(found)
    }

    /// For tests and tools that need raw access.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

fn row_to_version(r: &rusqlite::Row<'_>) -> rusqlite::Result<ContentVersion> {
    Ok(ContentVersion {
        id: r.get(0)?,
        hash: r.get(1)?,
        fork_id: r.get(2)?,
        fork_version: r.get(3)?,
        last_used: r.get(4)?,
        zip_hash: r.get(5)?,
    })
}

/// Rolls back a transaction that was dropped without a commit, including when the closure inside
/// [`ContentDb::transaction`] panics. Without it `BEGIN IMMEDIATE` would stay open on the
/// connection and every later write in the process would fail.
struct TxGuard<'a> {
    conn: &'a Connection,
    finished: bool,
}

impl TxGuard<'_> {
    /// The transaction was committed: the guard must not roll anything back.
    fn finish(mut self) {
        self.finished = true;
    }
}

impl Drop for TxGuard<'_> {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.conn.execute_batch("ROLLBACK");
        }
    }
}

#[cfg(test)]
mod tests;
