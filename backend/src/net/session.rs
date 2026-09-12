use tokio::net::TcpStream;

use crate::{
    crypto::{
        aead::{Aes256Gcm, RustCryptoAes256Gcm},
        identity::IdentityDocument,
        sign::{PureMlDsa, RustCryptoPureMlDsa, WRAP_CONTEXT},
        wrap::{ConstructionBWrap, PairSession, RustCryptoConstructionBWrap, WrapMessage},
    },
    demo_log::{self, Kind},
    encoding,
    ids::{Epoch, PeerId},
    keystore::{IdentityKeyStore, KeyStore},
    net::{
        directory::DirectoryAd,
        frame::{read_frame, write_frame, Frame},
    },
    protocol::packet::{ControlPacket, PacketHeader, PayloadType, PROTOCOL_VERSION},
    Error, Result,
};

const IDENTITY_KIND: u8 = 1;
const EPOCH_HINT_KIND: u8 = 2;
const WRAP_KIND: u8 = 3;
const WRAP_ACK_KIND: u8 = 4;

pub struct EstablishedSession {
    pub peer: IdentityDocument,
    pub session: PairSession,
}

#[derive(Default)]
pub struct HandshakeProgress {
    pub peer_id: Option<PeerId>,
    pub wrap_acknowledged: bool,
    pub had_pair: bool,
}

pub async fn establish(
    stream: &mut TcpStream,
    keys: &KeyStore,
    expected: Option<&DirectoryAd>,
    progress: &mut HandshakeProgress,
) -> Result<EstablishedSession> {
    establish_guarded(stream, keys, expected, progress, |_| Ok(false)).await
}

pub async fn establish_guarded<F>(
    stream: &mut TcpStream,
    keys: &KeyStore,
    expected: Option<&DirectoryAd>,
    progress: &mut HandshakeProgress,
    mut preserve_existing: F,
) -> Result<EstablishedSession>
where
    F: FnMut(PeerId) -> Result<bool>,
{
    let local = keys.identity()?;
    write_frame(
        stream,
        &Frame::new(IDENTITY_KIND, encoding::encode_identity(&local)?)?,
    )
    .await?;
    let peer_frame = read_frame(stream).await?;
    require_kind(&peer_frame, IDENTITY_KIND)?;
    let peer = encoding::decode_identity(&peer_frame.payload)?;
    peer.verify()?;
    if expected.is_some_and(|ad| ad.peer_id != peer.peer_id || ad.ek != peer.ek || ad.vk != peer.vk)
    {
        return Err(Error::AuthenticationFailed);
    }
    progress.peer_id = Some(peer.peer_id);
    progress.had_pair =
        keys.current_session(peer.peer_id).is_ok() || keys.cached_pair(peer.peer_id)?.is_some();
    let preserve_existing = preserve_existing(peer.peer_id)?;
    if preserve_existing {
        if keys.load_verified_peer(&peer.peer_id)? != peer || !progress.had_pair {
            return Err(Error::AuthenticationFailed);
        }
    } else {
        keys.import_peer(peer.clone())?;
    }

    let local_epoch = keys.peer_epoch(peer.peer_id)?;
    write_frame(
        stream,
        &Frame::new(
            EPOCH_HINT_KIND,
            encoding::epoch_hint_m(&local.peer_id, local_epoch),
        )?,
    )
    .await?;
    let hint_frame = read_frame(stream).await?;
    require_kind(&hint_frame, EPOCH_HINT_KIND)?;
    let (hint_peer, remote_epoch) = encoding::decode_epoch_hint(&hint_frame.payload)?;
    if hint_peer != peer.peer_id {
        return Err(Error::AuthenticationFailed);
    }

    if preserve_existing {
        return finish_preserved(stream, keys, &peer, local_epoch, remote_epoch, progress).await;
    }
    let wrap = RustCryptoConstructionBWrap::new(keys.clone());
    if let Some(message) = initial_wrap(keys, &wrap, &local, &peer, local_epoch, remote_epoch)? {
        send_wrap(stream, &message).await?;
    }
    finish_wrap(stream, keys, &wrap, &peer, progress).await
}

