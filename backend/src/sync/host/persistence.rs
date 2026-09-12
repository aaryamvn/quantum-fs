use super::*;

impl HostState {
    fn metadata(&self) -> ReplicaMetadata {
        let mut metadata = ReplicaMetadata::new(self.expected_root);
        metadata.members = self.members.clone();
        metadata.denied = self.denied.clone();
        metadata.historical_members = self.historical_members.clone();
        metadata.identity_documents = self.identity_documents.clone();
        metadata.bootstrap_through = self.bootstrap_through.clone();
        metadata.admission = self.admission.clone();
        metadata.dirents = self.tree.dirents();
        metadata.manifests = self
            .manifests
            .iter()
            .map(|(&id, trusted)| (id, trusted.manifest().clone()))
            .collect();
        metadata.log = self.log.clone();
        metadata.next_control = self.next_control;
        metadata.acked_through = self.acked_through.clone();
        metadata.mailboxes = self
            .mailboxes
            .iter()
            .map(|(&peer, queue)| {
                (
                    peer,
                    queue.iter().map(|entry| entry.content.clone()).collect(),
                )
            })
            .collect();
        // A returned online packet is not an acknowledgment. Keep every unacked
        // instruction recoverable even after take_online_control hands it out.
        for &peer in &self.members {
            if peer == self.host_id {
                continue;
            }
            let ack = self.acked_through.get(&peer).copied().unwrap_or(0);
            let covered = ack.max(self.bootstrap_through.get(&peer).copied().unwrap_or(0));
            let queue = metadata.mailboxes.entry(peer).or_default();
            let known: BTreeSet<_> = queue
                .iter()
                .filter_map(|entry| match entry {
                    QueueContent::Control(record) => Some(record.id),
                    _ => None,
                })
                .collect();
            let missing: Vec<_> = self
                .log
                .iter()
                .filter(|record| record.id > covered && !known.contains(&record.id))
                .cloned()
                .map(QueueContent::Control)
                .collect();
            // Online and mailbox queues do not interleave: earlier undelivered
            // controls are migrated by the existing commit path before bodies.
            if !missing.is_empty() {
                let mut controls: Vec<_> = queue
                    .iter()
                    .filter_map(|entry| match entry {
                        QueueContent::Control(record) => Some(record.clone()),
                        _ => None,
                    })
                    .chain(missing.into_iter().filter_map(|entry| match entry {
                        QueueContent::Control(record) => Some(record),
                        _ => None,
                    }))
                    .collect();
                controls.sort_by_key(|record| record.id);
                let mut merged: Vec<_> = controls.into_iter().map(QueueContent::Control).collect();
                merged.extend(
                    queue
                        .iter()
                        .filter(|entry| matches!(entry, QueueContent::Chunk { .. }))
                        .cloned(),
                );
                *queue = merged;
            }
        }
        metadata.mailboxes.retain(|_, queue| !queue.is_empty());
        metadata
    }
}

