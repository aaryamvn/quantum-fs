use crate::ids::{ChunkId, FileId, PeerId};

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
