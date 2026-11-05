//! Updating the launcher itself from the releases of a GitHub repository.
//!
//! A release carries the installer (`*-setup.exe`) and a signature file next to it
//! (`*-setup.exe.sig`). The signature is Ed25519 over `bebra-update:<version>:<sha256 of the
//! installer>`, made with a private key that never leaves the developer's machine; the public key
//! is built into the launcher. So a stolen GitHub token or a swapped asset cannot make the
//! launcher run a foreign installer, and an old signed installer cannot be passed off as a new one.
//!
//! Versions are `a.b.cdd`: `a` global changes, `b` significant ones, `c` between significant and
//! small, `dd` micro patches. For Cargo and Tauri this is plain `a.b.patch` with `patch = c*100+dd`,
//! so `0.2.5` is shown as `0.2.005` and `0.2.305` is `0.2.305`.

use std::fmt;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use futures_util::StreamExt;
use reqwest::header::{HeaderValue, ACCEPT, AUTHORIZATION};
use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::secret::Token;
use crate::{signature, Error, Result};

/// The repository the launcher looks at unless the settings say otherwise.
pub const DEFAULT_REPO: &str = "Bebranot/vabecide-musor";

/// The public half of the key that signs the launcher's releases.
const UPDATE_KEY_PEM: &str = include_str!("update_key.pem");

/// Largest installer we agree to download.
const MAX_INSTALLER: u64 = 400 * 1024 * 1024;
/// A signature file is one line of hex.
const MAX_SIGNATURE: usize = 1024;
/// A download that sends nothing for this long is treated as dead.
const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const API: &str = "https://api.github.com";
/// Files of public repositories. Never gets the token.
const RAW: &str = "https://raw.githubusercontent.com";

/// Installers we make start with this, followed by the version.
const PRODUCT_PREFIX: &str = "BebraLauncher_";

/// Hosts the access token may be sent to. GitHub itself only: an asset URL from a hostile or
/// compromised answer must not receive the user's token.
const TOKEN_HOSTS: [&str; 2] = ["api.github.com", "github.com"];

// ---------------------------------------------------------------- versions

/// `a.b.cdd` as three numbers; `patch` is `c*100 + dd`, at most 999.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    /// Reads `0.2.5`, `0.2.005` or a tag like `v0.2.005`. Anything else (a suffix, a fourth
    /// number, a patch above 999) is not a version of ours.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
        let mut parts = text.split('.');
        let mut number = || -> Option<u32> {
            let p = parts.next()?;
            if p.is_empty() || p.len() > 6 || !p.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            p.parse().ok()
        };
        let (major, minor, patch) = (number()?, number()?, number()?);
        if parts.next().is_some() || patch > 999 {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
        })
    }

    /// The version this build was made as.
    pub fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).expect("the package version is a.b.c")
    }

    /// The `c` digit: between significant changes and small patches.
    pub fn c(&self) -> u32 {
        self.patch / 100
    }

    /// The `dd` part: micro patches.
    pub fn dd(&self) -> u32 {
        self.patch % 100
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{:03}", self.major, self.minor, self.patch)
    }
}

