use super::{JoinCode, VaultId};
use crate::{
    crypto::{
        identity::IdentityDocument,
        sign::{PureMlDsa, RustCryptoPureMlDsa, JOIN_CONTEXT},
    },
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
}

#[derive(Clone, PartialEq, Eq)]
pub struct FlushOffer {
    pub challenge: crate::sync::host::FlushChallenge,
    pub frame_count: u32,
}

#[derive(Clone, PartialEq, Eq)]
pub enum NetControl {
    JoinAccepted {
        vault_id: VaultId,
        members: Vec<IdentityDocument>,
    },
    FlushEnd {
        digest: [u8; 32],
    },
    FlushApplied {
        digest: [u8; 32],
    },
    Ready,
    Heartbeat,
}

#[derive(Clone)]
pub struct VaultMetadata {
    pub vault_id: VaultId,
    pub join_code: JoinCode,
    pub issued_at: u64,
    pub members: Vec<crate::ids::PeerId>,
}

use super::{
    directory::{DirForget, DirectoryAd, DirectoryClient},
    frame::{self, read_frame, write_frame as write_wire_frame, Frame},
    session::{self, HandshakeProgress},
};
use crate::{
    ids::PeerId,
    keystore::IdentityKeyStore,
    protocol::{
        packet::{PacketHeader, PROTOCOL_VERSION},
        pull::ChunkBodyFrame,
    },
    store::chunks::shared_chunk_store,
    sync::host::{HostService, HostState, MailboxEnvelope, MemberReplica},
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    net::SocketAddr,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::net::{TcpListener, TcpStream};

pub const HANDSHAKE_DEADLINE: Duration = Duration::from_secs(5);
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
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
    metadata: VaultMetadata,
    path: PathBuf,
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
            },
        };
        let members: BTreeSet<_> = metadata.members.iter().copied().collect();
        let host = HostService::new(keys.clone(), members, shared_chunk_store())?;
        let vault = Self {
            keys,
            host,
            metadata,
            path: path.to_owned(),
            active_peers: BTreeSet::new(),
        };
        vault.persist()?;
        Ok(vault)
    }
    pub fn resume(keys: KeyStore, path: &Path, state: HostState) -> Result<Self> {
        let mut vault = Self::load_or_create(keys.clone(), path)?;
        vault.host = HostService::resume(keys, state)?;
        Ok(vault)
    }
    pub fn vault_id(&self) -> VaultId {
        self.metadata.vault_id
    }
    pub fn join_code(&self) -> JoinCode {
        self.metadata.join_code
    }
    fn persist(&self) -> Result<()> {
        crate::keystore::atomic_private_write(
            &self.path,
            &encoding::encode_vault_metadata(&self.metadata)?,
        )
    }
    pub async fn publish(
        &mut self,
        directory: &DirectoryClient,
        addr: SocketAddr,
    ) -> Result<DirectoryAd> {
        let issued_at = unix_time()?.max(
            self.metadata
                .issued_at
                .checked_add(1)
                .ok_or(Error::State("ad timestamp exhausted"))?,
        );
        let ad = DirectoryAd::sign(&self.keys, self.metadata.vault_id, addr, issued_at)?;
        directory.put(self.metadata.join_code, &ad).await?;
        self.metadata.issued_at = issued_at;
        self.persist()?;
        Ok(ad)
    }
    pub async fn rotate_code(
        vault: &Rc<RefCell<Self>>,
        directory: &DirectoryClient,
        addr: SocketAddr,
    ) -> Result<JoinCode> {
        let (code, ad, forget) = {
            let mut state = vault.borrow_mut();
            let old = state.metadata.join_code;
            let code = JoinCode::generate()?;
            let issued_at = unix_time()?.max(
                state
                    .metadata
                    .issued_at
                    .checked_add(1)
                    .ok_or(Error::State("ad timestamp exhausted"))?,
            );
            let forgotten_at = issued_at
                .checked_add(1)
                .ok_or(Error::State("ad timestamp exhausted"))?;
            let ad = DirectoryAd::sign(&state.keys, state.metadata.vault_id, addr, issued_at)?;
            let forget = DirForget::sign(&state.keys, state.metadata.vault_id, old, forgotten_at)?;
            state.metadata.join_code = code;
            state.metadata.issued_at = forgotten_at;
            // Admission changes before directory I/O; the borrow is released so
            // concurrent connections observe the new code immediately.
            state.persist()?;
            (code, ad, forget)
        };
        directory.put(code, &ad).await?;
        directory.forget(&forget).await?;
        Ok(code)
    }
    fn admit(&mut self, request: &JoinRequest, peer: &IdentityDocument) -> Result<()> {
        request.verify(peer)?;
        if request.vault_id != self.metadata.vault_id
            || request.join_code != self.metadata.join_code
        {
            return Err(Error::AuthenticationFailed);
        }
        if !self.host.is_running() {
            return Err(Error::State("host is not running"));
        }
        let mut metadata = self.metadata.clone();
        if !metadata.members.contains(&peer.peer_id) {
            metadata.members.push(peer.peer_id);
        }
        crate::keystore::atomic_private_write(
            &self.path,
            &encoding::encode_vault_metadata(&metadata)?,
        )?;
        self.host.add_member(peer.clone())?;
        self.metadata = metadata;
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
            vault_id: self.metadata.vault_id,
            members,
        })
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

