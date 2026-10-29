using System;
using System.Buffers;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Diagnostics;
using System.Diagnostics.CodeAnalysis;
using System.IO;
using System.IO.Compression;
using System.Threading;
using Microsoft.Data.Sqlite;
using Robust.LoaderApi;
using SharpZstd.Interop;
using SQLitePCL;
using SS14.Launcher.Models.ContentManagement;
using SS14.Launcher.Utility;
using static SQLitePCL.raw;
using static SharpZstd.Interop.Zstd;

namespace SS14.Loader;

internal sealed class ContentDbFileApi : IFileApi, IDisposable
{
    private readonly Dictionary<string, (long id, int length, ContentCompressionScheme compr)> _files = new();
    private readonly SemaphoreSlim _dbConnectionsSemaphore;
    private readonly ConcurrentBag<ConPoolEntry> _dbConnections = new();
    private readonly int _connectionPoolSize;
    private bool _disposed;

    public unsafe ContentDbFileApi(string contentDbPath, long version)
    {
        if (sqlite3_threadsafe() == 0)
            throw new InvalidOperationException("SQLite is not thread safe!");

        var err = sqlite3_open_v2(
            contentDbPath,
            out var db,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_NOMUTEX,
            null);

        // Check the handle before touching it: a failed open used to continue and crash on the
        // next call instead of reporting why the content DB could not be opened.
        ThrowOpenError(db, err);

        // The launcher may hold a write transaction at this moment; wait for it instead of
        // silently losing the RunningClient row (which would make cull treat us as dead).
        sqlite3_busy_timeout(db, 5000);

        AddRunningClient(db, version);

        // Make sure to have a read transaction on every database connection
        // so that the launcher can't delete anything from underneath us if the user does anything.
        sqlite3_exec(db, "BEGIN");

        LoadManifest(version, db, out var initBlob);

        // Create pool of connections to avoid lock contention on multithreaded scenarios.
        var poolSize = _connectionPoolSize = ConnectionPoolSize();
        _dbConnectionsSemaphore = new SemaphoreSlim(poolSize, poolSize);
        _dbConnections.Add(new ConPoolEntry(db, ZSTD_createDCtx(), InitBlob()) { IsMaster = true });

        for (var i = 1; i < poolSize; i++)
        {
            err = sqlite3_open_v2(
                contentDbPath,
                out db,
                SQLITE_OPEN_READONLY | SQLITE_OPEN_NOMUTEX,
                null);
            ThrowOpenError(db, err);
            sqlite3_busy_timeout(db, 5000);

            sqlite3_exec(db, "BEGIN");

            _dbConnections.Add(new ConPoolEntry(db, ZSTD_createDCtx(), InitBlob()));
        }

        sqlite3_blob InitBlob()
        {
            var rc = sqlite3_blob_open(db, "main", "Content", "Data", initBlob, 0, out var blob);
            SqliteException.ThrowExceptionForRC(rc, db);
            return blob;
        }
    }

    private static void AddRunningClient(sqlite3 db, long contentVersion)
    {
        var err = sqlite3_prepare_v2(
            db,
            """
            INSERT OR REPLACE INTO
                RunningClient (ProcessId, MainModule, UsedVersion)
            VALUES
                (?, ?, ?)
            """,
            out var stmt);
        CheckThrowSqliteErr(db, err);
        try
        {
            sqlite3_bind_int(stmt, 1, Environment.ProcessId);
            sqlite3_bind_text(stmt, 2, Process.GetCurrentProcess().MainModule?.FileName ?? "");
            sqlite3_bind_int64(stmt, 3, contentVersion);

            // A failed step (for example SQLITE_BUSY) must be reported: otherwise the row is
            // missing and the launcher's cull considers this version unused.
            CheckThrowSqliteErr(db, sqlite3_step(stmt), SQLITE_DONE);
        }
        finally
        {
            stmt.Dispose();
        }
    }

    private static void ClearRunningClient(sqlite3 db)
    {
        // Ensure we are not in a transaction anymore.
        sqlite3_exec(db, "ROLLBACK");

        var err = sqlite3_prepare_v2(
            db,
            """
            DELETE FROM RunningClient
            WHERE ProcessId = ?
            """,
            out var stmt);
        CheckThrowSqliteErr(db, err);
        try
        {
            sqlite3_bind_int(stmt, 1, Environment.ProcessId);
            CheckThrowSqliteErr(db, sqlite3_step(stmt), SQLITE_DONE);
        }
        finally
        {
            stmt.Dispose();
        }
    }

