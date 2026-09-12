use std::sync::Arc;

use ml_dsa::{KeyInit, Keypair, MlDsa65, Signature, SigningKey, VerifyingKey};
use rand::TryRng;
use zeroize::{Zeroize, Zeroizing};

use crate::{Error, Result};

pub const IDENTITY_CONTEXT: &[u8] = b"qfs/v1/id";
pub const WRAP_CONTEXT: &[u8] = b"qfs/v1/wrap";
pub const MANIFEST_CONTEXT: &[u8] = b"qfs/v1/manifest";
pub const FLUSH_CONTEXT: &[u8] = b"qfs/v1/flush";
pub const DIRECTORY_CONTEXT: &[u8] = b"qfs/v1/dir";
pub const JOIN_CONTEXT: &[u8] = b"qfs/v1/join";

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

/// An opaque signing-key seed. Expanded key material exists only while an
/// operation is in progress and is zeroized by the provider crate.
#[derive(Clone)]
pub struct SigningKeyHandle {
    seed: Arc<Zeroizing<[u8; 32]>>,
}

impl SigningKeyHandle {
    pub fn generate() -> Result<Self> {
        let mut seed = Zeroizing::new([0; 32]);
        rand::rngs::SysRng
            .try_fill_bytes(seed.as_mut())
            .map_err(|_| Error::State("OS entropy unavailable"))?;
        Ok(Self::from_seed(*seed))
    }

    pub(crate) fn from_seed(mut seed: [u8; 32]) -> Self {
        let handle = Self {
            seed: Arc::new(Zeroizing::new(seed)),
        };
        seed.zeroize();
        handle
    }

    pub(crate) fn seed(&self) -> &[u8; 32] {
        self.seed.as_ref()
    }

    pub fn verification_key(&self) -> Vec<u8> {
        signing_key(self.seed())
            .verifying_key()
            .encode()
            .as_slice()
            .to_vec()
    }
}

pub struct RustCryptoPureMlDsa;

impl PureMlDsa for RustCryptoPureMlDsa {
    fn sign(
        &self,
        signing_key_handle: &SigningKeyHandle,
        context: &[u8],
        message: &[u8],
    ) -> Result<Vec<u8>> {
        require_protocol_context(context)?;
        let key = signing_key(signing_key_handle.seed());
        let signature = key
            .expanded_key()
            .sign_deterministic(message, context)
            .map_err(|_| Error::AuthenticationFailed)?;
        Ok(signature.encode().as_slice().to_vec())
    }

    fn verify(
        &self,
        verification_key: &[u8],
        context: &[u8],
        message: &[u8],
        signature: &[u8],
    ) -> Result<()> {
        require_protocol_context(context)?;
        let key = VerifyingKey::<MlDsa65>::new_from_slice(verification_key)
            .map_err(|_| Error::AuthenticationFailed)?;
        let signature =
            Signature::<MlDsa65>::try_from(signature).map_err(|_| Error::AuthenticationFailed)?;
        if key.verify_with_context(message, context, &signature) {
            Ok(())
        } else {
            Err(Error::AuthenticationFailed)
        }
    }
}

fn signing_key(seed: &[u8; 32]) -> SigningKey<MlDsa65> {
    let mut seed = ml_dsa::Seed::from(*seed);
    let key = SigningKey::from_seed(&seed);
    seed.zeroize();
    key
}

fn require_protocol_context(context: &[u8]) -> Result<()> {
    if context == IDENTITY_CONTEXT
        || context == WRAP_CONTEXT
        || context == MANIFEST_CONTEXT
        || context == FLUSH_CONTEXT
        || context == DIRECTORY_CONTEXT
        || context == JOIN_CONTEXT
    {
        Ok(())
    } else {
        Err(Error::InvalidInput("unsupported ML-DSA context"))
    }
}
