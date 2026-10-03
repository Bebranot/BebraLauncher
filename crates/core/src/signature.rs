//! Ed25519 signatures of engine and module builds (docs/protocol.md, sections 6 and 7.5).
//!
//! Wizards' Den signs every build with one key; `SS14.Loader` re-checks the engine signature on
//! each start. We check too, as soon as a file is downloaded, so a bad file never sits on disk.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};

use crate::{Error, Result};

/// The public key from `SS14.Launcher/signing_key` (identical in the upstream repository).
const OFFICIAL_KEY_PEM: &str = include_str!("signing_key.pem");

/// DER prefix of an Ed25519 SubjectPublicKeyInfo; the 32 key bytes follow.
const SPKI_PREFIX: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];

pub fn official_key() -> Result<VerifyingKey> {
    parse_pem(OFFICIAL_KEY_PEM)
}

/// Parses a PEM `PUBLIC KEY` block holding an Ed25519 key.
pub fn parse_pem(pem: &str) -> Result<VerifyingKey> {
    let bad = |what: &str| Error::Other(format!("bad signing key: {what}"));

    let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    let der = STANDARD
        .decode(body.trim())
        .map_err(|e| bad(&format!("base64: {e}")))?;
    if der.len() != SPKI_PREFIX.len() + 32 || der[..SPKI_PREFIX.len()] != SPKI_PREFIX {
        return Err(bad("not an Ed25519 public key"));
    }
    let raw: [u8; 32] = der[SPKI_PREFIX.len()..]
        .try_into()
        .expect("length checked above");
    VerifyingKey::from_bytes(&raw).map_err(|e| bad(&e.to_string()))
}

/// Checks `sig_hex` (hex, 64 bytes) over the whole of `data`.
pub fn verify(key: &VerifyingKey, data: &[u8], sig_hex: &str) -> Result<()> {
    let bytes =
        hex::decode(sig_hex).map_err(|e| Error::Other(format!("bad signature hex: {e}")))?;
    let sig =
        Signature::from_slice(&bytes).map_err(|e| Error::Other(format!("bad signature: {e}")))?;
    key.verify(data, &sig)
        .map_err(|_| Error::Other("signature verification failed".into()))
}

#[cfg(test)]
pub(crate) mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    pub fn test_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    pub fn pem_of(key: &VerifyingKey) -> String {
        let mut der = SPKI_PREFIX.to_vec();
        der.extend(key.as_bytes());
        format!(
            "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
            STANDARD.encode(der)
        )
    }

    pub fn sign_hex(key: &SigningKey, data: &[u8]) -> String {
        hex::encode_upper(key.sign(data).to_bytes())
    }

    #[test]
    fn official_key_parses() {
        assert!(official_key().is_ok());
    }

    #[test]
    fn pem_roundtrip_and_verify() {
        let sk = test_key();
        let vk = parse_pem(&pem_of(&sk.verifying_key())).unwrap();
        assert_eq!(vk, sk.verifying_key());

        let sig = sign_hex(&sk, b"engine zip bytes");
        verify(&vk, b"engine zip bytes", &sig).unwrap();
        verify(&vk, b"engine zip bytes", &sig.to_lowercase()).unwrap();
    }

    #[test]
    fn tampered_data_or_signature_fails() {
        let sk = test_key();
        let vk = sk.verifying_key();
        let sig = sign_hex(&sk, b"data");
        assert!(verify(&vk, b"data!", &sig).is_err());

        let other = sign_hex(&SigningKey::from_bytes(&[8u8; 32]), b"data");
        assert!(verify(&vk, b"data", &other).is_err());
        assert!(verify(&vk, b"data", "nothex").is_err());
        assert!(verify(&vk, b"data", "ABCD").is_err(), "too short");
    }

    #[test]
    fn rejects_non_ed25519_pem() {
        assert!(parse_pem("").is_err());
        assert!(parse_pem("-----BEGIN PUBLIC KEY-----\nAAAA\n-----END PUBLIC KEY-----").is_err());
        assert!(parse_pem("not base64 at all!!").is_err());
    }
}
