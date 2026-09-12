use core::fmt;
use std::str::FromStr;

use crate::{keystore::random_bytes, Error, Result};

pub mod directory;
pub mod frame;
pub mod join;
pub mod locate;
pub mod session;

const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JoinCode(pub [u8; 16]);

impl JoinCode {
    pub fn generate() -> Result<Self> {
        Ok(Self(random_bytes()?))
    }
}

impl fmt::Display for JoinCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&encode_base32(&self.0))
    }
}

impl fmt::Debug for JoinCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JoinCode(<redacted>)")
    }
}

impl FromStr for JoinCode {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        if value.len() != 26 || !value.bytes().all(|byte| BASE32.contains(&byte)) {
            return Err(Error::InvalidInput(
                "join code must be 26 uppercase RFC 4648 Base32 characters",
            ));
        }
        let decoded = decode_base32(value)?;
        let bytes = decoded
            .try_into()
            .map_err(|_| Error::InvalidInput("join code must encode exactly 16 bytes"))?;
        Ok(Self(bytes))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VaultId(pub [u8; 32]);

impl VaultId {
    pub fn generate() -> Result<Self> {
        Ok(Self(random_bytes()?))
    }
}

impl fmt::Display for VaultId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&encode_base32(&self.0))
    }
}

impl fmt::Debug for VaultId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VaultId(<redacted>)")
    }
}

impl FromStr for VaultId {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        if value.len() != 52 || !value.bytes().all(|byte| BASE32.contains(&byte)) {
            return Err(Error::InvalidInput(
                "vault id must be 52 uppercase RFC 4648 Base32 characters",
            ));
        }
        let decoded = decode_base32(value)?;
        let bytes = decoded
            .try_into()
            .map_err(|_| Error::InvalidInput("vault id must encode exactly 32 bytes"))?;
        Ok(Self(bytes))
    }
}

fn encode_base32(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let mut bits = 0u16;
    let mut available = 0u8;
    for &byte in bytes {
        bits = (bits << 8) | u16::from(byte);
        available += 8;
        while available >= 5 {
            available -= 5;
            output.push(char::from(BASE32[usize::from((bits >> available) & 0x1f)]));
        }
    }
    if available != 0 {
        output.push(char::from(
            BASE32[usize::from((bits << (5 - available)) & 0x1f)],
        ));
    }
    output
}

fn decode_base32(value: &str) -> Result<Vec<u8>> {
    let mut output = Vec::with_capacity(value.len() * 5 / 8);
    let mut bits = 0u16;
    let mut available = 0u8;
    for byte in value.bytes() {
        let digit = BASE32
            .iter()
            .position(|candidate| *candidate == byte)
            .ok_or(Error::InvalidInput("invalid Base32 character"))?;
        bits = (bits << 5) | digit as u16;
        available += 5;
        if available >= 8 {
            available -= 8;
            output.push((bits >> available) as u8);
        }
    }
    if available != 0 && bits & ((1u16 << available) - 1) != 0 {
        return Err(Error::InvalidInput("non-canonical Base32 trailing bits"));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{JoinCode, VaultId};
    use std::str::FromStr;

    #[test]
    fn join_code_uses_canonical_uppercase_unpadded_base32() {
        let code = JoinCode([0xff; 16]);
        assert_eq!(code.to_string(), "77777777777777777777777774");
        assert_eq!(JoinCode::from_str(&code.to_string()).unwrap(), code);
        assert!(JoinCode::from_str("77777777777777777777777777").is_err());
        assert!(JoinCode::from_str("7777777777777777777777777=").is_err());
        assert!(JoinCode::from_str("aaaaaaaaaaaaaaaaaaaaaaaaaa").is_err());
        assert_eq!(format!("{code:?}"), "JoinCode(<redacted>)");
    }

    #[test]
    fn vault_id_uses_canonical_uppercase_unpadded_base32() {
        let vault = VaultId([0xff; 32]);
        assert_eq!(
            vault.to_string(),
            "777777777777777777777777777777777777777777777777777Q"
        );
        assert_eq!(VaultId::from_str(&vault.to_string()).unwrap(), vault);
        assert!(VaultId::from_str("7777777777777777777777777777777777777777777777777777").is_err());
        assert_eq!(format!("{vault:?}"), "VaultId(<redacted>)");
    }
}
