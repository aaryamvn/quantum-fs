//! Human-typable six-character join codes. The wire protocol keeps using the
//! 16-byte [`JoinCode`]; a short code is only a typing surface that derives
//! into one deterministically, so nothing else in admission changes.

use sha2::{Digest, Sha256};

use crate::{keystore::random_bytes, net::JoinCode, Error, Result};

pub const SHORT_LEN: usize = 6;

/// Same alphabet as the Base32 join-code text: no 0/1/8/9 to mistype.
const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const DOMAIN: &[u8] = b"qfs/v1/short-code/";

/// Six alphabet characters from OS randomness. The alphabet is a power of two,
/// so masking each byte is unbiased.
pub fn generate() -> Result<String> {
    let bytes: [u8; SHORT_LEN] = random_bytes()?;
    Ok(bytes
        .iter()
        .map(|byte| char::from(ALPHABET[usize::from(byte & 0x1f)]))
        .collect())
}

/// Canonical form of what a human typed, or `None` when it is not a short code.
pub fn normalize(value: &str) -> Option<String> {
    let upper = value.trim().to_uppercase();
    if upper.len() != SHORT_LEN || !upper.bytes().all(|byte| ALPHABET.contains(&byte)) {
        return None;
    }
    Some(upper)
}

/// The join code a short code stands for: the first 16 bytes of a domain
/// separated SHA-256 over its canonical ASCII.
pub fn derive(short: &str) -> Result<JoinCode> {
    let normalized = normalize(short).ok_or(Error::InvalidInput(
        "short code must be 6 characters from A-Z2-7",
    ))?;
    let mut hash = Sha256::new();
    hash.update(DOMAIN);
    hash.update(normalized.as_bytes());
    let digest: [u8; 32] = hash.finalize().into();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Ok(JoinCode(bytes))
}

#[cfg(test)]
mod tests {
    use super::{derive, generate, normalize, ALPHABET, SHORT_LEN};
    use crate::Result;

    #[test]
    fn derivation_is_deterministic_and_normalizing() -> Result<()> {
        assert_eq!(derive("ABC234")?, derive("ABC234")?);
        assert_eq!(derive("ABC234")?, derive("  abc234 ")?);
        assert_ne!(derive("ABC234")?, derive("ABC235")?);
        Ok(())
    }

    #[test]
    fn normalize_accepts_only_six_alphabet_characters() {
        assert_eq!(normalize("abc234").as_deref(), Some("ABC234"));
        assert_eq!(normalize("\tZZ7777\n").as_deref(), Some("ZZ7777"));
        assert!(normalize("ABC23").is_none());
        assert!(normalize("ABC2345").is_none());
        assert!(normalize("ABC201").is_none());
        assert!(normalize("ABC-23").is_none());
        assert!(normalize("ÄBC234").is_none());
        assert!(derive("ABC201").is_err());
    }

    #[test]
    fn generated_codes_are_canonical() -> Result<()> {
        for _ in 0..32 {
            let short = generate()?;
            assert_eq!(short.len(), SHORT_LEN);
            assert!(short.bytes().all(|byte| ALPHABET.contains(&byte)));
            assert_eq!(normalize(&short).as_deref(), Some(short.as_str()));
            derive(&short)?;
        }
        Ok(())
    }
}