    private void LoadManifest(long version, sqlite3 db, out long initBlob)
    {
        initBlob = default;

        var err = sqlite3_prepare_v2(
            db,
            @"
            SELECT c.ROWID, c.Size, c.Compression, cm.Path
            FROM Content c, ContentManifest cm
            WHERE cm.ContentId = c.Id AND cm.VersionId = ? AND cm.Path NOT LIKE '%/'",
            out var stmt);
        CheckThrowSqliteErr(db, err);

        sqlite3_bind_int64(stmt, 1, version);

        while ((err = sqlite3_step(stmt)) == SQLITE_ROW)
        {
            var rowId = sqlite3_column_int64(stmt, 0);
            var size = sqlite3_column_int(stmt, 1);
            var compression = (ContentCompressionScheme) sqlite3_column_int(stmt, 2);
            var path = sqlite3_column_text(stmt, 3).utf8_to_string();

            _files.Add(path, (rowId, size, compression));

            initBlob = rowId;
        }
        CheckThrowSqliteErr(db, err, SQLITE_DONE);

        err = sqlite3_finalize(stmt);
        CheckThrowSqliteErr(db, err);
    }

    private static void CheckThrowSqliteErr(sqlite3 db, int err, int expect=SQLITE_OK)
    {
        if (err != expect)
            SqliteException.ThrowExceptionForRC(err, db);
    }

    private static void ThrowOpenError(sqlite3? db, int err)
    {
        if (err == SQLITE_OK)
            return;

        if (db != null)
            SqliteException.ThrowExceptionForRC(err, db);

        throw new InvalidOperationException($"sqlite3_open_v2 failed with code {err}");
    }

    private static int ConnectionPoolSize()
    {
        var fallback = Math.Clamp(Environment.ProcessorCount, 2, 8);

        var envVar = Environment.GetEnvironmentVariable("SS14_LOADER_CONTENT_POOL_SIZE");
        if (string.IsNullOrEmpty(envVar))
            return fallback;

        // A malformed or non-positive value used to throw or deadlock the semaphore.
        if (!int.TryParse(envVar, out var requested))
        {
            Console.Error.WriteLine(
                "WARNING: SS14_LOADER_CONTENT_POOL_SIZE is not a number, using {0}", fallback);
            return fallback;
        }

        var clamped = Math.Clamp(requested, 1, 8);
        if (clamped != requested)
        {
            Console.Error.WriteLine(
                "WARNING: SS14_LOADER_CONTENT_POOL_SIZE {0} is out of range, using {1}",
                requested, clamped);
        }

        return clamped;
    }

    public unsafe void Dispose()
    {
        if (_disposed)
            return;

        _disposed = true;

        sqlite3? master = null;
        for (var i = 0; i < _connectionPoolSize; i++)
        {
            _dbConnectionsSemaphore.Wait();
            if (!_dbConnections.TryTake(out var db))
            {
                Console.Error.WriteLine("ERROR: Failed to retrieve content DB connection when shutting down!");
                continue;
            }

            if (db.IsMaster)
                master = db.Connection;
            else
                db.Connection.Close();

            db.Blob.Close();
            if (db.DecompressionContext != null)
                ZSTD_freeDCtx(db.DecompressionContext);
        }

        // No master means the pool was already emptied (or the constructor failed halfway);
        // Debug.Assert would have turned this into a NullReferenceException in Release.
        if (master == null)
            return;

        ClearRunningClient(master);

        master.Dispose();
    }

    public bool TryOpen(string path, [NotNullWhen(true)] out Stream? stream)
    {
        if (!_files.TryGetValue(path, out var tuple))
        {
            stream = null;
            return false;
        }

        var (id, length, compression) = tuple;

        _dbConnectionsSemaphore.Wait();
        ConPoolEntry? entry = null;
        try
        {
            if (!_dbConnections.TryTake(out entry))
                throw new InvalidOperationException("Entered semaphore but failed to retrieve DB connection??");

            var db = entry.Connection;
            var blob = entry.Blob;

            var err = sqlite3_blob_reopen(blob, id);
            if (err != SQLITE_OK)
                SqliteException.ThrowExceptionForRC(err, db);

            switch (compression)
            {
                case ContentCompressionScheme.Deflate:
                {
                    var buffer = GC.AllocateUninitializedArray<byte>(length);
                    stream = new MemoryStream(buffer);

                    using var blobStream = new SqliteBlobStream(blob, canWrite: false, ownsBlob: false);
                    using var deflater = new DeflateStream(blobStream, CompressionMode.Decompress);
                    deflater.CopyTo(stream);

                    // The buffer is uninitialized and its length is the declared file size.
                    // A short blob would leave the tail of the heap visible to the client.
                    if (stream.Length != length)
                    {
                        throw new InvalidOperationException(
                            $"Deflate blob '{path}' produced {stream.Length} bytes, expected {length}");
                    }

                    stream.Position = 0;
                    break;
                }
                case ContentCompressionScheme.ZStd:
                {
                    var buffer = GC.AllocateUninitializedArray<byte>(length);
                    stream = new MemoryStream(buffer, writable: false);

                    unsafe
                    {
                        ReadBlobZStd(buffer, blob, db, entry.DecompressionContext, path);
                    }
                    break;
                }
                case ContentCompressionScheme.None:
                {
                    var buffer = GC.AllocateUninitializedArray<byte>(length);
                    err = sqlite3_blob_read(blob, buffer.AsSpan(), 0);
                    if (err != SQLITE_OK)
                        SqliteException.ThrowExceptionForRC(err, db);

                    stream = new MemoryStream(buffer, writable: false);
                    break;
                }
                default:
                    throw new NotSupportedException($"Unknown compression scheme: {compression}");
            }
            return true;
        }
        finally
        {
            if (entry != null)
                _dbConnections.Add(entry);

            _dbConnectionsSemaphore.Release();
        }
    }

