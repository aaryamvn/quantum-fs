mod eviction;
mod filesystem;
mod membership;
mod persistence;
use persistence::verify_record_manifest;

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    crypto::{
        aead::{Aes256Gcm, RustCryptoAes256Gcm},
        identity::IdentityDocument,
        sign::{PureMlDsa, RustCryptoPureMlDsa, FLUSH_CONTEXT},
    },
    demo_log::{self, Kind},
    encoding::{self, MailboxFrame},
    ids::{ChunkId, Epoch, FileId, PeerId, Seq},
    keystore::{random_bytes, IdentityKeyStore, KeyStore},
    net::{join::VaultMetadata, JoinCode},
    protocol::{
        manifest::{Manifest, TrustedManifest},
        packet::{ControlPacket, PacketHeader, PayloadType, PROTOCOL_VERSION},
        pull::ChunkBodyFrame,
    },
    store::{
        chunks::{ChunkStore, MemoryChunkStore, SharedChunkStore},
        durable::DurableStore,
        replica::ReplicaMetadata,
        tree::DirectoryTree,
    },
    sync::pull::{encrypt_at_send, open_chunk},
    Error, Result,
};

#[derive(Clone, PartialEq, Eq)]
pub struct Presence {
    pub peer_id: PeerId,
    pub ttl: Duration,
}

#[derive(Clone, PartialEq, Eq)]
pub struct MailboxEnvelope {
    pub recipient_id: PeerId,
    pub sender_id: PeerId,
    pub epoch: Epoch,
    pub seq: Seq,
    pub queued_at: u64,
    /// Typed framing around one GCM ciphertext; never a second AEAD layer.
    pub ciphertext: Vec<u8>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct FlushChallenge(pub [u8; 32]);

#[derive(Clone, PartialEq, Eq)]
pub enum ControlUpdate {
    NewManifest(Manifest),
    Add(FileId),
    Kick(PeerId),
    Clear(FileId),
    Remove(FileId),
    Link {
        parent: FileId,
        name: String,
        child: FileId,
        is_dir: bool,
    },
    Unlink {
        parent: FileId,
        name: String,
    },
    Rename {
        src_parent: FileId,
        src_name: String,
        dst_parent: FileId,
        dst_name: String,
    },
}

#[derive(Clone, PartialEq, Eq)]
pub struct ControlRecord {
    pub id: u64,
    pub update: ControlUpdate,
}

#[derive(Clone, PartialEq, Eq)]
pub enum QueueContent {
    Control(ControlRecord),
    Chunk {
        file_id: FileId,
        index: u64,
        chunk_id: ChunkId,
    },
}

#[derive(Clone)]
struct Queued {
    envelope: MailboxEnvelope,
    content: QueueContent,
}

#[derive(Clone)]
struct OnlineControl {
    packet: ControlPacket,
    record: ControlRecord,
    queued_at: u64,
}

/// One vault's staged state. In-memory stores support hand-off with into_state.
/// A durable chunk store holds the keystore lock: drop it before reopening from disk.
#[derive(Clone)]
pub struct HostState {
    expected_root: FileId,
    tree: DirectoryTree,
    acked_through: BTreeMap<PeerId, u64>,
    host_id: PeerId,
    members: BTreeSet<PeerId>,
    denied: BTreeSet<PeerId>,
    historical_members: BTreeSet<PeerId>,
    identity_documents: BTreeMap<PeerId, IdentityDocument>,
    chunks: SharedChunkStore,
    manifests: BTreeMap<FileId, TrustedManifest>,
    log: Vec<ControlRecord>,
    next_control: u64,
    mailboxes: BTreeMap<PeerId, VecDeque<Queued>>,
    online: BTreeMap<PeerId, Vec<OnlineControl>>,
    gate_owner: u64,
    admission: Option<VaultMetadata>,
}

/// Compromising H compromises all shared plaintext; H is TCB for all shared files.
/// Each instance serves one vault in the ordinary member's qfsd process.
pub struct HostService {
    keys: KeyStore,
    state: HostState,
    presence: BTreeMap<PeerId, Instant>,
    challenges: BTreeMap<PeerId, FlushChallenge>,
    running: bool,
    defer_persistence: bool,
    admission_path: Option<PathBuf>,
    disconnects: Vec<PeerId>,
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct FlushReport {
    pub controls: usize,
    pub chunks_written: usize,
}

pub struct PreparedHostFlush {
    peer_id: PeerId,
    challenge: FlushChallenge,
    envelopes: Vec<MailboxEnvelope>,
}

impl PreparedHostFlush {
    pub fn envelopes(&self) -> &[MailboxEnvelope] {
        &self.envelopes
    }
}

pub struct PreparedReplicaFlush {
    peer_id: PeerId,
    host_id: PeerId,
    base_controls: BTreeMap<u64, ControlRecord>,
    staged: StagedReplica,
    report: FlushReport,
}

/// A recipient's applied host controls and plaintext replica. Only this control
/// path can turn received signed manifests into TrustedManifest capabilities.
pub struct MemberReplica {
    keys: KeyStore,
    host_id: PeerId,
    members: BTreeSet<PeerId>,
    denied: BTreeSet<PeerId>,
    historical_members: BTreeSet<PeerId>,
    identity_documents: BTreeMap<PeerId, IdentityDocument>,
    chunks: SharedChunkStore,
    manifests: BTreeMap<FileId, TrustedManifest>,
    controls: BTreeMap<u64, ControlRecord>,
    log: Vec<ControlRecord>,
    receipts: Vec<(MailboxEnvelope, Opened)>,
    online_receipts: Vec<ControlPacket>,
    expected_root: FileId,
    tree: DirectoryTree,
    last_applied: u64,
}

#[derive(Clone)]
enum Opened {
    Control(ControlRecord),
    Chunk {
        file_id: FileId,
        index: u64,
        plaintext: Vec<u8>,
    },
}

struct StagedReplica {
    members: BTreeSet<PeerId>,
    denied: BTreeSet<PeerId>,
    historical_members: BTreeSet<PeerId>,
    identity_documents: BTreeMap<PeerId, IdentityDocument>,
    tree: DirectoryTree,
    chunks: MemoryChunkStore,
    manifests: BTreeMap<FileId, TrustedManifest>,
    controls: BTreeMap<u64, ControlRecord>,
    log: Vec<ControlRecord>,
}

impl HostService {
    pub fn new(
        keys: KeyStore,
        members: BTreeSet<PeerId>,
        chunks: SharedChunkStore,
    ) -> Result<Self> {
        let host_id = keys.peer_id()?;
        if !members.contains(&host_id) {
            return Err(Error::InvalidInput("H must be a vault member"));
        }
        let mut metadata = ReplicaMetadata::new(FileId([0; 32]));
        metadata.members = members.clone();
        metadata.dirents = DirectoryTree::new(FileId([0; 32])).dirents();
        chunks
            .lock()
            .map_err(|_| Error::State("chunk store poisoned"))?
            .set_metadata(metadata);
        Ok(Self {
            keys,
            state: HostState {
                expected_root: FileId([0; 32]),
                tree: DirectoryTree::new(FileId([0; 32])),
                acked_through: BTreeMap::new(),
                host_id,
                historical_members: members.clone(),
                identity_documents: BTreeMap::new(),
                denied: BTreeSet::new(),
                members,
                chunks,
                manifests: BTreeMap::new(),
                log: Vec::new(),
                next_control: 1,
                mailboxes: BTreeMap::new(),
                online: BTreeMap::new(),
                gate_owner: u64::from_be_bytes(random_bytes()?),
                admission: None,
            },
            presence: BTreeMap::new(),
            challenges: BTreeMap::new(),
            running: true,
            defer_persistence: false,
            admission_path: None,
            disconnects: Vec::new(),
        })
    }

