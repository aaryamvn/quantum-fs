use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::{
    encoding,
    ids::{ChunkId, FileId},
    keystore::{atomic_private_write, read_private, KeyStore, MAX_STORE_BYTES},
    store::{
        chunks::{ChunkRecord, MemoryChunkStore},
        replica::ReplicaMetadata,
    },
    Error, Result,
};

pub const MAX_CHUNK_BYTES: u64 = 1024 * 1024;

#[derive(Clone)]
pub struct DurableStore {
    inner: Arc<Mutex<DurableInner>>,
}

struct DurableInner {
    _keys: KeyStore,
    chunks_dir: PathBuf,
    replica_path: PathBuf,
    generation: u64,
    expected_root: FileId,
    live_metadata: Option<ReplicaMetadata>,
    fail_next_persist: bool,
    failed: bool,
}

impl DurableStore {
    pub fn open(
        keys: &KeyStore,
        data_dir: &Path,
        expected_root: FileId,
    ) -> Result<(Self, Option<ReplicaMetadata>, MemoryChunkStore)> {
        keys.require_data_dir_lock(data_dir)?;
        let chunks_dir = data_dir.join("chunks");
        fs::create_dir_all(&chunks_dir)?;
        owner_only_directory(&chunks_dir)?;
        let replica_path = data_dir.join("replica.bin");
        let (generation, metadata) = match read_private(&replica_path)? {
            Some(bytes) => {
                if bytes.is_empty() {
                    return Err(Error::InvalidInput("empty replica snapshot"));
                }
                let (generation, metadata) = encoding::decode_replica(&bytes, expected_root)?;
                (generation, Some(metadata))
            }
            None => (0, None),
        };

        let mut records = BTreeMap::new();
        if let Some(metadata) = &metadata {
            let retained = metadata.retain_ids();
            for (chunk_id, (file_id, index)) in &metadata.chunk_index {
                if !retained.contains(chunk_id) {
                    continue;
                }
                let path = chunks_dir.join(chunk_file_name(chunk_id));
                match read_chunk(&path) {
                    Ok(Some(plaintext))
                        if encoding::chunk_id(file_id, *index, &plaintext) == *chunk_id =>
                    {
                        records.insert(
                            *chunk_id,
                            Arc::new(ChunkRecord {
                                file_id: *file_id,
                                index: *index,
                                plaintext,
                            }),
                        );
                    }
                    Ok(None) => {}
                    Ok(Some(_)) | Err(_) => {
                        discard_chunk_file(&path, &chunks_dir)?;
                    }
                }
            }
            for queue in metadata.mailboxes.values() {
                for content in queue {
                    if let crate::sync::host::QueueContent::Chunk { chunk_id, .. } = content {
                        if !records.contains_key(chunk_id) {
                            return Err(Error::State(
                                "mailbox references an unavailable plaintext chunk",
                            ));
                        }
                    }
                }
            }
        }

        sweep_directory(&chunks_dir, records.keys().copied())?;
        let store = Self {
            inner: Arc::new(Mutex::new(DurableInner {
                _keys: keys.clone(),
                chunks_dir,
                replica_path,
                generation,
                expected_root,
                live_metadata: metadata.clone(),
                fail_next_persist: false,
                failed: false,
            })),
        };
        let mut chunks = MemoryChunkStore::from_records(records, store.clone());
        if let Some(metadata) = metadata.clone() {
            chunks.set_metadata(metadata);
        }
        Ok((store, metadata, chunks))
    }

    pub fn live_metadata(&self) -> Option<ReplicaMetadata> {
        self.inner
            .lock()
            .ok()
            .and_then(|inner| inner.live_metadata.clone())
    }

    pub fn generation(&self) -> Result<u64> {
        Ok(self.lock()?.generation)
    }

