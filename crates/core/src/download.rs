//! Manifest-mode downloads: the manifest and the missing file contents
//! (docs/protocol.md, section 3).

use std::future::Future;
use std::io;
use std::time::Duration;

use std::path::Path;

use async_compression::tokio::bufread::ZstdDecoder;
use futures_util::{StreamExt, TryStreamExt};
use reqwest::header::{ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_TYPE};
use reqwest::{Client, Method, Response};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio_util::io::StreamReader;

use crate::content_db::{ContentDb, PreparedBlob};
use crate::hash::{blake2b256, to_hex, Hash256};
use crate::manifest::Manifest;
use crate::{Error, Result};

/// The only blob download protocol version we speak.
pub const DOWNLOAD_PROTOCOL_VERSION: u32 = 1;

const MANIFEST_TIMEOUT: Duration = Duration::from_secs(120);
/// A download that sends nothing for this long is treated as dead instead of waiting forever.
const IDLE_TIMEOUT: Duration = Duration::from_secs(45);
/// Upper bound for one downloaded file (engine zip, module zip, client zip).
const MAX_DOWNLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Upper bound for a single file inside a blob stream. The length comes from the server and is not
/// covered by the manifest, so it must not drive an allocation on its own.
const MAX_BLOB_BYTES: usize = 512 * 1024 * 1024;
/// Upper bound for everything one blob stream may deliver, decompressed.
const MAX_STREAM_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// Safety limits against a hostile or broken server.
const MAX_MANIFEST_COMPRESSED: u64 = 256 * 1024 * 1024;
const MAX_MANIFEST_TEXT: u64 = 512 * 1024 * 1024;

/// Flush the database transaction after this many files or bytes, so an interrupted download
/// keeps most of its work.
const COMMIT_EVERY_FILES: usize = 200;
const COMMIT_EVERY_BYTES: u64 = 32 * 1024 * 1024;

/// Size of one read from a network stream.
const READ_CHUNK: usize = 64 * 1024;

const FLAG_PRE_COMPRESSED: i32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressUnit {
    Items,
    Bytes,
}

#[derive(Debug, Clone, Copy)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
    /// Payload received over the network so far, whatever `unit` counts; the UI derives the
    /// speed from it. Stays 0 for stages that only work on local files.
    pub bytes: u64,
    pub unit: ProgressUnit,
}

pub type ProgressFn<'a> = &'a (dyn Fn(Progress) + Send + Sync);

fn is_zstd(resp: &Response) -> bool {
    resp.headers()
        .get_all(CONTENT_ENCODING)
        .iter()
        .next_back()
        .is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"zstd"))
}

fn check_status(resp: &Response) -> Result<()> {
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(Error::HttpStatus {
            status: resp.status().as_u16(),
            url: resp.url().to_string(),
        })
    }
}

fn too_large(url: &str, limit: u64) -> Error {
    Error::Other(format!(
        "{url}: download is larger than the {} MiB limit",
        limit / (1024 * 1024)
    ))
}

fn stalled(url: &str, idle: Duration) -> Error {
    Error::Io(io::Error::new(
        io::ErrorKind::TimedOut,
        format!("{url}: no data received for {}s", idle.as_secs()),
    ))
}

/// Runs one read of a body with an idle timeout, so a server that sends headers and stops cannot
/// hang the download forever.
async fn idle<T>(what: &str, fut: impl Future<Output = io::Result<T>>) -> Result<T> {
    tokio::time::timeout(IDLE_TIMEOUT, fut)
        .await
        .map_err(|_| stalled(what, IDLE_TIMEOUT))?
        .map_err(Error::Io)
}

/// Downloads `url` into `dest` (created or overwritten) and returns the SHA-256 of what arrived.
/// The caller decides whether that hash is acceptable and deletes `dest` if it is not.
pub async fn download_to_file(
    client: &Client,
    url: &str,
    dest: &Path,
    progress: Option<ProgressFn<'_>>,
) -> Result<Hash256> {
    download_to_file_limited(
        client,
        url,
        dest,
        progress,
        IDLE_TIMEOUT,
        MAX_DOWNLOAD_BYTES,
    )
    .await
}

