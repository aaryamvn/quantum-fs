use crate::error::Result;
use crate::ids::PeerId;

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
}

pub trait IdentityManager {
    /// Real providers generate independent X-Wing and ML-DSA-65 keypairs;
    /// their private keys must never share seed material.
    fn load_or_create(&self) -> Result<IdentityDocument>;
}

pub struct PendingIdentityManager;

impl IdentityManager for PendingIdentityManager {
    fn load_or_create(&self) -> Result<IdentityDocument> {
        Err(crate::error::Error::NotImplemented(
            "identity key generation",
        ))
    }
}