async fn finish_preserved(
    stream: &mut TcpStream,
    keys: &KeyStore,
    peer: &IdentityDocument,
    local_epoch: Epoch,
    remote_epoch: Epoch,
    progress: &mut HandshakeProgress,
) -> Result<EstablishedSession> {
    let session = keys.current_session(peer.peer_id)?;
    if local_epoch != session.epoch || remote_epoch != session.epoch {
        return finish_candidate(stream, keys, peer, local_epoch, remote_epoch, progress).await;
    }
    let (initiator, cached) = keys
        .cached_pair(peer.peer_id)?
        .ok_or(Error::State("active pair has no cached wrap"))?;
    let local_initiated = initiator == keys.peer_id()?;
    let wrap = RustCryptoConstructionBWrap::new(keys.clone());
    if local_initiated {
        send_wrap(stream, &wrap.retry(&cached)?).await?;
    }
    let frame = read_frame(stream).await?;
    match frame.kind {
        WRAP_KIND if !local_initiated => {
            let incoming = encoding::decode_wrap(&frame.payload)?;
            if incoming != cached {
                return Err(Error::State(
                    "active pair cannot rotate during another Join",
                ));
            }
            let opened = wrap.unwrap(peer.peer_id, &incoming)?;
            if opened.epoch != session.epoch {
                return Err(Error::State(
                    "active pair cannot rotate during another Join",
                ));
            }
            send_ack(stream, session.epoch, false).await?;
        }
        WRAP_ACK_KIND if local_initiated => {
            let (epoch, retry_pending) = encoding::decode_wrap_ack(&frame.payload)?;
            if retry_pending || epoch != session.epoch {
                return Err(Error::State(
                    "active pair cannot rotate during another Join",
                ));
            }
        }
        _ => {
            return Err(Error::State(
                "active pair cannot rotate during another Join",
            ));
        }
    }
    progress.wrap_acknowledged = true;
    log_established(peer, session.epoch);
    Ok(EstablishedSession {
        peer: peer.clone(),
        session,
    })
}

async fn finish_candidate(
    stream: &mut TcpStream,
    keys: &KeyStore,
    peer: &IdentityDocument,
    local_epoch: Epoch,
    remote_epoch: Epoch,
    progress: &mut HandshakeProgress,
) -> Result<EstablishedSession> {
    let wrap = RustCryptoConstructionBWrap::new(keys.clone());
    let confirmed_epoch = keys.current_session(peer.peer_id)?.epoch;
    if let Some((initiator, cached)) = keys.cached_candidate(peer.peer_id)? {
        if initiator == keys.peer_id()? {
            send_wrap(stream, &wrap.retry(&cached)?).await?;
        }
    } else if keys.peer_id()? < peer.peer_id
        || (local_epoch > confirmed_epoch && local_epoch >= remote_epoch)
    {
        // The hint only chooses who speaks first. It never advances the
        // watermark or replaces the confirmed pair.
        let epoch = keys.next_epoch(&peer.peer_id)?;
        let (_, message) = wrap.create_candidate(peer.peer_id, &peer.ek, epoch)?;
        send_wrap(stream, &message).await?;
    }

    loop {
        let frame = read_frame(stream).await?;
        match frame.kind {
            WRAP_KIND => {
                let incoming = encoding::decode_wrap(&frame.payload)?;
                if keys
                    .candidate_session(peer.peer_id)
                    .is_ok_and(|candidate| incoming.epoch < candidate.epoch)
                {
                    RustCryptoPureMlDsa.verify(
                        &peer.vk,
                        WRAP_CONTEXT,
                        &encoding::wrap_m(&incoming)?,
                        &incoming.signature,
                    )?;
                    continue;
                }
                match wrap.unwrap_candidate(peer.peer_id, &incoming) {
                    Ok(candidate) if keys.candidate_retry_epoch(&peer.peer_id)?.is_some() => {
                        send_ack(stream, candidate.epoch, true).await?;
                        let (_, retry) = wrap.retry_candidate_collision(peer.peer_id)?;
                        send_wrap(stream, &retry).await?;
                    }
                    Ok(candidate) => {
                        send_ack(stream, candidate.epoch, false).await?;
                        progress.wrap_acknowledged = true;
                        log_established(peer, candidate.epoch);
                        return Ok(EstablishedSession {
                            peer: peer.clone(),
                            session: candidate,
                        });
                    }
                    Err(Error::EpochConflict { .. }) => {
                        // The local candidate won; wait for its acknowledgment.
                    }
                    Err(error) => return Err(error),
                }
            }
            WRAP_ACK_KIND => {
                let (epoch, retry_pending) = encoding::decode_wrap_ack(&frame.payload)?;
                if retry_pending {
                    keys.discard_candidate(peer.peer_id)?;
                    continue;
                }
                let candidate = keys.candidate_session(peer.peer_id)?;
                if candidate.epoch != epoch {
                    return Err(Error::State(
                        "WrapAck epoch does not match admission candidate",
                    ));
                }
                progress.wrap_acknowledged = true;
                log_established(peer, candidate.epoch);
                return Ok(EstablishedSession {
                    peer: peer.clone(),
                    session: candidate,
                });
            }
            _ => return Err(Error::State("unexpected frame during candidate handshake")),
        }
    }
}