/// [`download_to_file`] with explicit limits, so tests need not wait 45 s.
async fn download_to_file_limited(
    client: &Client,
    url: &str,
    dest: &Path,
    progress: Option<ProgressFn<'_>>,
    idle_timeout: Duration,
    max_bytes: u64,
) -> Result<Hash256> {
    let resp = client.get(url).send().await?;
    check_status(&resp)?;
    let total = resp.content_length().unwrap_or(0);
    if total > max_bytes {
        return Err(too_large(url, max_bytes));
    }

    let file = tokio::fs::File::create(dest).await?;
    let mut writer = BufWriter::with_capacity(256 * 1024, file);
    let mut hasher = Sha256::new();
    let mut done = 0u64;
    let mut stream = resp.bytes_stream();
    loop {
        let next = tokio::time::timeout(idle_timeout, stream.next())
            .await
            .map_err(|_| stalled(url, idle_timeout))?;
        let Some(chunk) = next else {
            break;
        };
        let chunk = chunk?;
        done += chunk.len() as u64;
        if done > max_bytes {
            return Err(too_large(url, max_bytes));
        }
        hasher.update(&chunk);
        writer.write_all(&chunk).await?;
        if let Some(p) = progress {
            p(Progress {
                done,
                total,
                bytes: done,
                unit: ProgressUnit::Bytes,
            });
        }
    }
    writer.flush().await?;
    Ok(hasher.finalize().into())
}

/// Downloads the content manifest and checks it against the hash from `/info`.
pub async fn fetch_manifest(
    client: &Client,
    url: &str,
    expected_hash_hex: &str,
) -> Result<Manifest> {
    let resp = client
        .get(url)
        .header(ACCEPT_ENCODING, "zstd")
        .timeout(MANIFEST_TIMEOUT)
        .send()
        .await?;
    check_status(&resp)?;
    if resp
        .content_length()
        .is_some_and(|n| n > MAX_MANIFEST_COMPRESSED)
    {
        return Err(Error::InvalidManifest(
            "manifest is unreasonably large".into(),
        ));
    }
    let zstd = is_zstd(&resp);
    let body = resp.bytes().await?;

    let text = if zstd {
        let mut out = Vec::new();
        let decoder = zstd::stream::read::Decoder::new(&body[..])?;
        io::Read::read_to_end(
            &mut io::Read::take(decoder, MAX_MANIFEST_TEXT + 1),
            &mut out,
        )?;
        if out.len() as u64 > MAX_MANIFEST_TEXT {
            return Err(Error::InvalidManifest(
                "manifest is unreasonably large".into(),
            ));
        }
        out
    } else {
        body.to_vec()
    };

    let manifest = Manifest::parse(&text)?;
    manifest.verify_hash(expected_hash_hex)?;
    Ok(manifest)
}

/// `OPTIONS` on the download URL: the server must support our protocol version.
pub async fn check_download_protocol(client: &Client, url: &str) -> Result<()> {
    let resp = client
        .request(Method::OPTIONS, url)
        .timeout(MANIFEST_TIMEOUT)
        .send()
        .await?;
    check_status(&resp)?;

    let header = |name: &str| -> Result<u32> {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse().ok())
            .ok_or_else(|| {
                Error::Other(format!(
                    "download server did not send a valid {name} header"
                ))
            })
    };
    let min = header("X-Robust-Download-Min-Protocol")?;
    let max = header("X-Robust-Download-Max-Protocol")?;
    if min > DOWNLOAD_PROTOCOL_VERSION || max < DOWNLOAD_PROTOCOL_VERSION {
        return Err(Error::Other(format!(
            "download server speaks protocols {min}..{max}, we need {DOWNLOAD_PROTOCOL_VERSION}"
        )));
    }
    Ok(())
}

/// Downloads the manifest entries at `indices` and stores them in the database.
/// Every blob is checked against its manifest hash before it is stored, and linked to
/// `download_id` (see [`ContentDb::begin_interrupted_download`]). Work done before an error is kept.
pub async fn download_blobs(
    client: &Client,
    db: &ContentDb,
    url: &str,
    manifest: &Manifest,
    indices: &[usize],
    download_id: i64,
    progress: Option<ProgressFn<'_>>,
) -> Result<()> {
    if indices.is_empty() {
        return Ok(());
    }

    let mut body = Vec::with_capacity(indices.len() * 4);
    for &i in indices {
        body.extend_from_slice(&(i as i32).to_le_bytes());
    }

    let resp = client
        .post(url)
        .header(
            "X-Robust-Download-Protocol",
            DOWNLOAD_PROTOCOL_VERSION.to_string(),
        )
        .header(ACCEPT_ENCODING, "zstd")
        .header(CONTENT_TYPE, "application/octet-stream")
        .body(body)
        .send()
        .await?;
    check_status(&resp)?;
    let zstd = is_zstd(&resp);

    let stream = Box::pin(resp.bytes_stream().map_err(io::Error::other));
    let reader = StreamReader::new(stream);
    if zstd {
        let mut decoder = ZstdDecoder::new(BufReader::with_capacity(256 * 1024, reader));
        decoder.multiple_members(true);
        read_blob_stream(&mut decoder, db, manifest, indices, download_id, progress).await
    } else {
        let mut reader = BufReader::with_capacity(256 * 1024, reader);
        read_blob_stream(&mut reader, db, manifest, indices, download_id, progress).await
    }
}

