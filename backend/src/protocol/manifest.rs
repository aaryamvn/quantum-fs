use std::collections::BTreeSet;

use crate::{
    crypto::sign::{PureMlDsa, RustCryptoPureMlDsa, MANIFEST_CONTEXT},
    encoding,
    error::{Error, Result},
    ids::{ChunkId, FileId, PeerId},
    keystore::{IdentityKeyStore, KeyStore},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub file_id: FileId,
    /// Chunk identifiers are in file order.
    pub chunk_ids: Vec<ChunkId>,
    pub size: u64,
    pub writer_id: PeerId,
    pub version: u64,
    pub signature: Vec<u8>,
}

/// A writer manifest authenticated by the host control path. Keeping the
/// constructor crate-private prevents a chunk holder from supplying trust data.
#[derive(Clone)]
pub struct TrustedManifest(Manifest);

impl TrustedManifest {
    pub(crate) fn verify(
        manifest: Manifest,
        keys: &KeyStore,
        members: &BTreeSet<PeerId>,
    ) -> Result<Self> {
        // Membership is checked before canonicalization touches chunk_ids.
        if !members.contains(&manifest.writer_id) {
            return Err(Error::AuthenticationFailed);
        }
        let writer = if manifest.writer_id == keys.peer_id()? {
            keys.identity()?
        } else {
            keys.load_verified_peer(&manifest.writer_id)?
        };
        RustCryptoPureMlDsa.verify(
            &writer.vk,
            MANIFEST_CONTEXT,
            &encoding::manifest_m(&manifest)?,
            &manifest.signature,
        )?;
        Ok(Self(manifest))
    }

    pub fn manifest(&self) -> &Manifest {
        &self.0
    }
}
