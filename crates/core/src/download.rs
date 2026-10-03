//! Manifest-mode downloads: the manifest and the missing file contents
//! (docs/protocol.md, section 3).

use std::io;
use std::time::Duration;

use std::path::Path;

use async_compression::tokio::bufread::ZstdDecoder;
use futures_util::{StreamExt, TryStreamExt};
use reqwest::header::{ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_TYPE};
use reqwest::{Client, Method, Response};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio_util::io::StreamReader;

use crate::content_db::ContentDb;
use crate::hash::{blake2b256, to_hex, Hash256};
use crate::manifest::Manifest;
use crate::{Error, Result};

/// The only blob download protocol version we speak.
pub const DOWNLOAD_PROTOCOL_VERSION: u32 = 1;

const MANIFEST_TIMEOUT: Duration = Duration::from_secs(120);
/// Safety limits against a hostile or broken server.
const MAX_MANIFEST_COMPRESSED: u64 = 256 * 1024 * 1024;
const MAX_MANIFEST_TEXT: u64 = 512 * 1024 * 1024;

/// Flush the database transaction after this many files or bytes, so an interrupted download
/// keeps most of its work.
const COMMIT_EVERY_FILES: usize = 200;
const COMMIT_EVERY_BYTES: u64 = 32 * 1024 * 1024;

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

/// Downloads `url` into `dest` (created or overwritten) and returns the SHA-256 of what arrived.
/// The caller decides whether that hash is acceptable and deletes `dest` if it is not.
pub async fn download_to_file(
    client: &Client,
    url: &str,
    dest: &Path,
    progress: Option<ProgressFn<'_>>,
) -> Result<Hash256> {
    let resp = client.get(url).send().await?;
    check_status(&resp)?;
    let total = resp.content_length().unwrap_or(0);

    let mut file = tokio::fs::File::create(dest).await?;
    let mut hasher = Sha256::new();
    let mut done = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        done += chunk.len() as u64;
        if let Some(p) = progress {
            p(Progress {
                done,
                total,
                unit: ProgressUnit::Bytes,
            });
        }
    }
    file.flush().await?;
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
        let mut decoder = ZstdDecoder::new(BufReader::new(reader));
        decoder.multiple_members(true);
        read_blob_stream(&mut decoder, db, manifest, indices, download_id, progress).await
    } else {
        let mut reader = reader;
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

    db.begin()?;
    let result = read_files(
        reader,
        db,
        manifest,
        indices,
        download_id,
        pre_compressed,
        progress,
    )
    .await;
    // Commit even after an error: every stored blob was verified, and a retry resumes from them.
    let committed = db.commit();
    result.and(committed)
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
    let mut since_commit_files = 0;
    let mut since_commit_bytes = 0u64;

    for (n, &index) in indices.iter().enumerate() {
        let entry = manifest.entries.get(index).ok_or_else(|| {
            Error::Other(format!("download index {index} is outside the manifest"))
        })?;

        let len = read_len(reader).await?;
        let compressed_len = if pre_compressed {
            read_len(reader).await?
        } else {
            0
        };

        let content_id = if compressed_len > 0 {
            let packed = read_exact_vec(reader, compressed_len).await?;
            let data = zstd::bulk::decompress(&packed, len)
                .map_err(|e| Error::Other(format!("{}: bad zstd data: {e}", entry.path)))?;
            if data.len() != len {
                return Err(Error::Other(format!(
                    "{}: decompressed to the wrong size",
                    entry.path
                )));
            }
            let actual = blake2b256(&data);
            if actual != entry.hash {
                return Err(Error::HashMismatch {
                    expected: to_hex(&entry.hash),
                    actual: to_hex(&actual),
                });
            }
            db.store_precompressed_blob(&entry.hash, len, &packed)?
        } else {
            let data = read_exact_vec(reader, len).await?;
            db.store_blob(&entry.hash, &data)?.0
        };
        db.link_interrupted_blob(download_id, content_id)?;

        since_commit_files += 1;
        since_commit_bytes += len as u64;
        if since_commit_files >= COMMIT_EVERY_FILES || since_commit_bytes >= COMMIT_EVERY_BYTES {
            db.commit()?;
            db.begin()?;
            since_commit_files = 0;
            since_commit_bytes = 0;
        }

        if let Some(p) = progress {
            p(Progress {
                done: n as u64 + 1,
                total,
                unit: ProgressUnit::Items,
            });
        }
    }
    Ok(())
}

async fn read_i32<R: AsyncRead + Unpin>(reader: &mut R) -> Result<i32> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf).await?;
    Ok(i32::from_le_bytes(buf))
}

async fn read_len<R: AsyncRead + Unpin>(reader: &mut R) -> Result<usize> {
    let v = read_i32(reader).await?;
    usize::try_from(v).map_err(|_| Error::Other(format!("negative length {v} in download stream")))
}

/// Reads exactly `len` bytes without trusting `len` for the allocation: memory grows only as
/// data really arrives.
async fn read_exact_vec<R: AsyncRead + Unpin>(reader: &mut R, len: usize) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(len.min(1 << 20));
    let got = AsyncReadExt::take(&mut *reader, len as u64)
        .read_to_end(&mut buf)
        .await?;
    if got != len {
        return Err(Error::Io(io::Error::from(io::ErrorKind::UnexpectedEof)));
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
