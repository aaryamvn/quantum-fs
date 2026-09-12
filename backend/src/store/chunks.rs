use std::collections::BTreeMap;

use crate::{
    encoding,
    ids::{ChunkId, FileId},
};

/// A v1 member is in the TCB for plaintext it stores. Zeroization is for keys,
/// not file bytes.
pub trait ChunkStore {
    fn put(&mut self, file_id: &FileId, index: u64, plaintext: Vec<u8>) -> ChunkId;
    fn get(&self, chunk_id: &ChunkId) -> Option<&[u8]>;
    fn has(&self, chunk_id: &ChunkId) -> bool;

    /// Returns a packed bitset in request order, least-significant bit first.
    fn have_bitset(&self, chunk_ids: &[ChunkId]) -> Vec<u8> {
        let mut bits = vec![0u8; chunk_ids.len().div_ceil(8)];
        for (index, chunk_id) in chunk_ids.iter().enumerate() {
            if self.has(chunk_id) {
                bits[index / 8] |= 1 << (index % 8);
            }
        }
        bits
    }
}

#[derive(Debug, Default)]
pub struct MemoryChunkStore {
    plaintext: BTreeMap<ChunkId, Vec<u8>>,
}

impl MemoryChunkStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ChunkStore for MemoryChunkStore {
    fn put(&mut self, file_id: &FileId, index: u64, plaintext: Vec<u8>) -> ChunkId {
        let chunk_id = encoding::chunk_id(file_id, index, &plaintext);
        self.plaintext.insert(chunk_id, plaintext);
        chunk_id
    }

    fn get(&self, chunk_id: &ChunkId) -> Option<&[u8]> {
        self.plaintext.get(chunk_id).map(Vec::as_slice)
    }

    fn has(&self, chunk_id: &ChunkId) -> bool {
        self.plaintext.contains_key(chunk_id)
    }
}

#[cfg(test)]
mod tests {
    use super::{ChunkStore, MemoryChunkStore};
    use crate::ids::{ChunkId, FileId};

    #[test]
    fn stores_plaintext_and_reports_have_vector_in_request_order() {
        let mut store = MemoryChunkStore::new();
        let present = store.put(&FileId([3; 32]), 7, b"plaintext".to_vec());
        let absent = ChunkId([9; 32]);

        assert!(store.has(&present));
        assert_eq!(store.get(&present), Some(b"plaintext".as_slice()));
        assert_eq!(store.have_bitset(&[absent, present]), vec![0b0000_0010]);
    }
}
