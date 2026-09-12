use std::time::{SystemTime, UNIX_EPOCH};

use crate::crypto::{
    sign::{PureMlDsa, RustCryptoPureMlDsa, SigningKeyHandle, IDENTITY_CONTEXT},
    wrap::{validate_public_key, XWingPrivateKeyHandle},
};
use crate::ids::PeerId;
use crate::{Error, Result};

/// The public, self-certifying identity document exchanged by peers.
#[derive(Clone, PartialEq, Eq)]
pub struct IdentityDocument {
    pub peer_id: PeerId,
    /// Static v1 X-Wing public-key encoding. Rotating it does not provide forward secrecy.
    pub ek: Vec<u8>,
    /// FIPS 204 ML-DSA-65 public-key encoding.
    pub vk: Vec<u8>,
    pub created_at: u64,
    pub signature: Vec<u8>,
}

impl IdentityDocument {
    pub fn peer_id_for(vk: &[u8]) -> PeerId {
        crate::encoding::peer_id(vk)
    }

    /// Verify the self-certifying principal, the signed public document, and
    /// the X-Wing public-key encoding before its `ek` is used.
    pub fn verify(&self) -> Result<()> {
        validate_public_key(&self.ek)?;
        if self.peer_id != Self::peer_id_for(&self.vk) {
            return Err(Error::AuthenticationFailed);
        }
        RustCryptoPureMlDsa.verify(
            &self.vk,
            IDENTITY_CONTEXT,
            &crate::encoding::identity_m(self)?,
            &self.signature,
        )
    }
}

/// A verified public identity and the two independent private-key seeds that
/// back it. The handles keep secret bytes opaque and zeroize them on drop.
pub struct LocalIdentity {
    pub document: IdentityDocument,
    pub signing_key: SigningKeyHandle,
    pub decapsulation_key: XWingPrivateKeyHandle,
}

impl LocalIdentity {
    pub fn generate() -> Result<Self> {
        let signing_key = SigningKeyHandle::generate()?;
        let decapsulation_key = XWingPrivateKeyHandle::generate()?;
        Self::new(signing_key, decapsulation_key, unix_time()?)
    }

    pub fn from_seeds(
        signing_seed: [u8; 32],
        kem_seed: [u8; 32],
        document: IdentityDocument,
    ) -> Result<Self> {
        let signing_key = SigningKeyHandle::from_seed(signing_seed);
        let decapsulation_key = XWingPrivateKeyHandle::from_seed(kem_seed);
        document.verify()?;
        if document.vk != signing_key.verification_key()
            || document.ek != decapsulation_key.public_key()
        {
            return Err(Error::AuthenticationFailed);
        }
        Ok(Self {
            document,
            signing_key,
            decapsulation_key,
        })
    }

    /// Rotate the static X-Wing key while preserving the ML-DSA principal.
    pub fn rotate_ek(&self) -> Result<Self> {
        let decapsulation_key = XWingPrivateKeyHandle::generate()?;
        let next_created_at = self
            .document
            .created_at
            .checked_add(1)
            .ok_or(Error::State("identity creation time exhausted"))?;
        Self::new(
            self.signing_key.clone(),
            decapsulation_key,
            unix_time()?.max(next_created_at),
        )
    }

    fn new(
        signing_key: SigningKeyHandle,
        decapsulation_key: XWingPrivateKeyHandle,
        created_at: u64,
    ) -> Result<Self> {
        let vk = signing_key.verification_key();
        let mut document = IdentityDocument {
            peer_id: IdentityDocument::peer_id_for(&vk),
            ek: decapsulation_key.public_key(),
            vk,
            created_at,
            signature: Vec::new(),
        };
        document.signature = RustCryptoPureMlDsa.sign(
            &signing_key,
            IDENTITY_CONTEXT,
            &crate::encoding::identity_m(&document)?,
        )?;
        Ok(Self {
            document,
            signing_key,
            decapsulation_key,
        })
    }
}

fn unix_time() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| Error::State("system clock is before Unix epoch"))
}

pub trait IdentityManager {
    /// Real providers generate independent X-Wing and ML-DSA-65 keypairs;
    /// their private keys must never share seed material.
    fn load_or_create(&self) -> Result<IdentityDocument>;
}
