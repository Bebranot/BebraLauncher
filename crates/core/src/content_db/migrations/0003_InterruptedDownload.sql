-- A download that was interrupted. Keeps the blobs that did arrive from being garbage collected,
-- so the next attempt can continue from them.
CREATE TABLE InterruptedDownload(
    Id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- When it was interrupted; used to drop it eventually.
    Added DATE NOT NULL
);

CREATE TABLE InterruptedDownloadContent(
    Id INTEGER PRIMARY KEY AUTOINCREMENT,
    InterruptedDownloadId INTEGER NOT NULL REFERENCES InterruptedDownload(Id) ON DELETE CASCADE,
    -- Unique: a new download must not fetch a blob we already have. The index also serves garbage collection.
    ContentId INTEGER NOT NULL UNIQUE REFERENCES Content(Id) ON DELETE CASCADE
);