    pub fn resume(keys: KeyStore, state: HostState) -> Result<Self> {
        if keys.peer_id()? != state.host_id {
            return Err(Error::AuthenticationFailed);
        }
        let mut host = Self {
            keys,
            state,
            presence: BTreeMap::new(),
            challenges: BTreeMap::new(),
            running: true,
            defer_persistence: false,
            admission_path: None,
            disconnects: Vec::new(),
        };
        // Undelivered live controls become queued catch-up on a replacement host.
        let pending = std::mem::take(&mut host.state.online);
        for (peer, updates) in pending {
            let queue = host.state.mailboxes.entry(peer).or_default();
            for update in updates {
                queue.push_back(Queued {
                    envelope: control_envelope(&update.packet, update.queued_at)?,
                    content: QueueContent::Control(update.record),
                });
            }
        }
        for (&peer, queue) in &host.state.mailboxes {
            if !queue.is_empty() {
                host.keys.block_live_traffic(peer, host.state.gate_owner)?;
            }
        }
        host.refresh_mailboxes()?;
        Ok(host)
    }

    pub fn into_state(self) -> HostState {
        self.state
    }
    pub fn stop(&mut self) {
        self.running = false;
    }
    pub fn is_running(&self) -> bool {
        self.running
    }
    pub fn chunks(&self) -> SharedChunkStore {
        self.state.chunks.clone()
    }
    pub fn instruction_log(&self) -> &[ControlRecord] {
        &self.state.log
    }
    pub fn trusted_manifest(&self, file_id: &FileId) -> Option<&TrustedManifest> {
        self.state.manifests.get(file_id)
    }
    pub fn members(&self) -> &BTreeSet<PeerId> {
        &self.state.members
    }
    pub fn has_member(&self, peer_id: &PeerId) -> bool {
        self.state.members.contains(peer_id)
    }
    pub fn add_member(
        &mut self,
        document: crate::crypto::identity::IdentityDocument,
    ) -> Result<()> {
        self.require_running()?;
        document.verify()?;
        let peer_id = document.peer_id;
        if self.state.denied.contains(&peer_id) {
            return Err(Error::AuthenticationFailed);
        }
        self.keys.import_peer(document.clone())?;
        let mut staged = self.stage_state()?;
        staged.identity_documents.insert(peer_id, document);
        staged.historical_members.insert(peer_id);
        staged.members.insert(peer_id);
        self.publish_state(staged)
    }

    fn require_running(&self) -> Result<()> {
        if self.running {
            Ok(())
        } else {
            Err(Error::State("host is not running"))
        }
    }
    fn require_member(&self, peer: PeerId) -> Result<()> {
        if self.state.members.contains(&peer) {
            Ok(())
        } else {
            Err(Error::AuthenticationFailed)
        }
    }

    pub fn heartbeat(&mut self, peer: PeerId, ttl: Duration) -> Result<()> {
        self.require_running()?;
        self.require_member(peer)?;
        let expires = Instant::now()
            .checked_add(ttl)
            .ok_or(Error::InvalidInput("presence TTL overflow"))?;
        self.presence.insert(peer, expires);
        // Heartbeats never clear queued traffic gates.
        Ok(())
    }

    pub fn presence(&self, peer: PeerId) -> Result<Option<Presence>> {
        self.require_running()?;
        self.require_member(peer)?;
        Ok(self
            .presence
            .get(&peer)
            .and_then(|expires| expires.checked_duration_since(Instant::now()))
            .filter(|ttl| !ttl.is_zero())
            .map(|ttl| Presence { peer_id: peer, ttl }))
    }

    pub fn commit(&mut self, manifest: Manifest) -> Result<()> {
        self.require_running()?;
        let trusted = TrustedManifest::verify(manifest, &self.keys, &self.state.members)?;
        // No chunk identifiers are interpreted before the writer's signature verifies.
        validate_replica(&self.state.chunks, &trusted)?;
        if self
            .state
            .manifests
            .get(&trusted.manifest().file_id)
            .is_some_and(|old| old.manifest() == trusted.manifest())
        {
            return Ok(());
        }
        self.commit_verified(
            ControlUpdate::NewManifest(trusted.manifest().clone()),
            Some(trusted),
        )
    }

    fn commit_instruction(&mut self, update: ControlUpdate) -> Result<()> {
        match update {
            ControlUpdate::NewManifest(manifest) => self.commit(manifest),
            ControlUpdate::Kick(target) => self.kick(target).map(|_| ()),
            update => {
                self.require_running()?;
                self.commit_verified(update, None)
            }
        }
    }