/// Parses the blob stream (docs/protocol.md, section 3.3, step 3). Separate from the HTTP part so it
/// can be tested on plain bytes.
pub(crate) async fn read_blob_stream<R: AsyncRead + Unpin>(
    reader: &mut R,
    db: &ContentDb,
    manifest: &Manifest,
    indices: &[usize],
    download_id: i64,
    progress: Option<ProgressFn<'_>>,
) -> Result<()> {
    let flags = read_i32(reader).await?;
    let pre_compressed = flags & FLAG_PRE_COMPRESSED != 0;
    read_files(
        reader,
        db,
        manifest,
        indices,
        download_id,
        pre_compressed,
        progress,
    )
    .await
}

/// Verified blobs waiting for insertion. The write transaction is opened only while they are
/// inserted, never while waiting for the network.
#[derive(Default)]
struct BlobBatch {
    blobs: Vec<PreparedBlob>,
    bytes: u64,
}

impl BlobBatch {
    fn push(&mut self, blob: PreparedBlob) {
        self.bytes += blob.size() as u64;
        self.blobs.push(blob);
    }

    fn should_flush(&self) -> bool {
        self.blobs.len() >= COMMIT_EVERY_FILES || self.bytes >= COMMIT_EVERY_BYTES
    }

    fn flush(&mut self, db: &ContentDb, download_id: i64) -> Result<()> {
        if self.blobs.is_empty() {
            return Ok(());
        }
        db.transaction(|db| {
            for blob in &self.blobs {
                let (content_id, _) = db.store_prepared_blob(blob)?;
                db.link_interrupted_blob(download_id, content_id)?;
            }
            // Heartbeat: cleanup in another process must see that this download is still alive.
            db.touch_interrupted_download(download_id)?;
            Ok(())
        })?;
        self.blobs.clear();
        self.bytes = 0;
        Ok(())
    }
}

async fn read_files<R: AsyncRead + Unpin>(
    reader: &mut R,
    db: &ContentDb,
    manifest: &Manifest,
    indices: &[usize],
    download_id: i64,
    pre_compressed: bool,
    progress: Option<ProgressFn<'_>>,
) -> Result<()> {
    let total = indices.len() as u64;
    let mut batch = BlobBatch::default();
    let mut stream_bytes = 0u64;
    let mut received = 0u64;

    for (n, &index) in indices.iter().enumerate() {
        match read_one_file(
            reader,
            manifest,
            index,
            pre_compressed,
            &mut stream_bytes,
            &mut received,
            &mut batch,
        )
        .await
        {
            Ok(()) => {
                if batch.should_flush() {
                    batch.flush(db, download_id)?;
                }
            }
            Err(e) => {
                // Commit even after an error: every stored blob was verified, and a retry resumes
                // from them.
                let _ = batch.flush(db, download_id);
                return Err(e);
            }
        }

        if let Some(p) = progress {
            p(Progress {
                done: n as u64 + 1,
                total,
                bytes: received,
                unit: ProgressUnit::Items,
            });
        }
    }
    batch.flush(db, download_id)
}

