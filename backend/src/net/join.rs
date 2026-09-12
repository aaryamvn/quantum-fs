use super::{JoinCode, VaultId};
use crate::{
    crypto::{
        identity::IdentityDocument,
        sign::{PureMlDsa, RustCryptoPureMlDsa, JOIN_CONTEXT, MANIFEST_CONTEXT},
        wrap::PairSession,
    },
    demo_log::{self, Kind},
    encoding,
    keystore::KeyStore,
    Error, Result,
};

#[derive(Clone, PartialEq, Eq)]
pub struct JoinRequest {
    pub vault_id: VaultId,
    pub join_code: JoinCode,
    pub document: IdentityDocument,
    pub signature: Vec<u8>,
}

impl JoinRequest {
    pub fn sign(keys: &KeyStore, vault_id: VaultId, join_code: JoinCode) -> Result<Self> {
        let mut request = Self {
            vault_id,
            join_code,
            document: keys.identity()?,
            signature: Vec::new(),
        };
        request.signature = RustCryptoPureMlDsa.sign(
            &keys.signing_key()?,
            JOIN_CONTEXT,
            &encoding::join_request_m(&request)?,
        )?;
        Ok(request)
    }

    pub fn verify(&self, exchanged: &IdentityDocument) -> Result<()> {
        if self.document != *exchanged {
            return Err(Error::AuthenticationFailed);
        }
        RustCryptoPureMlDsa.verify(
            &exchanged.vk,
            JOIN_CONTEXT,
            &encoding::join_request_m(self)?,
            &self.signature,
        )
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct NetWelcome {
    pub vault_id: VaultId,
    pub members: Vec<IdentityDocument>,
    pub historical: Vec<IdentityDocument>,
    pub denied: BTreeSet<PeerId>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct FlushOffer {
    pub challenge: crate::sync::host::FlushChallenge,
    pub frame_count: u32,
    pub historical: Vec<IdentityDocument>,
    pub bootstrap_through: Option<u64>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum NetControl {
    JoinAccepted {
        vault_id: VaultId,
        members: Vec<IdentityDocument>,
        historical: Vec<IdentityDocument>,
        denied: BTreeSet<PeerId>,
    },
    JoinRejected {
        bound: VaultId,
        requested: VaultId,
    },
    AdmissionDenied,
    FlushEnd {
        digest: [u8; 32],
    },
    FlushApplied {
        digest: [u8; 32],
    },
    Ready,
    Heartbeat,
    HeartbeatApplied {
        through: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultMetadata {
    pub vault_id: VaultId,
    pub join_code: JoinCode,
    pub issued_at: u64,
    pub members: Vec<crate::ids::PeerId>,
    pub denied: BTreeSet<PeerId>,
}

use super::{
    directory::{DirForget, DirectoryAd, DirectoryClient},
    frame::{self, read_frame, write_frame as write_wire_frame, Frame},
    session::{self, HandshakeProgress},
};
use crate::{
    ids::{FileId, PeerId},
    keystore::{random_bytes, IdentityKeyStore},
    protocol::{
        locate::{HaveQuery, HaveReply, HAVE_MAGIC},
        manifest::{Manifest, TrustedManifest},
        packet::{PacketHeader, PROTOCOL_VERSION},
        pull::{ChunkBodyFrame, PullRequest, PullResponse},
    },
    store::chunks::{shared_chunk_store, ChunkStore},
    sync::{
        host::{
            ControlRecord, ControlUpdate, HostService, HostState, MailboxEnvelope, MemberReplica,
        },
        pull::{encrypt_at_send, open_chunk, InProcessPullCoordinator},
    },
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    net::SocketAddr,
    path::Path,
    rc::Rc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::net::{TcpListener, TcpStream};

pub const HANDSHAKE_DEADLINE: Duration = Duration::from_secs(5);
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(100);
pub const MAX_CONNECTIONS: usize = 128;
pub const MAX_PROVISIONAL: usize = 32;
const MAX_DRAIN_BYTES: usize = 64 * 1024 * 1024;
const MAX_DRAIN_FRAMES: u32 = 100_000;
const TRANSPORT_GATE: u64 = u64::MAX;

pub fn unix_time() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|_| Error::State("clock predates Unix epoch"))
}

/// One hosted vault; durable metadata contains no file replica or pair keys.
pub struct VaultHost {
    pub keys: KeyStore,
    pub host: HostService,
    active_peers: BTreeSet<PeerId>,
}

impl VaultHost {
    pub fn load_or_create(keys: KeyStore, path: &Path) -> Result<Self> {
        let metadata = match crate::keystore::read_private(path)? {
            Some(bytes) => encoding::decode_vault_metadata(&bytes)?,
            None => VaultMetadata {
                vault_id: VaultId::generate()?,
                join_code: JoinCode::generate()?,
                issued_at: 0,
                members: vec![keys.peer_id()?],
                denied: BTreeSet::new(),
            },
        };
        let members: BTreeSet<_> = metadata.members.iter().copied().collect();
        let mut host = HostService::new_in_vault(
            keys.clone(),
            members,
            shared_chunk_store(),
            FileId(metadata.vault_id.0),
        )?;
        host.attach_admission(path, metadata)?;
        let vault = Self {
            keys,
            host,
            active_peers: BTreeSet::new(),
        };
        Ok(vault)
    }
    pub fn open_durable(keys: KeyStore, path: &Path, data_dir: &Path) -> Result<Self> {
        crate::store::transaction::recover_admission_transaction(path)?;
        let metadata = match crate::keystore::read_private(path)? {
            Some(bytes) => encoding::decode_vault_metadata(&bytes)?,
            None => VaultMetadata {
                vault_id: VaultId::generate()?,
                join_code: JoinCode::generate()?,
                issued_at: 0,
                members: vec![keys.peer_id()?],
                denied: BTreeSet::new(),
            },
        };
        let mut host = HostService::open_durable(
            keys.clone(),
            data_dir,
            crate::ids::FileId(metadata.vault_id.0),
            metadata.members.iter().copied().collect(),
        )?;
        host.attach_admission(path, metadata)?;
        Ok(Self {
            keys,
            host,
            active_peers: BTreeSet::new(),
        })
    }
    pub fn resume(keys: KeyStore, path: &Path, state: HostState) -> Result<Self> {
        let metadata = match crate::keystore::read_private(path)? {
            Some(bytes) => encoding::decode_vault_metadata(&bytes)?,
            None => return Err(Error::State("vault admission metadata is missing")),
        };
        let mut host = HostService::resume(keys.clone(), state)?;
        host.attach_admission(path, metadata)?;
        Ok(Self {
            keys,
            host,
            active_peers: BTreeSet::new(),
        })
    }
    pub fn vault_id(&self) -> VaultId {
        self.admission().vault_id
    }
    pub fn join_code(&self) -> JoinCode {
        self.admission().join_code
    }
    fn admission(&self) -> &VaultMetadata {
        self.host
            .admission()
            .expect("VaultHost always attaches admission metadata")
    }
    pub async fn publish(
        &mut self,
        directory: &DirectoryClient,
        addr: SocketAddr,
    ) -> Result<DirectoryAd> {
        let issued_at = unix_time()?.max(
            self.admission()
                .issued_at
                .checked_add(1)
                .ok_or(Error::State("ad timestamp exhausted"))?,
        );
        let mut metadata = self.admission().clone();
        let ad = DirectoryAd::sign(&self.keys, metadata.vault_id, addr, issued_at)?;
        directory.put(metadata.join_code, &ad).await?;
        metadata.issued_at = issued_at;
        self.host.set_admission(metadata)?;
        Ok(ad)
    }
    pub async fn rotate_code(
        vault: &Rc<RefCell<Self>>,
        directory: &DirectoryClient,
        addr: SocketAddr,
    ) -> Result<JoinCode> {
        let (code, ad, forget) = {
            let mut state = vault.borrow_mut();
            let old = state.admission().join_code;
            let code = JoinCode::generate()?;
            let issued_at = unix_time()?.max(
                state
                    .admission()
                    .issued_at
                    .checked_add(1)
                    .ok_or(Error::State("ad timestamp exhausted"))?,
            );
            let forgotten_at = issued_at
                .checked_add(1)
                .ok_or(Error::State("ad timestamp exhausted"))?;
            let mut metadata = state.admission().clone();
            let ad = DirectoryAd::sign(&state.keys, metadata.vault_id, addr, issued_at)?;
            let forget = DirForget::sign(&state.keys, metadata.vault_id, old, forgotten_at)?;
            metadata.join_code = code;
            metadata.issued_at = forgotten_at;
            // Admission changes before directory I/O; the borrow is released so
            // concurrent connections observe the new code immediately.
            state.host.set_admission(metadata)?;
            (code, ad, forget)
        };
        directory.put(code, &ad).await?;
        directory.forget(&forget).await?;
        Ok(code)
    }
    fn admit(&mut self, request: &JoinRequest, peer: &IdentityDocument) -> Result<()> {
        request.verify(peer)?;
        if request.vault_id != self.admission().vault_id
            || request.join_code != self.admission().join_code
            || self.host.denied().contains(&peer.peer_id)
        {
            return Err(Error::AuthenticationFailed);
        }
        if !self.host.is_running() {
            return Err(Error::State("host is not running"));
        }
        self.host.add_member(peer.clone())?;
        Ok(())
    }
    fn welcome(&self) -> Result<NetControl> {
        let mut members = Vec::new();
        for &peer in self.host.members() {
            members.push(if peer == self.keys.peer_id()? {
                self.keys.identity()?
            } else {
                self.keys.load_verified_peer(&peer)?
            });
        }
        Ok(NetControl::JoinAccepted {
            vault_id: self.admission().vault_id,
            members,
            historical: self.host.historical_documents(),
            denied: self.host.denied().clone(),
        })
    }

    pub async fn kick(
        vault: &Rc<RefCell<Self>>,
        directory: &DirectoryClient,
        addr: SocketAddr,
        target: PeerId,
    ) -> Result<PeerId> {
        let (kicked, code, ad, forget) = {
            let mut state = vault.borrow_mut();
            let old = state.join_code();
            let kicked = state.host.kick(target)?;
            let metadata = state.admission().clone();
            let forgotten_at = metadata.issued_at;
            let issued_at = forgotten_at
                .checked_sub(1)
                .ok_or(Error::State("ad timestamp is not reserved"))?;
            let ad = DirectoryAd::sign(&state.keys, metadata.vault_id, addr, issued_at)?;
            let forget = DirForget::sign(&state.keys, metadata.vault_id, old, forgotten_at)?;
            (kicked, metadata.join_code, ad, forget)
        };
        directory.put(code, &ad).await?;
        directory.forget(&forget).await?;
        Ok(kicked)
    }

    pub fn take_disconnects(&mut self) -> Vec<PeerId> {
        self.host.take_disconnects()
    }
}

struct CounterPermit(Rc<Cell<usize>>);
impl CounterPermit {
    fn acquire(counter: &Rc<Cell<usize>>, limit: usize) -> Option<Self> {
        if counter.get() >= limit {
            return None;
        }
        counter.set(counter.get() + 1);
        Some(Self(counter.clone()))
    }
}
impl Drop for CounterPermit {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

struct HandshakePermit {
    peers: Rc<RefCell<BTreeSet<PeerId>>>,
    peer: PeerId,
}

impl HandshakePermit {
    fn acquire(peers: &Rc<RefCell<BTreeSet<PeerId>>>, peer: PeerId) -> Result<Self> {
        if !peers.borrow_mut().insert(peer) {
            return Err(Error::State("peer handshake already in progress"));
        }
        Ok(Self {
            peers: peers.clone(),
            peer,
        })
    }
}

impl Drop for HandshakePermit {
    fn drop(&mut self) {
        self.peers.borrow_mut().remove(&self.peer);
    }
}

struct PeerPermit {
    vault: Rc<RefCell<VaultHost>>,
    vaults: crate::net::vaults::VaultSet,
    vault_id: VaultId,
    peer: PeerId,
}
impl Drop for PeerPermit {
    fn drop(&mut self) {
        if let Ok(mut vault) = self.vault.try_borrow_mut() {
            vault.active_peers.remove(&self.peer);
            let _ = vault.host.heartbeat(self.peer, Duration::ZERO);
        }
        if self.vault.borrow().keys.current_session(self.peer).is_err() {
            self.vaults.unbind(self.peer, self.vault_id);
        }
        demo_log::event(
            Kind::Membership,
            "AES-256-GCM",
            "Vault member disconnected",
            &[format!("peer  {}", demo_log::peer(self.peer))],
        );
    }
}

pub async fn serve_host<V>(listener: TcpListener, vaults: V) -> Result<()>
where
    V: Into<crate::net::vaults::VaultSet>,
{
    let vaults = vaults.into();
    let connections = Rc::new(Cell::new(0));
    let provisional = Rc::new(Cell::new(0));
    let handshakes = Rc::new(RefCell::new(BTreeSet::new()));
    loop {
        let _ = vaults.take_disconnects();
        let (mut stream, _) = tokio::select! {
            accepted = listener.accept() => accepted?,
            _ = tokio::time::sleep(Duration::from_millis(25)) => continue,
        };
        let Some(connection) = CounterPermit::acquire(&connections, MAX_CONNECTIONS) else {
            continue;
        };
        let Some(provisional) = CounterPermit::acquire(&provisional, MAX_PROVISIONAL) else {
            continue;
        };
        let vaults = vaults.clone();
        let handshakes = handshakes.clone();
        tokio::task::spawn_local(async move {
            let _connection = connection;
            let mut progress = HandshakeProgress::default();
            let keys = vaults.keys();
            let mut handshake_permit = None;
            let admission = tokio::time::timeout(HANDSHAKE_DEADLINE, async {
                let established = session::establish_guarded(
                    &mut stream,
                    &keys,
                    None,
                    &mut progress,
                    |peer_id| {
                        handshake_permit = Some(HandshakePermit::acquire(&handshakes, peer_id)?);
                        Ok(vaults.has_confirmed_pair(peer_id))
                    },
                )
                .await?;
                let peer = established.peer;
                let selected = established.session;
                let frame = read_after_ack(&mut stream, selected.epoch).await?;
                if frame.kind != frame::GCM_PACKET_KIND {
                    let denial = send_control_for_session(
                        &mut stream,
                        &keys,
                        &selected,
                        &NetControl::AdmissionDenied,
                    )
                    .await;
                    if !progress.had_pair {
                        keys.discard_pair(peer.peer_id)?;
                    }
                    denial?;
                    return Err(Error::AuthenticationFailed);
                }
                let request = match (|| -> Result<_> {
                    let packet = encoding::decode_control_packet(&frame.payload)?;
                    let plaintext = session::open_packet_with_session(&keys, &selected, &packet)?;
                    encoding::decode_join_request(&plaintext, &peer)
                })() {
                    Ok(request) => request,
                    Err(error) => {
                        let denial = send_control_for_session(
                            &mut stream,
                            &keys,
                            &selected,
                            &NetControl::AdmissionDenied,
                        )
                        .await;
                        if !progress.had_pair {
                            keys.discard_pair(peer.peer_id)?;
                        }
                        denial?;
                        return Err(error);
                    }
                };
                if let Some(bound) = vaults.bound_vault(peer.peer_id) {
                    if bound != request.vault_id {
                        let denial = send_control_for_session(
                            &mut stream,
                            &keys,
                            &selected,
                            &NetControl::JoinRejected {
                                bound,
                                requested: request.vault_id,
                            },
                        )
                        .await;
                        discard_rejected_candidate(&keys, &selected, progress.had_pair)?;
                        denial?;
                        return Err(Error::VaultSessionConflict {
                            peer_id: peer.peer_id,
                            bound,
                            requested: request.vault_id,
                        });
                    }
                }
                let admit = (|| -> Result<_> {
                    let vault = vaults
                        .get(request.vault_id)
                        .ok_or(Error::AuthenticationFailed)?;
                    {
                        let mut state = vault.borrow_mut();
                        if !state.active_peers.insert(peer.peer_id) {
                            return Err(Error::State("peer already connected"));
                        }
                        if let Err(error) = state.admit(&request, &peer) {
                            state.active_peers.remove(&peer.peer_id);
                            return Err(error);
                        }
                    }
                    if keys
                        .candidate_session(peer.peer_id)
                        .is_ok_and(|s| s.epoch == selected.epoch)
                    {
                        let old = keys.current_session(peer.peer_id)?;
                        keys.promote_candidate(peer.peer_id, selected.epoch)?;
                        // active_peers rejected any still-live old TCP before promotion.
                        // Mailboxes retain plaintext and refresh under the new epoch.
                        keys.retire(old.key_handle())?;
                    }
                    vaults.bind(peer.peer_id, request.vault_id)?;
                    keys.block_live_traffic(peer.peer_id, TRANSPORT_GATE)?;
                    Ok((vault, request.vault_id))
                })();
                let (vault, vault_id) = match admit {
                    Ok(value) => value,
                    Err(error) => {
                        let denial = send_control_for_session(
                            &mut stream,
                            &keys,
                            &selected,
                            &NetControl::AdmissionDenied,
                        )
                        .await;
                        discard_rejected_candidate(&keys, &selected, progress.had_pair)?;
                        denial?;
                        return Err(error);
                    }
                };
                let permit = PeerPermit {
                    vault: vault.clone(),
                    vaults: vaults.clone(),
                    vault_id,
                    peer: peer.peer_id,
                };
                Ok((peer.peer_id, vault, permit))
            })
            .await;
            drop(handshake_permit);
            drop(provisional);
            let result = match admission {
                Ok(Ok((peer, vault, permit))) => {
                    let _peer = permit;
                    async {
                        flush_to_peer(&mut stream, &vault, peer).await?;
                        let welcome = vault.borrow().welcome()?;
                        send_control(&mut stream, &keys, peer, &welcome).await?;
                        keys.unblock_live_traffic(peer, TRANSPORT_GATE)?;
                        vault.borrow_mut().host.heartbeat(peer, IDLE_TIMEOUT)?;
                        demo_log::event(
                            Kind::Membership,
                            "X-Wing + ML-DSA-65",
                            "qfsd: accepted vault member; pair live",
                            &[format!("peer  {}", demo_log::peer(peer))],
                        );
                        serve_live(&mut stream, &vault, peer).await
                    }
                    .await
                }
                Ok(Err(error)) => Err(error),
                Err(_) => Err(Error::State("handshake deadline exceeded")),
            };
            if let Err(error) = result {
                demo_log::event(
                    Kind::Warning,
                    "TCP",
                    "qfsd: peer connection closed",
                    &[format!("reason  {error}")],
                );
            }
        });
    }
}

fn discard_rejected_candidate(
    keys: &KeyStore,
    session: &PairSession,
    had_pair: bool,
) -> Result<()> {
    if keys
        .candidate_session(session.peer_id)
        .is_ok_and(|s| s.epoch == session.epoch)
    {
        keys.discard_candidate(session.peer_id)
    } else if !had_pair {
        keys.discard_pair(session.peer_id)
    } else {
        Ok(())
    }
}

async fn read_after_ack(stream: &mut TcpStream, epoch: crate::ids::Epoch) -> Result<Frame> {
    loop {
        let frame = read_frame(stream).await?;
        if frame.kind == frame::WRAP_ACK_KIND {
            let (ack, retry) = encoding::decode_wrap_ack(&frame.payload)?;
            if ack == epoch && !retry {
                continue;
            }
        }
        return Ok(frame);
    }
}

async fn send_control(
    stream: &mut TcpStream,
    keys: &KeyStore,
    peer: PeerId,
    control: &NetControl,
) -> Result<()> {
    send_control_for_session(stream, keys, &keys.current_session(peer)?, control).await
}

async fn send_control_for_session(
    stream: &mut TcpStream,
    keys: &KeyStore,
    selected: &PairSession,
    control: &NetControl,
) -> Result<()> {
    let packet =
        session::seal_packet_with_session(keys, selected, &encoding::encode_net_control(control)?)?;
    send_frame(
        stream,
        &Frame::new(
            frame::GCM_PACKET_KIND,
            encoding::encode_control_packet(&packet)?,
        )?,
    )
    .await
}
async fn read_control(stream: &mut TcpStream, keys: &KeyStore, peer: PeerId) -> Result<NetControl> {
    read_control_for_session(stream, keys, &keys.current_session(peer)?).await
}

async fn read_control_for_session(
    stream: &mut TcpStream,
    keys: &KeyStore,
    selected: &PairSession,
) -> Result<NetControl> {
    let received = read_after_ack(stream, selected.epoch).await?;
    if received.kind != frame::GCM_PACKET_KIND {
        return Err(Error::State("expected encrypted control"));
    }
    let packet = encoding::decode_control_packet(&received.payload)?;
    encoding::decode_net_control(&session::open_packet_with_session(keys, selected, &packet)?)
}

async fn flush_to_peer(
    stream: &mut TcpStream,
    vault: &Rc<RefCell<VaultHost>>,
    peer: PeerId,
) -> Result<()> {
    let keys = vault.borrow().keys.clone();
    let (offer, challenge) = {
        let mut state = vault.borrow_mut();
        // This call is mandatory after WrapAck, including every restarted epoch.
        state.host.refresh_mailboxes()?;
        let count = u32::try_from(state.host.mailbox(peer)?.len())
            .map_err(|_| Error::State("mailbox too large"))?;
        if count > MAX_DRAIN_FRAMES {
            return Err(Error::State("mailbox drain exceeds frame budget"));
        }
        let challenge = state.host.issue_flush_challenge(peer)?;
        let historical = state.host.historical_documents();
        (
            FlushOffer {
                challenge: challenge.clone(),
                frame_count: count,
                historical,
                bootstrap_through: state.host.bootstrap_through(peer),
            },
            challenge,
        )
    };
    send_frame(
        stream,
        &Frame::new(
            frame::FLUSH_CHALLENGE_KIND,
            encoding::encode_flush_offer(&offer)?.to_vec(),
        )?,
    )
    .await?;
    let sig = tokio::time::timeout(
        IDLE_TIMEOUT,
        read_after_ack(stream, keys.current_session(peer)?.epoch),
    )
    .await
    .map_err(|_| Error::State("flush signature timeout"))??;
    if sig.kind != frame::FLUSH_SIGNATURE_KIND {
        return Err(Error::State("expected flush signature"));
    }
    let prepared = vault
        .borrow_mut()
        .host
        .prepare_flush(peer, &challenge, &sig.payload)?;
    if prepared.envelopes().len() != offer.frame_count as usize
        || prepared.bootstrap_through() != offer.bootstrap_through
    {
        return Err(Error::State("mailbox changed during challenge"));
    }
    let digest = encoding::flush_transport_digest_with_bootstrap(
        prepared.envelopes(),
        &offer.historical,
        offer.bootstrap_through,
    )?;
    for envelope in prepared.envelopes() {
        send_frame(stream, &body_frame(envelope)?).await?;
    }
    // This new Packet counter is sent only after all old queue counters.
    send_control(stream, &keys, peer, &NetControl::FlushEnd { digest }).await?;
    let ack = tokio::time::timeout(IDLE_TIMEOUT, read_control(stream, &keys, peer))
        .await
        .map_err(|_| Error::State("flush apply timeout"))??;
    if ack != (NetControl::FlushApplied { digest }) {
        return Err(Error::AuthenticationFailed);
    }
    vault.borrow_mut().host.acknowledge_flush(peer, prepared)?;
    send_control(stream, &keys, peer, &NetControl::Ready).await
}

fn body_frame(envelope: &MailboxEnvelope) -> Result<Frame> {
    let header = PacketHeader {
        version: PROTOCOL_VERSION,
        sender_id: envelope.sender_id,
        receiver_id: envelope.recipient_id,
        epoch: envelope.epoch,
        seq: envelope.seq,
    };
    match encoding::decode_mailbox_frame(&envelope.ciphertext)? {
        encoding::MailboxFrame::Control { ciphertext } => Frame::new(
            frame::GCM_PACKET_KIND,
            encoding::encode_control_packet(&crate::protocol::packet::ControlPacket {
                header,
                ciphertext,
            })?,
        ),
        encoding::MailboxFrame::ChunkBody {
            file_id,
            index,
            ciphertext,
        } => Frame::new(
            frame::GCM_CHUNK_KIND,
            encoding::encode_chunk_body_frame(&ChunkBodyFrame {
                header,
                file_id,
                index,
                ciphertext,
            })?,
        ),
    }
}
fn queued_frame(received: Frame) -> Result<MailboxEnvelope> {
    let (header, inner) = match received.kind {
        frame::GCM_PACKET_KIND => {
            let packet = encoding::decode_control_packet(&received.payload)?;
            (
                packet.header,
                encoding::MailboxFrame::Control {
                    ciphertext: packet.ciphertext,
                },
            )
        }
        frame::GCM_CHUNK_KIND => {
            let body = encoding::decode_chunk_body_frame(&received.payload)?;
            (
                body.header,
                encoding::MailboxFrame::ChunkBody {
                    file_id: body.file_id,
                    index: body.index,
                    ciphertext: body.ciphertext,
                },
            )
        }
        _ => return Err(Error::State("unexpected frame during mailbox drain")),
    };
    Ok(MailboxEnvelope {
        sender_id: header.sender_id,
        recipient_id: header.receiver_id,
        epoch: header.epoch,
        seq: header.seq,
        queued_at: 0,
        ciphertext: encoding::encode_mailbox_frame(&inner)?,
    })
}

pub struct JoinedPeer {
    stream: TcpStream,
    keys: KeyStore,
    peer_id: PeerId,
    pub replica: Rc<RefCell<MemberReplica>>,
    pub vault_id: VaultId,
    pending_controls: Vec<ControlRecord>,
    pending_control_bytes: usize,
}

pub async fn join_host(
    keys: KeyStore,
    ad: &DirectoryAd,
    code: JoinCode,
    replica: Option<Rc<RefCell<MemberReplica>>>,
) -> Result<JoinedPeer> {
    // Authenticity and numeric target validation precede any connection attempt.
    ad.verify(unix_time()?)?;
    let mut progress = HandshakeProgress::default();
    let mut had_admitted_pair = false;
    let result = tokio::time::timeout(HANDSHAKE_DEADLINE, async {
        let mut stream = TcpStream::connect(ad.addr).await?;
        let established =
            session::establish_guarded(&mut stream, &keys, Some(ad), &mut progress, |peer| {
                had_admitted_pair = keys.has_admitted_pair(peer)?;
                Ok(had_admitted_pair)
            })
            .await?;
        let selected = established.session;
        let request = JoinRequest::sign(&keys, ad.vault_id, code)?;
        let packet = session::seal_packet_with_session(
            &keys,
            &selected,
            &encoding::encode_join_request(&request)?,
        )?;
        send_frame(
            &mut stream,
            &Frame::new(
                frame::GCM_PACKET_KIND,
                encoding::encode_control_packet(&packet)?,
            )?,
        )
        .await?;
        let offer = read_after_ack(&mut stream, selected.epoch).await?;
        if offer.kind == frame::GCM_PACKET_KIND {
            let packet = encoding::decode_control_packet(&offer.payload)?;
            let plaintext = session::open_packet_with_session(&keys, &selected, &packet)?;
            return match encoding::decode_net_control(&plaintext)? {
                NetControl::JoinRejected { bound, requested } => {
                    discard_rejected_candidate(&keys, &selected, had_admitted_pair)?;
                    Err(Error::VaultSessionConflict {
                        peer_id: keys.peer_id()?,
                        bound,
                        requested,
                    })
                }
                NetControl::AdmissionDenied => {
                    discard_rejected_candidate(&keys, &selected, had_admitted_pair)?;
                    Err(Error::AuthenticationFailed)
                }
                _ => Err(Error::AuthenticationFailed),
            };
        }
        if offer.kind != frame::FLUSH_CHALLENGE_KIND {
            return Err(Error::State("expected flush challenge after Join"));
        }
        Ok((
            stream,
            encoding::decode_flush_offer(&offer.payload)?,
            selected,
        ))
    })
    .await;
    let (mut stream, offer, selected) = match result {
        Ok(Ok(value)) => value,
        failure => {
            return match failure {
                Ok(Err(error)) => Err(error),
                _ => Err(Error::State("handshake deadline exceeded")),
            };
        }
    };
    let replica = match replica {
        Some(replica) => {
            if replica.borrow().host_id() != ad.peer_id {
                return Err(Error::AuthenticationFailed);
            }
            replica
        }
        None => Rc::new(RefCell::new(MemberReplica::new_in_vault(
            keys.clone(),
            ad.peer_id,
            BTreeSet::from([keys.peer_id()?, ad.peer_id]),
            shared_chunk_store(),
            FileId(ad.vault_id.0),
        )?)),
    };
    keys.block_live_traffic(ad.peer_id, TRANSPORT_GATE)?;
    receive_flush(&mut stream, &keys, &selected, &replica, offer).await?;
    let welcome = tokio::time::timeout(IDLE_TIMEOUT, read_control(&mut stream, &keys, ad.peer_id))
        .await
        .map_err(|_| Error::State("welcome timeout"))??;
    match welcome {
        NetControl::JoinAccepted {
            vault_id,
            members,
            historical,
            denied,
        } if vault_id == ad.vault_id => {
            let ids = import_current_identities(&keys, &members)?;
            if !ids.is_disjoint(&denied) {
                return Err(Error::AuthenticationFailed);
            }
            let mut replica = replica.borrow_mut();
            replica.learn_history(&historical)?;
            replica.reconcile_members(&ids, &denied)?;
        }
        _ => return Err(Error::AuthenticationFailed),
    }
    keys.unblock_live_traffic(ad.peer_id, TRANSPORT_GATE)?;
    keys.mark_admitted(ad.peer_id)?;
    replica.borrow_mut().finish_receipts();
    Ok(JoinedPeer {
        stream,
        keys,
        peer_id: ad.peer_id,
        replica,
        vault_id: ad.vault_id,
        pending_controls: Vec::new(),
        pending_control_bytes: 0,
    })
}

fn import_current_identities(
    keys: &KeyStore,
    members: &[IdentityDocument],
) -> Result<BTreeSet<PeerId>> {
    let local = keys.identity()?;
    let mut ids = BTreeSet::new();
    for identity in members {
        ids.insert(identity.peer_id);
        if identity.peer_id == local.peer_id {
            if *identity != local {
                return Err(Error::AuthenticationFailed);
            }
            continue;
        }
        if keys
            .load_verified_peer(&identity.peer_id)
            .is_ok_and(|cached| cached == *identity)
        {
            continue;
        }
        keys.import_peer(identity.clone())?;
    }
    Ok(ids)
}

async fn receive_flush(
    stream: &mut TcpStream,
    keys: &KeyStore,
    selected: &PairSession,
    replica: &Rc<RefCell<MemberReplica>>,
    offer: FlushOffer,
) -> Result<()> {
    let peer = selected.peer_id;
    if offer.frame_count > MAX_DRAIN_FRAMES {
        return Err(Error::InvalidInput("mailbox frame budget exceeded"));
    }
    let signature = RustCryptoPureMlDsa.sign(
        &keys.signing_key()?,
        crate::crypto::sign::FLUSH_CONTEXT,
        &encoding::flush_m(&offer.challenge),
    )?;
    send_frame(stream, &Frame::new(frame::FLUSH_SIGNATURE_KIND, signature)?).await?;
    let mut envelopes = Vec::new();
    let mut bytes = 0usize;
    for _ in 0..offer.frame_count {
        let received = tokio::time::timeout(IDLE_TIMEOUT, read_after_ack(stream, selected.epoch))
            .await
            .map_err(|_| Error::State("mailbox idle timeout"))??;
        bytes = bytes
            .checked_add(received.payload.len())
            .ok_or(Error::InvalidInput("mailbox size overflow"))?;
        if bytes > MAX_DRAIN_BYTES {
            return Err(Error::InvalidInput("mailbox memory budget exceeded"));
        }
        envelopes.push(queued_frame(received)?);
    }
    // Open every old counter in FIFO order into existing staging BEFORE the
    // newer authenticated end marker can advance the Packet replay window.
    let prepared = replica.borrow_mut().prepare_mailbox_for_session(
        &envelopes,
        &offer.historical,
        offer.bootstrap_through,
        selected,
    )?;
    let digest = encoding::flush_transport_digest_with_bootstrap(
        &envelopes,
        &offer.historical,
        offer.bootstrap_through,
    )?;
    let end = tokio::time::timeout(
        IDLE_TIMEOUT,
        read_control_for_session(stream, keys, selected),
    )
    .await
    .map_err(|_| Error::State("mailbox end timeout"))??;
    if end != (NetControl::FlushEnd { digest }) {
        return Err(Error::AuthenticationFailed);
    }
    // An unauthenticated FlushOffer cannot replace an admitted session. The
    // complete encrypted end marker authenticates H's accepted admission/batch.
    if keys
        .candidate_session(peer)
        .is_ok_and(|s| s.epoch == selected.epoch)
    {
        let old = keys.current_session(peer)?;
        keys.promote_candidate(peer, selected.epoch)?;
        keys.retire(old.key_handle())?;
    }
    replica.borrow_mut().commit_prepared(prepared)?;
    send_control(stream, keys, peer, &NetControl::FlushApplied { digest }).await?;
    if tokio::time::timeout(IDLE_TIMEOUT, read_control(stream, keys, peer))
        .await
        .map_err(|_| Error::State("flush confirmation timeout"))??
        != NetControl::Ready
    {
        return Err(Error::AuthenticationFailed);
    }
    if offer.frame_count != 0 {
        demo_log::event(
            Kind::Sync,
            "ML-DSA-65 + AES-256-GCM",
            "Offline mailbox recovered",
            &[
                format!("host    {}", demo_log::peer(peer)),
                format!("applied  {} authenticated frame(s)", offer.frame_count),
            ],
        );
    }
    Ok(())
}

async fn serve_live(
    stream: &mut TcpStream,
    vault: &Rc<RefCell<VaultHost>>,
    peer: PeerId,
) -> Result<()> {
    let keys = vault.borrow().keys.clone();
    let mut upload: Option<PendingUpload> = None;
    loop {
        keys.require_live_traffic(peer)?;
        let epoch = keys.current_session(peer)?.epoch;
        let received = tokio::time::timeout(IDLE_TIMEOUT, async {
            let received = read_after_ack(stream, epoch);
            tokio::pin!(received);
            loop {
                if !vault.borrow().host.has_member(&peer) {
                    return Err(Error::State("peer membership was revoked"));
                }
                tokio::select! {
                    result = &mut received => return result,
                    _ = tokio::time::sleep(Duration::from_millis(25)) => {}
                }
            }
        })
        .await
        .map_err(|_| Error::State("peer idle timeout"))??;
        // A mailbox refresh can close the pair gate while this read is pending.
        keys.require_live_traffic(peer)?;
        match received.kind {
            frame::GCM_PACKET_KIND => {
                let packet = encoding::decode_control_packet(&received.payload)?;
                require_live_header(&packet.header, peer, keys.peer_id()?, &keys)?;
                let plaintext = session::open_packet(&keys, peer, &packet)?;
                if plaintext.starts_with(HAVE_MAGIC) {
                    let query = encoding::decode_have_query(&plaintext)?;
                    let reply =
                        crate::net::locate::answer_have(&vault.borrow().host.chunks(), &query)?;
                    send_member_state(stream, vault, &keys, peer).await?;
                    let packet =
                        session::seal_packet(&keys, peer, &encoding::encode_have_reply(&reply)?)?;
                    send_frame(
                        stream,
                        &Frame::new(
                            frame::GCM_PACKET_KIND,
                            encoding::encode_control_packet(&packet)?,
                        )?,
                    )
                    .await?;
                    send_control(stream, &keys, peer, &NetControl::Heartbeat).await?;
                    continue;
                }
                if let Ok(control) = encoding::decode_net_control(&plaintext) {
                    match control {
                        NetControl::Heartbeat => {
                            if upload
                                .as_ref()
                                .is_some_and(|pending| !pending.is_complete())
                            {
                                send_control(
                                    stream,
                                    &keys,
                                    peer,
                                    &NetControl::HeartbeatApplied { through: 0 },
                                )
                                .await?;
                                continue;
                            }
                            if upload.is_some() {
                                finalize_upload(vault, peer, &mut upload, None)?;
                            }
                        }
                        NetControl::HeartbeatApplied { through } => {
                            vault.borrow_mut().host.acknowledge_applied(peer, through)?;
                        }
                        _ => return Err(Error::State("unsupported live control")),
                    }
                    vault.borrow_mut().host.heartbeat(peer, IDLE_TIMEOUT)?;
                    send_pending_controls(stream, vault, &keys, peer).await?;
                    continue;
                }
                if let Ok(request) = encoding::decode_pull_request(&plaintext) {
                    let pull =
                        InProcessPullCoordinator::new(keys.clone(), vault.borrow().host.chunks());
                    let responses = pull.serve(&request, peer)?;
                    send_member_state(stream, vault, &keys, peer).await?;
                    let sent = responses.len();
                    for response in responses {
                        send_frame(
                            stream,
                            &Frame::new(
                                frame::GCM_CHUNK_KIND,
                                encoding::encode_chunk_body_frame(&response.body)?,
                            )?,
                        )
                        .await?;
                    }
                    send_control(stream, &keys, peer, &NetControl::Heartbeat).await?;
                    if sent != 0 {
                        demo_log::event(
                            Kind::Transfer,
                            "AES-256-GCM",
                            "Encrypted file pieces sent",
                            &[
                                format!("recipient  {}", demo_log::peer(peer)),
                                format!(
                                    "pieces     {sent}/{} requested",
                                    request.chunk_ids().len()
                                ),
                            ],
                        );
                    }
                    continue;
                }
                let record = encoding::decode_control_record(&plaintext)?;
                if record.id != 0 {
                    return Err(Error::AuthenticationFailed);
                }
                receive_member_control(vault, peer, record.update, &mut upload)?;
                if upload.is_none() {
                    send_pending_controls(stream, vault, &keys, peer).await?;
                }
            }
            frame::GCM_CHUNK_KIND => {
                let body = encoding::decode_chunk_body_frame(&received.payload)?;
                require_live_header(&body.header, peer, keys.peer_id()?, &keys)?;
                receive_upload_body(peer, &keys, body, &mut upload)?;
                if upload.is_none() {
                    send_pending_controls(stream, vault, &keys, peer).await?;
                }
            }
            _ => return Err(Error::State("unsupported live frame")),
        }
    }
}

struct PendingUpload {
    trusted: TrustedManifest,
    bodies: BTreeMap<u64, Vec<u8>>,
    bytes: usize,
}

impl PendingUpload {
    fn is_complete(&self) -> bool {
        self.bodies.len() == self.trusted.manifest().chunk_ids.len()
    }
}

fn require_live_header(
    header: &PacketHeader,
    sender: PeerId,
    receiver: PeerId,
    keys: &KeyStore,
) -> Result<()> {
    if header.sender_id != sender
        || header.receiver_id != receiver
        || header.version != PROTOCOL_VERSION
        || header.epoch != keys.current_session(sender)?.epoch
    {
        return Err(Error::AuthenticationFailed);
    }
    Ok(())
}

fn receive_member_control(
    vault: &Rc<RefCell<VaultHost>>,
    peer: PeerId,
    update: ControlUpdate,
    upload: &mut Option<PendingUpload>,
) -> Result<()> {
    match update {
        ControlUpdate::NewManifest(manifest) => {
            if upload.is_some() {
                return Err(Error::State("file upload is incomplete"));
            }
            let trusted = vault.borrow().host.verify_writer_manifest(peer, manifest)?;
            *upload = Some(PendingUpload {
                trusted,
                bodies: BTreeMap::new(),
                bytes: 0,
            });
        }
        link @ ControlUpdate::Link {
            child,
            is_dir: false,
            ..
        } => {
            let pending = upload
                .as_ref()
                .ok_or(Error::State("file link arrived without an upload"))?;
            if child != pending.trusted.manifest().file_id || !pending.is_complete() {
                return Err(Error::State("file link does not complete the upload"));
            }
            finalize_upload(vault, peer, upload, Some(link))?;
        }
        update => {
            if upload.is_some() {
                return Err(Error::State("file upload is incomplete"));
            }
            vault.borrow_mut().host.fan_out_control(peer, &update)?;
            log_remote_control(peer, &update);
        }
    }
    Ok(())
}

fn receive_upload_body(
    peer: PeerId,
    keys: &KeyStore,
    body: ChunkBodyFrame,
    upload: &mut Option<PendingUpload>,
) -> Result<()> {
    let pending = upload
        .as_mut()
        .ok_or(Error::State("chunk body arrived without a manifest"))?;
    let session = keys.current_session(peer)?;
    let plaintext = open_chunk(keys, &session, &body, &pending.trusted)?;
    pending.bytes = pending
        .bytes
        .checked_add(plaintext.len())
        .ok_or(Error::InvalidInput("upload size overflow"))?;
    if pending.bytes > MAX_DRAIN_BYTES {
        return Err(Error::InvalidInput("upload exceeds memory budget"));
    }
    if pending.bodies.insert(body.index, plaintext).is_some() {
        return Err(Error::InvalidInput("duplicate upload chunk index"));
    }
    Ok(())
}

fn finalize_upload(
    vault: &Rc<RefCell<VaultHost>>,
    peer: PeerId,
    upload: &mut Option<PendingUpload>,
    link: Option<ControlUpdate>,
) -> Result<()> {
    let completed = upload
        .take()
        .ok_or(Error::State("completed upload disappeared"))?;
    if !completed.is_complete() {
        *upload = Some(completed);
        return Err(Error::State("file upload is incomplete"));
    }
    let file_id = completed.trusted.manifest().file_id;
    let chunks = completed.trusted.manifest().chunk_ids.len();
    let bytes = completed.trusted.manifest().size;
    let linked_as = link.as_ref().and_then(|update| match update {
        ControlUpdate::Link { name, .. } => Some(name.clone()),
        _ => None,
    });
    vault.borrow_mut().host.commit_writer_upload(
        peer,
        completed.trusted.manifest().clone(),
        completed.bodies,
        link,
    )?;
    let mut details = vec![
        format!("file    {}", demo_log::file(file_id)),
        format!("writer  {} · signature verified", demo_log::peer(peer)),
        format!("pieces  {chunks} · {bytes} bytes committed"),
    ];
    if let Some(name) = linked_as {
        details.push(format!("linked  {name}"));
    }
    demo_log::event(
        Kind::File,
        "ML-DSA-65 + AES-256-GCM",
        "Remote file committed",
        &details,
    );
    Ok(())
}

fn log_remote_control(peer: PeerId, update: &ControlUpdate) {
    let (headline, mut details) = match update {
        ControlUpdate::Link {
            parent,
            name,
            child,
            is_dir,
        } => (
            if *is_dir {
                format!("Remote directory committed  {name}")
            } else {
                format!("Remote file linked  {name}")
            },
            vec![
                format!("parent  {}", demo_log::file(*parent)),
                format!("inode   {}", demo_log::file(*child)),
            ],
        ),
        ControlUpdate::Unlink { parent, name } => (
            format!("Remote path unlinked  {name}"),
            vec![format!("parent  {}", demo_log::file(*parent))],
        ),
        ControlUpdate::Rename {
            src_parent,
            src_name,
            dst_parent,
            dst_name,
        } => (
            format!("Remote path renamed  {src_name} → {dst_name}"),
            vec![
                format!("from  {}", demo_log::file(*src_parent)),
                format!("to    {}", demo_log::file(*dst_parent)),
            ],
        ),
        _ => return,
    };
    details.push(format!("writer  {}", demo_log::peer(peer)));
    demo_log::event(Kind::File, "AES-256-GCM", headline, &details);
}

async fn send_pending_controls(
    stream: &mut TcpStream,
    vault: &Rc<RefCell<VaultHost>>,
    keys: &KeyStore,
    peer: PeerId,
) -> Result<()> {
    send_member_state(stream, vault, keys, peer).await?;
    send_control(stream, keys, peer, &NetControl::Heartbeat).await
}

async fn send_member_state(
    stream: &mut TcpStream,
    vault: &Rc<RefCell<VaultHost>>,
    keys: &KeyStore,
    peer: PeerId,
) -> Result<()> {
    // These packets may carry counters much older than a freshly sealed
    // membership update. Open them first so W=1024 cannot expire the queue.
    let packets = vault.borrow_mut().host.take_online_control(peer)?;
    for packet in packets {
        send_frame(
            stream,
            &Frame::new(
                frame::GCM_PACKET_KIND,
                encoding::encode_control_packet(&packet)?,
            )?,
        )
        .await?;
    }
    // The receiver buffers authenticated records until this verified identity
    // refresh arrives, then verifies each writer signature before applying it.
    let welcome = vault.borrow().welcome()?;
    send_control(stream, keys, peer, &welcome).await?;
    Ok(())
}

impl JoinedPeer {
    pub(crate) fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    pub async fn have_query(&mut self, query: &HaveQuery) -> Result<HaveReply> {
        self.keys.require_live_traffic(self.peer_id)?;
        let packet = session::seal_packet(
            &self.keys,
            self.peer_id,
            &encoding::encode_have_query(query)?,
        )?;
        send_frame(
            &mut self.stream,
            &Frame::new(
                frame::GCM_PACKET_KIND,
                encoding::encode_control_packet(&packet)?,
            )?,
        )
        .await?;
        let reply = tokio::time::timeout(Duration::from_secs(5), async {
            let mut reply = None;
            loop {
                let plaintext = self.read_live_packet().await?;
                if plaintext.starts_with(HAVE_MAGIC) {
                    if reply.is_some() {
                        return Err(Error::AuthenticationFailed);
                    }
                    let candidate = encoding::decode_have_reply(&plaintext)?;
                    candidate.validate(query)?;
                    reply = Some(candidate);
                } else if self.apply_live_plaintext(&plaintext)? {
                    break;
                }
            }
            reply.ok_or(Error::State("host omitted have reply"))
        })
        .await
        .map_err(|_| Error::State("have query deadline exceeded"))??;
        self.acknowledge_after_drain().await?;
        Ok(reply)
    }

    pub async fn pull(
        &mut self,
        request: &PullRequest,
        trusted_manifest: &TrustedManifest,
    ) -> Result<usize> {
        self.keys.require_live_traffic(self.peer_id)?;
        let packet = session::seal_packet(
            &self.keys,
            self.peer_id,
            &encoding::encode_pull_request(request)?,
        )?;
        send_frame(
            &mut self.stream,
            &Frame::new(
                frame::GCM_PACKET_KIND,
                encoding::encode_control_packet(&packet)?,
            )?,
        )
        .await?;
        let mut responses = Vec::new();
        let mut frames = 0u32;
        loop {
            let received = tokio::time::timeout(
                IDLE_TIMEOUT,
                read_after_ack(
                    &mut self.stream,
                    self.keys.current_session(self.peer_id)?.epoch,
                ),
            )
            .await
            .map_err(|_| Error::State("host idle timeout"))??;
            self.keys.require_live_traffic(self.peer_id)?;
            frames = frames
                .checked_add(1)
                .ok_or(Error::InvalidInput("pull response frame overflow"))?;
            if frames > MAX_DRAIN_FRAMES {
                return Err(Error::InvalidInput("pull response frame budget exceeded"));
            }
            match received.kind {
                frame::GCM_CHUNK_KIND => {
                    if responses.len() >= request.chunk_ids().len() {
                        return Err(Error::AuthenticationFailed);
                    }
                    responses.push(PullResponse {
                        body: encoding::decode_chunk_body_frame(&received.payload)?,
                    });
                }
                frame::GCM_PACKET_KIND => {
                    let packet = encoding::decode_control_packet(&received.payload)?;
                    require_live_header(
                        &packet.header,
                        self.peer_id,
                        self.keys.peer_id()?,
                        &self.keys,
                    )?;
                    let plaintext = session::open_packet(&self.keys, self.peer_id, &packet)?;
                    if self.apply_live_plaintext(&plaintext)? {
                        break;
                    }
                }
                _ => return Err(Error::State("unexpected pull response frame")),
            }
        }
        let chunks = self.replica.borrow().chunks();
        let unavailable = if responses.is_empty() && !request.chunk_ids().is_empty() {
            let chunks = chunks
                .lock()
                .map_err(|_| Error::State("chunk store poisoned"))?;
            request
                .chunk_ids()
                .iter()
                .any(|chunk_id| !chunks.has(chunk_id))
        } else {
            false
        };
        if unavailable {
            self.acknowledge_after_drain().await?;
            return Err(Error::State("holder returned no requested chunks"));
        }
        let pull = InProcessPullCoordinator::new(self.keys.clone(), chunks);
        let written = pull.accept(&responses, trusted_manifest)?;
        self.acknowledge_after_drain().await?;
        Ok(written)
    }

    pub async fn mkdir(&mut self, path: &str) -> Result<FileId> {
        let (parent, name) = self.replica.borrow().tree().resolve_parent(path)?;
        let child = FileId(random_bytes()?);
        self.submit_control(&ControlUpdate::Link {
            parent,
            name,
            child,
            is_dir: true,
        })
        .await?;
        Ok(child)
    }

    pub async fn unlink(&mut self, path: &str) -> Result<()> {
        let (parent, name) = self.replica.borrow().tree().resolve_parent(path)?;
        self.submit_control(&ControlUpdate::Unlink { parent, name })
            .await
    }

    pub async fn rename(&mut self, source: &str, destination: &str) -> Result<()> {
        let (src_parent, src_name) = self.replica.borrow().tree().resolve_parent(source)?;
        let (dst_parent, dst_name) = self.replica.borrow().tree().resolve_parent(destination)?;
        self.submit_control(&ControlUpdate::Rename {
            src_parent,
            src_name,
            dst_parent,
            dst_name,
        })
        .await
    }

    pub async fn save_file(&mut self, path: &str, bodies: &[Vec<u8>]) -> Result<FileId> {
        self.keys.require_live_traffic(self.peer_id)?;
        let (existing, parent, name, version) = {
            let replica = self.replica.borrow();
            let (parent, name) = replica.tree().resolve_parent(path)?;
            let existing = replica.tree().resolve(path).ok();
            if existing.is_some_and(|file_id| replica.tree().is_dir(&file_id)) {
                return Err(Error::InvalidInput("cannot save a directory"));
            }
            let version = existing
                .and_then(|file_id| replica.trusted_manifest(&file_id))
                .map_or(Ok(1), |trusted| {
                    trusted
                        .manifest()
                        .version
                        .checked_add(1)
                        .ok_or(Error::State("manifest version exhausted"))
                })?;
            (existing, parent, name, version)
        };
        let file_id = existing.unwrap_or(FileId(random_bytes()?));
        let mut manifest = Manifest {
            file_id,
            chunk_ids: Vec::with_capacity(bodies.len()),
            size: 0,
            writer_id: self.keys.peer_id()?,
            version,
            signature: Vec::new(),
        };
        for (index, plaintext) in bodies.iter().enumerate() {
            if plaintext.len() as u64 > crate::store::durable::MAX_CHUNK_BYTES {
                return Err(Error::InvalidInput("chunk exceeds 1 MiB"));
            }
            manifest.size = manifest
                .size
                .checked_add(plaintext.len() as u64)
                .ok_or(Error::InvalidInput("file size overflow"))?;
            manifest.chunk_ids.push(encoding::chunk_id(
                &file_id,
                u64::try_from(index).map_err(|_| Error::InvalidInput("chunk index exceeds u64"))?,
                plaintext,
            ));
        }
        manifest.signature = RustCryptoPureMlDsa.sign(
            &self.keys.signing_key()?,
            MANIFEST_CONTEXT,
            &encoding::manifest_m(&manifest)?,
        )?;
        self.send_manifest_bodies(&manifest, bodies).await?;
        if existing.is_none() {
            self.send_record(&ControlUpdate::Link {
                parent,
                name,
                child: file_id,
                is_dir: false,
            })
            .await?;
        } else {
            send_control(
                &mut self.stream,
                &self.keys,
                self.peer_id,
                &NetControl::Heartbeat,
            )
            .await?;
        }
        self.finish_submission().await?;
        self.store_local_bodies(&manifest, bodies)?;
        Ok(file_id)
    }

    pub async fn submit_control(&mut self, update: &ControlUpdate) -> Result<()> {
        if matches!(update, ControlUpdate::NewManifest(_)) {
            return Err(Error::InvalidInput(
                "use submit_manifest to send file content",
            ));
        }
        self.keys.require_live_traffic(self.peer_id)?;
        self.send_record(update).await?;
        self.finish_submission().await
    }

    pub async fn submit_manifest(&mut self, manifest: Manifest, bodies: &[Vec<u8>]) -> Result<()> {
        self.keys.require_live_traffic(self.peer_id)?;
        self.send_manifest_bodies(&manifest, bodies).await?;
        send_control(
            &mut self.stream,
            &self.keys,
            self.peer_id,
            &NetControl::Heartbeat,
        )
        .await?;
        self.finish_submission().await?;
        self.store_local_bodies(&manifest, bodies)
    }

    async fn send_manifest_bodies(
        &mut self,
        manifest: &Manifest,
        bodies: &[Vec<u8>],
    ) -> Result<()> {
        if manifest.writer_id != self.keys.peer_id()? || manifest.chunk_ids.len() != bodies.len() {
            return Err(Error::InvalidInput("manifest does not match upload bodies"));
        }
        for (index, (expected, plaintext)) in manifest.chunk_ids.iter().zip(bodies).enumerate() {
            let index =
                u64::try_from(index).map_err(|_| Error::InvalidInput("chunk index exceeds u64"))?;
            if encoding::chunk_id(&manifest.file_id, index, plaintext) != *expected {
                return Err(Error::AuthenticationFailed);
            }
        }
        self.send_record(&ControlUpdate::NewManifest(manifest.clone()))
            .await?;
        let session = self.keys.current_session(self.peer_id)?;
        for (index, plaintext) in bodies.iter().enumerate() {
            let body = encrypt_at_send(
                &self.keys,
                &session,
                manifest.file_id,
                u64::try_from(index).map_err(|_| Error::InvalidInput("chunk index exceeds u64"))?,
                plaintext,
            )?;
            send_frame(
                &mut self.stream,
                &Frame::new(
                    frame::GCM_CHUNK_KIND,
                    encoding::encode_chunk_body_frame(&body)?,
                )?,
            )
            .await?;
        }
        Ok(())
    }

    fn store_local_bodies(&self, manifest: &Manifest, bodies: &[Vec<u8>]) -> Result<()> {
        let chunks = self.replica.borrow().chunks();
        let mut current = chunks
            .lock()
            .map_err(|_| Error::State("chunk store poisoned"))?;
        let mut staged = current.clone();
        for (index, plaintext) in bodies.iter().enumerate() {
            staged.put(
                &manifest.file_id,
                u64::try_from(index).map_err(|_| Error::InvalidInput("chunk index exceeds u64"))?,
                plaintext.clone(),
            )?;
        }
        staged.persist_current()?;
        *current = staged;
        Ok(())
    }

    async fn send_record(&mut self, update: &ControlUpdate) -> Result<()> {
        self.keys.require_live_traffic(self.peer_id)?;
        let plaintext = encoding::encode_control_record(&ControlRecord {
            id: 0,
            update: update.clone(),
        })?;
        let packet = session::seal_packet(&self.keys, self.peer_id, &plaintext)?;
        send_frame(
            &mut self.stream,
            &Frame::new(
                frame::GCM_PACKET_KIND,
                encoding::encode_control_packet(&packet)?,
            )?,
        )
        .await
    }

    async fn finish_submission(&mut self) -> Result<()> {
        self.drain_live_reply().await?;
        self.acknowledge_after_drain().await
    }

    async fn acknowledge_after_drain(&mut self) -> Result<()> {
        self.keys.require_live_traffic(self.peer_id)?;
        let through = self.replica.borrow().last_applied();
        send_control(
            &mut self.stream,
            &self.keys,
            self.peer_id,
            &NetControl::HeartbeatApplied { through },
        )
        .await?;
        self.drain_live_reply().await.map(|_| ())
    }

    async fn read_live_packet(&mut self) -> Result<Vec<u8>> {
        let received = tokio::time::timeout(
            IDLE_TIMEOUT,
            read_after_ack(
                &mut self.stream,
                self.keys.current_session(self.peer_id)?.epoch,
            ),
        )
        .await
        .map_err(|_| Error::State("host idle timeout"))??;
        self.keys.require_live_traffic(self.peer_id)?;
        if received.kind != frame::GCM_PACKET_KIND {
            return Err(Error::State("unexpected live frame"));
        }
        let packet = encoding::decode_control_packet(&received.payload)?;
        require_live_header(
            &packet.header,
            self.peer_id,
            self.keys.peer_id()?,
            &self.keys,
        )?;
        session::open_packet(&self.keys, self.peer_id, &packet)
    }

    /// Returns true only for the heartbeat terminating one ordered reply.
    fn apply_live_plaintext(&mut self, plaintext: &[u8]) -> Result<bool> {
        if let Ok(control) = encoding::decode_net_control(plaintext) {
            return match control {
                NetControl::Heartbeat => {
                    if !self.pending_controls.is_empty() {
                        return Err(Error::AuthenticationFailed);
                    }
                    Ok(true)
                }
                NetControl::JoinAccepted {
                    vault_id,
                    members,
                    historical,
                    denied,
                } if vault_id == self.vault_id => {
                    self.apply_member_identities(members, historical, denied)?;
                    Ok(false)
                }
                _ => Err(Error::State("unexpected live reply")),
            };
        }
        let record = encoding::decode_control_record(plaintext)?;
        self.pending_control_bytes = self
            .pending_control_bytes
            .checked_add(plaintext.len())
            .ok_or(Error::InvalidInput("pending control size overflow"))?;
        if self.pending_control_bytes > MAX_DRAIN_BYTES
            || self.pending_controls.len() >= MAX_DRAIN_FRAMES as usize
        {
            return Err(Error::InvalidInput("pending control budget exceeded"));
        }
        self.pending_controls.push(record);
        Ok(false)
    }

    fn apply_member_identities(
        &mut self,
        members: Vec<IdentityDocument>,
        historical: Vec<IdentityDocument>,
        denied: BTreeSet<PeerId>,
    ) -> Result<()> {
        let ids = import_current_identities(&self.keys, &members)?;
        if !ids.is_disjoint(&denied) {
            return Err(Error::AuthenticationFailed);
        }
        self.replica.borrow_mut().learn_history(&historical)?;
        let pending = std::mem::take(&mut self.pending_controls);
        self.pending_control_bytes = 0;
        for record in pending {
            self.replica
                .borrow_mut()
                .apply_authenticated_control(record)?;
        }
        self.replica.borrow_mut().reconcile_members(&ids, &denied)?;
        Ok(())
    }

    async fn drain_live_reply(&mut self) -> Result<u64> {
        loop {
            let plaintext = self.read_live_packet().await?;
            if self.apply_live_plaintext(&plaintext)? {
                return Ok(self.replica.borrow().last_applied());
            }
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            self.keys.require_live_traffic(self.peer_id)?;
            let through = self.replica.borrow().last_applied();
            send_control(
                &mut self.stream,
                &self.keys,
                self.peer_id,
                &NetControl::HeartbeatApplied { through },
            )
            .await?;
            self.drain_live_reply().await?;
        }
    }
}

async fn send_frame(stream: &mut TcpStream, frame: &Frame) -> Result<()> {
    tokio::time::timeout(IDLE_TIMEOUT, write_wire_frame(stream, frame))
        .await
        .map_err(|_| Error::State("peer write timeout"))?
}