    /// Online fan-out contains authenticated control only; offline file bytes
    /// are enqueued by the commit path using the shared encrypt-at-send helper.
    pub fn fan_out_control(&mut self, sender_id: PeerId, update: &ControlUpdate) -> Result<()> {
        self.require_running()?;
        self.require_member(sender_id)?;
        if let ControlUpdate::NewManifest(manifest) = update {
            if manifest.writer_id != sender_id {
                return Err(Error::AuthenticationFailed);
            }
        } else if !matches!(
            update,
            ControlUpdate::Link { .. }
                | ControlUpdate::Unlink { .. }
                | ControlUpdate::Rename { .. }
        ) && sender_id != self.state.host_id
        {
            return Err(Error::AuthenticationFailed);
        }
        self.commit_instruction(update.clone())
    }

    fn commit_verified(
        &mut self,
        update: ControlUpdate,
        trusted: Option<TrustedManifest>,
    ) -> Result<()> {
        let kicked = if let ControlUpdate::Kick(target) = &update {
            Some(*target)
        } else {
            None
        };
        filesystem::validate_link_kind(&self.state.manifests, &update)?;
        let (tree, removed_file) = filesystem::apply_tree_update(&self.state.tree, &update)?;
        self.refresh_mailboxes()?;
        let next = self
            .state
            .next_control
            .checked_add(1)
            .ok_or(Error::State("instruction ids exhausted"))?;
        let record = ControlRecord {
            id: self.state.next_control,
            update,
        };
        let queued_at = unix_time()?;
        let recipients: Vec<_> = self
            .state
            .members
            .iter()
            .copied()
            .filter(|id| *id != self.state.host_id && Some(*id) != kicked)
            .collect();
        // Resolve every pair before any queue mutation or plaintext application.
        for &peer in &recipients {
            self.keys.current_session(peer)?;
        }
        let mut queues = self.state.mailboxes.clone();
        let mut online = self.state.online.clone();
        let mut blocked = Vec::new();
        let prepared = (|| -> Result<()> {
            for peer in recipients {
                let live =
                    self.presence(peer)?.is_some() && self.keys.require_live_traffic(peer).is_ok();
                if !live {
                    self.keys.block_live_traffic(peer, self.state.gate_owner)?;
                    blocked.push(peer);
                }
                let session = self.keys.current_session(peer)?;
                // Older undelivered controls get current-epoch counters before
                // this commit, so migration to a mailbox preserves FIFO order.
                if let Some(pending) = online.get_mut(&peer) {
                    for old in pending {
                        if old.packet.header.epoch != session.epoch {
                            old.packet = seal_control(&self.keys, &session, &old.record)?;
                        }
                    }
                }
                let packet = seal_control(&self.keys, &session, &record)?;
                if live {
                    online.entry(peer).or_default().push(OnlineControl {
                        packet,
                        record: record.clone(),
                        queued_at,
                    });
                    continue;
                }
                let queue = queues.entry(peer).or_default();
                if let Some(undelivered) = online.remove(&peer) {
                    for old in undelivered {
                        queue.push_back(Queued {
                            envelope: control_envelope(&old.packet, old.queued_at)?,
                            content: QueueContent::Control(old.record),
                        });
                    }
                }
                queue.push_back(Queued {
                    envelope: control_envelope(&packet, queued_at)?,
                    content: QueueContent::Control(record.clone()),
                });
                if let Some(file_id) = removed_file {
                    queue.retain(|entry| !matches!(entry.content, QueueContent::Chunk {file_id: old, ..} if old == file_id));
                }
                if matches!(&record.update, ControlUpdate::NewManifest(_)) {
                    let current = trusted
                        .as_ref()
                        .ok_or(Error::State("missing verified commit"))?
                        .manifest();
                    // Remove obsolete bodies, never instruction records. A replacement
                    // is appended at the end so its higher seq cannot precede older seqs.
                    queue.retain(|entry| match entry.content {
                        QueueContent::Chunk {
                            file_id,
                            index,
                            chunk_id,
                        } if file_id == current.file_id => {
                            usize::try_from(index)
                                .ok()
                                .and_then(|i| current.chunk_ids.get(i))
                                == Some(&chunk_id)
                        }
                        _ => true,
                    });
                    let prior = self.state.manifests.get(&current.file_id);
                    let chunks = self
                        .state
                        .chunks
                        .lock()
                        .map_err(|_| Error::State("chunk store poisoned"))?;
                    for (index, &chunk_id) in current.chunk_ids.iter().enumerate() {
                        let changed = prior.is_none_or(|old| {
                            old.manifest().chunk_ids.get(index) != Some(&chunk_id)
                        });
                        if !changed {
                            continue;
                        }
                        let index = u64::try_from(index)
                            .map_err(|_| Error::InvalidInput("chunk index overflow"))?;
                        let plaintext = chunks
                            .get(&chunk_id)
                            .ok_or(Error::State("committed plaintext missing"))?;
                        let frame = encrypt_at_send(
                            &self.keys,
                            &session,
                            current.file_id,
                            index,
                            plaintext,
                        )?;
                        queue.retain(|entry| !matches!(entry.content,
                                QueueContent::Chunk {file_id, index: old_index, ..} if file_id == current.file_id && old_index == index));
                        queue.push_back(Queued {
                            envelope: chunk_envelope(&frame, queued_at)?,
                            content: QueueContent::Chunk {
                                file_id: current.file_id,
                                index,
                                chunk_id,
                            },
                        });
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = prepared {
            for peer in blocked {
                if self
                    .state
                    .mailboxes
                    .get(&peer)
                    .is_none_or(VecDeque::is_empty)
                {
                    self.keys
                        .unblock_live_traffic(peer, self.state.gate_owner)?;
                }
            }
            return Err(error);
        }
        let applied = (|| -> Result<()> {
            let mut staged = self.stage_state()?;
            staged.tree = tree;
            if let ControlUpdate::NewManifest(manifest) = &record.update {
                staged.manifests.insert(
                    manifest.file_id,
                    trusted.ok_or(Error::State("missing trusted commit"))?,
                );
            }
            if let Some(file_id) = removed_file {
                staged
                    .chunks
                    .lock()
                    .map_err(|_| Error::State("chunk store poisoned"))?
                    .remove_file(&file_id);
                staged.manifests.remove(&file_id);
            }
            staged.mailboxes = queues;
            staged.online = online;
            if let Some(target) = kicked {
                staged.members.remove(&target);
                staged.acked_through.remove(&target);
                staged.mailboxes.remove(&target);
                staged.online.remove(&target);
                staged.denied.insert(target);
                if let Some(admission) = &mut staged.admission {
                    admission.join_code = JoinCode::generate()?;
                    let next_ad = unix_time()?.max(
                        admission
                            .issued_at
                            .checked_add(1)
                            .ok_or(Error::State("ad timestamp exhausted"))?,
                    );
                    admission.issued_at = next_ad
                        .checked_add(1)
                        .ok_or(Error::State("ad timestamp exhausted"))?;
                }
            }
            staged.acked_through.insert(staged.host_id, record.id);
            staged.log.push(record);
            staged.next_control = next;
            self.publish_state(staged)
        })();
        if applied.is_err() {
            for peer in blocked {
                if self
                    .state
                    .mailboxes
                    .get(&peer)
                    .is_none_or(VecDeque::is_empty)
                {
                    self.keys
                        .unblock_live_traffic(peer, self.state.gate_owner)?;
                }
            }
        }
        applied
    }

    pub fn take_online_control(&mut self, recipient: PeerId) -> Result<Vec<ControlPacket>> {
        self.require_running()?;
        self.require_member(recipient)?;
        self.keys.require_live_traffic(recipient)?;
        let session = self.keys.current_session(recipient)?;
        if self.state.online.get(&recipient).is_none_or(Vec::is_empty) {
            return Ok(Vec::new());
        }
        if let Some(pending) = self.state.online.get_mut(&recipient) {
            for old in pending {
                if old.packet.header.epoch != session.epoch {
                    old.packet = seal_control(&self.keys, &session, &old.record)?;
                }
            }
        }
        let mut staged = self.stage_state()?;
        let packets = staged
            .online
            .remove(&recipient)
            .unwrap_or_default()
            .into_iter()
            .map(|entry| entry.packet)
            .collect();
        self.publish_state(staged)?;
        Ok(packets)
    }

    /// Refresh before queue inspection, flush, new commits, or after a daemon rotation.
    /// Rebuild each frame from the plaintext/control replica using the current key.
    pub fn refresh_mailboxes(&mut self) -> Result<()> {
        self.require_running()?;
        for (&peer, queue) in &mut self.state.mailboxes {
            if queue.is_empty() {
                continue;
            }
            self.keys.block_live_traffic(peer, self.state.gate_owner)?;
            let session = self.keys.current_session(peer)?;
            let stale_chunks = queue.iter().any(|entry| {
                entry.envelope.epoch != session.epoch
                    && matches!(entry.content, QueueContent::Chunk { .. })
            });
            let new_chunks = queue.iter().any(|entry| {
                entry.envelope.epoch == session.epoch
                    && matches!(entry.content, QueueContent::Chunk { .. })
            });
            if stale_chunks
                && !new_chunks
                && self
                    .keys
                    .next_outbound_seq(session.key_handle(), PayloadType::ChunkBody)?
                    != Seq(1)
            {
                return Err(Error::State("new epoch was used before mailbox re-seal"));
            }
            for entry in queue.iter_mut() {
                if entry.envelope.epoch == session.epoch {
                    continue;
                }
                let time = entry.envelope.queued_at;
                entry.envelope = match &entry.content {
                    QueueContent::Control(record) => {
                        control_envelope(&seal_control(&self.keys, &session, record)?, time)?
                    }
                    QueueContent::Chunk {
                        file_id,
                        index,
                        chunk_id,
                    } => {
                        let chunks = self
                            .state
                            .chunks
                            .lock()
                            .map_err(|_| Error::State("chunk store poisoned"))?;
                        let plaintext = chunks
                            .get(chunk_id)
                            .ok_or(Error::State("queued plaintext missing"))?;
                        chunk_envelope(
                            &encrypt_at_send(&self.keys, &session, *file_id, *index, plaintext)?,
                            time,
                        )?
                    }
                };
            }
        }
        Ok(())
    }

    pub fn mailbox(&mut self, recipient: PeerId) -> Result<Vec<MailboxEnvelope>> {
        self.require_member(recipient)?;
        self.refresh_mailboxes()?;
        Ok(self
            .state
            .mailboxes
            .get(&recipient)
            .map(|queue| queue.iter().map(|entry| entry.envelope.clone()).collect())
            .unwrap_or_default())
    }

    /// Queue a committed plaintext slot, sealing once with the current pair key.
    /// The caller supplies metadata, never unauthenticated ciphertext provenance.
    pub fn append_mailbox(&mut self, recipient: PeerId, file_id: FileId, index: u64) -> Result<()> {
        self.require_running()?;
        self.require_member(recipient)?;
        let trusted = self
            .state
            .manifests
            .get(&file_id)
            .ok_or(Error::AuthenticationFailed)?;
        let slot =
            usize::try_from(index).map_err(|_| Error::InvalidInput("chunk index overflow"))?;
        let chunk_id = *trusted
            .manifest()
            .chunk_ids
            .get(slot)
            .ok_or(Error::AuthenticationFailed)?;
        self.refresh_mailboxes()?;
        let session = self.keys.current_session(recipient)?;
        let chunks = self
            .state
            .chunks
            .lock()
            .map_err(|_| Error::State("chunk store poisoned"))?;
        let plaintext = chunks
            .get(&chunk_id)
            .ok_or(Error::State("queued plaintext missing"))?;
        self.keys
            .block_live_traffic(recipient, self.state.gate_owner)?;
        let sealed = encrypt_at_send(&self.keys, &session, file_id, index, plaintext)?;
        drop(chunks);
        let mut staged = self.stage_state()?;
        let queue = staged.mailboxes.entry(recipient).or_default();
        queue.retain(|entry| {
            !matches!(entry.content, QueueContent::Chunk {file_id:old,index:old_index,..}
            if old == file_id && old_index == index)
        });
        queue.push_back(Queued {
            envelope: chunk_envelope(&sealed, unix_time()?)?,
            content: QueueContent::Chunk {
                file_id,
                index,
                chunk_id,
            },
        });
        let result = self.publish_state(staged);
        if result.is_err()
            && self
                .state
                .mailboxes
                .get(&recipient)
                .is_none_or(VecDeque::is_empty)
        {
            self.keys
                .unblock_live_traffic(recipient, self.state.gate_owner)?;
        }
        result
    }

    pub fn issue_flush_challenge(&mut self, recipient: PeerId) -> Result<FlushChallenge> {
        self.require_running()?;
        self.require_member(recipient)?;
        let challenge = FlushChallenge(random_bytes()?);
        self.challenges.insert(recipient, challenge.clone());
        Ok(challenge)
    }

    pub fn prepare_flush(
        &mut self,
        peer: PeerId,
        challenge: &FlushChallenge,
        signature: &[u8],
    ) -> Result<PreparedHostFlush> {
        self.require_running()?;
        self.require_member(peer)?;
        if self.challenges.get(&peer) != Some(challenge) {
            return Err(Error::AuthenticationFailed);
        }
        let identity = self.keys.load_verified_peer(&peer)?;
        RustCryptoPureMlDsa.verify(
            &identity.vk,
            FLUSH_CONTEXT,
            &encoding::flush_m(challenge),
            signature,
        )?;
        self.refresh_mailboxes()?;
        self.keys.block_live_traffic(peer, self.state.gate_owner)?;
        let envelopes = self
            .state
            .mailboxes
            .get(&peer)
            .map(|queue| queue.iter().map(|entry| entry.envelope.clone()).collect())
            .unwrap_or_default();
        Ok(PreparedHostFlush {
            peer_id: peer,
            challenge: challenge.clone(),
            envelopes,
        })
    }

    pub fn acknowledge_flush(&mut self, peer: PeerId, prepared: PreparedHostFlush) -> Result<()> {
        self.require_running()?;
        if prepared.peer_id != peer || self.challenges.get(&peer) != Some(&prepared.challenge) {
            return Err(Error::AuthenticationFailed);
        }
        let current: Vec<_> = self
            .state
            .mailboxes
            .get(&peer)
            .map(|queue| queue.iter().map(|entry| entry.envelope.clone()).collect())
            .unwrap_or_default();
        if current != prepared.envelopes {
            return Err(Error::State("mailbox changed before flush acknowledgement"));
        }
        let mut staged = self.stage_state()?;
        let last = staged
            .mailboxes
            .get(&peer)
            .into_iter()
            .flatten()
            .filter_map(|entry| match &entry.content {
                QueueContent::Control(record) => Some(record.id),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        let ack = staged.acked_through.entry(peer).or_default();
        *ack = (*ack).max(last);
        staged.mailboxes.remove(&peer);
        self.publish_state(staged)?;
        self.challenges.remove(&peer);
        self.keys.unblock_live_traffic(peer, self.state.gate_owner)
    }

    /// Completes a FIFO authenticated drain in-process. Receipt caching permits
    /// retry after a corrupt body without reopening counters already accepted.
    pub fn flush_mailbox(
        &mut self,
        recipient: &mut MemberReplica,
        challenge: &FlushChallenge,
        signature: &[u8],
    ) -> Result<FlushReport> {
        self.require_running()?;
        let peer = recipient.keys.peer_id()?;
        self.require_member(peer)?;
        if recipient.host_id != self.state.host_id || self.challenges.get(&peer) != Some(challenge)
        {
            return Err(Error::AuthenticationFailed);
        }
        // Finish the live epoch wrap before re-sealing or opening any queued bytes.
        if recipient.keys.current_session(self.state.host_id)?.epoch
            != self.keys.current_session(peer)?.epoch
        {
            return Err(Error::State(
                "finish the current pair wrap before mailbox flush",
            ));
        }
        let prepared = self.prepare_flush(peer, challenge, signature)?;
        recipient
            .keys
            .block_live_traffic(self.state.host_id, self.state.gate_owner)?;
        let staged = recipient
            .prepare_mailbox_with_history(prepared.envelopes(), &self.historical_documents())?;
        let report = recipient.commit_prepared(staged)?;
        recipient.finish_receipts();
        self.acknowledge_flush(peer, prepared)?;
        recipient
            .keys
            .unblock_live_traffic(self.state.host_id, self.state.gate_owner)?;
        if report.controls != 0 || report.chunks_written != 0 {
            demo_log::event(
                Kind::Sync,
                "ML-DSA-65 + AES-256-GCM",
                format!("offline mailbox restored  {}", demo_log::peer(peer)),
                &[
                    format!("{} authenticated control(s)", report.controls),
                    format!("{} encrypted chunk body(s) written", report.chunks_written),
                ],
            );
        }
        Ok(report)
    }
}

impl MemberReplica {
    pub fn new(
        keys: KeyStore,
        host_id: PeerId,
        members: BTreeSet<PeerId>,
        chunks: SharedChunkStore,
    ) -> Result<Self> {
        if !members.contains(&host_id) || !members.contains(&keys.peer_id()?) {
            return Err(Error::AuthenticationFailed);
        }
        let mut metadata = ReplicaMetadata::new(FileId([0; 32]));
        metadata.members = members.clone();
        metadata.dirents = DirectoryTree::new(FileId([0; 32])).dirents();
        chunks
            .lock()
            .map_err(|_| Error::State("chunk store poisoned"))?
            .set_metadata(metadata);
        Ok(Self {
            keys,
            host_id,
            historical_members: members.clone(),
            identity_documents: BTreeMap::new(),
            denied: BTreeSet::new(),
            members,
            chunks,
            manifests: BTreeMap::new(),
            controls: BTreeMap::new(),
            log: Vec::new(),
            receipts: Vec::new(),
            online_receipts: Vec::new(),
            expected_root: FileId([0; 32]),
            tree: DirectoryTree::new(FileId([0; 32])),
            last_applied: 0,
        })
    }
    pub fn chunks(&self) -> SharedChunkStore {
        self.chunks.clone()
    }
    pub fn trusted_manifest(&self, file_id: &FileId) -> Option<&TrustedManifest> {
        self.manifests.get(file_id)
    }
    pub fn instruction_log(&self) -> &[ControlRecord] {
        &self.log
    }
    pub fn host_id(&self) -> PeerId {
        self.host_id
    }
    pub fn peer_id(&self) -> Result<PeerId> {
        self.keys.peer_id()
    }
    pub fn add_members(&mut self, members: &BTreeSet<PeerId>) -> Result<()> {
        if !members.contains(&self.host_id) || !members.contains(&self.keys.peer_id()?) {
            return Err(Error::AuthenticationFailed);
        }
        let mut staged = self.stage()?;
        staged
            .members
            .extend(members.difference(&staged.denied).copied());
        staged.historical_members.extend(members.iter().copied());
        self.apply_staged(staged)
    }

    fn stage(&self) -> Result<StagedReplica> {
        Ok(StagedReplica {
            members: self.members.clone(),
            denied: self.denied.clone(),
            historical_members: self.historical_members.clone(),
            identity_documents: self.identity_documents.clone(),
            tree: self.tree.clone(),
            chunks: self
                .chunks
                .lock()
                .map_err(|_| Error::State("chunk store poisoned"))?
                .clone(),
            manifests: self.manifests.clone(),
            controls: self.controls.clone(),
            log: self.log.clone(),
        })
    }
    fn apply_staged(&mut self, mut staged: StagedReplica) -> Result<()> {
        membership::archive_replica(&self.keys, &mut staged)?;
        let metadata = self.metadata_for(&staged)?;
        staged.chunks.persist_metadata(metadata)?;
        let last = staged
            .log
            .last()
            .map_or(self.last_applied, |record| record.id);
        *self
            .chunks
            .lock()
            .map_err(|_| Error::State("chunk store poisoned"))? = staged.chunks;
        let kicked: Vec<_> = staged.denied.difference(&self.denied).copied().collect();
        self.members = staged.members;
        self.denied = staged.denied;
        self.historical_members = staged.historical_members;
        self.identity_documents = staged.identity_documents;
        self.tree = staged.tree;
        self.manifests = staged.manifests;
        self.controls = staged.controls;
        self.log = staged.log;
        self.last_applied = last;
        for peer in kicked {
            self.keys.discard_pair(peer)?;
        }
        Ok(())
    }
    fn stage_control(&self, staged: &mut StagedReplica, record: &ControlRecord) -> Result<bool> {
        if let Some(prior) = staged.controls.get(&record.id) {
            return if prior == record {
                Ok(false)
            } else {
                Err(Error::AuthenticationFailed)
            };
        }
        if staged
            .controls
            .last_key_value()
            .is_some_and(|(&last, _)| record.id <= last)
        {
            return Err(Error::State("host instructions arrived out of order"));
        }
        filesystem::validate_link_kind(&staged.manifests, &record.update)?;
        let (tree, removed_file) = filesystem::apply_tree_update(&staged.tree, &record.update)?;
        if let ControlUpdate::NewManifest(manifest) = &record.update {
            // H's authenticated history supplies admissions before the retained
            // record; a later Kick removes the writer from the running set.
            if !staged.denied.contains(&manifest.writer_id)
                && staged.historical_members.contains(&manifest.writer_id)
            {
                staged.members.insert(manifest.writer_id);
            }
            let trusted = verify_record_manifest(
                manifest.clone(),
                &self.keys,
                &staged.members,
                &staged.identity_documents,
            )?;
            staged.manifests.insert(trusted.manifest().file_id, trusted);
        }
        if let ControlUpdate::Kick(target) = &record.update {
            if *target == self.host_id || !staged.members.remove(target) {
                return Err(Error::AuthenticationFailed);
            }
            staged.denied.insert(*target);
        }
        if let Some(file_id) = removed_file {
            staged.chunks.remove_file(&file_id);
            staged.manifests.remove(&file_id);
        }
        staged.tree = tree;
        staged.controls.insert(record.id, record.clone());
        staged.log.push(record.clone());
        Ok(true)
    }
    fn check_header(&self, header: &PacketHeader) -> Result<()> {
        if header.sender_id != self.host_id
            || header.receiver_id != self.keys.peer_id()?
            || header.version != PROTOCOL_VERSION
            || header.epoch != self.keys.current_session(self.host_id)?.epoch
        {
            return Err(Error::AuthenticationFailed);
        }
        Ok(())
    }
    pub fn apply_control(&mut self, packet: &ControlPacket) -> Result<()> {
        self.keys.require_live_traffic(self.host_id)?;
        self.check_header(&packet.header)?;
        if self.online_receipts.contains(packet) {
            return Ok(());
        }
        let session = self.keys.current_session(self.host_id)?;
        let plaintext = RustCryptoAes256Gcm::new(self.keys.clone()).open(
            session.key_handle(),
            &encoding::nonce(&packet.header, PayloadType::Packet),
            &encoding::packet_aad(&packet.header),
            &packet.ciphertext,
        )?;
        let record = encoding::decode_control_record(&plaintext)?;
        let mut staged = self.stage()?;
        self.stage_control(&mut staged, &record)?;
        self.apply_staged(staged)?;
        self.online_receipts.push(packet.clone());
        Ok(())
    }

    pub fn prepare_mailbox(
        &mut self,
        envelopes: &[MailboxEnvelope],
    ) -> Result<PreparedReplicaFlush> {
        self.prepare_mailbox_with_history(envelopes, &[])
    }

    /// Public identity history is staged, never published before the transport
    /// authenticates the entire batch and calls commit_prepared. This lets an
    /// offline member verify writers admitted after its last applied record.
    pub fn prepare_mailbox_with_history(
        &mut self,
        envelopes: &[MailboxEnvelope],
        history: &[IdentityDocument],
    ) -> Result<PreparedReplicaFlush> {
        self.receipts.retain(|(prior, _)| envelopes.contains(prior));
        let mut staged = self.stage()?;
        for document in history {
            document.verify()?;
            staged.historical_members.insert(document.peer_id);
            staged
                .identity_documents
                .insert(document.peer_id, document.clone());
        }
        let mut report = FlushReport::default();
        let session = self.keys.current_session(self.host_id)?;
        let mut last_packet = None;
        let mut last_chunk = None;
        let mut bodies = Vec::new();
        for envelope in envelopes {
            let header = PacketHeader {
                version: PROTOCOL_VERSION,
                sender_id: envelope.sender_id,
                receiver_id: envelope.recipient_id,
                epoch: envelope.epoch,
                seq: envelope.seq,
            };
            self.check_header(&header)?;
            let frame = encoding::decode_mailbox_frame(&envelope.ciphertext)?;
            let last = match frame {
                MailboxFrame::Control { .. } => &mut last_packet,
                MailboxFrame::ChunkBody { .. } => &mut last_chunk,
            };
            if last.is_some_and(|seq| envelope.seq <= seq) {
                return Err(Error::State("mailbox counters are not in queued order"));
            }
            *last = Some(envelope.seq);
            let cached = self
                .receipts
                .iter()
                .find(|(prior, _)| prior == envelope)
                .map(|(_, opened)| opened.clone());
            let opened = if let Some(opened) = cached {
                opened
            } else {
                match frame {
                    MailboxFrame::Control { ciphertext } => {
                        let plaintext = RustCryptoAes256Gcm::new(self.keys.clone()).open(
                            session.key_handle(),
                            &encoding::nonce(&header, PayloadType::Packet),
                            &encoding::packet_aad(&header),
                            &ciphertext,
                        )?;
                        Opened::Control(encoding::decode_control_record(&plaintext)?)
                    }
                    MailboxFrame::ChunkBody {
                        file_id,
                        index,
                        ciphertext,
                    } => {
                        let trusted = staged
                            .manifests
                            .get(&file_id)
                            .ok_or(Error::AuthenticationFailed)?;
                        let plaintext = open_chunk(
                            &self.keys,
                            &session,
                            &ChunkBodyFrame {
                                header,
                                file_id,
                                index,
                                ciphertext,
                            },
                            trusted,
                        )?;
                        Opened::Chunk {
                            file_id,
                            index,
                            plaintext,
                        }
                    }
                }
            };
            // Cache authenticated plaintext before dependency validation. A
            // damaged public history offer can omit a writer; retry with the
            // authentic offer must not reopen an already accepted GCM counter.
            if !self.receipts.iter().any(|(prior, _)| prior == envelope) {
                self.receipts.push((envelope.clone(), opened.clone()));
            }
            match &opened {
                Opened::Control(record) => {
                    if self.stage_control(&mut staged, record)? {
                        report.controls += 1;
                    }
                }
                Opened::Chunk {
                    file_id,
                    index,
                    plaintext,
                } => {
                    // Recheck cached plaintext against the staged control path too.
                    let trusted = staged
                        .manifests
                        .get(file_id)
                        .ok_or(Error::AuthenticationFailed)?;
                    let slot = usize::try_from(*index).map_err(|_| Error::AuthenticationFailed)?;
                    if trusted.manifest().chunk_ids.get(slot)
                        != Some(&encoding::chunk_id(file_id, *index, plaintext))
                    {
                        return Err(Error::AuthenticationFailed);
                    }
                    bodies.push((*file_id, *index, plaintext.clone()));
                }
            }
        }
        // All instructions apply before any body reaches the actual replica.
        for (file_id, index, plaintext) in bodies {
            let id = encoding::chunk_id(&file_id, index, &plaintext);
            let slot = usize::try_from(index).map_err(|_| Error::AuthenticationFailed)?;
            if staged
                .manifests
                .get(&file_id)
                .and_then(|m| m.manifest().chunk_ids.get(slot))
                != Some(&id)
            {
                continue;
            }
            if !staged.tree.is_linked_file(&file_id) {
                continue;
            }
            if !staged.chunks.has(&id) {
                staged.chunks.put(&file_id, index, plaintext)?;
                report.chunks_written += 1;
            }
        }
        Ok(PreparedReplicaFlush {
            peer_id: self.keys.peer_id()?,
            host_id: self.host_id,
            base_controls: self.controls.clone(),
            staged,
            report,
        })
    }

    pub fn commit_prepared(&mut self, prepared: PreparedReplicaFlush) -> Result<FlushReport> {
        if prepared.peer_id != self.keys.peer_id()?
            || prepared.host_id != self.host_id
            || prepared.base_controls != self.controls
        {
            return Err(Error::State("replica changed before prepared flush commit"));
        }
        self.apply_staged(prepared.staged)?;
        Ok(prepared.report)
    }

    pub fn finish_receipts(&mut self) {
        self.receipts.clear();
    }
}

fn unix_time() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|_| Error::State("clock predates Unix epoch"))
}

fn seal_control(
    keys: &KeyStore,
    session: &crate::crypto::wrap::PairSession,
    record: &ControlRecord,
) -> Result<ControlPacket> {
    let header = PacketHeader {
        version: PROTOCOL_VERSION,
        sender_id: keys.peer_id()?,
        receiver_id: session.peer_id,
        epoch: session.epoch,
        seq: keys.next_outbound_seq(session.key_handle(), PayloadType::Packet)?,
    };
    let ciphertext = RustCryptoAes256Gcm::new(keys.clone()).seal(
        session.key_handle(),
        &encoding::nonce(&header, PayloadType::Packet),
        &encoding::packet_aad(&header),
        &encoding::encode_control_record(record)?,
    )?;
    Ok(ControlPacket { header, ciphertext })
}

fn control_envelope(packet: &ControlPacket, queued_at: u64) -> Result<MailboxEnvelope> {
    Ok(MailboxEnvelope {
        recipient_id: packet.header.receiver_id,
        sender_id: packet.header.sender_id,
        epoch: packet.header.epoch,
        seq: packet.header.seq,
        queued_at,
        ciphertext: encoding::encode_mailbox_frame(&MailboxFrame::Control {
            ciphertext: packet.ciphertext.clone(),
        })?,
    })
}
fn chunk_envelope(frame: &ChunkBodyFrame, queued_at: u64) -> Result<MailboxEnvelope> {
    Ok(MailboxEnvelope {
        recipient_id: frame.header.receiver_id,
        sender_id: frame.header.sender_id,
        epoch: frame.header.epoch,
        seq: frame.header.seq,
        queued_at,
        ciphertext: encoding::encode_mailbox_frame(&MailboxFrame::ChunkBody {
            file_id: frame.file_id,
            index: frame.index,
            ciphertext: frame.ciphertext.clone(),
        })?,
    })
}
fn validate_replica(chunks: &SharedChunkStore, trusted: &TrustedManifest) -> Result<()> {
    let manifest = trusted.manifest();
    let chunks = chunks
        .lock()
        .map_err(|_| Error::State("chunk store poisoned"))?;
    let mut size = 0u64;
    for (index, id) in manifest.chunk_ids.iter().enumerate() {
        let index = u64::try_from(index).map_err(|_| Error::AuthenticationFailed)?;
        let record = chunks
            .get_record(id)
            .ok_or(Error::State("committed chunk missing on H"))?;
        if record.file_id != manifest.file_id
            || record.index != index
            || encoding::chunk_id(&manifest.file_id, index, &record.plaintext) != *id
        {
            return Err(Error::AuthenticationFailed);
        }
        size = size
            .checked_add(
                u64::try_from(record.plaintext.len()).map_err(|_| Error::AuthenticationFailed)?,
            )
            .ok_or(Error::AuthenticationFailed)?;
    }
    if size != manifest.size {
        return Err(Error::AuthenticationFailed);
    }
    Ok(())
}

// Live cursors use direct pairwise GCM, no DSA, and never pass through H.
// If H is down, commits fail; successor election is outside v1.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        crypto::{
            sign::MANIFEST_CONTEXT,
            wrap::{ConstructionBWrap, RustCryptoConstructionBWrap},
        },
        protocol::pull::PullRequest,
        store::chunks::shared_chunk_store,
        sync::pull::InProcessPullCoordinator,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct TestDirectory(std::path::PathBuf);
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn corrupt_late_mailbox_body_is_atomic_and_retry_does_not_reopen_accepted_counters(
    ) -> Result<()> {
        let directory = TestDirectory(std::env::temp_dir().join(format!(
            "qfs-drain-retry-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )));
        std::fs::create_dir(&directory.0)?;
        let host_keys = KeyStore::open(&directory.0.join("host"))?;
        let member_keys = KeyStore::open(&directory.0.join("member"))?;
        let host_id = host_keys.peer_id()?;
        let member_id = member_keys.peer_id()?;
        host_keys.import_peer(member_keys.identity()?)?;
        member_keys.import_peer(host_keys.identity()?)?;
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
        let members = BTreeSet::from([host_id, member_id]);
        let chunks = shared_chunk_store();
        let replica_chunks = shared_chunk_store();
        let mut host = HostService::new(host_keys.clone(), members.clone(), chunks.clone())?;
        let mut replica = MemberReplica::new(
            member_keys.clone(),
            host_id,
            members,
            replica_chunks.clone(),
        )?;
        let file_id = FileId([71; 32]);
        host.link_file(host_id, "file", file_id)?;
        let ids = {
            let mut chunks = chunks
                .lock()
                .map_err(|_| Error::State("test lock poisoned"))?;
            vec![
                chunks.put(&file_id, 0, b"first".to_vec())?,
                chunks.put(&file_id, 1, b"second".to_vec())?,
            ]
        };
        let mut manifest = Manifest {
            file_id,
            chunk_ids: ids.clone(),
            size: 11,
            writer_id: host_id,
            version: 1,
            signature: Vec::new(),
        };
        manifest.signature = RustCryptoPureMlDsa.sign(
            &host_keys.signing_key()?,
            MANIFEST_CONTEXT,
            &encoding::manifest_m(&manifest)?,
        )?;
        host.commit(manifest)?;
        let original = host
            .state
            .mailboxes
            .get(&member_id)
            .and_then(|q| q.back())
            .ok_or(Error::State("test queue missing"))?
            .envelope
            .clone();
        let mut corrupt = original.clone();
        let byte = corrupt
            .ciphertext
            .last_mut()
            .ok_or(Error::State("empty test ciphertext"))?;
        *byte ^= 1;
        host.state
            .mailboxes
            .get_mut(&member_id)
            .and_then(|q| q.back_mut())
            .ok_or(Error::State("test queue missing"))?
            .envelope = corrupt;
        let challenge = host.issue_flush_challenge(member_id)?;
        let signature = RustCryptoPureMlDsa.sign(
            &member_keys.signing_key()?,
            FLUSH_CONTEXT,
            &encoding::flush_m(&challenge),
        )?;
        assert!(host
            .flush_mailbox(&mut replica, &challenge, &signature)
            .is_err());
        assert!(replica_chunks
            .lock()
            .map_err(|_| Error::State("test lock poisoned"))?
            .is_empty());
        assert!(replica.instruction_log().is_empty());
        assert!(replica.trusted_manifest(&file_id).is_none());
        assert!(host_keys.require_live_traffic(member_id).is_err());
        assert!(member_keys.require_live_traffic(host_id).is_err());
        assert_eq!(replica.receipts.len(), 3); // link, manifest and first body authenticated
        host.state
            .mailboxes
            .get_mut(&member_id)
            .and_then(|q| q.back_mut())
            .ok_or(Error::State("test queue missing"))?
            .envelope = original;
        // Same challenge remains usable after failed apply. Failed GCM did not
        // accept its counter; successful earlier frames use exact receipts.
        let report = host.flush_mailbox(&mut replica, &challenge, &signature)?;
        assert_eq!(
            report,
            FlushReport {
                controls: 2,
                chunks_written: 2
            }
        );
        assert!(host
            .flush_mailbox(&mut replica, &challenge, &signature)
            .is_err());
        assert!(host.mailbox(member_id)?.is_empty());
        assert!(host_keys.require_live_traffic(member_id).is_ok());
        assert!(member_keys.require_live_traffic(host_id).is_ok());
        let pull = InProcessPullCoordinator::new(host_keys, chunks);
        assert_eq!(pull.serve(&PullRequest::new(ids)?, member_id)?.len(), 2);
        Ok(())
    }
}