/// Reads one file from the stream and adds it to `batch`. The hash is checked before anything is
/// queued for storage. Blocking work (zstd, BLAKE2b) runs on a blocking thread.
async fn read_one_file<R: AsyncRead + Unpin>(
    reader: &mut R,
    manifest: &Manifest,
    index: usize,
    pre_compressed: bool,
    stream_bytes: &mut u64,
    received: &mut u64,
    batch: &mut BlobBatch,
) -> Result<()> {
    let entry = manifest
        .entries
        .get(index)
        .ok_or_else(|| Error::Other(format!("download index {index} is outside the manifest")))?;

    let len = read_len(reader).await?;
    check_blob_len(&entry.path, len)?;
    let compressed_len = if pre_compressed {
        read_len(reader).await?
    } else {
        0
    };
    check_blob_len(&entry.path, compressed_len)?;

    *stream_bytes += (len + compressed_len) as u64;
    // What actually crosses the wire: the packed form when there is one, for the speed shown.
    *received += (if compressed_len > 0 {
        compressed_len
    } else {
        len
    }) as u64;
    if *stream_bytes > MAX_STREAM_BYTES {
        return Err(Error::Other(
            "blob download is larger than the size limit".into(),
        ));
    }

    if compressed_len > 0 {
        let packed = read_exact_vec(reader, compressed_len).await?;
        let hash = entry.hash;
        let (packed, actual) =
            tokio::task::spawn_blocking(move || -> Result<(Vec<u8>, Hash256)> {
                let data = decompress_checked(&packed, len)?;
                Ok((packed, blake2b256(&data)))
            })
            .await
            .map_err(|e| Error::Other(format!("blob task failed: {e}")))??;
        if actual != hash {
            return Err(Error::HashMismatch {
                expected: to_hex(&hash),
                actual: to_hex(&actual),
            });
        }
        batch.push(PreparedBlob::precompressed(hash, len, packed));
    } else {
        let data = read_exact_vec(reader, len).await?;
        let hash = entry.hash;
        let prepared = tokio::task::spawn_blocking(move || PreparedBlob::verify(&hash, data))
            .await
            .map_err(|e| Error::Other(format!("blob task failed: {e}")))??;
        batch.push(prepared);
    }
    Ok(())
}

/// Checks a length that came from the stream. It decides how much memory we are willing to spend,
/// so it is capped before any allocation happens.
fn check_blob_len(path: &str, len: usize) -> Result<()> {
    if len > MAX_BLOB_BYTES {
        return Err(Error::Other(format!(
            "{path}: unpacks to an unreasonable size ({len} bytes)"
        )));
    }
    Ok(())
}

/// Decompresses `packed` without trusting it to produce exactly `len` bytes: output is collected
/// only as it really arrives and stops at `len + 1`.
fn decompress_checked(packed: &[u8], len: usize) -> Result<Vec<u8>> {
    let decoder = zstd::stream::read::Decoder::new(packed)
        .map_err(|e| Error::Other(format!("bad zstd data: {e}")))?;
    let mut out = Vec::with_capacity(len.min(1 << 20));
    io::Read::read_to_end(&mut io::Read::take(decoder, len as u64 + 1), &mut out)
        .map_err(|e| Error::Other(format!("bad zstd data: {e}")))?;
    if out.len() != len {
        return Err(Error::Other(format!(
            "zstd data decompressed to the wrong size ({} instead of {len} bytes)",
            out.len()
        )));
    }
    Ok(out)
}

async fn read_i32<R: AsyncRead + Unpin>(reader: &mut R) -> Result<i32> {
    let mut buf = [0u8; 4];
    idle("blob download", reader.read_exact(&mut buf)).await?;
    Ok(i32::from_le_bytes(buf))
}

async fn read_len<R: AsyncRead + Unpin>(reader: &mut R) -> Result<usize> {
    let v = read_i32(reader).await?;
    usize::try_from(v).map_err(|_| Error::Other(format!("negative length {v} in download stream")))
}

