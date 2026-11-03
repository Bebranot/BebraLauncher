//! Override for the hardware ID the game client sends to servers.
//!
//! The engine computes the HWID from the machine (`Robust.Client.HWId.BasicHWId`), and a
//! `Bebra.HwidPatch` client patch replaces the result with the value passed in these variables.
//! The format matches what the engine expects: the legacy value is 32 bytes, the modern one is
//! 33 bytes (a leading zero, then the same 32 bytes), both as lower-case hex without separators.

/// The number of random bytes in a HWID (before the modern leading zero).
pub const HWID_BYTES: usize = 32;

/// A generated HWID override, ready to be passed to the client patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HwidOverride {
    /// Hex of the 32 bytes (64 characters).
    pub legacy: String,
    /// Hex of the same 32 bytes behind a leading zero byte (66 characters).
    pub modern: String,
}

impl HwidOverride {
    /// Generates a new random HWID.
    pub fn random() -> Self {
        // Two UUIDs are 32 random bytes; uuid v4 is a well-tested source of OS randomness, and
        // the crate is already a dependency. Anything that depends on the OS RNG failing would
        // be a system-level problem, not a launcher one.
        let mut bytes = [0u8; HWID_BYTES];
        bytes[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        bytes[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        Self::from_bytes(&bytes)
    }

    /// Builds the override from exactly 32 bytes.
    pub fn from_bytes(bytes: &[u8; HWID_BYTES]) -> Self {
        let legacy = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let modern = format!("00{legacy}");
        Self { legacy, modern }
    }

    /// Parses a stored legacy hex string (64 characters of hex). Returns `None` if it is not
    /// exactly that: a stored value that cannot be used must not reach the client.
    pub fn from_legacy_hex(hex: &str) -> Option<Self> {
        let clean = hex.trim();
        if clean.len() != HWID_BYTES * 2 || !clean.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let mut bytes = [0u8; HWID_BYTES];
        for (i, chunk) in clean.as_bytes().chunks(2).enumerate() {
            let hi = (chunk[0] as char).to_digit(16)? as u8;
            let lo = (chunk[1] as char).to_digit(16)? as u8;
            bytes[i] = (hi << 4) | lo;
        }
        Some(Self::from_bytes(&bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_values_have_the_expected_shape() {
        let a = HwidOverride::random();
        let b = HwidOverride::random();
        assert_eq!(a.legacy.len(), 64);
        assert_eq!(a.modern.len(), 66);
        assert!(a.legacy.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(a.modern.starts_with("00"));
        assert_ne!(a.legacy, b.legacy, "two draws should differ");
    }

    #[test]
    fn a_legacy_string_round_trips() {
        let original = HwidOverride::random();
        let parsed = HwidOverride::from_legacy_hex(&original.legacy).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn invalid_hex_is_rejected() {
        assert!(HwidOverride::from_legacy_hex("").is_none());
        assert!(HwidOverride::from_legacy_hex("zz").is_none());
        assert!(HwidOverride::from_legacy_hex(&"a".repeat(63)).is_none());
        assert!(HwidOverride::from_legacy_hex(&"g".repeat(64)).is_none());
    }
}