impl HostService {
    pub fn new_in_vault(
        keys: KeyStore,
        members: BTreeSet<PeerId>,
        chunks: SharedChunkStore,
        root: FileId,
    ) -> Result<Self> {
        let mut host = Self::new(keys, members, chunks)?;
        host.state.expected_root = root;
        host.state.tree = DirectoryTree::new(root);
        let metadata = host.state.metadata();
        host.state
            .chunks
            .lock()
            .map_err(|_| Error::State("chunk store poisoned"))?
            .set_metadata(metadata);
        Ok(host)
    }
    /// Open a single durable vault under this data directory's live keystore lock.
    pub fn open_durable(
        keys: KeyStore,
        data_dir: &Path,
        expected_root: FileId,
        initial_members: BTreeSet<PeerId>,
    ) -> Result<Self> {
        let (_, stored, chunks) = DurableStore::open(&keys, data_dir, expected_root)?;
        let local = keys.peer_id()?;
        let fresh = stored.is_none();
        let metadata = stored.unwrap_or_else(|| {
            let mut metadata = ReplicaMetadata::new(expected_root);
            metadata.members = initial_members;
            metadata
        });
        if !metadata.members.contains(&local) {
            return Err(Error::AuthenticationFailed);
        }
        let manifests = verified_manifests(&keys, &metadata)?;
        let tree = DirectoryTree::from_dirents(expected_root, metadata.dirents.clone())?;
        let mut host = Self {
            keys,
            state: HostState {
                expected_root,
                tree,
                acked_through: metadata.acked_through,
                host_id: local,
                members: metadata.members,
                denied: metadata.denied,
                historical_members: metadata.historical_members,
                identity_documents: metadata.identity_documents,
                bootstrap_through: metadata.bootstrap_through,
                admission: metadata.admission,
                chunks: Arc::new(Mutex::new(chunks)),
                manifests,
                log: metadata.log,
                next_control: metadata.next_control,
                mailboxes: metadata
                    .mailboxes
                    .into_iter()
                    .map(|(peer, queue)| {
                        (
                            peer,
                            queue
                                .into_iter()
                                .map(|content| Queued {
                                    envelope: MailboxEnvelope {
                                        recipient_id: peer,
                                        sender_id: local,
                                        epoch: Epoch(0),
                                        seq: Seq(0),
                                        queued_at: 0,
                                        ciphertext: Vec::new(),
                                    },
                                    content,
                                })
                                .collect(),
                        )
                    })
                    .collect(),
                online: BTreeMap::new(),
                gate_owner: u64::from_be_bytes(random_bytes()?),
            },
            presence: BTreeMap::new(),
            challenges: BTreeMap::new(),
            running: true,
            defer_persistence: false,
            admission_path: None,
            disconnects: Vec::new(),
            recent_ops: VecDeque::new(),
            last_seen: BTreeMap::new(),
        };
        for (&peer, queue) in &host.state.mailboxes {
            if !queue.is_empty() {
                host.keys.block_live_traffic(peer, host.state.gate_owner)?;
            }
        }
        if fresh {
            let staged = host.stage_state()?;
            host.publish_state(staged)?;
        }
        // Ciphertext is never loaded from disk. The transport must finish the
        // live wrap and call refresh_mailboxes before inspection or flush.
        Ok(host)
    }

    pub(super) fn stage_state(&self) -> Result<HostState> {
        let mut staged = self.state.clone();
        staged.chunks = Arc::new(Mutex::new(
            self.state
                .chunks
                .lock()
                .map_err(|_| Error::State("chunk store poisoned"))?
                .clone(),
        ));
        Ok(staged)
    }

    pub(super) fn publish_state(&mut self, mut staged: HostState) -> Result<()> {
        membership::archive_host(&self.keys, &mut staged)?;
        if let Some(admission) = &mut staged.admission {
            admission.members = staged.members.iter().copied().collect();
            admission.denied = staged.denied.clone();
        }
        let floor = staged
            .members
            .iter()
            .map(|peer| staged.acked_through.get(peer).copied().unwrap_or(0))
            .min()
            .unwrap_or(0);
        staged.log.retain(|record| record.id > floor);
        let metadata = staged.metadata();
        let mut next_chunks = staged
            .chunks
            .lock()
            .map_err(|_| Error::State("chunk store poisoned"))?
            .clone();
        // This is the visibility boundary: no queue/log/member assignment or
        // outgoing return occurs before the atomic metadata write succeeds.
        if self.defer_persistence {
            next_chunks.set_metadata(metadata);
        } else if let (Some(path), Some(admission)) = (&self.admission_path, &metadata.admission) {
            let bytes = encoding::encode_vault_metadata(admission)?;
            next_chunks.persist_metadata_with_admission(metadata, path, &bytes)?;
        } else {
            next_chunks.persist_metadata(metadata)?;
        }
        *self
            .state
            .chunks
            .lock()
            .map_err(|_| Error::State("chunk store poisoned"))? = next_chunks;
        staged.chunks = self.state.chunks.clone();
        self.state = staged;
        Ok(())
    }

    pub fn expected_root(&self) -> FileId {
        self.state.expected_root
    }

    /// Called only after a member has successfully applied H's instruction N.
    /// TCP flush acknowledgment supplies the same information from its batch.
    pub fn acknowledge_applied(&mut self, peer: PeerId, through: u64) -> Result<()> {
        self.require_running()?;
        self.require_member(peer)?;
        if through >= self.state.next_control {
            return Err(Error::InvalidInput("ack exceeds host log"));
        }
        if self.acked_through(peer) >= through {
            return Ok(());
        }
        let mut staged = self.stage_state()?;
        let ack = staged.acked_through.entry(peer).or_default();
        *ack = (*ack).max(through);
        self.publish_state(staged)
    }

    pub fn acked_through(&self, peer: PeerId) -> u64 {
        self.state.acked_through.get(&peer).copied().unwrap_or(0)
    }
}

