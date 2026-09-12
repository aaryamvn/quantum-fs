use std::collections::{BTreeMap, BTreeSet};

use crate::{
    crypto::identity::IdentityDocument,
    ids::{ChunkId, FileId, PeerId},
    net::join::VaultMetadata,
    protocol::manifest::Manifest,
    store::tree::{DirectoryTree, Dirent},
    sync::host::{ControlRecord, QueueContent},
};

/// Durable replica metadata. Chunk plaintext and pairwise ciphertext are kept
/// outside this snapshot.
#[derive(Clone, PartialEq, Eq)]
pub struct ReplicaMetadata {
    pub expected_root: FileId,
    pub dirents: Vec<Dirent>,
    pub members: BTreeSet<PeerId>,
    pub manifests: BTreeMap<FileId, Manifest>,
    pub log: Vec<ControlRecord>,
    pub next_control: u64,
    pub mailboxes: BTreeMap<PeerId, Vec<QueueContent>>,
    pub acked_through: BTreeMap<PeerId, u64>,
    pub chunk_index: BTreeMap<ChunkId, (FileId, u64)>,
    pub denied: BTreeSet<PeerId>,
    pub admission: Option<VaultMetadata>,
    pub historical_members: BTreeSet<PeerId>,
    pub identity_documents: BTreeMap<PeerId, IdentityDocument>,
}

impl ReplicaMetadata {
    pub fn new(expected_root: FileId) -> Self {
        Self {
            expected_root,
            dirents: DirectoryTree::new(expected_root).dirents(),
            members: BTreeSet::new(),
            manifests: BTreeMap::new(),
            log: Vec::new(),
            next_control: 1,
            mailboxes: BTreeMap::new(),
            acked_through: BTreeMap::new(),
            chunk_index: BTreeMap::new(),
            denied: BTreeSet::new(),
            admission: None,
            historical_members: BTreeSet::new(),
            identity_documents: BTreeMap::new(),
        }
    }

    /// Chunk files stay pinned while referenced by a live manifest or any
    /// undrained mailbox body.
    pub fn retain_ids(&self) -> BTreeSet<ChunkId> {
        let mut retained = BTreeSet::new();
        for manifest in self.manifests.values() {
            retained.extend(manifest.chunk_ids.iter().copied());
        }
        for queue in self.mailboxes.values() {
            for content in queue {
                if let QueueContent::Chunk { chunk_id, .. } = content {
                    retained.insert(*chunk_id);
                }
            }
        }
        retained
    }

    /// Acknowledgements are inclusive: once every current member has applied
    /// N, records through N can be removed. A missing watermark is zero and
    /// therefore pins the positive, one-based instruction log.
    pub fn truncate_log(&mut self) {
        let Some(minimum) = self
            .members
            .iter()
            .map(|member| self.acked_through.get(member).copied().unwrap_or(0))
            .min()
        else {
            return;
        };
        self.log.retain(|record| record.id > minimum);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::host::ControlUpdate;

    #[test]
    fn log_waits_for_every_member_ack() {
        let a = PeerId([1; 32]);
        let b = PeerId([2; 32]);
        let mut metadata = ReplicaMetadata::new(FileId([0; 32]));
        metadata.members.extend([a, b]);
        metadata.log = (1..=3)
            .map(|id| ControlRecord {
                id,
                update: ControlUpdate::Add(FileId([id as u8; 32])),
            })
            .collect();

        metadata.acked_through.insert(a, 3);
        metadata.truncate_log();
        assert_eq!(metadata.log.len(), 3);

        metadata.acked_through.insert(b, 2);
        metadata.truncate_log();
        assert_eq!(metadata.log.len(), 1);
        assert_eq!(metadata.log[0].id, 3);
    }
}
