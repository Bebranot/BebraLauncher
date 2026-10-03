//! Hashes used by the content system (docs/protocol.md, sections 3-5).
//!
//! - file blobs and manifests: BLAKE2b with a 32-byte digest and no key
//! - whole client zip: SHA-256
//!
//! Hex is written in uppercase (as servers do) and read in either case.

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use sha2::Sha256;

use crate::{Error, Result};

pub type Hash256 = [u8; 32];

pub fn blake2b256(data: &[u8]) -> Hash256 {
    Blake2b::<U32>::digest(data).into()
}

pub fn sha256(data: &[u8]) -> Hash256 {
    Sha256::digest(data).into()
}

pub fn to_hex(hash: &[u8]) -> String {
    hex::encode_upper(hash)
}

/// Exactly 64 hex characters, any case.
pub fn parse_hash_hex(s: &str) -> Result<Hash256> {
    let mut out = [0u8; 32];
    if s.len() != 64 {
        return Err(Error::InvalidHash(format!(
            "expected 64 hex chars, got {}",
            s.len()
        )));
    }
    hex::decode_to_slice(s, &mut out).map_err(|e| Error::InvalidHash(format!("{e}: {s:?}")))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference values computed with Python's hashlib.
    #[test]
    fn blake2b256_matches_reference() {
        assert_eq!(
            to_hex(&blake2b256(b"hello")),
            "324DCF027DD4A30A932C441F365A25E86B173DEFA4B8E58948253471B81B72CF"
        );
        assert_eq!(
            to_hex(&blake2b256(b"")),
            "0E5751C026E543B2E8AB2EB06099DAA1D1E5DF47778F7787FAAB45CDF12FE3A8"
        );
    }

    #[test]
    fn sha256_matches_reference() {
        assert_eq!(
            to_hex(&sha256(b"abc")),
            "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD"
        );
    }

    #[test]
    fn hex_roundtrip_any_case() {
        let h = blake2b256(b"hello");
        let upper = to_hex(&h);
        assert_eq!(parse_hash_hex(&upper).unwrap(), h);
        assert_eq!(parse_hash_hex(&upper.to_lowercase()).unwrap(), h);
    }

    #[test]
    fn hex_rejects_bad_input() {
        assert!(parse_hash_hex("").is_err());
        assert!(parse_hash_hex("ABCD").is_err());
        assert!(parse_hash_hex(&"Z".repeat(64)).is_err());
        assert!(parse_hash_hex(&"A".repeat(66)).is_err());
    }
}
