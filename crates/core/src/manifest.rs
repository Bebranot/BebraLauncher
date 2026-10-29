//! Content manifest: the file list of a build (docs/protocol.md, sections 3.1 and 5.3).
//!
//! ```text
//! Robust Content Manifest 1
//! <HEX64 BLAKE2b-256 of the file> <path>
//! ...
//! ```

use crate::hash::{blake2b256, parse_hash_hex, to_hex, Hash256};
use crate::{Error, Result};

pub const HEADER: &str = "Robust Content Manifest 1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    pub hash: Hash256,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct Manifest {
    pub entries: Vec<ManifestEntry>,
    /// BLAKE2b-256 of the manifest text exactly as received.
    pub hash: Hash256,
}

impl Manifest {
    /// Parses the (already decompressed) manifest text. Does not check the hash against anything:
    /// call [`Manifest::verify_hash`] with the value from `/info`.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let text = std::str::from_utf8(data)
            .map_err(|e| Error::InvalidManifest(format!("not valid UTF-8: {e}")))?;

        let mut lines = text
            .split_terminator('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l));

        if lines.next() != Some(HEADER) {
            return Err(Error::InvalidManifest("unknown manifest header".into()));
        }

        let mut entries = Vec::new();
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for (i, line) in lines.enumerate() {
            let line_no = i + 2;
            // The first space separates hash and path; paths may contain spaces.
            let (hash, path) = line
                .split_once(' ')
                .ok_or_else(|| Error::InvalidManifest(format!("line {line_no}: no separator")))?;
            let hash = parse_hash_hex(hash)
                .map_err(|e| Error::InvalidManifest(format!("line {line_no}: {e}")))?;
            check_path(path, line_no)?;
            if !seen.insert(path) {
                return Err(Error::InvalidManifest(format!(
                    "line {line_no}: the path {path:?} appears twice"
                )));
            }
            entries.push(ManifestEntry {
                hash,
                path: path.to_string(),
            });
        }

        Ok(Self {
            entries,
            hash: blake2b256(data),
        })
    }

    /// Compares the manifest hash with the hex string the server promised (any case).
    pub fn verify_hash(&self, expected_hex: &str) -> Result<()> {
        let expected = parse_hash_hex(expected_hex)?;
        if expected == self.hash {
            Ok(())
        } else {
            Err(Error::HashMismatch {
                expected: to_hex(&expected),
                actual: to_hex(&self.hash),
            })
        }
    }
}

/// A path in the manifest ends up in `ContentManifest` and, through the Loader, on disk. Only
/// plain relative `/`-separated paths are accepted: no `..`, no drive letters, no backslashes,
/// no NUL or control characters and no directory entries.
fn check_path(path: &str, line_no: usize) -> Result<()> {
    let why = if path.is_empty() {
        Some("empty path")
    } else if path.ends_with('/') {
        Some("path is a directory")
    } else if path.starts_with('/') {
        Some("absolute path")
    } else if path.contains('\\') {
        Some("backslash in path")
    } else if path.contains(':') {
        Some("drive letter or stream in path")
    } else if path.chars().any(char::is_control) {
        Some("control character in path")
    } else if path
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        Some("path escapes its folder")
    } else {
        None
    };
    match why {
        None => Ok(()),
        Some(why) => Err(Error::InvalidManifest(format!(
            "line {line_no}: {why}: {path:?}"
        ))),
    }
}