    private static unsafe void ReadBlobZStd(
        Span<byte> into,
        sqlite3_blob blob,
        sqlite3 db,
        ZSTD_DCtx* context,
        string path)
    {
        var remainingInput = sqlite3_blob_bytes(blob);
        var blobOffset = 0;
        var buffer = ArrayPool<byte>.Shared.Rent((int)ZSTD_DStreamInSize());
        // Each iteration consumes at least one input chunk (or terminates the stream), so a
        // generous bound on the chunk count keeps a malformed blob from spinning forever.
        var maxIterations = remainingInput / Math.Max(1, buffer.Length) + 1024;
        var iterations = 0;
        try
        {
            while (true)
            {
                if (++iterations > maxIterations)
                {
                    throw new InvalidOperationException(
                        $"zstd blob '{path}' did not finish decompressing after {iterations} iterations");
                }

                var toRead = Math.Min(buffer.Length, remainingInput);
                var rc = sqlite3_blob_read(blob, buffer.AsSpan(0, toRead), blobOffset);
                SqliteException.ThrowExceptionForRC(rc, db);
                blobOffset += toRead;
                remainingInput -= toRead;

                fixed (byte* inputPtr = buffer)
                fixed (byte* outputPtr = into)
                {
                    var inputBuf = new ZSTD_inBuffer { src =  inputPtr, pos = 0, size = (nuint)toRead};
                    var outputBuf = new ZSTD_outBuffer { dst = outputPtr, pos = 0, size = (nuint)into.Length };

                    var err = ZSTD_decompressStream(context, &outputBuf, &inputBuf);
                    ZStdException.ThrowIfError(err);

                    into = into[(int)outputBuf.pos..];

                    // We know the output buffer always has enough space so if this returns > 0 then we need more input.
                    if (err == (UIntPtr)0)
                    {
                        if (into.Length != 0)
                        {
                            // Safety check to avoid leaking any uninitialized data if I just screwed up.
                            throw new InvalidOperationException("Failed to fill buffer!");
                        }
                        return;
                    }

                    // Input is exhausted but zstd still expects more: the blob is truncated.
                    // Without this check the loop made no progress and burned a core forever.
                    if (toRead == 0 && remainingInput == 0)
                    {
                        throw new InvalidOperationException(
                            $"truncated zstd blob '{path}': {into.Length} byte(s) of the declared size are missing");
                    }

                    // The whole declared size is filled, yet zstd reports more output: the
                    // declared size is wrong and another zero-length iteration would spin.
                    if (into.Length == 0)
                    {
                        throw new InvalidOperationException(
                            $"zstd blob '{path}' decompresses to more than the declared size");
                    }
                }
            }
        }
        finally
        {
            ZSTD_DCtx_reset(context, ZSTD_ResetDirective.ZSTD_reset_session_only);
            ArrayPool<byte>.Shared.Return(buffer);
        }
    }

    private sealed unsafe class ConPoolEntry
    {
        public readonly sqlite3 Connection;
        public readonly ZSTD_DCtx* DecompressionContext;
        public readonly sqlite3_blob Blob;
        public bool IsMaster;

        public ConPoolEntry(sqlite3 connection, ZSTD_DCtx* decompressionContext, sqlite3_blob blob)
        {
            Connection = connection;
            DecompressionContext = decompressionContext;
            Blob = blob;
        }
    }

    public IEnumerable<string> AllFiles => _files.Keys;
}