fn initial_wrap(
    keys: &KeyStore,
    wrap: &RustCryptoConstructionBWrap,
    local: &IdentityDocument,
    peer: &IdentityDocument,
    local_epoch: Epoch,
    remote_epoch: Epoch,
) -> Result<Option<WrapMessage>> {
    let floor = local_epoch.max(remote_epoch);
    let cached = keys.cached_pair(peer.peer_id)?;
    let has_active = keys.current_session(peer.peer_id).is_ok();
    let cached_is_eligible = cached.as_ref().is_some_and(|(initiator, message)| {
        *initiator == local.peer_id && message.epoch == floor && has_active
    });
    if cached_is_eligible && (local_epoch >= remote_epoch || local.peer_id < peer.peer_id) {
        let message = cached
            .ok_or(Error::State("eligible cached wrap disappeared"))?
            .1;
        return Ok(Some(wrap.retry(&message)?));
    }

    let should_initiate = local.peer_id < peer.peer_id
        && (local_epoch != remote_epoch
            || cached.is_none()
            || local_epoch == Epoch(0)
            || !has_active);
    if should_initiate {
        keys.observe_remote_epoch(peer.peer_id, floor)?;
        let epoch = keys.next_epoch(&peer.peer_id)?;
        return wrap
            .create(peer.peer_id, &peer.ek, epoch)
            .map(|(_, message)| Some(message));
    }
    Ok(None)
}

async fn finish_wrap(
    stream: &mut TcpStream,
    keys: &KeyStore,
    wrap: &RustCryptoConstructionBWrap,
    peer: &IdentityDocument,
    progress: &mut HandshakeProgress,
) -> Result<EstablishedSession> {
    loop {
        let frame = read_frame(stream).await?;
        match frame.kind {
            WRAP_KIND => {
                let message = encoding::decode_wrap(&frame.payload)?;
                if keys
                    .current_session(peer.peer_id)
                    .is_ok_and(|session| message.epoch < session.epoch)
                {
                    RustCryptoPureMlDsa.verify(
                        &peer.vk,
                        WRAP_CONTEXT,
                        &encoding::wrap_m(&message)?,
                        &message.signature,
                    )?;
                    continue;
                }
                prepare_incoming_floor(keys, peer, &message)?;
                match wrap.unwrap(peer.peer_id, &message) {
                    Ok(session) => {
                        if keys.retry_epoch(&peer.peer_id)?.is_some() {
                            send_ack(stream, message.epoch, true).await?;
                            let (_, retry) = wrap.retry_collision(peer.peer_id)?;
                            send_wrap(stream, &retry).await?;
                        } else {
                            send_ack(stream, session.epoch, false).await?;
                            progress.wrap_acknowledged = true;
                            log_established(peer, session.epoch);
                            return Ok(EstablishedSession {
                                peer: peer.clone(),
                                session,
                            });
                        }
                    }
                    Err(Error::EpochConflict { .. }) => {
                        // The local cached wrap won. Its acknowledgment remains pending.
                    }
                    Err(error) => return Err(error),
                }
            }
            WRAP_ACK_KIND => {
                let (epoch, retry_pending) = encoding::decode_wrap_ack(&frame.payload)?;
                if retry_pending {
                    continue;
                }
                let session = keys.current_session(peer.peer_id)?;
                if epoch != session.epoch {
                    return Err(Error::State("WrapAck epoch does not match active session"));
                }
                progress.wrap_acknowledged = true;
                log_established(peer, session.epoch);
                return Ok(EstablishedSession {
                    peer: peer.clone(),
                    session,
                });
            }
            _ => return Err(Error::State("unexpected frame during pair handshake")),
        }
    }
}

