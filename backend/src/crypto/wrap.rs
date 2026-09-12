use crate::crypto::aead::PairKeyHandle;
use crate::error::Result;
use crate::ids::{Epoch, PeerId};

#[derive(Clone, PartialEq, Eq)]
pub struct WrapMessage {
    pub kem_ct: Vec<u8>,
    pub wrap_ct: Vec<u8>,
    pub epoch: Epoch,
    pub min_id: PeerId,
    pub max_id: PeerId,
    pub signature: Vec<u8>,
}

pub struct PairSession {
    pub peer_id: PeerId,
    pub epoch: Epoch,
    key_handle: PairKeyHandle,
}

impl PairSession {
    pub fn new(peer_id: PeerId, epoch: Epoch, key_handle: PairKeyHandle) -> Self {
        Self {
            peer_id,
            epoch,
            key_handle,
        }
    }

    pub fn key_handle(&self) -> &PairKeyHandle {
        &self.key_handle
    }
}

/// X-Wing encapsulation only; protocol code must not call raw ML-KEM.
pub trait XWing {
    fn encapsulate(&self, peer_ek: &[u8]) -> Result<(SharedSecretHandle, Vec<u8>)>;
    fn decapsulate(
        &self,
        private_key: &XWingPrivateKeyHandle,
        kem_ct: &[u8],
    ) -> Result<SharedSecretHandle>;
}

pub struct SharedSecretHandle {
    _slot: u64,
}

pub struct XWingPrivateKeyHandle {
    _slot: u64,
}

/// Construction B generates a random 32-byte K_ab, derives a single-use wrap_key
/// from the X-Wing secret using HKDF-SHA256 with an empty salt, canonical wrap info,
/// and L=32, then persists only an opaque K_ab handle. Real providers must zeroize
/// K_ab when retired and zeroize the X-Wing secret and wrap_key after wrapping.
/// A retry returns the original ciphertext rather than encapsulating again.
///
/// On first contact the smaller peer id initiates. Epochs strictly increase; startup
/// creates a new epoch for every known pair. If two initiators choose the same epoch,
/// the smaller initiator's wrap wins and the other retries at last+2. The static v1
/// X-Wing ek means rotation limits key lifetime and GCM volume but is not forward secrecy.
pub trait ConstructionBWrap {
    fn create(
        &self,
        peer_id: PeerId,
        peer_ek: &[u8],
        epoch: Epoch,
    ) -> Result<(PairSession, WrapMessage)>;
    fn unwrap(&self, sender_id: PeerId, message: &WrapMessage) -> Result<PairSession>;
    fn retry(&self, message: &WrapMessage) -> Result<WrapMessage>;
}

pub struct PendingXWing;

impl XWing for PendingXWing {
    fn encapsulate(&self, _: &[u8]) -> Result<(SharedSecretHandle, Vec<u8>)> {
        Err(crate::error::Error::NotImplemented("X-Wing encapsulation"))
    }

    fn decapsulate(&self, _: &XWingPrivateKeyHandle, _: &[u8]) -> Result<SharedSecretHandle> {
        Err(crate::error::Error::NotImplemented("X-Wing decapsulation"))
    }
}

pub struct PendingConstructionBWrap;

impl ConstructionBWrap for PendingConstructionBWrap {
    fn create(&self, _: PeerId, _: &[u8], _: Epoch) -> Result<(PairSession, WrapMessage)> {
        Err(crate::error::Error::NotImplemented(
            "Construction B wrap creation",
        ))
    }

    fn unwrap(&self, _: PeerId, _: &WrapMessage) -> Result<PairSession> {
        Err(crate::error::Error::NotImplemented("Construction B unwrap"))
    }

    fn retry(&self, _: &WrapMessage) -> Result<WrapMessage> {
        Err(crate::error::Error::NotImplemented(
            "Construction B wrap retry",
        ))
    }
}