struct PeerPermit {
    vault: Rc<RefCell<VaultHost>>,
    peer: PeerId,
}
impl Drop for PeerPermit {
    fn drop(&mut self) {
        if let Ok(mut vault) = self.vault.try_borrow_mut() {
            vault.active_peers.remove(&self.peer);
            let _ = vault.host.heartbeat(self.peer, Duration::ZERO);
        }
    }
}

pub async fn serve_host(listener: TcpListener, vault: Rc<RefCell<VaultHost>>) -> Result<()> {
    let connections = Rc::new(Cell::new(0));
    let provisional = Rc::new(Cell::new(0));
    loop {
        let (mut stream, _) = listener.accept().await?;
        let Some(connection) = CounterPermit::acquire(&connections, MAX_CONNECTIONS) else {
            continue;
        };
        let Some(provisional) = CounterPermit::acquire(&provisional, MAX_PROVISIONAL) else {
            continue;
        };
        let vault = vault.clone();
        tokio::task::spawn_local(async move {
            let _connection = connection;
            let mut progress = HandshakeProgress::default();
            let keys = vault.borrow().keys.clone();
            let admission = tokio::time::timeout(HANDSHAKE_DEADLINE, async {
                let established =
                    session::establish(&mut stream, &keys, None, &mut progress).await?;
                let peer = established.peer;
                {
                    let mut state = vault.borrow_mut();
                    if !state.active_peers.insert(peer.peer_id) {
                        return Err(Error::State("peer already connected"));
                    }
                }
                let permit = PeerPermit {
                    vault: vault.clone(),
                    peer: peer.peer_id,
                };
                keys.block_live_traffic(peer.peer_id, TRANSPORT_GATE)?;
                let frame = read_after_ack(&mut stream, established.session.epoch).await?;
                if frame.kind != frame::GCM_PACKET_KIND {
                    keys.discard_pair(peer.peer_id)?;
                    return Err(Error::AuthenticationFailed);
                }
                let admit = (|| -> Result<()> {
                    let packet = encoding::decode_control_packet(&frame.payload)?;
                    let plaintext = session::open_packet(&keys, peer.peer_id, &packet)?;
                    let request = encoding::decode_join_request(&plaintext, &peer)?;
                    vault.borrow_mut().admit(&request, &peer)
                })();
                if let Err(error) = admit {
                    keys.discard_pair(peer.peer_id)?;
                    return Err(error);
                }
                Ok((peer.peer_id, permit))
            })
            .await;
            drop(provisional);
            let result = match admission {
                Ok(Ok((peer, permit))) => {
                    let _peer = permit;
                    async {
                        flush_to_peer(&mut stream, &vault, peer).await?;
                        let welcome = vault.borrow().welcome()?;
                        send_control(&mut stream, &keys, peer, &welcome).await?;
                        keys.unblock_live_traffic(peer, TRANSPORT_GATE)?;
                        vault.borrow_mut().host.heartbeat(peer, IDLE_TIMEOUT)?;
                        eprintln!("qfsd: accepted vault member; pair live");
                        serve_live(&mut stream, &vault, peer).await
                    }
                    .await
                }
                Ok(Err(error)) => Err(error),
                Err(_) => Err(Error::State("handshake deadline exceeded")),
            };
            if let Err(error) = result {
                eprintln!("qfsd: peer connection closed: {error}");
            }
        });
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
    let packet = session::seal_packet(keys, peer, &encoding::encode_net_control(control)?)?;
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
    let received = read_after_ack(stream, keys.current_session(peer)?.epoch).await?;
    if received.kind != frame::GCM_PACKET_KIND {
        return Err(Error::State("expected encrypted control"));
    }
    let packet = encoding::decode_control_packet(&received.payload)?;
    encoding::decode_net_control(&session::open_packet(keys, peer, &packet)?)
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
        (
            FlushOffer {
                challenge: challenge.clone(),
                frame_count: count,
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
    if prepared.envelopes().len() != offer.frame_count as usize {
        return Err(Error::State("mailbox changed during challenge"));
    }
    let digest = encoding::mailbox_digest(prepared.envelopes())?;
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
    let mut joined_packet_sent = false;
    let result = tokio::time::timeout(HANDSHAKE_DEADLINE, async {
        let mut stream = TcpStream::connect(ad.addr).await?;
        session::establish(&mut stream, &keys, Some(ad), &mut progress).await?;
        let request = JoinRequest::sign(&keys, ad.vault_id, code)?;
        let packet =
            session::seal_packet(&keys, ad.peer_id, &encoding::encode_join_request(&request)?)?;
        joined_packet_sent = true;
        send_frame(
            &mut stream,
            &Frame::new(
                frame::GCM_PACKET_KIND,
                encoding::encode_control_packet(&packet)?,
            )?,
        )
        .await?;
        let offer = read_after_ack(&mut stream, keys.current_session(ad.peer_id)?.epoch).await?;
        if offer.kind != frame::FLUSH_CHALLENGE_KIND {
            return Err(Error::State("expected flush challenge after Join"));
        }
        Ok((stream, encoding::decode_flush_offer(&offer.payload)?))
    })
    .await;
    let (mut stream, offer) = match result {
        Ok(Ok(value)) => value,
        failure => {
            if joined_packet_sent {
                keys.discard_pair(ad.peer_id)?;
            }
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
        None => Rc::new(RefCell::new(MemberReplica::new(
            keys.clone(),
            ad.peer_id,
            BTreeSet::from([keys.peer_id()?, ad.peer_id]),
            shared_chunk_store(),
        )?)),
    };
    keys.block_live_traffic(ad.peer_id, TRANSPORT_GATE)?;
    receive_flush(&mut stream, &keys, ad.peer_id, &replica, offer).await?;
    let welcome = tokio::time::timeout(IDLE_TIMEOUT, read_control(&mut stream, &keys, ad.peer_id))
        .await
        .map_err(|_| Error::State("welcome timeout"))??;
    match welcome {
        NetControl::JoinAccepted { vault_id, members } if vault_id == ad.vault_id => {
            let mut ids = BTreeSet::new();
            for identity in members {
                identity.verify()?;
                ids.insert(identity.peer_id);
                if identity.peer_id != keys.peer_id()? {
                    keys.import_peer(identity)?;
                }
            }
            replica.borrow_mut().add_members(&ids)?;
        }
        _ => return Err(Error::AuthenticationFailed),
    }
    keys.unblock_live_traffic(ad.peer_id, TRANSPORT_GATE)?;
    replica.borrow_mut().finish_receipts();
    Ok(JoinedPeer {
        stream,
        keys,
        peer_id: ad.peer_id,
        replica,
        vault_id: ad.vault_id,
    })
}

async fn receive_flush(
    stream: &mut TcpStream,
    keys: &KeyStore,
    peer: PeerId,
    replica: &Rc<RefCell<MemberReplica>>,
    offer: FlushOffer,
) -> Result<()> {
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
        let received = tokio::time::timeout(
            IDLE_TIMEOUT,
            read_after_ack(stream, keys.current_session(peer)?.epoch),
        )
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
    let prepared = replica.borrow_mut().prepare_mailbox(&envelopes)?;
    let digest = encoding::mailbox_digest(&envelopes)?;
    let end = tokio::time::timeout(IDLE_TIMEOUT, read_control(stream, keys, peer))
        .await
        .map_err(|_| Error::State("mailbox end timeout"))??;
    if end != (NetControl::FlushEnd { digest }) {
        return Err(Error::AuthenticationFailed);
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
    Ok(())
}

async fn serve_live(
    stream: &mut TcpStream,
    vault: &Rc<RefCell<VaultHost>>,
    peer: PeerId,
) -> Result<()> {
    let keys = vault.borrow().keys.clone();
    loop {
        keys.require_live_traffic(peer)?;
        let control = tokio::time::timeout(IDLE_TIMEOUT, read_control(stream, &keys, peer))
            .await
            .map_err(|_| Error::State("peer idle timeout"))??;
        if control != NetControl::Heartbeat {
            return Err(Error::State("unsupported live control"));
        }
        vault.borrow_mut().host.heartbeat(peer, IDLE_TIMEOUT)?;
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
        send_control(stream, &keys, peer, &NetControl::Heartbeat).await?;
    }
}

impl JoinedPeer {
    pub async fn run(&mut self) -> Result<()> {
        loop {
            self.keys.require_live_traffic(self.peer_id)?;
            send_control(
                &mut self.stream,
                &self.keys,
                self.peer_id,
                &NetControl::Heartbeat,
            )
            .await?;
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
                if received.kind != frame::GCM_PACKET_KIND {
                    return Err(Error::State("unexpected live frame"));
                }
                let packet = encoding::decode_control_packet(&received.payload)?;
                // Heartbeat is a one-byte canonical control. Writer manifests are
                // much larger, so dispatch without opening/replaying them twice.
                if packet.ciphertext.len() == 17 {
                    if encoding::decode_net_control(&session::open_packet(
                        &self.keys,
                        self.peer_id,
                        &packet,
                    )?)? != NetControl::Heartbeat
                    {
                        return Err(Error::State("unexpected live reply"));
                    }
                    break;
                }
                self.replica.borrow_mut().apply_control(&packet)?;
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }
}

async fn send_frame(stream: &mut TcpStream, frame: &Frame) -> Result<()> {
    tokio::time::timeout(IDLE_TIMEOUT, write_wire_frame(stream, frame))
        .await
        .map_err(|_| Error::State("peer write timeout"))?
}
