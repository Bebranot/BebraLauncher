-- The official launcher switched from SHA-256 to BLAKE2b and wiped the database here.
-- Kept so that script names match the official SchemaVersions table.
DELETE FROM ContentVersion;
DELETE FROM Content;
