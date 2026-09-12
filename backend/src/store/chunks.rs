use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
};

use crate::{
    encoding,
    ids::{ChunkId, FileId},
    store::{durable::DurableStore, replica::ReplicaMetadata},
    Result,
};

/// A v1 member is in the TCB for plaintext it stores. Zeroization is for keys,
/// not file bytes.
pub trait ChunkStore {
    fn put(&mut self, file_id: &FileId, index: u64, plaintext: Vec<u8>) -> Result<ChunkId>;
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

#[derive(Clone, Default)]
pub struct MemoryChunkStore {
    chunks: BTreeMap<ChunkId, Arc<ChunkRecord>>,
    durable: Option<DurableStore>,
    metadata: Option<ReplicaMetadata>,
}

impl MemoryChunkStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_record(&self, chunk_id: &ChunkId) -> Option<&ChunkRecord> {
        self.chunks.get(chunk_id).map(Arc::as_ref)
    }

    pub(crate) fn from_records(
        chunks: BTreeMap<ChunkId, Arc<ChunkRecord>>,
        durable: DurableStore,
    ) -> Self {
        Self {
            chunks,
            durable: Some(durable),
            metadata: None,
        }
    }

    pub(crate) fn set_metadata(&mut self, metadata: ReplicaMetadata) {
        self.metadata = Some(metadata);
    }

    pub fn metadata(&self) -> Option<ReplicaMetadata> {
        self.metadata.clone()
    }

    pub fn accepts_file(&self, file_id: &FileId) -> bool {
        self.metadata.as_ref().is_some_and(|metadata| {
            metadata.manifests.contains_key(file_id)
                && metadata
                    .dirents
                    .iter()
                    .any(|entry| entry.child == *file_id && !entry.is_dir)
        })
    }

    pub fn accepts_chunk(&self, file_id: &FileId, index: u64, chunk_id: &ChunkId) -> bool {
        let Ok(index) = usize::try_from(index) else {
            return false;
        };
        self.metadata.as_ref().is_some_and(|metadata| {
            if !metadata
                .dirents
                .iter()
                .any(|entry| entry.child == *file_id && !entry.is_dir)
            {
                return false;
            }
            metadata
                .manifests
                .get(file_id)
                .and_then(|manifest| manifest.chunk_ids.get(index))
                == Some(chunk_id)
        })
    }

    pub fn persist_metadata(&mut self, metadata: ReplicaMetadata) -> Result<()> {
        if let Some(durable) = self.durable.clone() {
            self.metadata = Some(durable.persist(&metadata, &mut self.chunks)?);
        } else {
            self.metadata = Some(metadata);
        }
        Ok(())
    }

    pub fn persist_metadata_with_admission(
        &mut self,
        metadata: ReplicaMetadata,
        path: &Path,
        admission_bytes: &[u8],
    ) -> Result<()> {
        if let Some(durable) = self.durable.clone() {
            self.metadata = Some(durable.persist_with_admission(
                &metadata,
                &mut self.chunks,
                path,
                admission_bytes,
            )?);
        } else {
            crate::store::transaction::persist(None, path, None, admission_bytes)?;
            self.metadata = Some(metadata);
        }
        Ok(())
    }

    pub fn persist_current(&mut self) -> Result<()> {
        let Some(metadata) = self.metadata() else {
            return Ok(());
        };
        self.persist_metadata(metadata)
    }

    #[cfg(test)]
    pub(crate) fn fail_next_persist(&self) {
        if let Some(durable) = &self.durable {
            durable.fail_next_persist();
        }
    }

    pub fn remove_file(&mut self, file_id: &FileId) {
        self.chunks.retain(|_, record| record.file_id != *file_id);
        if let Some(metadata) = &mut self.metadata {
            metadata
                .chunk_index
                .retain(|_, (indexed_file, _)| indexed_file != file_id);
        }
    }

    pub(crate) fn file_chunk_count(&self, file_id: &FileId) -> usize {
        self.chunks
            .values()
            .filter(|record| record.file_id == *file_id)
            .count()
    }

    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }
}

impl ChunkStore for MemoryChunkStore {
    fn put(&mut self, file_id: &FileId, index: u64, plaintext: Vec<u8>) -> Result<ChunkId> {
        let chunk_id = encoding::chunk_id(file_id, index, &plaintext);
        if self.chunks.contains_key(&chunk_id) {
            return Ok(chunk_id);
        }
        if let Some(durable) = &self.durable {
            durable.write_chunk(&chunk_id, &plaintext)?;
        }
        self.chunks.insert(
            chunk_id,
            Arc::new(ChunkRecord {
                file_id: *file_id,
                index,
                plaintext,
            }),
        );
        Ok(chunk_id)
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
    use crate::{
        ids::{ChunkId, FileId},
        store::replica::ReplicaMetadata,
    };

    #[test]
    fn stores_plaintext_and_reports_have_vector_in_request_order() {
        let mut store = MemoryChunkStore::new();
        let present = store
            .put(&FileId([3; 32]), 7, b"plaintext".to_vec())
            .unwrap();
        let absent = ChunkId([9; 32]);

        assert!(store.has(&present));
        assert_eq!(store.get(&present), Some(b"plaintext".as_slice()));
        assert_eq!(store.get_record(&present).unwrap().index, 7);
        assert_eq!(store.have_bitset(&[absent, present]), vec![0b0000_0010]);

        store.remove_file(&FileId([3; 32]));
        assert!(store.is_empty());
    }

    #[test]
    fn failed_memory_admission_persist_leaves_metadata_unchanged() {
        let mut store = MemoryChunkStore::new();
        let metadata = ReplicaMetadata::new(FileId([8; 32]));
        let missing_parent = std::env::temp_dir()
            .join(format!(
                "qfs-missing-admission-parent-{}",
                std::process::id()
            ))
            .join("vault");

        assert!(store
            .persist_metadata_with_admission(metadata, &missing_parent, b"admission")
            .is_err());
        assert!(store.metadata().is_none());
    }
}