    pub(crate) fn write_chunk(&self, chunk_id: &ChunkId, plaintext: &[u8]) -> Result<()> {
        if plaintext.len() as u64 > MAX_CHUNK_BYTES {
            return Err(Error::InvalidInput("chunk exceeds 1 MiB"));
        }
        let mut inner = self.lock()?;
        if inner.failed {
            return Err(Error::State("durable store is poisoned"));
        }
        let path = inner.chunks_dir.join(chunk_file_name(chunk_id));
        if path.try_exists()? {
            return match read_chunk(&path)? {
                Some(existing) if existing == plaintext => Ok(()),
                Some(_) => Err(Error::State(
                    "existing content-addressed chunk bytes differ",
                )),
                None => Err(Error::State("existing chunk disappeared during put")),
            };
        }
        if let Err(error) = atomic_private_write(&path, plaintext) {
            inner.failed = true;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn persist(
        &self,
        metadata: &ReplicaMetadata,
        chunks: &mut BTreeMap<ChunkId, Arc<ChunkRecord>>,
    ) -> Result<ReplicaMetadata> {
        let mut inner = self.lock()?;
        if inner.failed {
            return Err(Error::State("durable store is poisoned"));
        }
        if inner.fail_next_persist {
            inner.fail_next_persist = false;
            return Err(Error::State("injected replica persist failure"));
        }
        if metadata.expected_root != inner.expected_root {
            return Err(Error::InvalidInput("replica root differs from open store"));
        }
        let retained = metadata.retain_ids();
        let mut staged = metadata.clone();
        staged.chunk_index.clear();
        for chunk_id in &retained {
            if let Some(record) = chunks.get(chunk_id) {
                staged
                    .chunk_index
                    .insert(*chunk_id, (record.file_id, record.index));
            }
        }
        for queue in staged.mailboxes.values() {
            for content in queue {
                if let crate::sync::host::QueueContent::Chunk { chunk_id, .. } = content {
                    if !chunks.contains_key(chunk_id) {
                        return Err(Error::State("mailbox references a missing plaintext chunk"));
                    }
                }
            }
        }
        let generation = inner
            .generation
            .checked_add(1)
            .ok_or(Error::State("replica generation exhausted"))?;
        let encoded = encoding::encode_replica(&staged, generation)?;
        if encoded.len() as u64 > MAX_STORE_BYTES {
            return Err(Error::InvalidInput("replica snapshot is too large"));
        }
        if let Err(error) = atomic_private_write(&inner.replica_path, &encoded) {
            inner.failed = true;
            return Err(error);
        }
        inner.generation = generation;
        inner.live_metadata = Some(staged.clone());

        chunks.retain(|chunk_id, _| retained.contains(chunk_id));
        if let Err(error) = sweep_directory(&inner.chunks_dir, chunks.keys().copied()) {
            eprintln!("qfsd: deferred chunk cleanup after durable snapshot: {error}");
        }
        Ok(staged)
    }

    #[cfg(test)]
    pub(crate) fn fail_next_persist(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.fail_next_persist = true;
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, DurableInner>> {
        self.inner
            .lock()
            .map_err(|_| Error::State("durable store lock poisoned"))
    }
}

fn read_chunk(path: &Path) -> Result<Option<Vec<u8>>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() || metadata.len() > MAX_CHUNK_BYTES {
        return Err(Error::InvalidInput("invalid chunk file"));
    }
    let mut plaintext = Vec::with_capacity(metadata.len() as usize);
    File::open(path)?
        .take(MAX_CHUNK_BYTES + 1)
        .read_to_end(&mut plaintext)?;
    if plaintext.len() as u64 > MAX_CHUNK_BYTES {
        return Err(Error::InvalidInput("chunk exceeds 1 MiB"));
    }
    Ok(Some(plaintext))
}

fn sweep_directory(chunks_dir: &Path, retained: impl IntoIterator<Item = ChunkId>) -> Result<()> {
    let retained = retained
        .into_iter()
        .map(|chunk_id| chunk_file_name(&chunk_id))
        .collect::<std::collections::BTreeSet<_>>();
    let mut changed = false;
    for entry in fs::read_dir(chunks_dir)? {
        let entry = entry?;
        if !retained.contains(&entry.file_name()) {
            let metadata = fs::symlink_metadata(entry.path())?;
            if metadata.is_file() || metadata.file_type().is_symlink() {
                fs::remove_file(entry.path())?;
                changed = true;
            }
        }
    }
    if changed {
        File::open(chunks_dir)?.sync_all()?;
    }
    Ok(())
}

fn discard_chunk_file(path: &Path, chunks_dir: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => File::open(chunks_dir)?.sync_all().map_err(Into::into),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn chunk_file_name(chunk_id: &ChunkId) -> std::ffi::OsString {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut name = String::with_capacity(64);
    for byte in chunk_id.0 {
        name.push(HEX[(byte >> 4) as usize] as char);
        name.push(HEX[(byte & 0x0f) as usize] as char);
    }
    name.into()
}

fn owner_only_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::{
        ids::PeerId, protocol::manifest::Manifest, store::chunks::ChunkStore,
        sync::host::QueueContent,
    };

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(1);

    fn test_directory(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "qfs-durable-{name}-{}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn chunk_plaintext_survives_but_is_not_in_snapshot() -> Result<()> {
        let directory = test_directory("roundtrip");
        fs::create_dir(&directory)?;
        let identity = directory.join("identity");
        let root = FileId([3; 32]);
        let plaintext = vec![0xa7; 256 * 1024];
        let result = (|| -> Result<()> {
            let keys = KeyStore::open(&identity)?;
            let (durable, _, mut chunks) = DurableStore::open(&keys, &directory, root)?;
            let chunk_id = chunks.put(&FileId([4; 32]), 0, plaintext.clone())?;
            let mut metadata = ReplicaMetadata::new(root);
            metadata.members.insert(keys.peer_id()?);
            metadata.manifests.insert(
                FileId([4; 32]),
                Manifest {
                    file_id: FileId([4; 32]),
                    chunk_ids: vec![chunk_id],
                    size: plaintext.len() as u64,
                    writer_id: PeerId([5; 32]),
                    version: 1,
                    signature: vec![6; 32],
                },
            );
            chunks.persist_metadata(metadata)?;
            let snapshot_size = fs::metadata(directory.join("replica.bin"))?.len();
            assert!(snapshot_size < plaintext.len() as u64 / 2);
            drop(chunks);
            drop(durable);
            drop(keys);

            let keys = KeyStore::open(&identity)?;
            let (_durable, loaded, chunks) = DurableStore::open(&keys, &directory, root)?;
            assert!(loaded.is_some());
            assert_eq!(chunks.get(&chunk_id), Some(plaintext.as_slice()));
            Ok(())
        })();
        let _ = fs::remove_dir_all(&directory);
        result
    }

    #[test]
    fn corrupt_referenced_chunk_is_discarded_without_rejecting_snapshot() -> Result<()> {
        let directory = test_directory("corrupt-chunk");
        fs::create_dir(&directory)?;
        let identity = directory.join("identity");
        let root = FileId([7; 32]);
        let result = (|| -> Result<()> {
            let keys = KeyStore::open(&identity)?;
            let (durable, _, mut chunks) = DurableStore::open(&keys, &directory, root)?;
            let file_id = FileId([8; 32]);
            let chunk_id = chunks.put(&file_id, 2, b"original".to_vec())?;
            let mut metadata = ReplicaMetadata::new(root);
            metadata.manifests.insert(
                file_id,
                Manifest {
                    file_id,
                    chunk_ids: vec![ChunkId([0; 32]), ChunkId([0; 32]), chunk_id],
                    size: 8,
                    writer_id: PeerId([9; 32]),
                    version: 1,
                    signature: Vec::new(),
                },
            );
            chunks.persist_metadata(metadata)?;
            let path = directory.join("chunks").join(chunk_file_name(&chunk_id));
            fs::write(&path, b"torn")?;
            drop(chunks);
            drop(durable);
            drop(keys);

            let keys = KeyStore::open(&identity)?;
            let (_durable, metadata, chunks) = DurableStore::open(&keys, &directory, root)?;
            assert!(metadata.is_some());
            assert!(!chunks.has(&chunk_id));
            assert!(!path.exists());
            Ok(())
        })();
        let _ = fs::remove_dir_all(&directory);
        result
    }

    #[test]
    fn rejects_chunks_over_wire_body_cap() -> Result<()> {
        let directory = test_directory("oversized");
        fs::create_dir(&directory)?;
        let result = (|| -> Result<()> {
            let keys = KeyStore::open(&directory.join("identity"))?;
            let (_durable, _, mut chunks) = DurableStore::open(&keys, &directory, FileId([0; 32]))?;
            assert!(chunks
                .put(&FileId([1; 32]), 0, vec![0; MAX_CHUNK_BYTES as usize + 1])
                .is_err());
            Ok(())
        })();
        let _ = fs::remove_dir_all(&directory);
        result
    }

    #[test]
    fn missing_mailbox_pinned_chunk_fails_reopen() -> Result<()> {
        let directory = test_directory("missing-mailbox-chunk");
        fs::create_dir(&directory)?;
        let identity = directory.join("identity");
        let root = FileId([0x31; 32]);
        let result = (|| -> Result<()> {
            let keys = KeyStore::open(&identity)?;
            let (durable, _, mut chunks) = DurableStore::open(&keys, &directory, root)?;
            let file_id = FileId([0x32; 32]);
            let chunk_id = chunks.put(&file_id, 4, b"mailbox pinned".to_vec())?;
            let mut metadata = ReplicaMetadata::new(root);
            metadata.mailboxes.insert(
                PeerId([0x33; 32]),
                vec![QueueContent::Chunk {
                    file_id,
                    index: 4,
                    chunk_id,
                }],
            );
            chunks.persist_metadata(metadata)?;
            fs::remove_file(directory.join("chunks").join(chunk_file_name(&chunk_id)))?;
            drop(chunks);
            drop(durable);
            drop(keys);

            let keys = KeyStore::open(&identity)?;
            assert!(matches!(
                DurableStore::open(&keys, &directory, root),
                Err(Error::State(
                    "mailbox references an unavailable plaintext chunk"
                ))
            ));
            Ok(())
        })();
        let _ = fs::remove_dir_all(&directory);
        result
    }

    #[test]
    fn existing_chunk_filename_must_contain_identical_bytes() -> Result<()> {
        let directory = test_directory("existing-mismatch");
        fs::create_dir(&directory)?;
        let result = (|| -> Result<()> {
            let keys = KeyStore::open(&directory.join("identity"))?;
            let (_durable, _, mut chunks) = DurableStore::open(&keys, &directory, FileId([0; 32]))?;
            let file_id = FileId([0x41; 32]);
            let expected = b"expected immutable bytes";
            let chunk_id = encoding::chunk_id(&file_id, 0, expected);
            let path = directory.join("chunks").join(chunk_file_name(&chunk_id));
            fs::write(&path, b"different bytes")?;

            assert!(matches!(
                chunks.put(&file_id, 0, expected.to_vec()),
                Err(Error::State(
                    "existing content-addressed chunk bytes differ"
                ))
            ));
            assert_eq!(fs::read(path)?, b"different bytes");
            assert!(!chunks.has(&chunk_id));
            Ok(())
        })();
        let _ = fs::remove_dir_all(&directory);
        result
    }
}
