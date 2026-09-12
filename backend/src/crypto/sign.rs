use crate::error::Result;

pub const IDENTITY_CONTEXT: &[u8] = b"qfs/v1/id";
pub const WRAP_CONTEXT: &[u8] = b"qfs/v1/wrap";
pub const MANIFEST_CONTEXT: &[u8] = b"qfs/v1/manifest";
pub const FLUSH_CONTEXT: &[u8] = b"qfs/v1/flush";

/// Pure ML-DSA-65 with the FIPS 204 context parameter.
pub trait PureMlDsa {
    fn sign(
        &self,
        signing_key_handle: &SigningKeyHandle,
        context: &[u8],
        message: &[u8],
    ) -> Result<Vec<u8>>;

    fn verify(
        &self,
        verification_key: &[u8],
        context: &[u8],
        message: &[u8],
        signature: &[u8],
    ) -> Result<()>;
}

/// An opaque reference to secret signing-key material owned and zeroized by the keystore.
pub struct SigningKeyHandle {
    _slot: u64,
}

pub struct PendingPureMlDsa;

impl PureMlDsa for PendingPureMlDsa {
    fn sign(&self, _: &SigningKeyHandle, _: &[u8], _: &[u8]) -> Result<Vec<u8>> {
        Err(crate::error::Error::NotImplemented(
            "Pure ML-DSA-65 signing",
        ))
    }

    fn verify(&self, _: &[u8], _: &[u8], _: &[u8], _: &[u8]) -> Result<()> {
        Err(crate::error::Error::NotImplemented(
            "Pure ML-DSA-65 verification",
        ))
    }
}
