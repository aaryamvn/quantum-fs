use super::{HostService, MemberReplica, QueueContent};
use crate::{ids::FileId, Error, Result};

impl HostService {
    pub fn evict_file(&mut self, _file_id: FileId) -> Result<usize> {
        Err(Error::State("H cannot evict its availability copy"))
    }
}

impl MemberReplica {
    pub fn evict_file(&mut self, file_id: FileId) -> Result<usize> {
        let local = self.keys.peer_id()?;
        if local == self.host_id
            || !self.members.contains(&local)
            || !self.members.contains(&self.host_id)
        {
            return Err(Error::State("replica is not a live member with H"));
        }

        let mut store = self
            .chunks
            .lock()
            .map_err(|_| Error::State("chunk store poisoned"))?;
        self.keys.current_session(self.host_id)?;
        self.keys.require_live_traffic(self.host_id)?;

        let metadata = store
            .metadata()
            .ok_or(Error::State("replica metadata is unavailable"))?;
        if metadata.mailboxes.values().flatten().any(|content| {
            matches!(content, QueueContent::Chunk { file_id: queued, .. } if *queued == file_id)
        }) {
            return Err(Error::State("mailbox pins file chunks"));
        }

        let removed = store.file_chunk_count(&file_id);
        let mut staged = store.clone();
        staged.remove_file(&file_id);
        let staged_metadata = staged
            .metadata()
            .ok_or(Error::State("replica metadata is unavailable"))?;
        staged.persist_metadata(staged_metadata)?;
        *store = staged;
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fs};

    use super::*;
    use crate::{
        crypto::wrap::{ConstructionBWrap, RustCryptoConstructionBWrap},
        ids::{Epoch, PeerId},
        keystore::KeyStore,
        protocol::manifest::Manifest,
        store::chunks::{shared_chunk_store, ChunkStore},
    };

    #[test]
    fn durable_failure_keeps_member_chunks_visible() -> Result<()> {
        let base =
            std::env::temp_dir().join(format!("qfs-eviction-failure-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir(&base)?;
        let result = (|| -> Result<()> {
            let host_keys = KeyStore::open(&base.join("host").join("identity"))?;
            let member_dir = base.join("member");
            fs::create_dir(&member_dir)?;
            let member_keys = KeyStore::open(&member_dir.join("identity"))?;
            host_keys.import_peer(member_keys.identity()?)?;
            member_keys.import_peer(host_keys.identity()?)?;
            let host_id = host_keys.peer_id()?;
            let member_id = member_keys.peer_id()?;
            let (sender, receiver) = if host_id < member_id {
                (&host_keys, &member_keys)
            } else {
                (&member_keys, &host_keys)
            };
            let (_, wrap) = RustCryptoConstructionBWrap::new(sender.clone()).create(
                receiver.peer_id()?,
                &receiver.identity()?.ek,
                Epoch(1),
            )?;
            RustCryptoConstructionBWrap::new(receiver.clone()).unwrap(sender.peer_id()?, &wrap)?;

            let root = FileId([0x51; 32]);
            let members = BTreeSet::from([host_id, member_id]);
            let mut member =
                MemberReplica::open_durable(member_keys, &member_dir, root, host_id, members)?;
            let file_id = FileId([0x52; 32]);
            let chunks = member.chunks();
            let mut store = chunks
                .lock()
                .map_err(|_| Error::State("chunk store poisoned"))?;
            let chunk_id = store.put(&file_id, 0, b"evict me".to_vec())?;
            let mut metadata = store
                .metadata()
                .ok_or(Error::State("replica metadata is unavailable"))?;
            metadata.manifests.insert(
                file_id,
                Manifest {
                    file_id,
                    chunk_ids: vec![chunk_id],
                    size: 8,
                    writer_id: PeerId([0x53; 32]),
                    version: 1,
                    signature: Vec::new(),
                },
            );
            store.persist_metadata(metadata)?;
            store.fail_next_persist();
            drop(store);

            assert!(member.evict_file(file_id).is_err());
            let store = chunks
                .lock()
                .map_err(|_| Error::State("chunk store poisoned"))?;
            assert!(store.has(&chunk_id));
            assert_eq!(store.have_bitset(&[chunk_id]), vec![1]);
            assert!(member_dir.join("chunks").read_dir()?.next().is_some());
            Ok(())
        })();
        let _ = fs::remove_dir_all(&base);
        result
    }

    #[test]
    fn host_refuses_without_removing_plaintext() -> Result<()> {
        let base = std::env::temp_dir().join(format!("qfs-host-eviction-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir(&base)?;
        let result = (|| -> Result<()> {
            let keys = KeyStore::open(&base.join("identity"))?;
            let host_id = keys.peer_id()?;
            let chunks = shared_chunk_store();
            let file_id = FileId([0x61; 32]);
            let chunk_id = chunks
                .lock()
                .map_err(|_| Error::State("chunk store poisoned"))?
                .put(&file_id, 0, b"host copy".to_vec())?;
            let mut host = HostService::new(keys, BTreeSet::from([host_id]), chunks.clone())?;
            assert!(host.evict_file(file_id).is_err());
            assert!(chunks
                .lock()
                .map_err(|_| Error::State("chunk store poisoned"))?
                .has(&chunk_id));
            Ok(())
        })();
        let _ = fs::remove_dir_all(&base);
        result
    }
}
