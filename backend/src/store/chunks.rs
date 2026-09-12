use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkRecord {
    pub file_id: FileId,
    pub index: u64,
    pub plaintext: Vec<u8>,
}

pub type SharedChunkStore = Arc<Mutex<MemoryChunkStore>>;

pub fn shared_chunk_store() -> SharedChunkStore {
    Arc::new(Mutex::new(MemoryChunkStore::new()))
}

#[derive(Clone, Debug, Default)]
pub struct MemoryChunkStore {
    chunks: BTreeMap<ChunkId, ChunkRecord>,
}

impl MemoryChunkStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_record(&self, chunk_id: &ChunkId) -> Option<&ChunkRecord> {
        self.chunks.get(chunk_id)
    }

    pub fn remove_file(&mut self, file_id: &FileId) {
        self.chunks.retain(|_, record| record.file_id != *file_id);
    }

    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }
}

impl ChunkStore for MemoryChunkStore {
    fn put(&mut self, file_id: &FileId, index: u64, plaintext: Vec<u8>) -> ChunkId {
        let chunk_id = encoding::chunk_id(file_id, index, &plaintext);
        self.chunks.insert(
            chunk_id,
            ChunkRecord {
                file_id: *file_id,
                index,
                plaintext,
            },
        );
        chunk_id
    }

    fn get(&self, chunk_id: &ChunkId) -> Option<&[u8]> {
        self.chunks
            .get(chunk_id)
            .map(|record| record.plaintext.as_slice())
    }

    fn has(&self, chunk_id: &ChunkId) -> bool {
        self.chunks.contains_key(chunk_id)
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
        assert_eq!(store.get_record(&present).unwrap().index, 7);
        assert_eq!(store.have_bitset(&[absent, present]), vec![0b0000_0010]);

        store.remove_file(&FileId([3; 32]));
        assert!(store.is_empty());
    }
}
