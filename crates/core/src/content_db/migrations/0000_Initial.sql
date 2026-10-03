-- Schema of the content database. Must stay compatible with the official launcher's content.db:
-- SS14.Loader reads Content / ContentManifest / RunningClient directly (docs/protocol.md, section 5).

-- One server version (build) stored in this database.
CREATE TABLE ContentVersion(
    -- AUTOINCREMENT so ids are never reused if the DB is cleared while a game is running.
    Id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- BLAKE2b-256 of the FULL manifest text of this version.
    Hash BLOB NOT NULL,
    -- Fork and version as reported by the server. Only a heuristic for cache eviction; not trusted.
    ForkId TEXT NULL,
    ForkVersion TEXT NULL,
    -- Last time this version was used, so old ones can be culled.
    LastUsed DATE NOT NULL,
    -- SHA-256 of the zip if the version was downloaded as a plain zip (servers without delta support).
    ZipHash BLOB NULL
    -- The engine version is stored in ContentEngineDependency as the 'Robust' module.
);

-- Actual file contents, deduplicated by hash.
CREATE TABLE Content(
    Id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- BLAKE2b-256 of the UNCOMPRESSED data.
    Hash BLOB NOT NULL UNIQUE,
    -- Uncompressed size.
    Size INTEGER NOT NULL,
    -- 0 = none, 1 = deflate, 2 = zstd.
    Compression INTEGER NOT NULL,
    Data BLOB NOT NULL,
    -- An uncompressed file must be exactly Size bytes.
    CONSTRAINT UncompressedSameSize CHECK(Compression != 0 OR length(Data) = Size)
);

-- File list of each version.
CREATE TABLE ContentManifest(
    Id INTEGER PRIMARY KEY,
    VersionId INTEGER NOT NULL REFERENCES ContentVersion(Id) ON DELETE CASCADE,
    Path TEXT NOT NULL,
    -- A blob in use cannot be deleted.
    ContentId INTEGER NOT NULL REFERENCES Content(Id) ON DELETE RESTRICT,
    -- No directory entries.
    CONSTRAINT NotDirectory CHECK (Path NOT LIKE '%/')
);

CREATE UNIQUE INDEX ContentManifestUniqueIndex ON ContentManifest(VersionId, Path);

-- Engine and modules a version needs. ModuleName 'Robust' is the engine itself.
CREATE TABLE ContentEngineDependency(
    Id INTEGER PRIMARY KEY,
    VersionId INTEGER NOT NULL REFERENCES ContentVersion(Id) ON DELETE CASCADE,
    ModuleName TEXT NOT NULL,
    ModuleVersion TEXT NOT NULL
);

CREATE UNIQUE INDEX ContentEngineModuleUniqueIndex ON ContentEngineDependency(VersionId, ModuleName);
