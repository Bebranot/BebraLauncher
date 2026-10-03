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
use reqwest::header::{ACCEPT, AUTHORIZATION};
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
const API: &str = "https://api.github.com";

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

/// Reads GitHub's answer for one release. `Ok(None)`: a draft, a pre-release, or one without our
/// installer and its signature (not for us). `Err`: the tag is not a version of ours.
pub fn parse_release(json: &str) -> Result<Option<Release>> {
    let raw: RawRelease = serde_json::from_str(json)
        .map_err(|e| Error::Other(format!("cannot read the release: {e}")))?;
    if raw.draft || raw.prerelease {
        return Ok(None);
    }
    let version = Version::parse(&raw.tag_name).ok_or_else(|| {
        Error::Other(format!(
            "the release tag {:?} is not a version",
            raw.tag_name
        ))
    })?;

    let find = |name: &str| {
        raw.assets.iter().find(|a| a.name == name).map(|a| Asset {
            name: a.name.clone(),
            url: a.url.clone(),
            size: a.size,
        })
    };
    let Some(setup_name) = raw
        .assets
        .iter()
        .map(|a| a.name.as_str())
        .find(|n| is_setup_name(n))
    else {
        return Ok(None);
    };
    let (Some(setup), Some(signature)) = (find(setup_name), find(&format!("{setup_name}.sig")))
    else {
        return Ok(None);
    };
    Ok(Some(Release {
        version,
        tag: raw.tag_name,
        notes: raw.body.unwrap_or_default(),
        published: raw.published_at,
        page_url: raw.html_url,
        setup,
        signature,
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

fn request(
    client: &Client,
    url: &str,
    accept: &str,
    token: Option<&Token>,
) -> reqwest::RequestBuilder {
    let mut req = client
        .get(url)
        .header(ACCEPT, accept)
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = token {
        req = req.header(AUTHORIZATION, format!("Bearer {}", token.expose()));
    }
    req
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
    let resp = request(client, &url, "application/vnd.github+json", token)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await?;
    let text = ok(resp, token.is_some())?.text().await?;
    parse_release(&text)
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
    let resp = request(
        client,
        &release.signature.url,
        "application/octet-stream",
        token,
    )
    .timeout(std::time::Duration::from_secs(30))
    .send()
    .await?;
    let sig_bytes = ok(resp, token.is_some())?.bytes().await?;
    if sig_bytes.len() > MAX_SIGNATURE {
        return Err(Error::Other("the signature file is too large".into()));
    }
    let sig_hex = String::from_utf8_lossy(&sig_bytes).trim().to_string();

    let part = dir.join(format!("{}.part", release.setup.name));
    let fin = dir.join(&release.setup.name);

    // Downloaded before and never installed: the same file, if it still checks out, is reused.
    if let Ok(sha) = sha256_file(&fin) {
        if verify_update(key, &release.version, &sha, &sig_hex).is_ok() {
            progress(release.setup.size, release.setup.size);
            return Ok(fin);
        }
    }
    let result = fetch_checked(client, release, token, &part, key, &sig_hex, &mut progress).await;
    match result {
        Ok(()) => {
            let _ = tokio::fs::remove_file(&fin).await;
            tokio::fs::rename(&part, &fin).await?;
            Ok(fin)
        }
        Err(e) => {
            let _ = tokio::fs::remove_file(&part).await;
            Err(e)
        }
    }
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
    let resp = request(
        client,
        &release.setup.url,
        "application/octet-stream",
        token,
    )
    .send()
    .await?;
    let resp = ok(resp, token.is_some())?;
    let total = resp.content_length().unwrap_or(release.setup.size);
    let mut file = tokio::fs::File::create(part).await?;
    let mut hasher = Sha256::new();
    let mut done = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
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

/// Deletes installers left in `dir` by earlier updates, except `keep`.
pub fn clean_old(dir: &Path, keep: Option<&Path>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if (name.ends_with("-setup.exe") || name.ends_with(".part")) && Some(path.as_path()) != keep
        {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Deletes installers of this version or older: they are installed already (or beaten).
pub fn clean_installed(dir: &Path) {
    let current = Version::current();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // `BebraLauncher_0.2.5_x64-setup.exe`: the version sits between the first two `_`.
        let version = name.split('_').nth(1).and_then(Version::parse);
        let leftover = name.ends_with(".part") || version.is_some_and(|v| v <= current);
        if leftover && (name.ends_with("-setup.exe") || name.ends_with(".part")) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

// ---------------------------------------------------------------- installing

/// Runs the installer silently over the current installation. With `restart` it starts the
/// launcher again when done. The caller exits right after: the installer replaces the files.
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
        // a tag that is no version is an error, not a silent skip
        assert!(parse_release(&release_json("nightly", "", &[SETUP, &sig])).is_err());
        assert!(parse_release("not json").is_err());
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
        clean_installed(dir.path());
        assert!(!dir.path().join(older).exists());
        assert!(!dir.path().join(same).exists());
        assert!(!dir.path().join(part).exists());
        assert!(
            dir.path().join(newer).exists(),
            "a newer installer is waiting to be installed"
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
}