/// The manifest hash computed from a list of files. `entries` MUST already be sorted by path
/// (bytewise on UTF-8) and without duplicates, which is how the content DB hands them out.
pub fn compute_hash<'a>(entries: impl IntoIterator<Item = (&'a Hash256, &'a str)>) -> Hash256 {
    let mut text = String::from(HEADER);
    text.push('\n');
    let mut previous: Option<&str> = None;
    for (hash, path) in entries {
        debug_assert!(
            previous.is_none_or(|p| p < path),
            "compute_hash needs entries sorted by path without duplicates, got {path:?} after {previous:?}"
        );
        previous = Some(path);
        text.push_str(&to_hex(hash));
        text.push(' ');
        text.push_str(path);
        text.push('\n');
    }
    blake2b256(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference values computed with Python's hashlib (blake2b, digest_size=32).
    pub const SAMPLE: &str = "Robust Content Manifest 1\n\
        324DCF027DD4A30A932C441F365A25E86B173DEFA4B8E58948253471B81B72CF Assemblies/Content.Client.dll\n\
        0E5751C026E543B2E8AB2EB06099DAA1D1E5DF47778F7787FAAB45CDF12FE3A8 Resources/my file.txt\n\
        9A3440C9D1529B122FACEEF33739B6E814616658D53FAAF6E4F129FB20EDFB13 zeta.bin\n";
    pub const SAMPLE_HASH: &str =
        "F141EEE7856A6BD5F897B576ACF205348F790B3B31CBB24F47D21F114A4CBCD0";

    #[test]
    fn parses_entries_and_hashes_text() {
        let m = Manifest::parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(m.entries.len(), 3);
        assert_eq!(m.entries[0].path, "Assemblies/Content.Client.dll");
        assert_eq!(
            m.entries[1].path, "Resources/my file.txt",
            "paths may contain spaces"
        );
        assert_eq!(to_hex(&m.hash), SAMPLE_HASH);
        m.verify_hash(SAMPLE_HASH).unwrap();
        m.verify_hash(&SAMPLE_HASH.to_lowercase()).unwrap();
    }

    #[test]
    fn verify_detects_wrong_hash() {
        let m = Manifest::parse(SAMPLE.as_bytes()).unwrap();
        let wrong = "0".repeat(64);
        assert!(matches!(
            m.verify_hash(&wrong),
            Err(Error::HashMismatch { .. })
        ));
        assert!(matches!(
            m.verify_hash("nothex"),
            Err(Error::InvalidHash(_))
        ));
    }

    #[test]
    fn computed_hash_matches_text_hash() {
        let m = Manifest::parse(SAMPLE.as_bytes()).unwrap();
        let again = compute_hash(m.entries.iter().map(|e| (&e.hash, e.path.as_str())));
        assert_eq!(again, m.hash);
    }

    #[test]
    fn any_change_changes_the_hash() {
        let a = Manifest::parse(SAMPLE.as_bytes()).unwrap();
        let b = Manifest::parse(SAMPLE.replace("zeta.bin", "zeta.bi").as_bytes()).unwrap();
        assert_ne!(a.hash, b.hash);
    }

    #[test]
    fn header_only_is_an_empty_manifest() {
        let m = Manifest::parse(b"Robust Content Manifest 1\n").unwrap();
        assert!(m.entries.is_empty());
    }

    #[test]
    fn rejects_wrong_header() {
        for bad in ["", "Robust Content Manifest 2\n", "hello\n", "\n"] {
            assert!(Manifest::parse(bad.as_bytes()).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn rejects_malformed_lines() {
        let h = "324DCF027DD4A30A932C441F365A25E86B173DEFA4B8E58948253471B81B72CF";
        for body in [
            "nospace".to_string(),
            h.to_string(),
            format!("{h} "),
            "ZZ path".to_string(),
            format!("{} path", &h[..62]),
            String::new(),
        ] {
            let text = format!("{HEADER}\n{body}\n");
            assert!(Manifest::parse(text.as_bytes()).is_err(), "{body:?}");
        }
    }

    #[test]
    fn rejects_invalid_utf8() {
        assert!(Manifest::parse(&[0xff, 0xfe, 0x00]).is_err());
    }

    #[test]
    fn unsafe_and_duplicate_paths_are_refused() {
        let h = "324DCF027DD4A30A932C441F365A25E86B173DEFA4B8E58948253471B81B72CF";
        for bad in [
            "../evil.txt",
            "a/../../evil.txt",
            "a//b.txt",
            "./a.txt",
            "/abs.txt",
            "C:evil.txt",
            "a\\b.txt",
            "dir/",
            "nul\0.txt",
            "bell\u{7}.txt",
        ] {
            let text = format!("{HEADER}\n{h} {bad}\n");
            let err = Manifest::parse(text.as_bytes()).unwrap_err();
            assert!(
                err.to_string().contains("line 2"),
                "{bad:?} must be refused with its line: {err}"
            );
        }

        let twice = format!("{HEADER}\n{h} a.txt\n{h} a.txt\n");
        let err = Manifest::parse(twice.as_bytes()).unwrap_err();
        assert!(err.to_string().contains("twice"), "{err}");
        assert!(err.to_string().contains("line 3"), "{err}");
    }

    #[test]
    fn missing_final_newline_is_tolerated() {
        let text = SAMPLE.trim_end_matches('\n');
        assert_eq!(Manifest::parse(text.as_bytes()).unwrap().entries.len(), 3);
    }
}