impl MemberReplica {
    pub fn new_in_vault(
        keys: KeyStore,
        host_id: PeerId,
        members: BTreeSet<PeerId>,
        chunks: SharedChunkStore,
        root: FileId,
    ) -> Result<Self> {
        let mut replica = Self::new(keys, host_id, members, chunks)?;
        replica.expected_root = root;
        replica.tree = DirectoryTree::new(root);
        let metadata = replica.metadata_for(&replica.stage()?)?;
        replica
            .chunks
            .lock()
            .map_err(|_| Error::State("chunk store poisoned"))?
            .set_metadata(metadata);
        Ok(replica)
    }
    pub fn open_durable(
        keys: KeyStore,
        data_dir: &Path,
        expected_root: FileId,
        host_id: PeerId,
        initial_members: BTreeSet<PeerId>,
    ) -> Result<Self> {
        let (_, stored, chunks) = DurableStore::open(&keys, data_dir, expected_root)?;
        let local = keys.peer_id()?;
        let fresh = stored.is_none();
        let metadata = stored.unwrap_or_else(|| {
            let mut metadata = ReplicaMetadata::new(expected_root);
            metadata.members = initial_members;
            metadata
        });
        if !metadata.members.contains(&host_id) || !metadata.members.contains(&local) {
            return Err(Error::AuthenticationFailed);
        }
        if !metadata.mailboxes.is_empty() {
            return Err(Error::InvalidInput(
                "host replica cannot be opened as a member",
            ));
        }
        let manifests = verified_manifests(&keys, &metadata)?;
        let tree = DirectoryTree::from_dirents(expected_root, metadata.dirents.clone())?;
        let last_applied = metadata.acked_through.get(&local).copied().unwrap_or(0);
        let bootstrap_through = metadata.bootstrap_through.get(&local).copied().unwrap_or(0);
        let mut replica = Self {
            keys,
            host_id,
            members: metadata.members,
            denied: metadata.denied,
            historical_members: metadata.historical_members,
            identity_documents: metadata.identity_documents,
            chunks: Arc::new(Mutex::new(chunks)),
            manifests,
            controls: metadata
                .log
                .iter()
                .map(|record| (record.id, record.clone()))
                .collect(),
            log: metadata.log,
            receipts: Vec::new(),
            online_receipts: Vec::new(),
            expected_root,
            tree,
            last_applied,
            bootstrap_through,
        };
        if fresh {
            let staged = replica.stage()?;
            replica.apply_staged(staged)?;
        }
        Ok(replica)
    }

    pub(super) fn metadata_for(&self, staged: &StagedReplica) -> Result<ReplicaMetadata> {
        let mut metadata = ReplicaMetadata::new(self.expected_root);
        metadata.members = staged.members.clone();
        metadata.denied = staged.denied.clone();
        metadata.historical_members = staged.historical_members.clone();
        metadata.identity_documents = staged.identity_documents.clone();
        metadata.dirents = staged.tree.dirents();
        metadata.manifests = staged
            .manifests
            .iter()
            .map(|(&id, trusted)| (id, trusted.manifest().clone()))
            .collect();
        metadata.log = staged.log.clone();
        if staged.bootstrap_through != 0 {
            metadata
                .bootstrap_through
                .insert(self.keys.peer_id()?, staged.bootstrap_through);
        }
        let last = staged
            .log
            .last()
            .map_or(self.last_applied, |record| record.id)
            .max(staged.bootstrap_through);
        metadata.next_control = last
            .checked_add(1)
            .ok_or(Error::State("instruction ids exhausted"))?;
        metadata.acked_through.insert(self.keys.peer_id()?, last);
        Ok(metadata)
    }

    pub fn last_applied(&self) -> u64 {
        self.last_applied
    }
    pub fn expected_root(&self) -> FileId {
        self.expected_root
    }
    pub fn members(&self) -> &BTreeSet<PeerId> {
        &self.members
    }
}

fn verified_manifests(
    keys: &KeyStore,
    metadata: &ReplicaMetadata,
) -> Result<BTreeMap<FileId, TrustedManifest>> {
    for (&peer, document) in &metadata.identity_documents {
        if peer != document.peer_id {
            return Err(Error::AuthenticationFailed);
        }
        document.verify()?;
    }
    metadata
        .manifests
        .iter()
        .map(|(&id, manifest)| {
            Ok((id, {
                let writer = match metadata.identity_documents.get(&manifest.writer_id) {
                    Some(document) => document.clone(),
                    None if manifest.writer_id == keys.peer_id()? => keys.identity()?,
                    None => keys.load_verified_peer(&manifest.writer_id)?,
                };
                TrustedManifest::verify_accepted(manifest.clone(), &writer)?
            }))
        })
        .collect()
}

