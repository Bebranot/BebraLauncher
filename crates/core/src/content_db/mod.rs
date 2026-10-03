//! The content database: downloaded game files, deduplicated (docs/protocol.md, section 5).
//!
//! The schema is the one the official launcher uses, because `SS14.Loader` reads this file directly.
//! Do not change tables or their meaning without checking the Loader.

pub mod cull;

use std::collections::HashSet;
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
            tx.execute_batch(sql)?;
            tx.execute(
                "INSERT INTO SchemaVersions(ScriptName, Applied) VALUES (?1, datetime('now'))",
                [name],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Runs `f` in one transaction: commit on `Ok`, roll back on `Err`. Must not be nested.
    pub fn transaction<T>(&self, f: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        self.begin()?;
        match f(self) {
            Ok(v) => {
                self.commit()?;
                Ok(v)
            }
            Err(e) => {
                let _ = self.rollback();
                Err(e)
            }
        }
    }

    /// Manual transaction control, for code that awaits between writes
    /// (a closure for [`ContentDb::transaction`] can't). Never nest.
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
        self.conn.execute(
            "INSERT OR IGNORE INTO InterruptedDownloadContent(InterruptedDownloadId, ContentId) VALUES (?1, ?2)",
            [download_id, content_id],
        )?;
        Ok(())
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

    /// Indices of manifest entries whose blob we don't have yet. A blob that appears under several
    /// paths is listed once (at its first index).
    pub fn missing_entries(&self, manifest: &Manifest) -> Result<Vec<usize>> {
        let mut stmt = self.conn.prepare("SELECT 1 FROM Content WHERE Hash = ?1")?;
        let mut queued: HashSet<Hash256> = HashSet::new();
        let mut missing = Vec::new();
        for (i, entry) in manifest.entries.iter().enumerate() {
            if queued.contains(&entry.hash) {
                continue;
            }
            if !stmt.exists([entry.hash.as_slice()])? {
                queued.insert(entry.hash);
                missing.push(i);
            }
        }
        Ok(missing)
    }

    /// Stores a file after checking that it really has `hash`. Compresses with zstd when that saves
    /// enough space. Returns the content id and whether a new row was created.
    pub fn store_blob(&self, hash: &Hash256, data: &[u8]) -> Result<(i64, bool)> {
        let actual = blake2b256(data);
        if &actual != hash {
            return Err(Error::HashMismatch {
                expected: to_hex(hash),
                actual: to_hex(&actual),
            });
        }
        if let Some(id) = self.find_content_id(hash)? {
            return Ok((id, false));
        }

        if !data.is_empty() {
            let compressed = zstd::bulk::compress(data, ZSTD_LEVEL)?;
            if compressed.len() + COMPRESSION_SAVINGS_THRESHOLD < data.len() {
                let id = self.insert_content(hash, data.len(), Compression::ZStd, &compressed)?;
                return Ok((id, true));
            }
        }
        let id = self.insert_content(hash, data.len(), Compression::None, data)?;
        Ok((id, true))
    }

    /// Stores a zstd-compressed file as delivered by the server. The caller must already have
    /// decompressed it and checked `hash` and `size`.
    pub fn store_precompressed_blob(
        &self,
        hash: &Hash256,
        size: usize,
        zstd_data: &[u8],
    ) -> Result<i64> {
        if let Some(id) = self.find_content_id(hash)? {
            return Ok(id);
        }
        self.insert_content(hash, size, Compression::ZStd, zstd_data)
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
    pub fn fill_manifest(&self, version_id: i64, manifest: &Manifest) -> Result<()> {
        let mut find = self
            .conn
            .prepare("SELECT Id FROM Content WHERE Hash = ?1")?;
        let mut insert = self.conn.prepare(
            "INSERT INTO ContentManifest(VersionId, Path, ContentId) VALUES (?1, ?2, ?3)",
        )?;
        for entry in &manifest.entries {
            let content_id: i64 = find
                .query_row([entry.hash.as_slice()], |r| r.get(0))
                .optional()?
                .ok_or_else(|| Error::Other(format!("missing blob for {}", entry.path)))?;
            insert.execute(params![version_id, entry.path, content_id])?;
        }
        Ok(())
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

    pub fn add_engine_dependency(
        &self,
        version_id: i64,
        module: &str,
        module_version: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO ContentEngineDependency(VersionId, ModuleName, ModuleVersion) VALUES (?1, ?2, ?3)",
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

    /// Do we already have this build? (docs/protocol.md, section 2, step 2.)
    /// The manifest hash is the source of truth, then the zip hash, then fork and version.
    /// With several candidates, one that matches fork, version and engine exactly comes first.
    pub fn find_version(&self, build: &BuildInfo) -> Result<Option<ContentVersion>> {
        const ENGINE_OF: &str = "(SELECT ModuleVersion FROM ContentEngineDependency ced \
                                 WHERE ced.VersionId = cv.Id AND ModuleName = 'Robust')";
        let cols = "cv.Id, cv.Hash, cv.ForkId, cv.ForkVersion, cv.LastUsed, cv.ZipHash";

        let found = if let Some(h) = build.manifest_hash() {
            let hash = parse_hash_hex(h)?;
            self.conn
                .query_row(
                    &format!(
                        "SELECT {cols} FROM ContentVersion cv WHERE cv.Hash = ?1
                         ORDER BY cv.ForkVersion = ?2 AND cv.ForkId = ?3 AND {ENGINE_OF} = ?4 DESC"
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
                         ORDER BY cv.ForkVersion = ?2 AND cv.ForkId = ?3 AND {ENGINE_OF} = ?4 DESC"
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
                        "SELECT {cols} FROM ContentVersion cv WHERE cv.ForkId = ?1 AND cv.ForkVersion = ?2
                         ORDER BY {ENGINE_OF} = ?3 DESC"
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

#[cfg(test)]
mod tests;