/// Reads exactly `len` bytes without trusting `len` for the allocation: memory grows only as
/// data really arrives, one chunk at a time.
async fn read_exact_vec<R: AsyncRead + Unpin>(reader: &mut R, len: usize) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(len.min(1 << 20));
    let mut chunk = [0u8; READ_CHUNK];
    let mut remaining = len;
    while remaining > 0 {
        let want = remaining.min(READ_CHUNK);
        let got = idle("blob download", reader.read(&mut chunk[..want])).await?;
        if got == 0 {
            return Err(Error::Io(io::Error::from(io::ErrorKind::UnexpectedEof)));
        }
        buf.extend_from_slice(&chunk[..got]);
        remaining -= got;
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::blake2b256;
    use crate::http::build_client;
    use crate::test_util::serve;

    fn manifest_text(files: &[(&str, &[u8])]) -> String {
        let mut text = String::from(crate::manifest::HEADER);
        text.push('\n');
        for (path, data) in files {
            text.push_str(&format!("{} {}\n", to_hex(&blake2b256(data)), path));
        }
        text
    }

    fn manifest_of(files: &[(&str, &[u8])]) -> Manifest {
        Manifest::parse(manifest_text(files).as_bytes()).unwrap()
    }

    fn sample_text() -> String {
        manifest_text(&[("a", b"alpha"), ("b", b"beta"), ("c", b"gamma")])
    }

    fn raw_stream(files: &[&[u8]]) -> Vec<u8> {
        let mut out = 0i32.to_le_bytes().to_vec();
        for f in files {
            out.extend((f.len() as i32).to_le_bytes());
            out.extend(*f);
        }
        out
    }

    /// `true` = send compressed, `false` = compressed length 0 and raw bytes.
    fn pre_compressed_stream(files: &[(&[u8], bool)]) -> Vec<u8> {
        let mut out = FLAG_PRE_COMPRESSED.to_le_bytes().to_vec();
        for (data, compress) in files {
            out.extend((data.len() as i32).to_le_bytes());
            if *compress {
                let packed = zstd::bulk::compress(data, 3).unwrap();
                out.extend((packed.len() as i32).to_le_bytes());
                out.extend(packed);
            } else {
                out.extend(0i32.to_le_bytes());
                out.extend(*data);
            }
        }
        out
    }

    async fn run(db: &ContentDb, m: &Manifest, idx: &[usize], stream: &[u8]) -> Result<()> {
        let id = db.begin_interrupted_download().unwrap();
        let mut r = stream;
        read_blob_stream(&mut r, db, m, idx, id, None).await
    }

    fn blob_count(db: &ContentDb) -> i64 {
        db.connection()
            .query_row("SELECT COUNT(*) FROM Content", [], |r| r.get(0))
            .unwrap()
    }

    #[tokio::test]
    async fn raw_stream_is_stored_and_linked() {
        let db = ContentDb::open_in_memory().unwrap();
        let big = vec![b'x'; 4000];
        let m = manifest_of(&[("a", b"hello"), ("b", &big)]);
        let id = db.begin_interrupted_download().unwrap();
        let mut r = &raw_stream(&[b"hello", &big])[..];
        read_blob_stream(&mut r, &db, &m, &[0, 1], id, None)
            .await
            .unwrap();

        let a = db.find_content_id(&m.entries[0].hash).unwrap().unwrap();
        let b = db.find_content_id(&m.entries[1].hash).unwrap().unwrap();
        assert_eq!(db.read_blob(a).unwrap(), b"hello");
        assert_eq!(db.read_blob(b).unwrap(), big);

        let linked: i64 = db
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM InterruptedDownloadContent WHERE InterruptedDownloadId = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(linked, 2);

        db.finish_download(id).unwrap();
        let left: i64 = db
            .connection()
            .query_row("SELECT COUNT(*) FROM InterruptedDownloadContent", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(left, 0, "finishing a download drops its links");
        assert_eq!(blob_count(&db), 2, "...but not the blobs");
    }

    #[tokio::test]
    async fn pre_compressed_stream_mixes_compressed_and_plain_files() {
        let db = ContentDb::open_in_memory().unwrap();
        let big = vec![7u8; 5000];
        let m = manifest_of(&[("a", &big), ("b", b"tiny")]);
        run(
            &db,
            &m,
            &[0, 1],
            &pre_compressed_stream(&[(&big, true), (b"tiny", false)]),
        )
        .await
        .unwrap();

        let a = db.find_content_id(&m.entries[0].hash).unwrap().unwrap();
        let b = db.find_content_id(&m.entries[1].hash).unwrap().unwrap();
        assert_eq!(db.read_blob(a).unwrap(), big);
        assert_eq!(db.read_blob(b).unwrap(), b"tiny");
    }

    #[tokio::test]
    async fn only_requested_indices_are_read_in_order() {
        let db = ContentDb::open_in_memory().unwrap();
        let m = manifest_of(&[("a", b"one"), ("b", b"two"), ("c", b"three")]);
        run(&db, &m, &[2, 0], &raw_stream(&[b"three", b"one"]))
            .await
            .unwrap();
        assert!(db.find_content_id(&m.entries[2].hash).unwrap().is_some());
        assert!(db.find_content_id(&m.entries[0].hash).unwrap().is_some());
        assert!(db.find_content_id(&m.entries[1].hash).unwrap().is_none());
    }

    #[tokio::test]
    async fn hash_mismatch_is_rejected_and_earlier_work_is_kept() {
        let db = ContentDb::open_in_memory().unwrap();
        let m = manifest_of(&[("a", b"good"), ("b", b"expected")]);
        let err = run(&db, &m, &[0, 1], &raw_stream(&[b"good", b"tampered"]))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::HashMismatch { .. }), "{err}");

        assert!(
            db.find_content_id(&m.entries[0].hash).unwrap().is_some(),
            "verified blob stays"
        );
        assert!(db.find_content_id(&m.entries[1].hash).unwrap().is_none());
        db.begin().unwrap(); // the transaction was closed
        db.rollback().unwrap();
    }

    #[tokio::test]
    async fn pre_compressed_mismatch_is_rejected() {
        let db = ContentDb::open_in_memory().unwrap();
        let big = vec![1u8; 3000];
        let other = vec![2u8; 3000];
        let m = manifest_of(&[("a", &big)]);
        let err = run(&db, &m, &[0], &pre_compressed_stream(&[(&other, true)]))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::HashMismatch { .. }), "{err}");
        assert_eq!(blob_count(&db), 0);
    }

    #[tokio::test]
    async fn truncated_stream_is_an_error_and_keeps_earlier_blobs() {
        let db = ContentDb::open_in_memory().unwrap();
        let m = manifest_of(&[("a", b"first"), ("b", b"second file")]);
        let mut stream = raw_stream(&[b"first", b"second file"]);
        stream.truncate(stream.len() - 4);
        assert!(run(&db, &m, &[0, 1], &stream).await.is_err());
        assert!(db.find_content_id(&m.entries[0].hash).unwrap().is_some());
        assert!(db.find_content_id(&m.entries[1].hash).unwrap().is_none());
    }

    #[tokio::test]
    async fn bogus_lengths_and_indices_are_rejected() {
        let db = ContentDb::open_in_memory().unwrap();
        let m = manifest_of(&[("a", b"x")]);

        // Claims 2 GB of data but ends right away: must fail without allocating 2 GB.
        let mut huge = 0i32.to_le_bytes().to_vec();
        huge.extend(i32::MAX.to_le_bytes());
        assert!(run(&db, &m, &[0], &huge).await.is_err());

        let mut negative = 0i32.to_le_bytes().to_vec();
        negative.extend((-5i32).to_le_bytes());
        assert!(run(&db, &m, &[0], &negative).await.is_err());

        assert!(run(&db, &m, &[7], &raw_stream(&[b"x"])).await.is_err());
        assert!(
            run(&db, &m, &[0], &[]).await.is_err(),
            "missing flags header"
        );
    }

    // ---- P1-5: no write transaction across network reads ----

    /// Sends one signal on the first read, then blocks the reading thread, which gives a probe on
    /// another thread a window to try a write.
    struct SignalReader {
        data: Vec<u8>,
        signal: Option<std::sync::mpsc::Sender<()>>,
    }

    impl tokio::io::AsyncRead for SignalReader {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<io::Result<()>> {
            let this = self.get_mut();
            if let Some(tx) = this.signal.take() {
                let _ = tx.send(());
                // Longer than the probe's own deadline, so a held lock is never released in time.
                std::thread::sleep(Duration::from_millis(500));
            }
            let n = buf.remaining().min(this.data.len());
            buf.put_slice(&this.data[..n]);
            this.data.drain(..n);
            std::task::Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn the_write_lock_is_not_held_while_the_stream_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("content.db");
        let db = ContentDb::open(&path).unwrap();
        let m = manifest_of(&[("a", b"payload")]);
        let id = db.begin_interrupted_download().unwrap();

        let mut stream = 0i32.to_le_bytes().to_vec();
        stream.extend((b"payload".len() as i32).to_le_bytes());
        stream.extend(b"payload");
        let (tx, rx) = std::sync::mpsc::channel();
        let mut reader = SignalReader {
            data: stream,
            signal: Some(tx),
        };

        let probe_path = path.clone();
        let probe = std::thread::spawn(move || {
            // Wait until the download is reading the body, then try to take the write lock.
            if rx.recv().is_err() {
                return false;
            }
            let conn = rusqlite::Connection::open(&probe_path).unwrap();
            conn.busy_timeout(Duration::from_millis(0)).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_millis(400);
            loop {
                if conn.execute_batch("BEGIN IMMEDIATE").is_ok() {
                    let _ = conn.execute_batch("ROLLBACK");
                    return true;
                }
                if std::time::Instant::now() > deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        });

        read_blob_stream(&mut reader, &db, &m, &[0], id, None)
            .await
            .unwrap();
        assert!(
            probe.join().unwrap(),
            "another writer was locked out while the body was still being downloaded"
        );
    }

    // ---- P1-6: idle timeout and size limit on the body ----

    async fn stalling_server() -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf).await;
            let _ = sock
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\nfirst")
                .await;
            // Send the headers and a little body, then go quiet forever.
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        base
    }

    #[tokio::test]
    async fn a_stalled_body_times_out_instead_of_hanging() {
        let base = stalling_server().await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("stalled.bin");
        let err = download_to_file_limited(
            &build_client().unwrap(),
            &base,
            &dest,
            None,
            Duration::from_millis(100),
            MAX_DOWNLOAD_BYTES,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&err, Error::Io(e) if e.kind() == io::ErrorKind::TimedOut),
            "{err}"
        );
    }

    #[tokio::test]
    async fn a_body_over_the_size_limit_is_refused() {
        let body = vec![0u8; 64];
        let (base, _) = serve(move |_| (200, vec![], body.clone())).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("big.bin");

        // Content-Length already says it is too big: no body is even read.
        let err = download_to_file_limited(
            &build_client().unwrap(),
            &base,
            &dest,
            None,
            IDLE_TIMEOUT,
            16,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("limit"), "{err}");
        assert!(!dest.exists(), "nothing is even created");
    }

    #[tokio::test]
    async fn a_chunked_body_over_the_size_limit_is_refused() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf).await;
            let _ = sock
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n\
                      5\r\nhello\r\n5\r\nworld\r\n0\r\n\r\n",
                )
                .await;
        });

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("chunked.bin");
        // No Content-Length, so only the byte counter can stop this.
        let err = download_to_file_limited(
            &build_client().unwrap(),
            &base,
            &dest,
            None,
            IDLE_TIMEOUT,
            6,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("limit"), "{err}");
    }

    // ---- P1-7: the size in the stream does not drive the allocation ----

    #[tokio::test]
    async fn a_file_above_the_per_file_cap_is_refused_before_reading_it() {
        let db = ContentDb::open_in_memory().unwrap();
        let m = manifest_of(&[("a", b"x")]);
        let too_big = (MAX_BLOB_BYTES as i32) + 1;

        let mut raw = 0i32.to_le_bytes().to_vec();
        raw.extend(too_big.to_le_bytes());
        let err = run(&db, &m, &[0], &raw).await.unwrap_err();
        assert!(err.to_string().contains("unreasonable size"), "{err}");

        // Same in the compressed branch: nothing is decompressed into a huge buffer.
        let mut packed = FLAG_PRE_COMPRESSED.to_le_bytes().to_vec();
        packed.extend(too_big.to_le_bytes());
        packed.extend(4i32.to_le_bytes());
        packed.extend([0u8; 4]);
        let err = run(&db, &m, &[0], &packed).await.unwrap_err();
        assert!(err.to_string().contains("unreasonable size"), "{err}");
        assert_eq!(blob_count(&db), 0);
    }

    #[tokio::test]
    async fn zstd_data_that_expands_past_the_claimed_size_is_rejected() {
        let db = ContentDb::open_in_memory().unwrap();
        let real = vec![b'a'; 100];
        let m = manifest_of(&[("a", &real)]);
        // Claims 10 bytes but the frame expands to 100.
        let packed = zstd::bulk::compress(&real, 3).unwrap();
        let mut stream = FLAG_PRE_COMPRESSED.to_le_bytes().to_vec();
        stream.extend(10i32.to_le_bytes());
        stream.extend((packed.len() as i32).to_le_bytes());
        stream.extend(packed);

        let err = run(&db, &m, &[0], &stream).await.unwrap_err();
        assert!(err.to_string().contains("wrong size"), "{err}");
        assert_eq!(blob_count(&db), 0);
    }

    // ---- HTTP level, against a throwaway local server ----

    #[tokio::test]
    async fn fetch_manifest_decodes_zstd_and_verifies_the_hash() {
        let text = sample_text();
        let packed = zstd::bulk::compress(text.as_bytes(), 3).unwrap();
        let (base, seen) = serve(move |_| {
            (
                200,
                vec![("Content-Encoding", "zstd".into())],
                packed.clone(),
            )
        })
        .await;
        let expected = to_hex(&blake2b256(text.as_bytes()));

        let m = fetch_manifest(
            &build_client().unwrap(),
            &format!("{base}manifest"),
            &expected,
        )
        .await
        .unwrap();
        assert_eq!(m.entries.len(), 3);
        assert_eq!(
            seen.lock().unwrap()[0]
                .headers
                .get("accept-encoding")
                .map(String::as_str),
            Some("zstd")
        );

        let wrong = "0".repeat(64);
        let err = fetch_manifest(&build_client().unwrap(), &format!("{base}manifest"), &wrong)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::HashMismatch { .. }));
    }

    #[tokio::test]
    async fn fetch_manifest_accepts_plain_text() {
        let text = sample_text();
        let body = text.clone().into_bytes();
        let (base, _) = serve(move |_| (200, vec![], body.clone())).await;
        let expected = to_hex(&blake2b256(text.as_bytes()));
        let m = fetch_manifest(&build_client().unwrap(), &base, &expected)
            .await
            .unwrap();
        assert_eq!(m.entries.len(), 3);
    }

    #[tokio::test]
    async fn fetch_manifest_reports_http_errors() {
        let (base, _) = serve(|_| (404, vec![], b"nope".to_vec())).await;
        let err = fetch_manifest(&build_client().unwrap(), &base, &"0".repeat(64))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::HttpStatus { status: 404, .. }));
    }

    #[tokio::test]
    async fn protocol_check_accepts_a_range_with_our_version() {
        let (base, seen) = serve(|_| {
            (
                204,
                vec![
                    ("X-Robust-Download-Min-Protocol", "1".into()),
                    ("X-Robust-Download-Max-Protocol", "3".into()),
                ],
                vec![],
            )
        })
        .await;
        check_download_protocol(&build_client().unwrap(), &base)
            .await
            .unwrap();
        assert_eq!(seen.lock().unwrap()[0].method, "OPTIONS");
    }

    #[tokio::test]
    async fn protocol_check_rejects_other_versions_and_missing_headers() {
        let (too_new, _) = serve(|_| {
            (
                204,
                vec![
                    ("X-Robust-Download-Min-Protocol", "2".into()),
                    ("X-Robust-Download-Max-Protocol", "3".into()),
                ],
                vec![],
            )
        })
        .await;
        assert!(check_download_protocol(&build_client().unwrap(), &too_new)
            .await
            .is_err());

        let (none, _) = serve(|_| (204, vec![], vec![])).await;
        assert!(check_download_protocol(&build_client().unwrap(), &none)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn download_blobs_end_to_end_with_zstd_response() {
        let big = vec![9u8; 6000];
        let m = manifest_of(&[("a", b"alpha"), ("b", &big), ("c", b"gamma")]);

        // The server sends entries 2 and 0 (what we ask for), pre-compressed flag set,
        // and splits the zstd stream into two frames.
        let stream = pre_compressed_stream(&[(b"gamma", false), (b"alpha", false)]);
        let (first, second) = stream.split_at(10);
        let mut body = zstd::bulk::compress(first, 3).unwrap();
        body.extend(zstd::bulk::compress(second, 3).unwrap());

        let (base, seen) =
            serve(move |_| (200, vec![("Content-Encoding", "zstd".into())], body.clone())).await;

        let db = ContentDb::open_in_memory().unwrap();
        let id = db.begin_interrupted_download().unwrap();
        download_blobs(&build_client().unwrap(), &db, &base, &m, &[2, 0], id, None)
            .await
            .unwrap();

        let req = seen.lock().unwrap()[0].clone();
        assert_eq!(req.method, "POST");
        assert_eq!(
            req.headers
                .get("x-robust-download-protocol")
                .map(String::as_str),
            Some("1")
        );
        assert_eq!(
            req.headers.get("content-type").map(String::as_str),
            Some("application/octet-stream")
        );
        assert_eq!(
            req.body,
            [2, 0, 0, 0, 0, 0, 0, 0],
            "indices as int32 little-endian"
        );

        assert!(db.find_content_id(&m.entries[0].hash).unwrap().is_some());
        assert!(db.find_content_id(&m.entries[2].hash).unwrap().is_some());
        assert!(db.find_content_id(&m.entries[1].hash).unwrap().is_none());
    }

    #[tokio::test]
    async fn download_blobs_plain_response_and_nothing_to_do() {
        let m = manifest_of(&[("a", b"alpha")]);
        let body = raw_stream(&[b"alpha"]);
        let (base, seen) = serve(move |_| (200, vec![], body.clone())).await;
        let db = ContentDb::open_in_memory().unwrap();
        let id = db.begin_interrupted_download().unwrap();
        let client = build_client().unwrap();

        download_blobs(&client, &db, &base, &m, &[], id, None)
            .await
            .unwrap();
        assert!(
            seen.lock().unwrap().is_empty(),
            "no request when nothing is missing"
        );

        download_blobs(&client, &db, &base, &m, &[0], id, None)
            .await
            .unwrap();
        assert!(db.find_content_id(&m.entries[0].hash).unwrap().is_some());
    }
}
