use crate::error::Result;

/// One opaque K_ab handle serves both ordinary packets and file chunk bodies.
pub struct PairKeyHandle {
    _slot: u64,
}

pub trait Aes256Gcm {
    fn seal(
        &self,
        key: &PairKeyHandle,
        nonce: &[u8; 12],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>>;

    fn open(
        &self,
        key: &PairKeyHandle,
        nonce: &[u8; 12],
        aad: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>>;
}

pub struct PendingAes256Gcm;

impl Aes256Gcm for PendingAes256Gcm {
    fn seal(&self, _: &PairKeyHandle, _: &[u8; 12], _: &[u8], _: &[u8]) -> Result<Vec<u8>> {
        Err(crate::error::Error::NotImplemented("AES-256-GCM sealing"))
    }

    fn open(&self, _: &PairKeyHandle, _: &[u8; 12], _: &[u8], _: &[u8]) -> Result<Vec<u8>> {
        Err(crate::error::Error::NotImplemented("AES-256-GCM opening"))
    }
}