fn log_established(peer: &IdentityDocument, epoch: Epoch) {
    demo_log::event(
        Kind::Security,
        "X-Wing + ML-DSA-65",
        "Encrypted peer session established",
        &[
            format!("peer   {}", demo_log::peer(peer.peer_id)),
            format!("epoch  {} · AES-256-GCM transport", epoch.0),
        ],
    );
}

fn prepare_incoming_floor(
    keys: &KeyStore,
    peer: &IdentityDocument,
    message: &WrapMessage,
) -> Result<()> {
    if keys.current_session(peer.peer_id).is_ok() {
        return Ok(());
    }
    let local_id = keys.peer_id()?;
    if peer.peer_id >= local_id
        || message.min_id != peer.peer_id.min(local_id)
        || message.max_id != peer.peer_id.max(local_id)
    {
        return Ok(());
    }
    let floor = keys.peer_epoch(peer.peer_id)?;
    let Some(predecessor) = message.epoch.0.checked_sub(1) else {
        return Ok(());
    };
    if predecessor > floor.0 {
        RustCryptoPureMlDsa.verify(
            &peer.vk,
            WRAP_CONTEXT,
            &encoding::wrap_m(message)?,
            &message.signature,
        )?;
        keys.observe_remote_epoch(peer.peer_id, Epoch(predecessor))?;
    }
    Ok(())
}

async fn send_wrap(stream: &mut TcpStream, message: &WrapMessage) -> Result<()> {
    write_frame(
        stream,
        &Frame::new(WRAP_KIND, encoding::encode_wrap(message)?)?,
    )
    .await
}

async fn send_ack(stream: &mut TcpStream, epoch: Epoch, retry_pending: bool) -> Result<()> {
    write_frame(
        stream,
        &Frame::new(
            WRAP_ACK_KIND,
            encoding::encode_wrap_ack(epoch, retry_pending).to_vec(),
        )?,
    )
    .await
}

fn require_kind(frame: &Frame, expected: u8) -> Result<()> {
    if frame.kind != expected {
        return Err(Error::State("unexpected frame during pair handshake"));
    }
    Ok(())
}

pub fn seal_packet(keys: &KeyStore, peer_id: PeerId, plaintext: &[u8]) -> Result<ControlPacket> {
    let session = keys.current_session(peer_id)?;
    seal_packet_with_session(keys, &session, plaintext)
}

pub(crate) fn seal_packet_with_session(
    keys: &KeyStore,
    session: &PairSession,
    plaintext: &[u8],
) -> Result<ControlPacket> {
    let peer_id = session.peer_id;
    let header = PacketHeader {
        version: PROTOCOL_VERSION,
        sender_id: keys.peer_id()?,
        receiver_id: peer_id,
        epoch: session.epoch,
        seq: keys.next_outbound_seq(session.key_handle(), PayloadType::Packet)?,
    };
    let aad = encoding::packet_aad(&header);
    let nonce = encoding::nonce(&header, PayloadType::Packet);
    let ciphertext = RustCryptoAes256Gcm::new(keys.clone()).seal(
        session.key_handle(),
        &nonce,
        &aad,
        plaintext,
    )?;
    Ok(ControlPacket { header, ciphertext })
}

pub fn open_packet(keys: &KeyStore, peer_id: PeerId, packet: &ControlPacket) -> Result<Vec<u8>> {
    let session = keys.current_session(peer_id)?;
    open_packet_with_session(keys, &session, packet)
}

pub(crate) fn open_packet_with_session(
    keys: &KeyStore,
    session: &PairSession,
    packet: &ControlPacket,
) -> Result<Vec<u8>> {
    let aad = encoding::packet_aad(&packet.header);
    let nonce = encoding::nonce(&packet.header, PayloadType::Packet);
    RustCryptoAes256Gcm::new(keys.clone()).open(
        session.key_handle(),
        &nonce,
        &aad,
        &packet.ciphertext,
    )
}