// ---------------------------------------------------------------- releases

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Asset {
    pub name: String,
    /// The API address of the asset (works for private repositories too, with a token).
    pub url: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Release {
    pub version: Version,
    pub tag: String,
    /// The release text: the changelog section of this version.
    pub notes: String,
    pub published: Option<String>,
    pub page_url: String,
    pub setup: Asset,
    pub signature: Asset,
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    url: String,
    #[serde(default)]
    size: u64,
}

/// An installer name we accept: plain characters, ends with `-setup.exe`, no path parts.
fn is_setup_name(name: &str) -> bool {
    name.len() <= 120
        && name.ends_with("-setup.exe")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        && !name.starts_with('.')
}

/// The version in an installer name: `BebraLauncher_0.2.5_x64-setup.exe` -> `0.2.5`.
fn setup_version(name: &str) -> Option<Version> {
    name.strip_suffix("-setup.exe")?
        .split('_')
        .nth(1)
        .and_then(Version::parse)
}

/// Reads GitHub's answer for one release. `Ok(None)`: a draft, a pre-release, a tag that is not a
/// version of ours (e.g. `latest`), or one without our installer and its signature (not for us).
/// A non-version tag is not an error: the repository may legitimately publish such a release, and
/// failing on it would stop updates forever.
pub fn parse_release(json: &str) -> Result<Option<Release>> {
    let raw: RawRelease = serde_json::from_str(json)
        .map_err(|e| Error::Other(format!("cannot read the release: {e}")))?;
    if raw.draft || raw.prerelease {
        return Ok(None);
    }
    let Some(version) = Version::parse(&raw.tag_name) else {
        eprintln!(
            "self-update: release tag {:?} is not a version; ignoring it",
            raw.tag_name
        );
        return Ok(None);
    };

    // Several installers may be attached (32-bit, x64, a debug build...). Take one whose version
    // matches the release tag; if none does, the highest version with a signature next to it.
    let mut candidates: Vec<&RawAsset> = raw
        .assets
        .iter()
        .filter(|a| is_setup_name(&a.name))
        .filter(|a| {
            raw.assets
                .iter()
                .any(|s| s.name == format!("{}.sig", a.name))
        })
        .collect();
    candidates.sort_by(|a, b| {
        let rank = |asset: &&RawAsset| {
            let v = setup_version(&asset.name);
            (v == Some(version), v)
        };
        let (a_match, a_version) = rank(a);
        let (b_match, b_version) = rank(b);
        b_match
            .cmp(&a_match)
            .then_with(|| b_version.cmp(&a_version))
            .then_with(|| a.name.cmp(&b.name))
    });
    let Some(setup) = candidates.first() else {
        return Ok(None);
    };
    let Some(signature) = raw
        .assets
        .iter()
        .find(|a| a.name == format!("{}.sig", setup.name))
    else {
        return Ok(None);
    };
    Ok(Some(Release {
        version,
        tag: raw.tag_name,
        notes: raw.body.unwrap_or_default(),
        published: raw.published_at,
        page_url: raw.html_url,
        setup: Asset {
            name: setup.name.clone(),
            url: setup.url.clone(),
            size: setup.size,
        },
        signature: Asset {
            name: signature.name.clone(),
            url: signature.url.clone(),
            size: signature.size,
        },
    }))
}

/// `owner/name` made of the characters GitHub allows; anything else could bend the URL.
pub fn valid_repo(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let ok = |p: &str| {
        !p.is_empty()
            && p != "."
            && p != ".."
            && p.len() <= 100
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    };
    matches!((parts.next(), parts.next(), parts.next()), (Some(a), Some(b), None) if ok(a) && ok(b))
}

/// Is this an HTTPS URL for one of [`TOKEN_HOSTS`] (no userinfo, no tricks in the host)?
fn is_token_host(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if host.contains('@') || host.contains(':') {
        return false;
    }
    TOKEN_HOSTS.contains(&host.as_str())
}

/// Builds a request and says whether the token was attached. The token goes only to GitHub
/// itself: release assets may point anywhere, and a stolen token is worse than a failed update.
fn request(
    client: &Client,
    url: &str,
    accept: &str,
    token: Option<&Token>,
) -> (reqwest::RequestBuilder, bool) {
    let mut req = client
        .get(url)
        .header(ACCEPT, accept)
        .header("X-GitHub-Api-Version", "2022-11-28");
    let mut sent = false;
    if let Some(token) = token {
        if is_token_host(url) {
            if let Ok(mut value) = HeaderValue::from_str(&format!("Bearer {}", token.expose())) {
                // Keeps the token out of logs and crash reports.
                value.set_sensitive(true);
                req = req.header(AUTHORIZATION, value);
                sent = true;
            }
        }
    }
    (req, sent)
}

/// Reads a small body without trusting its `Content-Length`: never more than `max` bytes are kept.
async fn read_limited(resp: Response, max: usize, what: &str) -> Result<Vec<u8>> {
    if resp.content_length().is_some_and(|n| n > max as u64) {
        return Err(Error::Other(format!("{what} is too large")));
    }
    let mut out = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if out.len() + chunk.len() > max {
            return Err(Error::Other(format!("{what} is too large")));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// A readable reason for a refused request.
fn explain(status: StatusCode, token: bool) -> Error {
    let text = match status {
        StatusCode::NOT_FOUND if !token => {
            "repository or release not found: if the repository is private, add an access token"
        }
        StatusCode::NOT_FOUND => {
            "repository or release not found (check the name and the token's access)"
        }
        StatusCode::UNAUTHORIZED => "GitHub did not accept the access token",
        StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS => {
            "GitHub refused the request (the token has no access, or the rate limit is used up)"
        }
        _ => {
            return Error::Other(format!("GitHub answered with HTTP {}", status.as_u16()));
        }
    };
    Error::Other(text.into())
}

fn ok(resp: Response, token: bool) -> Result<Response> {
    if resp.status().is_success() {
        Ok(resp)
    } else {
        Err(explain(resp.status(), token))
    }
}

/// The newest published release of `repo` that carries a signed installer.
pub async fn latest(client: &Client, repo: &str, token: Option<&Token>) -> Result<Option<Release>> {
    if !valid_repo(repo) {
        return Err(Error::Other(format!("not a repository name: {repo}")));
    }
    let url = format!("{API}/repos/{repo}/releases/latest");
    let (req, token_sent) = request(client, &url, "application/vnd.github+json", token);
    let resp = req
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await?;
    let text = ok(resp, token_sent)?.text().await?;
    parse_release(&text)
}

/// Reads a small file from the `main` branch of `repo`; `None` if there is no such file, or the
/// repository is closed and there is no token. The public address is asked first and without the
/// token, so nothing changes for anyone when the repository becomes public.
pub async fn read_repo_file(
    client: &Client,
    repo: &str,
    path: &str,
    token: Option<&Token>,
    max: usize,
) -> Result<Option<Vec<u8>>> {
    if !valid_repo(repo) {
        return Err(Error::Other(format!("not a repository name: {repo}")));
    }
    let timeout = std::time::Duration::from_secs(15);
    let resp = client
        .get(format!("{RAW}/{repo}/main/{path}"))
        .timeout(timeout)
        .send()
        .await?;
    if resp.status().is_success() {
        return read_limited(resp, max, path).await.map(Some);
    }
    if resp.status() != StatusCode::NOT_FOUND {
        return Err(explain(resp.status(), false));
    }
    let Some(token) = token else {
        return Ok(None);
    };
    let url = format!("{API}/repos/{repo}/contents/{path}");
    let (req, token_sent) = request(client, &url, "application/vnd.github.raw+json", Some(token));
    let resp = req.timeout(timeout).send().await?;
    if resp.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    read_limited(ok(resp, token_sent)?, max, path)
        .await
        .map(Some)
}

// ---------------------------------------------------------------- the signature

/// What the signature covers: the version and the installer's SHA-256 (lower-case hex).
pub fn signed_message(version: &Version, sha256_hex: &str) -> String {
    format!("bebra-update:{version}:{}", sha256_hex.to_ascii_lowercase())
}

/// The built-in public key.
pub fn update_key() -> Result<VerifyingKey> {
    signature::parse_pem(UPDATE_KEY_PEM)
}

/// Checks the release signature of an installer whose SHA-256 is `sha256_hex`.
pub fn verify_update(
    key: &VerifyingKey,
    version: &Version,
    sha256_hex: &str,
    sig_hex: &str,
) -> Result<()> {
    signature::verify(
        key,
        signed_message(version, sha256_hex).as_bytes(),
        sig_hex.trim(),
    )
}

/// Signs an installer (for the release script): the hex signature to publish as `*.sig`.
pub fn sign_update(key: &SigningKey, version: &Version, sha256_hex: &str) -> String {
    hex::encode_upper(
        key.sign(signed_message(version, sha256_hex).as_bytes())
            .to_bytes(),
    )
}

/// A signing key from the 64 hex digits of its seed.
pub fn signing_key_from_hex(text: &str) -> Result<SigningKey> {
    let bytes = hex::decode(text.trim())
        .map_err(|e| Error::Other(format!("the key file is not hex: {e}")))?;
    let seed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| Error::Other("the key must be 32 bytes (64 hex digits)".into()))?;
    Ok(SigningKey::from_bytes(&seed))
}

// ---------------------------------------------------------------- downloading

/// SHA-256 of a file, as lower-case hex.
pub fn sha256_file(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Downloads the installer of `release` into `dir`, checking the signature before the file gets
/// its final name. `progress(done, total)` is called as the bytes arrive. Returns the path.
pub async fn download(
    client: &Client,
    release: &Release,
    token: Option<&Token>,
    dir: &Path,
    key: &VerifyingKey,
    mut progress: impl FnMut(u64, u64),
) -> Result<PathBuf> {
    if !is_setup_name(&release.setup.name) {
        return Err(Error::Other("the installer has a suspicious name".into()));
    }
    if release.setup.size > MAX_INSTALLER {
        return Err(Error::Other("the installer is unreasonably large".into()));
    }
    tokio::fs::create_dir_all(dir).await?;

    // The signature first: it is tiny, and without it there is nothing to download for.
    let (req, token_sent) = request(
        client,
        &release.signature.url,
        "application/octet-stream",
        token,
    );
    let resp = req
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await?;
    let sig_bytes =
        read_limited(ok(resp, token_sent)?, MAX_SIGNATURE, "the signature file").await?;
    let sig_hex = String::from_utf8_lossy(&sig_bytes).trim().to_string();

    let part = dir.join(format!("{}.part", release.setup.name));
    let fin = dir.join(&release.setup.name);

    // Downloaded before and never installed: the same file, if it still checks out, is reused.
    // Hashing a 400 MB installer is blocking work and must not run on the async runtime.
    let hash_path = fin.clone();
    let existing_sha = tokio::task::spawn_blocking(move || sha256_file(&hash_path))
        .await
        .map_err(|e| Error::Other(format!("hashing task failed: {e}")))?;
    if let Ok(sha) = existing_sha {
        if verify_update(key, &release.version, &sha, &sig_hex).is_ok() {
            write_sig_sidecar(&fin, &sig_hex).await?;
            progress(release.setup.size, release.setup.size);
            return Ok(fin);
        }
    }
    let result = fetch_checked(client, release, token, &part, key, &sig_hex, &mut progress).await;
    match result {
        Ok(()) => {
            let _ = tokio::fs::remove_file(&fin).await;
            tokio::fs::rename(&part, &fin).await?;
            // The signature travels with the installer so that `spawn_installer` can check the
            // file again right before running it.
            write_sig_sidecar(&fin, &sig_hex).await?;
            Ok(fin)
        }
        Err(e) => {
            let _ = tokio::fs::remove_file(&part).await;
            Err(e)
        }
    }
}

/// Where the verified signature of an installer is kept: `<installer>.sig`, next to it.
fn sig_sidecar(setup: &Path) -> PathBuf {
    let mut name = setup.as_os_str().to_os_string();
    name.push(".sig");
    PathBuf::from(name)
}

/// Writes the signature next to the installer, atomically (temp file, then rename).
async fn write_sig_sidecar(setup: &Path, sig_hex: &str) -> Result<()> {
    let path = sig_sidecar(setup);
    let tmp = path.with_extension("sig.tmp");
    tokio::fs::write(&tmp, sig_hex.trim()).await?;
    let _ = tokio::fs::remove_file(&path).await;
    tokio::fs::rename(&tmp, &path).await?;
    Ok(())
}

async fn fetch_checked(
    client: &Client,
    release: &Release,
    token: Option<&Token>,
    part: &Path,
    key: &VerifyingKey,
    sig_hex: &str,
    progress: &mut impl FnMut(u64, u64),
) -> Result<()> {
    let (req, token_sent) = request(
        client,
        &release.setup.url,
        "application/octet-stream",
        token,
    );
    let resp = req.send().await?;
    let resp = ok(resp, token_sent)?;
    let total = resp.content_length().unwrap_or(release.setup.size);
    let mut file = tokio::fs::File::create(part).await?;
    let mut hasher = Sha256::new();
    let mut done = 0u64;
    let mut stream = resp.bytes_stream();
    loop {
        // A server that stops sending must not hang the update forever.
        let next = tokio::time::timeout(IDLE_TIMEOUT, stream.next())
            .await
            .map_err(|_| {
                Error::Other(format!(
                    "the installer download stopped for {}s",
                    IDLE_TIMEOUT.as_secs()
                ))
            })?;
        let Some(chunk) = next else {
            break;
        };
        let chunk = chunk?;
        done += chunk.len() as u64;
        if done > MAX_INSTALLER {
            return Err(Error::Other("the installer is unreasonably large".into()));
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        progress(done, total);
    }
    file.flush().await?;
    drop(file);
    if done == 0 {
        return Err(Error::Other("the installer came empty".into()));
    }
    verify_update(
        key,
        &release.version,
        &hex::encode(hasher.finalize()),
        sig_hex,
    )
    .map_err(|_| Error::Other("the installer's signature is wrong: it was not installed".into()))
}

/// Deletes installers left in `dir` by earlier updates, except `keep` and its signature.
pub fn clean_old(dir: &Path, keep: Option<&Path>) {
    let keep_sig = keep.map(sig_sidecar);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let ours = name.ends_with("-setup.exe")
            || name.ends_with("-setup.exe.sig")
            || name.ends_with(".part");
        if ours && Some(path.as_path()) != keep && Some(&path) != keep_sig.as_ref() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Deletes installers of this version or older: they are installed already (or beaten).
///
/// Only files we name ourselves are touched: an unrelated `notes-setup.exe` that happens to sit
/// in the folder is left alone.
pub fn clean_installed(dir: &Path) {
    let current = Version::current();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(PRODUCT_PREFIX) {
            continue;
        }
        let ours = name.ends_with("-setup.exe")
            || name.ends_with("-setup.exe.sig")
            || name.ends_with(".part");
        if !ours {
            continue;
        }
        // `BebraLauncher_0.2.5_x64-setup.exe`: the version sits between the first two `_`.
        let version = name.split('_').nth(1).and_then(Version::parse);
        let leftover = name.ends_with(".part") || version.is_some_and(|v| v <= current);
        if leftover {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

// ---------------------------------------------------------------- installing

/// Runs the installer silently over the current installation. With `restart` it starts the
/// launcher again when done. The caller exits right after: the installer replaces the files.
///
/// The file has been sitting in a user-writable folder since it was checked, so it is checked
/// once more right here: the name gives the version, the sidecar `.sig` (written by [`download`])
/// is the signature over that version and the file's SHA-256. A replaced installer is refused.
pub fn spawn_installer(setup: &Path, restart: bool) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    let name = setup
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if !setup.is_file() || !is_setup_name(name) {
        return Err(Error::Other("the installer is not there".into()));
    }
    let version = setup_version(name)
        .ok_or_else(|| Error::Other("the installer's name carries no version".into()))?;
    let sig = std::fs::read_to_string(sig_sidecar(setup)).map_err(|_| {
        Error::Other("the installer's signature is missing: download the update again".into())
    })?;
    if sig.len() > MAX_SIGNATURE {
        return Err(Error::Other(
            "the installer's signature is too large".into(),
        ));
    }
    let sha = sha256_file(setup)?;
    let key = update_key()?;
    verify_update(&key, &version, &sha, sig.trim()).map_err(|_| {
        Error::Other("the installer changed after it was verified: it was not started".into())
    })?;

    let mut cmd = std::process::Command::new(setup);
    // `/S` silent, `/UPDATE` replaces a running copy without asking, `/R` starts the app after.
    cmd.arg("/S").arg("/UPDATE");
    if restart {
        cmd.arg("/R");
    }
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map_err(|e| Error::Other(format!("could not start the installer: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETUP: &str = "BebraLauncher_0.2.5_x64-setup.exe";

    fn release_json(tag: &str, extra: &str, assets: &[&str]) -> String {
        let assets: Vec<String> = assets
            .iter()
            .enumerate()
            .map(|(i, n)| {
                format!(r#"{{"name":"{n}","url":"https://api.github.com/repos/o/r/releases/assets/{i}","size":{}}}"#, 100 + i)
            })
            .collect();
        format!(
            r#"{{"tag_name":"{tag}","body":"- thing","published_at":"2026-10-04T10:00:00Z","html_url":"https://github.com/o/r/releases/tag/{tag}"{extra},"assets":[{}]}}"#,
            assets.join(",")
        )
    }

    #[test]
    fn versions_parse_order_and_show_as_a_b_cdd() {
        let v = Version::parse("v0.2.5").unwrap();
        assert_eq!(
            v,
            Version {
                major: 0,
                minor: 2,
                patch: 5
            }
        );
        assert_eq!(v.to_string(), "0.2.005");
        assert_eq!(Version::parse("0.2.005"), Some(v));
        assert_eq!(Version::parse("1.12.345").unwrap().to_string(), "1.12.345");
        assert_eq!(
            (
                Version::parse("1.0.345").unwrap().c(),
                Version::parse("1.0.345").unwrap().dd()
            ),
            (3, 45)
        );
        assert!(Version::parse("0.2.10").unwrap() > Version::parse("0.2.9").unwrap());
        assert!(Version::parse("0.10.0").unwrap() > Version::parse("0.9.999").unwrap());
        assert!(Version::parse("1.0.0").unwrap() > Version::parse("0.99.999").unwrap());
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "1.2.1000",
            "a.b.c",
            "1.2.3-beta",
            "1..3",
            "v",
            "1.2.-3",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_current_version_is_ours() {
        let _ = Version::current();
    }

    #[test]
    fn picks_the_installer_and_its_signature() {
        let json = release_json(
            "v0.2.005",
            "",
            &["notes.txt", SETUP, &format!("{SETUP}.sig")],
        );
        let r = parse_release(&json).unwrap().unwrap();
        assert_eq!(r.version.to_string(), "0.2.005");
        assert_eq!(r.setup.name, SETUP);
        assert_eq!(r.signature.name, format!("{SETUP}.sig"));
        assert_eq!(r.notes, "- thing");
        assert!(r.setup.url.contains("/assets/1"));
    }

    #[test]
    fn releases_that_are_not_for_us_are_skipped() {
        // no signature
        assert!(parse_release(&release_json("v0.2.5", "", &[SETUP]))
            .unwrap()
            .is_none());
        // no installer
        assert!(parse_release(&release_json("v0.2.5", "", &["a.zip"]))
            .unwrap()
            .is_none());
        // a draft and a pre-release
        let sig = format!("{SETUP}.sig");
        assert!(
            parse_release(&release_json("v0.2.5", r#","draft":true"#, &[SETUP, &sig]))
                .unwrap()
                .is_none()
        );
        assert!(parse_release(&release_json(
            "v0.2.5",
            r#","prerelease":true"#,
            &[SETUP, &sig]
        ))
        .unwrap()
        .is_none());
        // a tag that is no version is ignored, not fatal: otherwise one such release would stop
        // automatic updates for good
        assert!(parse_release(&release_json("nightly", "", &[SETUP, &sig]))
            .unwrap()
            .is_none());
        assert!(parse_release(&release_json("latest", "", &[SETUP, &sig]))
            .unwrap()
            .is_none());
        assert!(parse_release("not json").is_err());
    }

    #[test]
    fn the_installer_matching_the_tag_is_preferred() {
        let other = "BebraLauncher_0.1.0_x86-setup.exe";
        let json = release_json(
            "v0.2.005",
            "",
            &[
                "BebraLauncher_9.9.9_x64-setup.exe",
                "BebraLauncher_9.9.9_x64-setup.exe.sig",
                other,
                &format!("{other}.sig"),
                SETUP,
                &format!("{SETUP}.sig"),
            ],
        );
        let r = parse_release(&json).unwrap().unwrap();
        assert_eq!(
            r.setup.name, SETUP,
            "same version as the tag wins over a higher one"
        );

        // No candidate matches the tag: the highest version with a signature is used, and an
        // installer without one is ignored.
        let json = release_json(
            "v0.2.005",
            "",
            &[
                "BebraLauncher_0.1.0_x64-setup.exe",
                "BebraLauncher_0.3.0_x64-setup.exe.sig",
                "BebraLauncher_0.3.0_x64-setup.exe",
                "BebraLauncher_0.3.0_x64-setup.exe.sig",
            ],
        );
        let r = parse_release(&json).unwrap().unwrap();
        assert_eq!(r.setup.name, "BebraLauncher_0.3.0_x64-setup.exe");
    }

    #[test]
    fn the_token_goes_only_to_github() {
        assert!(is_token_host("https://api.github.com/repos/o/r/releases"));
        assert!(is_token_host("https://github.com/o/r"));
        for bad in [
            "http://api.github.com/x",
            "https://api.github.com.evil.example/x",
            "https://evil.example/?api.github.com",
            "https://api.github.com@evil.example/x",
            "https://user@api.github.com/x",
            "https://api.github.com:8443/x",
            "ftp://api.github.com/x",
            "",
        ] {
            assert!(!is_token_host(bad), "{bad:?} must not receive the token");
        }
    }

    #[test]
    fn installer_names_are_plain() {
        assert!(is_setup_name(SETUP));
        for bad in [
            "setup.exe",
            "../a-setup.exe",
            "a b-setup.exe",
            "a\\b-setup.exe",
            ".x-setup.exe",
            "a-setup.exe.sig",
            "a/b-setup.exe",
        ] {
            assert!(!is_setup_name(bad), "{bad:?}");
        }
        // a hostile release naming its installer with a path is not used
        let json = release_json(
            "v0.2.5",
            "",
            &["..\\\\evil-setup.exe", "..\\\\evil-setup.exe.sig"],
        );
        assert!(parse_release(&json).unwrap().is_none());
    }

    #[test]
    fn repository_names_are_checked() {
        assert!(valid_repo("Bebranot/vabecide-musor"));
        assert!(valid_repo(DEFAULT_REPO));
        for bad in [
            "", "a", "a/b/c", "a/", "/b", "a b/c", "a/b?x=1", "a/b#c", "../x", "a/..%2f",
        ] {
            assert!(!valid_repo(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_built_in_key_parses() {
        update_key().unwrap();
    }

    #[test]
    fn signatures_bind_version_and_file() {
        let sk = SigningKey::from_bytes(&[9u8; 32]);
        let vk = sk.verifying_key();
        let v = Version::parse("0.2.5").unwrap();
        let sig = sign_update(&sk, &v, "ABCDEF0123");
        verify_update(&vk, &v, "abcdef0123", &sig).unwrap();
        verify_update(&vk, &v, "abcdef0123", &format!(" {sig}\r\n")).unwrap();
        // another file, another version, another key: all refused
        assert!(verify_update(&vk, &v, "abcdef0124", &sig).is_err());
        assert!(verify_update(&vk, &Version::parse("0.2.6").unwrap(), "abcdef0123", &sig).is_err());
        let other = SigningKey::from_bytes(&[10u8; 32]).verifying_key();
        assert!(verify_update(&other, &v, "abcdef0123", &sig).is_err());
    }

    #[test]
    fn signing_keys_come_from_hex() {
        let hex_seed = "07".repeat(32);
        assert_eq!(
            signing_key_from_hex(&format!("{hex_seed}\n"))
                .unwrap()
                .to_bytes(),
            [7u8; 32]
        );
        assert!(signing_key_from_hex("zz").is_err());
        assert!(signing_key_from_hex("0707").is_err());
    }

    #[test]
    fn sha256_of_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(
            sha256_file(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn old_installers_are_cleaned_but_not_the_kept_one() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("A_0.1.0_x64-setup.exe");
        let part = dir.path().join("B-setup.exe.part");
        let keep = dir.path().join("C_0.2.0_x64-setup.exe");
        let other = dir.path().join("notes.txt");
        for p in [&old, &part, &keep, &other] {
            std::fs::write(p, b"x").unwrap();
        }
        clean_old(dir.path(), Some(&keep));
        assert!(!old.exists() && !part.exists());
        assert!(keep.exists() && other.exists());
    }

    /// A tiny HTTP server on localhost: `/sig` and `/setup` answer with what the test gives.
    fn serve(routes: Vec<(&'static str, Vec<u8>)>) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 2048];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = request.split_whitespace().nth(1).unwrap_or("").to_string();
                let body = routes
                    .iter()
                    .find(|(p, _)| *p == path)
                    .map(|(_, b)| b.clone());
                let head = match &body {
                    Some(b) => format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        b.len()
                    ),
                    None => {
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_string()
                    }
                };
                let _ = stream.write_all(head.as_bytes());
                if let Some(b) = body {
                    let _ = stream.write_all(&b);
                }
            }
        });
        base
    }

    fn release_at(base: &str, version: &str) -> Release {
        Release {
            version: Version::parse(version).unwrap(),
            tag: format!("v{version}"),
            notes: String::new(),
            published: None,
            page_url: String::new(),
            setup: Asset {
                name: SETUP.into(),
                url: format!("{base}/setup"),
                size: 0,
            },
            signature: Asset {
                name: format!("{SETUP}.sig"),
                url: format!("{base}/sig"),
                size: 0,
            },
        }
    }

    #[tokio::test]
    async fn a_signed_installer_downloads_and_a_tampered_one_does_not() {
        let sk = SigningKey::from_bytes(&[3u8; 32]);
        let key = sk.verifying_key();
        let installer = vec![42u8; 300_000];
        let sha = hex::encode(Sha256::digest(&installer));
        let version = Version::parse("0.2.5").unwrap();
        let sig = sign_update(&sk, &version, &sha);

        // honest
        let base = serve(vec![
            ("/setup", installer.clone()),
            ("/sig", format!("{sig}\n").into_bytes()),
        ]);
        let client = Client::new();
        let dir = tempfile::tempdir().unwrap();
        let mut last = (0, 0);
        let path = download(
            &client,
            &release_at(&base, "0.2.5"),
            None,
            dir.path(),
            &key,
            |d, t| last = (d, t),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), installer);
        assert_eq!(last.0, 300_000);
        assert!(!dir.path().join(format!("{SETUP}.part")).exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join(format!("{SETUP}.sig")))
                .unwrap()
                .trim(),
            sig,
            "the signature is kept next to the installer for the final check before spawn"
        );

        // asked again: the verified file on disk is reused (nothing to fetch for the installer)
        let again = serve(vec![("/sig", format!("{sig}\n").into_bytes())]);
        download(
            &client,
            &release_at(&again, "0.2.5"),
            None,
            dir.path(),
            &key,
            |_, _| {},
        )
        .await
        .unwrap();

        // the installer was swapped after signing
        let mut evil = installer.clone();
        evil[0] ^= 1;
        let base = serve(vec![("/setup", evil), ("/sig", sig.clone().into_bytes())]);
        let dir2 = tempfile::tempdir().unwrap();
        let err = download(
            &client,
            &release_at(&base, "0.2.5"),
            None,
            dir2.path(),
            &key,
            |_, _| {},
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("signature"), "{err}");
        // nothing is left behind, not even the partial file
        assert_eq!(std::fs::read_dir(dir2.path()).unwrap().count(), 0);

        // a genuine old installer offered as a newer version
        let base = serve(vec![
            ("/setup", installer.clone()),
            ("/sig", sig.into_bytes()),
        ]);
        let dir3 = tempfile::tempdir().unwrap();
        assert!(download(
            &client,
            &release_at(&base, "0.2.9"),
            None,
            dir3.path(),
            &key,
            |_, _| {}
        )
        .await
        .is_err());

        // no signature file at all
        let base = serve(vec![("/setup", installer)]);
        let dir4 = tempfile::tempdir().unwrap();
        assert!(download(
            &client,
            &release_at(&base, "0.2.5"),
            None,
            dir4.path(),
            &key,
            |_, _| {}
        )
        .await
        .is_err());
    }

    #[test]
    fn installers_of_installed_versions_are_cleaned() {
        let dir = tempfile::tempdir().unwrap();
        let current = Version::current();
        let older = "BebraLauncher_0.0.1_x64-setup.exe".to_string();
        let same = format!("BebraLauncher_{}_x64-setup.exe", env!("CARGO_PKG_VERSION"));
        let newer = format!(
            "BebraLauncher_{}.{}.{}_x64-setup.exe",
            current.major + 1,
            0,
            0
        );
        let part = "BebraLauncher_9.9.9_x64-setup.exe.part".to_string();
        for n in [&older, &same, &newer, &part] {
            std::fs::write(dir.path().join(n), b"x").unwrap();
        }
        std::fs::write(dir.path().join(format!("{older}.sig")), b"sig").unwrap();
        // Not ours: same shape, but another product's installer.
        std::fs::write(dir.path().join("Other_0.0.1_x64-setup.exe"), b"x").unwrap();
        clean_installed(dir.path());
        assert!(!dir.path().join(&older).exists());
        assert!(
            !dir.path().join(format!("{older}.sig")).exists(),
            "the signature of a removed installer goes too"
        );
        assert!(!dir.path().join(same).exists());
        assert!(!dir.path().join(part).exists());
        assert!(
            dir.path().join(newer).exists(),
            "a newer installer is waiting to be installed"
        );
        assert!(
            dir.path().join("Other_0.0.1_x64-setup.exe").exists(),
            "another program's installer is not touched"
        );
    }

    #[test]
    fn refusals_are_explained() {
        let text = |e: Error| e.to_string();
        assert!(text(explain(StatusCode::NOT_FOUND, false)).contains("private"));
        assert!(text(explain(StatusCode::NOT_FOUND, true)).contains("token"));
        assert!(text(explain(StatusCode::UNAUTHORIZED, true)).contains("token"));
        assert!(text(explain(StatusCode::FORBIDDEN, true)).contains("rate limit"));
        assert!(text(explain(StatusCode::BAD_GATEWAY, false)).contains("502"));
    }

    #[test]
    fn the_installer_is_not_started_from_a_bad_path() {
        assert!(spawn_installer(Path::new("C:\\no\\such-setup.exe"), false).is_err());
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("not-an-installer.txt");
        std::fs::write(&p, b"x").unwrap();
        assert!(spawn_installer(&p, false).is_err());
    }

    #[test]
    fn an_installer_changed_after_the_check_is_not_started() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(SETUP);
        std::fs::write(&p, b"installer bytes").unwrap();

        // No signature next to it: nothing can be verified, so nothing runs.
        let err = spawn_installer(&p, false).unwrap_err();
        assert!(err.to_string().contains("signature"), "{err}");

        // A signature that does not cover these bytes (signed with a key that is not ours).
        let sk = SigningKey::from_bytes(&[5u8; 32]);
        let sha = sha256_file(&p).unwrap();
        let version = setup_version(SETUP).unwrap();
        std::fs::write(sig_sidecar(&p), sign_update(&sk, &version, &sha)).unwrap();
        let err = spawn_installer(&p, false).unwrap_err();
        assert!(err.to_string().contains("changed"), "{err}");
    }
}