pub(super) fn verify_record_manifest(
    manifest: Manifest,
    keys: &KeyStore,
    members: &BTreeSet<PeerId>,
    archive: &BTreeMap<PeerId, IdentityDocument>,
) -> Result<TrustedManifest> {
    if !members.contains(&manifest.writer_id) {
        return Err(Error::AuthenticationFailed);
    }
    if let Some(writer) = archive.get(&manifest.writer_id) {
        TrustedManifest::verify_accepted(manifest, writer)
    } else {
        TrustedManifest::verify(manifest, keys, members)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::wrap::{ConstructionBWrap, RustCryptoConstructionBWrap};

    #[test]
    fn failed_snapshot_keeps_host_state_and_outgoing_controls_unpublished() -> Result<()> {
        let directory = std::env::temp_dir().join(format!(
            "qfs-host-persist-failure-{}-{}",
            std::process::id(),
            u64::from_be_bytes(random_bytes()?)
        ));
        std::fs::create_dir_all(&directory)?;
        let result = (|| -> Result<()> {
            let keys = KeyStore::open(&directory.join("identity"))?;
            let member = KeyStore::open(&directory.join("member"))?;
            let local = keys.peer_id()?;
            let peer = member.peer_id()?;
            keys.import_peer(member.identity()?)?;
            member.import_peer(keys.identity()?)?;
            let (sender, recipient) = if local < peer {
                (&keys, &member)
            } else {
                (&member, &keys)
            };
            let (_, wrap) = RustCryptoConstructionBWrap::new(sender.clone()).create(
                recipient.peer_id()?,
                &recipient.identity()?.ek,
                Epoch(1),
            )?;
            RustCryptoConstructionBWrap::new(recipient.clone()).unwrap(sender.peer_id()?, &wrap)?;
            let mut host = HostService::open_durable(
                keys.clone(),
                &directory,
                FileId([0; 32]),
                [local, peer].into(),
            )?;
            host.heartbeat(peer, Duration::from_secs(60))?;
            let before = std::fs::read(directory.join("replica.bin"))?;
            host.chunks()
                .lock()
                .map_err(|_| Error::State("chunk store poisoned"))?
                .fail_next_persist();
            assert!(host
                .fan_out_control(local, &ControlUpdate::Add(FileId([1; 32])))
                .is_err());
            assert!(host.instruction_log().is_empty());
            assert_eq!(before, std::fs::read(directory.join("replica.bin"))?);
            keys.require_live_traffic(peer)?;
            assert!(host.take_online_control(peer)?.is_empty());
            // Failure is injected before I/O: the unchanged instance can retry.
            host.fan_out_control(local, &ControlUpdate::Add(FileId([1; 32])))?;
            assert_eq!(host.instruction_log().len(), 1);
            assert_eq!(host.take_online_control(peer)?.len(), 1);
            let before_tree = host.tree().clone();
            host.chunks()
                .lock()
                .map_err(|_| Error::State("chunk store poisoned"))?
                .fail_next_persist();
            assert!(host.mkdir(peer, "/blocked").is_err());
            assert_eq!(host.tree(), &before_tree);
            assert!(host.take_online_control(peer)?.is_empty());
            let old_log_len = host.instruction_log().len();
            host.chunks()
                .lock()
                .map_err(|_| Error::State("chunk store poisoned"))?
                .fail_next_persist();
            assert!(host
                .save_file(&member, "/new-file", &[b"saved bytes".to_vec()])
                .is_err());
            assert_eq!(host.tree(), &before_tree);
            assert_eq!(host.instruction_log().len(), old_log_len);
            assert!(host
                .chunks()
                .lock()
                .map_err(|_| Error::State("chunk store poisoned"))?
                .is_empty());
            assert!(host.take_online_control(peer)?.is_empty());
            let before_generation = encoding::decode_replica(
                &std::fs::read(directory.join("replica.bin"))?,
                FileId([0; 32]),
            )?
            .0;
            let file_id = host.save_file(&member, "/new-file", &[b"saved bytes".to_vec()])?;
            assert_eq!(host.tree().resolve("/new-file")?, file_id);
            assert_eq!(
                encoding::decode_replica(
                    &std::fs::read(directory.join("replica.bin"))?,
                    FileId([0; 32])
                )?
                .0,
                before_generation + 1
            );
            assert_eq!(host.take_online_control(peer)?.len(), 2);
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(directory);
        result
    }
}
