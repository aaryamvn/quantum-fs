//! Canonical bytes from crypto-encoding.md. These object encoders supply both AAD
//! and Pure ML-DSA M where applicable; signatures themselves are not part of M.
//! IDs are raw bytes, integers big-endian, variable fields u32-length-prefixed.

use std::{net::SocketAddr, str::FromStr};

use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::{
    crypto::{
        identity::{IdentityDocument, LocalIdentity},
        wrap::WrapMessage,
    },
    ids::{ChunkId, Epoch, FileId, PeerId},
    keystore::{PersistedKey, PersistedPair, PersistedState},
    net::{
        directory::{DirForget, DirectoryAd, DirectoryState, DirectoryWatermark},
        join::{FlushOffer, JoinRequest, NetControl, NetWelcome, VaultMetadata},
        JoinCode, VaultId,
    },
    protocol::{
        locate::{require_count as require_have_count, HaveQuery, HaveReply, HAVE_MAGIC},
        manifest::Manifest,
        packet::{ControlPacket, PacketHeader, PayloadType},
        pull::{ChunkBodyFrame, PullRequest, MAX_PULL_CHUNK_IDS},
    },
    store::{replica::ReplicaMetadata, tree::DirectoryTree},
    sync::host::{ControlRecord, ControlUpdate, FlushChallenge, MailboxEnvelope, QueueContent},
    Error, Result,
};

pub const WRAP_GCM_NONCE: [u8; 12] = [0; 12];
const PEER_ID_DOMAIN: &[u8] = b"qfs/v1/peer";
const WRAP_PAIR_DOMAIN: &[u8] = b"qfs/v1/wrap/pair/";
const LOCAL_IDENTITY_MAGIC: &[u8] = b"qfs/local/identity/";
const STORE_STATE_MAGIC: &[u8] = b"qfs/local/keys/";
const DIRECTORY_STATE_MAGIC: &[u8] = b"qfs/local/directory/";
const VAULT_METADATA_MAGIC: &[u8] = b"qfs/local/vault/";
const REPLICA_MAGIC: &[u8] = b"qfs/local/replica/";
const LOCAL_FORMAT_VERSION: u8 = 1;
const MAX_REPLICA_BYTES: usize = 16 * 1024 * 1024;
const MAX_DIRECTORY_RECORDS: usize = 10_000;

pub fn peer_id(vk: &[u8]) -> PeerId {
    let mut hash = Sha256::new();
    hash.update(PEER_ID_DOMAIN);
    hash.update(vk);
    PeerId(hash.finalize().into())
}

pub fn chunk_id(file_id: &FileId, index: u64, plaintext: &[u8]) -> ChunkId {
    let mut hash = Sha256::new();
    hash.update(file_id.0);
    hash.update(index.to_be_bytes());
    hash.update(plaintext);
    ChunkId(hash.finalize().into())
}

fn require_sorted_pair(min_id: &PeerId, max_id: &PeerId) -> Result<()> {
    if min_id >= max_id {
        return Err(Error::InvalidInput(
            "wrap requires min_id < max_id as unsigned bytes",
        ));
    }
    Ok(())
}

fn push_length(out: &mut Vec<u8>, len: usize) -> Result<()> {
    let len = u32::try_from(len)
        .map_err(|_| Error::InvalidInput("variable field exceeds u32 encoding limit"))?;
    out.extend_from_slice(&len.to_be_bytes());
    Ok(())
}

fn push_variable(out: &mut Vec<u8>, value: &[u8]) -> Result<()> {
    push_length(out, value.len())?;
    out.extend_from_slice(value);
    Ok(())
}

pub fn wrap_hkdf_info(min_id: &PeerId, max_id: &PeerId, epoch: Epoch) -> Result<Vec<u8>> {
    require_sorted_pair(min_id, max_id)?;
    let mut out = Vec::with_capacity(WRAP_PAIR_DOMAIN.len() + 74);
    out.extend_from_slice(WRAP_PAIR_DOMAIN);
    out.extend_from_slice(&min_id.0);
    out.push(0x3a);
    out.extend_from_slice(&max_id.0);
    out.push(0x2f);
    out.extend_from_slice(&epoch.0.to_be_bytes());
    Ok(out)
}

pub fn packet_aad(header: &PacketHeader) -> Vec<u8> {
    let mut out = Vec::with_capacity(81);
    out.push(header.version);
    out.extend_from_slice(&header.sender_id.0);
    out.extend_from_slice(&header.receiver_id.0);
    out.extend_from_slice(&header.epoch.0.to_be_bytes());
    out.extend_from_slice(&header.seq.0.to_be_bytes());
    out
}

pub fn chunk_aad(header: &PacketHeader, file_id: &FileId, index: u64) -> Vec<u8> {
    let mut out = packet_aad(header);
    out.extend_from_slice(&file_id.0);
    out.extend_from_slice(&index.to_be_bytes());
    out
}

/// Decode one of the two canonical AEAD AAD layouts. Packet AAD is exactly 81
/// bytes; chunk-body AAD appends the canonical 32-byte file id and 8-byte index.
pub fn decode_aad(aad: &[u8]) -> Result<(PacketHeader, PayloadType)> {
    let payload_type = match aad.len() {
        81 => PayloadType::Packet,
        121 => PayloadType::ChunkBody,
        _ => return Err(Error::InvalidInput("invalid AEAD AAD length")),
    };
    if aad[0] != crate::protocol::packet::PROTOCOL_VERSION {
        return Err(Error::InvalidInput("unsupported packet version"));
    }

    let mut sender_id = [0; 32];
    sender_id.copy_from_slice(&aad[1..33]);
    let mut receiver_id = [0; 32];
    receiver_id.copy_from_slice(&aad[33..65]);
    let mut epoch = [0; 8];
    epoch.copy_from_slice(&aad[65..73]);
    let mut seq = [0; 8];
    seq.copy_from_slice(&aad[73..81]);

    Ok((
        PacketHeader {
            version: aad[0],
            sender_id: PeerId(sender_id),
            receiver_id: PeerId(receiver_id),
            epoch: Epoch(u64::from_be_bytes(epoch)),
            seq: crate::ids::Seq(u64::from_be_bytes(seq)),
        },
        payload_type,
    ))
}

pub fn wrap_aad(min_id: &PeerId, max_id: &PeerId, epoch: Epoch) -> Result<Vec<u8>> {
    require_sorted_pair(min_id, max_id)?;
    let mut out = Vec::with_capacity(72);
    out.extend_from_slice(&min_id.0);
    out.extend_from_slice(&max_id.0);
    out.extend_from_slice(&epoch.0.to_be_bytes());
    Ok(out)
}

pub fn identity_m(document: &IdentityDocument) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(&document.peer_id.0);
    push_variable(&mut out, &document.ek)?;
    push_variable(&mut out, &document.vk)?;
    out.extend_from_slice(&document.created_at.to_be_bytes());
    Ok(out)
}

pub fn encode_identity(document: &IdentityDocument) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    push_variable(&mut out, &identity_m(document)?)?;
    push_variable(&mut out, &document.signature)?;
    Ok(out)
}

pub fn decode_identity(bytes: &[u8]) -> Result<IdentityDocument> {
    let mut reader = Reader::new(bytes);
    let message = reader.variable()?;
    let signature = reader.variable()?;
    reader.finish()?;
    decode_identity_document(message, signature)
}

pub fn wrap_m(message: &WrapMessage) -> Result<Vec<u8>> {
    require_sorted_pair(&message.min_id, &message.max_id)?;
    let mut out = Vec::new();
    push_variable(&mut out, &message.kem_ct)?;
    push_variable(&mut out, &message.wrap_ct)?;
    out.extend_from_slice(&message.epoch.0.to_be_bytes());
    out.extend_from_slice(&message.min_id.0);
    out.extend_from_slice(&message.max_id.0);
    Ok(out)
}

pub fn encode_wrap(message: &WrapMessage) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    push_variable(&mut out, &wrap_m(message)?)?;
    push_variable(&mut out, &message.signature)?;
    Ok(out)
}

pub fn decode_wrap(bytes: &[u8]) -> Result<WrapMessage> {
    let mut reader = Reader::new(bytes);
    let message = reader.variable()?;
    let signature = reader.variable()?;
    reader.finish()?;
    decode_wrap_message(message, signature)
}

pub fn encode_control_packet(packet: &ControlPacket) -> Result<Vec<u8>> {
    let mut out = packet_aad(&packet.header);
    push_variable(&mut out, &packet.ciphertext)?;
    Ok(out)
}

pub fn decode_control_packet(bytes: &[u8]) -> Result<ControlPacket> {
    let mut reader = Reader::new(bytes);
    let aad = reader.take(81)?;
    let (header, payload_type) = decode_aad(aad)?;
    if payload_type != PayloadType::Packet {
        return Err(Error::InvalidInput("control packet has chunk AAD"));
    }
    let ciphertext = reader.variable()?.to_vec();
    reader.finish()?;
    Ok(ControlPacket { header, ciphertext })
}

pub fn encode_chunk_body_frame(frame: &ChunkBodyFrame) -> Result<Vec<u8>> {
    let mut out = chunk_aad(&frame.header, &frame.file_id, frame.index);
    push_variable(&mut out, &frame.ciphertext)?;
    Ok(out)
}

pub fn decode_chunk_body_frame(bytes: &[u8]) -> Result<ChunkBodyFrame> {
    let mut reader = Reader::new(bytes);
    let aad = reader.take(121)?;
    let (header, payload_type) = decode_aad(aad)?;
    if payload_type != PayloadType::ChunkBody {
        return Err(Error::InvalidInput("chunk body frame has packet AAD"));
    }
    let mut file_id = [0; 32];
    file_id.copy_from_slice(&aad[81..113]);
    let mut index = [0; 8];
    index.copy_from_slice(&aad[113..121]);
    let ciphertext = reader.variable()?.to_vec();
    reader.finish()?;
    Ok(ChunkBodyFrame {
        header,
        file_id: FileId(file_id),
        index: u64::from_be_bytes(index),
        ciphertext,
    })
}

pub fn epoch_hint_m(peer_id: &PeerId, epoch: Epoch) -> Vec<u8> {
    let mut out = Vec::with_capacity(40);
    out.extend_from_slice(&peer_id.0);
    out.extend_from_slice(&epoch.0.to_be_bytes());
    out
}

pub fn decode_epoch_hint(bytes: &[u8]) -> Result<(PeerId, Epoch)> {
    let mut reader = Reader::new(bytes);
    let peer_id = PeerId(reader.array()?);
    let epoch = Epoch(reader.u64()?);
    reader.finish()?;
    Ok((peer_id, epoch))
}

pub fn encode_wrap_ack(epoch: Epoch, retry_pending: bool) -> [u8; 9] {
    let mut out = [0; 9];
    out[..8].copy_from_slice(&epoch.0.to_be_bytes());
    out[8] = u8::from(retry_pending);
    out
}

pub fn decode_wrap_ack(bytes: &[u8]) -> Result<(Epoch, bool)> {
    let mut reader = Reader::new(bytes);
    let epoch = Epoch(reader.u64()?);
    let retry_pending = match reader.byte()? {
        0 => false,
        1 => true,
        _ => return Err(Error::InvalidInput("invalid WrapAck retry flag")),
    };
    reader.finish()?;
    Ok((epoch, retry_pending))
}

pub fn directory_ad_m(ad: &DirectoryAd) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(&ad.peer_id.0);
    out.extend_from_slice(&ad.vault_id.0);
    // Only numeric SocketAddr forms are admitted; hostnames and DNS are outside v1.
    push_variable(&mut out, ad.addr.to_string().as_bytes())?;
    push_variable(&mut out, &ad.ek)?;
    push_variable(&mut out, &ad.vk)?;
    out.extend_from_slice(&ad.issued_at.to_be_bytes());
    Ok(out)
}

pub fn encode_directory_ad(ad: &DirectoryAd) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    push_variable(&mut out, &directory_ad_m(ad)?)?;
    push_variable(&mut out, &ad.signature)?;
    Ok(out)
}

pub fn decode_directory_ad(bytes: &[u8]) -> Result<DirectoryAd> {
    let mut reader = Reader::new(bytes);
    let message = reader.variable()?;
    let signature = reader.variable()?.to_vec();
    reader.finish()?;
    decode_directory_ad_message(message, signature)
}

pub fn dir_forget_m(request: &DirForget) -> Vec<u8> {
    let mut out = Vec::with_capacity(88);
    out.extend_from_slice(&request.peer_id.0);
    out.extend_from_slice(&request.vault_id.0);
    out.extend_from_slice(&request.join_code.0);
    out.extend_from_slice(&request.issued_at.to_be_bytes());
    out
}

pub fn encode_dir_forget(request: &DirForget) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    push_variable(&mut out, &dir_forget_m(request))?;
    push_variable(&mut out, &request.signature)?;
    Ok(out)
}

pub fn decode_dir_forget(bytes: &[u8]) -> Result<DirForget> {
    let mut reader = Reader::new(bytes);
    let message = reader.variable()?;
    let signature = reader.variable()?.to_vec();
    reader.finish()?;
    let mut message = Reader::new(message);
    let request = DirForget {
        peer_id: PeerId(message.array()?),
        vault_id: VaultId(message.array()?),
        join_code: JoinCode(message.array()?),
        issued_at: message.u64()?,
        signature,
    };
    message.finish()?;
    Ok(request)
}

pub fn join_request_m(request: &JoinRequest) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(&request.vault_id.0);
    out.extend_from_slice(&request.join_code.0);
    out.extend_from_slice(&identity_m(&request.document)?);
    Ok(out)
}

pub fn encode_join_request(request: &JoinRequest) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    push_variable(&mut out, &join_request_m(request)?)?;
    push_variable(&mut out, &request.signature)?;
    Ok(out)
}

/// The identity signature was already verified during the preceding Identity
/// exchange and is not duplicated in Join M. The decoded request must contain
/// the exact canonical bytes of that exchanged document.
pub fn decode_join_request(bytes: &[u8], exchanged: &IdentityDocument) -> Result<JoinRequest> {
    let mut reader = Reader::new(bytes);
    let message = reader.variable()?;
    let signature = reader.variable()?.to_vec();
    reader.finish()?;

    let mut message_reader = Reader::new(message);
    let vault_id = VaultId(message_reader.array()?);
    let join_code = JoinCode(message_reader.array()?);
    let identity = message_reader.take(message_reader.remaining())?;
    if identity != identity_m(exchanged)? {
        return Err(Error::AuthenticationFailed);
    }
    message_reader.finish()?;
    Ok(JoinRequest {
        vault_id,
        join_code,
        document: exchanged.clone(),
        signature,
    })
}

pub fn encode_net_welcome(welcome: &NetWelcome) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(&welcome.vault_id.0);
    push_length(&mut out, welcome.members.len())?;
    for member in &welcome.members {
        push_variable(&mut out, &encode_identity(member)?)?;
    }
    Ok(out)
}

pub fn decode_net_welcome(bytes: &[u8]) -> Result<NetWelcome> {
    let mut reader = Reader::new(bytes);
    let vault_id = VaultId(reader.array()?);
    let count = reader.count(56)?;
    let mut members = Vec::with_capacity(count);
    for _ in 0..count {
        members.push(decode_identity(reader.variable()?)?);
    }
    reader.finish()?;
    Ok(NetWelcome { vault_id, members })
}

pub fn encode_flush_offer(offer: &FlushOffer) -> Result<[u8; 36]> {
    if offer.frame_count > 1_000_000 {
        return Err(Error::InvalidInput("flush frame count exceeds cap"));
    }
    let mut out = [0; 36];
    out[..32].copy_from_slice(&offer.challenge.0);
    out[32..].copy_from_slice(&offer.frame_count.to_be_bytes());
    Ok(out)
}

pub fn decode_flush_offer(bytes: &[u8]) -> Result<FlushOffer> {
    let mut reader = Reader::new(bytes);
    let challenge = FlushChallenge(reader.array()?);
    let frame_count = reader.u32()?;
    reader.finish()?;
    if frame_count > 1_000_000 {
        return Err(Error::InvalidInput("flush frame count exceeds cap"));
    }
    Ok(FlushOffer {
        challenge,
        frame_count,
    })
}

pub fn encode_net_control(control: &NetControl) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    match control {
        NetControl::JoinAccepted { vault_id, members } => {
            out.push(1);
            out.extend_from_slice(&encode_net_welcome(&NetWelcome {
                vault_id: *vault_id,
                members: members.clone(),
            })?);
        }
        NetControl::FlushEnd { digest } => {
            out.push(2);
            out.extend_from_slice(digest);
        }
        NetControl::FlushApplied { digest } => {
            out.push(3);
            out.extend_from_slice(digest);
        }
        NetControl::Ready => out.push(4),
        NetControl::Heartbeat => out.push(5),
        NetControl::HeartbeatApplied { through } => {
            out.push(5);
            out.extend_from_slice(&through.to_be_bytes());
        }
    }
    Ok(out)
}

pub fn decode_net_control(bytes: &[u8]) -> Result<NetControl> {
    let mut reader = Reader::new(bytes);
    let control = match reader.byte()? {
        1 => {
            let welcome = decode_net_welcome(reader.take(reader.remaining())?)?;
            NetControl::JoinAccepted {
                vault_id: welcome.vault_id,
                members: welcome.members,
            }
        }
        2 => NetControl::FlushEnd {
            digest: reader.array()?,
        },
        3 => NetControl::FlushApplied {
            digest: reader.array()?,
        },
        4 => NetControl::Ready,
        5 => match reader.remaining() {
            0 => NetControl::Heartbeat,
            8 => NetControl::HeartbeatApplied {
                through: reader.u64()?,
            },
            _ => return Err(Error::InvalidInput("invalid heartbeat control length")),
        },
        _ => {
            return Err(Error::InvalidInput(
                "unknown encrypted network control kind",
            ))
        }
    };
    reader.finish()?;
    Ok(control)
}

pub fn encode_vault_metadata(metadata: &VaultMetadata) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(VAULT_METADATA_MAGIC);
    out.push(LOCAL_FORMAT_VERSION);
    out.extend_from_slice(&metadata.vault_id.0);
    out.extend_from_slice(&metadata.join_code.0);
    out.extend_from_slice(&metadata.issued_at.to_be_bytes());
    push_length(&mut out, metadata.members.len())?;
    for member in &metadata.members {
        out.extend_from_slice(&member.0);
    }
    Ok(out)
}

pub fn decode_vault_metadata(bytes: &[u8]) -> Result<VaultMetadata> {
    let mut reader = Reader::new(bytes);
    reader.require_prefix(VAULT_METADATA_MAGIC)?;
    reader.require_version()?;
    let vault_id = VaultId(reader.array()?);
    let join_code = JoinCode(reader.array()?);
    let issued_at = reader.u64()?;
    let count = reader.count(32)?;
    let mut members = Vec::with_capacity(count);
    for _ in 0..count {
        let member = PeerId(reader.array()?);
        if members.contains(&member) {
            return Err(Error::InvalidInput("duplicate vault member"));
        }
        members.push(member);
    }
    reader.finish()?;
    Ok(VaultMetadata {
        vault_id,
        join_code,
        issued_at,
        members,
    })
}

fn decode_directory_ad_message(message: &[u8], signature: Vec<u8>) -> Result<DirectoryAd> {
    let mut reader = Reader::new(message);
    let peer_id = PeerId(reader.array()?);
    let vault_id = VaultId(reader.array()?);
    let addr = std::str::from_utf8(reader.variable()?)
        .map_err(|_| Error::InvalidInput("directory address is not UTF-8"))?;
    let addr = SocketAddr::from_str(addr)
        .map_err(|_| Error::InvalidInput("directory address is not a numeric socket address"))?;
    let ek = reader.variable()?.to_vec();
    let vk = reader.variable()?.to_vec();
    let issued_at = reader.u64()?;
    reader.finish()?;
    Ok(DirectoryAd {
        peer_id,
        vault_id,
        addr,
        ek,
        vk,
        issued_at,
        signature,
    })
}

pub fn manifest_m(manifest: &Manifest) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(&manifest.file_id.0);
    out.extend_from_slice(&manifest.version.to_be_bytes());
    out.extend_from_slice(&manifest.size.to_be_bytes());
    out.extend_from_slice(&manifest.writer_id.0);
    push_length(&mut out, manifest.chunk_ids.len())?;
    for chunk_id in &manifest.chunk_ids {
        out.extend_from_slice(&chunk_id.0);
    }
    Ok(out)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MailboxFrame {
    Control {
        ciphertext: Vec<u8>,
    },
    ChunkBody {
        file_id: FileId,
        index: u64,
        ciphertext: Vec<u8>,
    },
}

/// Encodes the typed content of a mailbox envelope. The ciphertext is already
/// pairwise GCM output; this framing does not apply another encryption layer.
pub fn encode_mailbox_frame(frame: &MailboxFrame) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    match frame {
        MailboxFrame::Control { ciphertext } => {
            out.push(0);
            push_variable(&mut out, ciphertext)?;
        }
        MailboxFrame::ChunkBody {
            file_id,
            index,
            ciphertext,
        } => {
            out.push(1);
            out.extend_from_slice(&file_id.0);
            out.extend_from_slice(&index.to_be_bytes());
            push_variable(&mut out, ciphertext)?;
        }
    }
    Ok(out)
}

pub fn decode_mailbox_frame(bytes: &[u8]) -> Result<MailboxFrame> {
    let mut reader = Reader::new(bytes);
    let frame = match reader.byte()? {
        0 => MailboxFrame::Control {
            ciphertext: reader.variable()?.to_vec(),
        },
        1 => MailboxFrame::ChunkBody {
            file_id: FileId(reader.array()?),
            index: reader.u64()?,
            ciphertext: reader.variable()?.to_vec(),
        },
        _ => return Err(Error::InvalidInput("unknown mailbox frame kind")),
    };
    reader.finish()?;
    Ok(frame)
}

/// Transport form for a queued envelope. `queued_at` is local host metadata
/// and is intentionally omitted; a received envelope reconstructs it as zero.
pub fn encode_mailbox_envelope(envelope: &MailboxEnvelope) -> Result<Vec<u8>> {
    let header = PacketHeader {
        version: crate::protocol::packet::PROTOCOL_VERSION,
        sender_id: envelope.sender_id,
        receiver_id: envelope.recipient_id,
        epoch: envelope.epoch,
        seq: envelope.seq,
    };
    let mut out = Vec::new();
    match decode_mailbox_frame(&envelope.ciphertext)? {
        MailboxFrame::Control { ciphertext } => {
            out.push(0);
            push_variable(
                &mut out,
                &encode_control_packet(&ControlPacket { header, ciphertext })?,
            )?;
        }
        MailboxFrame::ChunkBody {
            file_id,
            index,
            ciphertext,
        } => {
            out.push(1);
            push_variable(
                &mut out,
                &encode_chunk_body_frame(&ChunkBodyFrame {
                    header,
                    file_id,
                    index,
                    ciphertext,
                })?,
            )?;
        }
    }
    Ok(out)
}

pub fn decode_mailbox_envelope(bytes: &[u8]) -> Result<MailboxEnvelope> {
    let mut reader = Reader::new(bytes);
    let (header, ciphertext) = match reader.byte()? {
        0 => {
            let packet = decode_control_packet(reader.variable()?)?;
            let ciphertext = encode_mailbox_frame(&MailboxFrame::Control {
                ciphertext: packet.ciphertext,
            })?;
            (packet.header, ciphertext)
        }
        1 => {
            let frame = decode_chunk_body_frame(reader.variable()?)?;
            let ciphertext = encode_mailbox_frame(&MailboxFrame::ChunkBody {
                file_id: frame.file_id,
                index: frame.index,
                ciphertext: frame.ciphertext,
            })?;
            (frame.header, ciphertext)
        }
        _ => return Err(Error::InvalidInput("unknown mailbox transport kind")),
    };
    reader.finish()?;
    Ok(MailboxEnvelope {
        recipient_id: header.receiver_id,
        sender_id: header.sender_id,
        epoch: header.epoch,
        seq: header.seq,
        queued_at: 0,
        ciphertext,
    })
}

/// Canonical ordered mailbox commitment. Host-local queue timestamps are not
/// delivery data and do not affect the digest acknowledged by the recipient.
pub fn mailbox_wire_m(envelopes: &[MailboxEnvelope]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    push_length(&mut out, envelopes.len())?;
    for envelope in envelopes {
        push_variable(&mut out, &encode_mailbox_envelope(envelope)?)?;
    }
    Ok(out)
}

pub fn mailbox_digest(envelopes: &[MailboxEnvelope]) -> Result<[u8; 32]> {
    Ok(Sha256::digest(mailbox_wire_m(envelopes)?).into())
}

pub fn encode_pull_request(request: &PullRequest) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    push_length(&mut out, request.chunk_ids().len())?;
    for chunk_id in request.chunk_ids() {
        out.extend_from_slice(&chunk_id.0);
    }
    Ok(out)
}

pub fn decode_pull_request(bytes: &[u8]) -> Result<PullRequest> {
    let mut reader = Reader::new(bytes);
    let count = usize::try_from(reader.u32()?)
        .map_err(|_| Error::InvalidInput("pull request count is too large"))?;
    if count > MAX_PULL_CHUNK_IDS {
        return Err(Error::InvalidInput(
            "pull request exceeds the 32 chunk identifier cap",
        ));
    }
    if count > reader.remaining() / 32 {
        return Err(Error::InvalidInput("truncated pull request"));
    }
    let mut chunk_ids = Vec::with_capacity(count);
    for _ in 0..count {
        chunk_ids.push(ChunkId(reader.array()?));
    }
    reader.finish()?;
    PullRequest::new(chunk_ids)
}

pub fn encode_have_query(query: &HaveQuery) -> Result<Vec<u8>> {
    require_have_count(query.chunk_ids().len())?;
    let mut out = Vec::with_capacity(HAVE_MAGIC.len() + 1 + 32 + 4 + 32 * query.chunk_ids().len());
    out.extend_from_slice(HAVE_MAGIC);
    out.push(1);
    out.extend_from_slice(&query.file_id.0);
    push_length(&mut out, query.chunk_ids().len())?;
    for chunk_id in query.chunk_ids() {
        out.extend_from_slice(&chunk_id.0);
    }
    Ok(out)
}

pub fn decode_have_query(bytes: &[u8]) -> Result<HaveQuery> {
    let mut reader = Reader::new(bytes);
    reader.require_prefix(HAVE_MAGIC)?;
    if reader.byte()? != 1 {
        return Err(Error::InvalidInput("invalid have query kind"));
    }
    let file_id = FileId(reader.array()?);
    let count = usize::try_from(reader.u32()?)
        .map_err(|_| Error::InvalidInput("have query count is too large"))?;
    require_have_count(count)?;
    if count > reader.remaining() / 32 {
        return Err(Error::InvalidInput("truncated have query"));
    }
    let mut chunk_ids = Vec::with_capacity(count);
    for _ in 0..count {
        chunk_ids.push(ChunkId(reader.array()?));
    }
    reader.finish()?;
    HaveQuery::new(file_id, chunk_ids)
}

pub fn encode_have_reply(reply: &HaveReply) -> Result<Vec<u8>> {
    require_have_count(reply.chunk_ids.len())?;
    if reply.have_bitset.len() != reply.chunk_ids.len().div_ceil(8) {
        return Err(Error::InvalidInput("have reply bitset length mismatch"));
    }
    let mut out = Vec::with_capacity(
        HAVE_MAGIC.len() + 1 + 32 + 4 + 32 * reply.chunk_ids.len() + reply.have_bitset.len(),
    );
    out.extend_from_slice(HAVE_MAGIC);
    out.push(2);
    out.extend_from_slice(&reply.file_id.0);
    push_length(&mut out, reply.chunk_ids.len())?;
    for chunk_id in &reply.chunk_ids {
        out.extend_from_slice(&chunk_id.0);
    }
    out.extend_from_slice(&reply.have_bitset);
    Ok(out)
}

pub fn decode_have_reply(bytes: &[u8]) -> Result<HaveReply> {
    let mut reader = Reader::new(bytes);
    reader.require_prefix(HAVE_MAGIC)?;
    if reader.byte()? != 2 {
        return Err(Error::InvalidInput("invalid have reply kind"));
    }
    let file_id = FileId(reader.array()?);
    let count = usize::try_from(reader.u32()?)
        .map_err(|_| Error::InvalidInput("have reply count is too large"))?;
    require_have_count(count)?;
    let ids_bytes = count
        .checked_mul(32)
        .ok_or(Error::InvalidInput("have reply length overflow"))?;
    let bitset_len = count.div_ceil(8);
    if reader.remaining() != ids_bytes + bitset_len {
        return Err(Error::InvalidInput("have reply length mismatch"));
    }
    let mut chunk_ids = Vec::with_capacity(count);
    for _ in 0..count {
        chunk_ids.push(ChunkId(reader.array()?));
    }
    let have_bitset = reader.take(bitset_len)?.to_vec();
    reader.finish()?;
    Ok(HaveReply {
        file_id,
        chunk_ids,
        have_bitset,
    })
}

/// A signed manifest record stores canonical `manifest_m` bytes followed by
/// its detached signature. Decoding only parses fields; callers must verify
/// that signature before inspecting or applying the decoded chunk identifiers.
pub fn encode_control_record(record: &ControlRecord) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(&record.id.to_be_bytes());
    match &record.update {
        ControlUpdate::NewManifest(manifest) => {
            out.push(0);
            push_variable(&mut out, &manifest_m(manifest)?)?;
            push_variable(&mut out, &manifest.signature)?;
        }
        ControlUpdate::Add(file_id) => {
            out.push(1);
            out.extend_from_slice(&file_id.0);
        }
        ControlUpdate::Clear(file_id) => {
            out.push(2);
            out.extend_from_slice(&file_id.0);
        }
        ControlUpdate::Remove(file_id) => {
            out.push(3);
            out.extend_from_slice(&file_id.0);
        }
        ControlUpdate::Link {
            parent,
            name,
            child,
            is_dir,
        } => {
            out.push(4);
            out.extend_from_slice(&parent.0);
            push_variable(&mut out, name.as_bytes())?;
            out.extend_from_slice(&child.0);
            out.push(u8::from(*is_dir));
        }
        ControlUpdate::Unlink { parent, name } => {
            out.push(5);
            out.extend_from_slice(&parent.0);
            push_variable(&mut out, name.as_bytes())?;
        }
        ControlUpdate::Rename {
            src_parent,
            src_name,
            dst_parent,
            dst_name,
        } => {
            out.push(6);
            out.extend_from_slice(&src_parent.0);
            push_variable(&mut out, src_name.as_bytes())?;
            out.extend_from_slice(&dst_parent.0);
            push_variable(&mut out, dst_name.as_bytes())?;
        }
    }
    Ok(out)
}

pub fn decode_control_record(bytes: &[u8]) -> Result<ControlRecord> {
    let mut reader = Reader::new(bytes);
    let id = reader.u64()?;
    let update = match reader.byte()? {
        0 => {
            let message = reader.variable()?;
            let signature = reader.variable()?.to_vec();
            ControlUpdate::NewManifest(decode_manifest_message(message, signature)?)
        }
        1 => ControlUpdate::Add(FileId(reader.array()?)),
        2 => ControlUpdate::Clear(FileId(reader.array()?)),
        3 => ControlUpdate::Remove(FileId(reader.array()?)),
        4 => {
            let parent = FileId(reader.array()?);
            let name = decode_utf8_name(reader.variable()?)?;
            let child = FileId(reader.array()?);
            let is_dir = match reader.byte()? {
                0 => false,
                1 => true,
                _ => return Err(Error::InvalidInput("invalid Link directory flag")),
            };
            ControlUpdate::Link {
                parent,
                name,
                child,
                is_dir,
            }
        }
        5 => ControlUpdate::Unlink {
            parent: FileId(reader.array()?),
            name: decode_utf8_name(reader.variable()?)?,
        },
        6 => ControlUpdate::Rename {
            src_parent: FileId(reader.array()?),
            src_name: decode_utf8_name(reader.variable()?)?,
            dst_parent: FileId(reader.array()?),
            dst_name: decode_utf8_name(reader.variable()?)?,
        },
        _ => return Err(Error::InvalidInput("unknown control record kind")),
    };
    reader.finish()?;
    Ok(ControlRecord { id, update })
}

fn decode_utf8_name(bytes: &[u8]) -> Result<String> {
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| Error::InvalidInput("directory name is not UTF-8"))
}

fn decode_manifest_message(message: &[u8], signature: Vec<u8>) -> Result<Manifest> {
    let mut reader = Reader::new(message);
    let file_id = FileId(reader.array()?);
    let version = reader.u64()?;
    let size = reader.u64()?;
    let writer_id = PeerId(reader.array()?);
    let count = reader.count(32)?;
    let mut chunk_ids = Vec::with_capacity(count);
    for _ in 0..count {
        chunk_ids.push(ChunkId(reader.array()?));
    }
    reader.finish()?;
    Ok(Manifest {
        file_id,
        chunk_ids,
        size,
        writer_id,
        version,
        signature,
    })
}

/// Encode the authenticated local metadata snapshot. The hash covers the
/// complete header and body so decoding can reject damage before parsing any
/// body field.
pub fn encode_replica(metadata: &ReplicaMetadata, generation: u64) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    body.extend_from_slice(&metadata.expected_root.0);
    let tree = DirectoryTree::from_dirents(metadata.expected_root, metadata.dirents.clone())?;
    let dirents = tree.dirents();
    push_length(&mut body, dirents.len())?;
    for entry in dirents {
        body.extend_from_slice(&entry.parent.0);
        push_variable(&mut body, entry.name.as_bytes())?;
        body.extend_from_slice(&entry.child.0);
        body.push(u8::from(entry.is_dir));
    }

    push_length(&mut body, metadata.members.len())?;
    for member in &metadata.members {
        body.extend_from_slice(&member.0);
    }

    push_length(&mut body, metadata.manifests.len())?;
    for (file_id, manifest) in &metadata.manifests {
        if manifest.file_id != *file_id {
            return Err(Error::InvalidInput(
                "manifest map key does not match file id",
            ));
        }
        push_variable(&mut body, &manifest_m(manifest)?)?;
        push_variable(&mut body, &manifest.signature)?;
    }

    push_length(&mut body, metadata.log.len())?;
    let mut previous_record = None;
    for record in &metadata.log {
        if previous_record.is_some_and(|previous| previous >= record.id) {
            return Err(Error::InvalidInput(
                "instruction log is not strictly ordered",
            ));
        }
        push_variable(&mut body, &encode_control_record(record)?)?;
        previous_record = Some(record.id);
    }
    body.extend_from_slice(&metadata.next_control.to_be_bytes());

    push_length(&mut body, metadata.mailboxes.len())?;
    for (peer_id, queue) in &metadata.mailboxes {
        body.extend_from_slice(&peer_id.0);
        push_length(&mut body, queue.len())?;
        for content in queue {
            match content {
                QueueContent::Control(record) => {
                    body.push(0);
                    push_variable(&mut body, &encode_control_record(record)?)?;
                }
                QueueContent::Chunk {
                    file_id,
                    index,
                    chunk_id,
                } => {
                    body.push(1);
                    body.extend_from_slice(&file_id.0);
                    body.extend_from_slice(&index.to_be_bytes());
                    body.extend_from_slice(&chunk_id.0);
                }
            }
        }
    }

    push_length(&mut body, metadata.acked_through.len())?;
    for (peer_id, applied) in &metadata.acked_through {
        body.extend_from_slice(&peer_id.0);
        body.extend_from_slice(&applied.to_be_bytes());
    }

    push_length(&mut body, metadata.chunk_index.len())?;
    for (chunk_id, (file_id, index)) in &metadata.chunk_index {
        body.extend_from_slice(&chunk_id.0);
        body.extend_from_slice(&file_id.0);
        body.extend_from_slice(&index.to_be_bytes());
    }

    if body.len() > MAX_REPLICA_BYTES {
        return Err(Error::InvalidInput("replica metadata is too large"));
    }
    let body_len = u32::try_from(body.len())
        .map_err(|_| Error::InvalidInput("replica metadata is too large"))?;
    let mut out = Vec::with_capacity(REPLICA_MAGIC.len() + 1 + 8 + 4 + body.len() + 32);
    out.extend_from_slice(REPLICA_MAGIC);
    out.push(LOCAL_FORMAT_VERSION);
    out.extend_from_slice(&generation.to_be_bytes());
    out.extend_from_slice(&body_len.to_be_bytes());
    out.extend_from_slice(&body);
    let digest = Sha256::digest(&out);
    out.extend_from_slice(&digest);
    if out.len() > MAX_REPLICA_BYTES {
        return Err(Error::InvalidInput("replica metadata is too large"));
    }
    Ok(out)
}

pub fn decode_replica(bytes: &[u8], expected_root: FileId) -> Result<(u64, ReplicaMetadata)> {
    const HASH_LEN: usize = 32;
    let header_len = REPLICA_MAGIC.len() + 1 + 8 + 4;
    if bytes.len() > MAX_REPLICA_BYTES || bytes.len() < header_len + HASH_LEN {
        return Err(Error::InvalidInput("invalid replica metadata length"));
    }
    if &bytes[..REPLICA_MAGIC.len()] != REPLICA_MAGIC {
        return Err(Error::InvalidInput("invalid replica metadata magic"));
    }
    if bytes[REPLICA_MAGIC.len()] != LOCAL_FORMAT_VERSION {
        return Err(Error::InvalidInput("unsupported replica metadata version"));
    }
    let generation_start = REPLICA_MAGIC.len() + 1;
    let generation = u64::from_be_bytes(
        bytes[generation_start..generation_start + 8]
            .try_into()
            .map_err(|_| Error::InvalidInput("truncated replica metadata"))?,
    );
    let body_len_start = generation_start + 8;
    let body_len = usize::try_from(u32::from_be_bytes(
        bytes[body_len_start..body_len_start + 4]
            .try_into()
            .map_err(|_| Error::InvalidInput("truncated replica metadata"))?,
    ))
    .map_err(|_| Error::InvalidInput("replica metadata body is too large"))?;
    let body_end = header_len
        .checked_add(body_len)
        .ok_or(Error::InvalidInput("replica metadata length overflow"))?;
    let expected_len = body_end
        .checked_add(HASH_LEN)
        .ok_or(Error::InvalidInput("replica metadata length overflow"))?;
    if expected_len != bytes.len() {
        return Err(Error::InvalidInput("replica metadata length mismatch"));
    }
    let expected_digest = Sha256::digest(&bytes[..body_end]);
    if expected_digest[..] != bytes[body_end..] {
        return Err(Error::AuthenticationFailed);
    }

    // No body field is read until the complete outer envelope has passed its
    // length and SHA-256 checks.
    let mut reader = Reader::new(&bytes[header_len..body_end]);
    let stored_root = FileId(reader.array()?);
    if stored_root != expected_root {
        return Err(Error::State("replica belongs to a different vault root"));
    }
    let dirent_count = reader.count(69)?;
    let mut dirents = Vec::with_capacity(dirent_count);
    for _ in 0..dirent_count {
        let parent = FileId(reader.array()?);
        let name = decode_utf8_name(reader.variable()?)?;
        let child = FileId(reader.array()?);
        let is_dir = match reader.byte()? {
            0 => false,
            1 => true,
            _ => return Err(Error::InvalidInput("invalid dirent directory flag")),
        };
        dirents.push(crate::store::tree::Dirent {
            parent,
            name,
            child,
            is_dir,
        });
    }
    let dirents = DirectoryTree::from_dirents(expected_root, dirents)?.dirents();

    let member_count = reader.count(32)?;
    let mut members = std::collections::BTreeSet::new();
    let mut previous_member = None;
    for _ in 0..member_count {
        let member = PeerId(reader.array()?);
        if previous_member.is_some_and(|previous| previous >= member) {
            return Err(Error::InvalidInput("replica members are not canonical"));
        }
        members.insert(member);
        previous_member = Some(member);
    }

    let manifest_count = reader.count(4 + 4)?;
    let mut manifests = std::collections::BTreeMap::new();
    let mut previous_file = None;
    for _ in 0..manifest_count {
        let message = reader.variable()?;
        let signature = reader.variable()?.to_vec();
        let manifest = decode_manifest_message(message, signature)?;
        let file_id = manifest.file_id;
        if previous_file.is_some_and(|previous| previous >= file_id) {
            return Err(Error::InvalidInput("replica manifests are not canonical"));
        }
        manifests.insert(file_id, manifest);
        previous_file = Some(file_id);
    }

    let log_count = reader.count(4)?;
    let mut log = Vec::with_capacity(log_count);
    let mut previous_record = None;
    for _ in 0..log_count {
        let record = decode_control_record(reader.variable()?)?;
        if previous_record.is_some_and(|previous| previous >= record.id) {
            return Err(Error::InvalidInput(
                "instruction log is not strictly ordered",
            ));
        }
        previous_record = Some(record.id);
        log.push(record);
    }
    let next_control = reader.u64()?;

    let mailbox_count = reader.count(32 + 4)?;
    let mut mailboxes = std::collections::BTreeMap::new();
    let mut previous_peer = None;
    for _ in 0..mailbox_count {
        let peer_id = PeerId(reader.array()?);
        if previous_peer.is_some_and(|previous| previous >= peer_id) {
            return Err(Error::InvalidInput("replica mailboxes are not canonical"));
        }
        let queue_count = reader.count(1)?;
        let mut queue = Vec::with_capacity(queue_count);
        for _ in 0..queue_count {
            queue.push(match reader.byte()? {
                0 => QueueContent::Control(decode_control_record(reader.variable()?)?),
                1 => QueueContent::Chunk {
                    file_id: FileId(reader.array()?),
                    index: reader.u64()?,
                    chunk_id: ChunkId(reader.array()?),
                },
                _ => return Err(Error::InvalidInput("unknown durable mailbox content kind")),
            });
        }
        mailboxes.insert(peer_id, queue);
        previous_peer = Some(peer_id);
    }

    let ack_count = reader.count(40)?;
    let mut acked_through = std::collections::BTreeMap::new();
    let mut previous_peer = None;
    for _ in 0..ack_count {
        let peer_id = PeerId(reader.array()?);
        if previous_peer.is_some_and(|previous| previous >= peer_id) {
            return Err(Error::InvalidInput(
                "replica acknowledgements are not canonical",
            ));
        }
        acked_through.insert(peer_id, reader.u64()?);
        previous_peer = Some(peer_id);
    }

    let chunk_count = reader.count(72)?;
    let mut chunk_index = std::collections::BTreeMap::new();
    let mut previous_chunk = None;
    for _ in 0..chunk_count {
        let chunk_id = ChunkId(reader.array()?);
        if previous_chunk.is_some_and(|previous| previous >= chunk_id) {
            return Err(Error::InvalidInput("replica chunk index is not canonical"));
        }
        let file_id = FileId(reader.array()?);
        let index = reader.u64()?;
        chunk_index.insert(chunk_id, (file_id, index));
        previous_chunk = Some(chunk_id);
    }
    reader.finish()?;

    Ok((
        generation,
        ReplicaMetadata {
            expected_root,
            dirents,
            members,
            manifests,
            log,
            next_control,
            mailboxes,
            acked_through,
            chunk_index,
        },
    ))
}

pub fn flush_m(challenge: &FlushChallenge) -> [u8; 32] {
    challenge.0
}

/// Counter comes directly from the header so the nonce and AAD cannot pick
/// different counters. Allocation/exhaustion and replay acceptance are pending.
pub fn nonce(header: &PacketHeader, payload_type: PayloadType) -> [u8; 12] {
    let mut out = [0; 12];
    let type_bit = u8::from(payload_type == PayloadType::ChunkBody);
    let dir_bit = u8::from(header.sender_id > header.receiver_id);
    out[0] = (type_bit << 7) | (dir_bit << 6);
    out[4..].copy_from_slice(&header.seq.0.to_be_bytes());
    out
}

pub(crate) fn encode_local_identity(local: &LocalIdentity) -> Result<Zeroizing<Vec<u8>>> {
    let document_m = identity_m(&local.document)?;
    let mut out = Zeroizing::new(Vec::new());
    out.extend_from_slice(LOCAL_IDENTITY_MAGIC);
    out.push(LOCAL_FORMAT_VERSION);
    out.extend_from_slice(local.signing_key.seed());
    out.extend_from_slice(local.decapsulation_key.seed());
    push_variable(&mut out, &document_m)?;
    push_variable(&mut out, &local.document.signature)?;
    Ok(out)
}

pub(crate) fn decode_local_identity(bytes: &[u8]) -> Result<LocalIdentity> {
    let mut reader = Reader::new(bytes);
    reader.require_prefix(LOCAL_IDENTITY_MAGIC)?;
    reader.require_version()?;
    let signing_seed = Zeroizing::new(reader.array::<32>()?);
    let kem_seed = Zeroizing::new(reader.array::<32>()?);
    let document_m = reader.variable()?;
    let signature = reader.variable()?;
    reader.finish()?;

    let document = decode_identity_document(document_m, signature)?;
    LocalIdentity::from_seeds(*signing_seed, *kem_seed, document)
}

pub(crate) fn encode_store_state(state: &PersistedState) -> Result<Zeroizing<Vec<u8>>> {
    let mut out = Zeroizing::new(Vec::new());
    out.extend_from_slice(STORE_STATE_MAGIC);
    out.push(LOCAL_FORMAT_VERSION);
    out.extend_from_slice(&state.local_id.0);
    out.extend_from_slice(&state.next_slot.to_be_bytes());

    push_length(&mut out, state.peers.len())?;
    for peer in &state.peers {
        push_variable(&mut out, &identity_m(peer)?)?;
        push_variable(&mut out, &peer.signature)?;
    }

    push_length(&mut out, state.pairs.len())?;
    for pair in &state.pairs {
        out.extend_from_slice(&pair.peer_id.0);
        out.extend_from_slice(&pair.epoch.0.to_be_bytes());
        out.extend_from_slice(&pair.initiator.0);
        push_variable(&mut out, &wrap_m(&pair.wrap)?)?;
        push_variable(&mut out, &pair.wrap.signature)?;
    }

    push_length(&mut out, state.keys.len())?;
    for key in &state.keys {
        out.extend_from_slice(&key.slot.to_be_bytes());
        out.extend_from_slice(&key.peer_id.0);
        out.extend_from_slice(&key.epoch.0.to_be_bytes());
        out.extend_from_slice(key.key.as_ref());
    }

    push_length(&mut out, state.epoch_watermarks.len())?;
    for (peer_id, epoch) in &state.epoch_watermarks {
        out.extend_from_slice(&peer_id.0);
        out.extend_from_slice(&epoch.0.to_be_bytes());
    }
    Ok(out)
}

pub(crate) fn decode_store_state(bytes: &[u8]) -> Result<PersistedState> {
    let mut reader = Reader::new(bytes);
    reader.require_prefix(STORE_STATE_MAGIC)?;
    reader.require_version()?;
    let local_id = PeerId(reader.array()?);
    let next_slot = reader.u64()?;

    let peer_count = reader.count(52)?;
    let mut peers = Vec::with_capacity(peer_count);
    for _ in 0..peer_count {
        let document_m = reader.variable()?;
        let signature = reader.variable()?;
        peers.push(decode_identity_document(document_m, signature)?);
    }

    let pair_count = reader.count(160)?;
    let mut pairs = Vec::with_capacity(pair_count);
    for _ in 0..pair_count {
        let peer_id = PeerId(reader.array()?);
        let epoch = Epoch(reader.u64()?);
        let initiator = PeerId(reader.array()?);
        let message_m = reader.variable()?;
        let signature = reader.variable()?;
        let wrap = decode_wrap_message(message_m, signature)?;
        pairs.push(PersistedPair {
            peer_id,
            epoch,
            initiator,
            wrap,
        });
    }

    let key_count = reader.count(80)?;
    let mut keys = Vec::with_capacity(key_count);
    for _ in 0..key_count {
        keys.push(PersistedKey {
            slot: reader.u64()?,
            peer_id: PeerId(reader.array()?),
            epoch: Epoch(reader.u64()?),
            key: Zeroizing::new(reader.array()?),
        });
    }

    // Early v1 stores ended after keys. An appended watermark section keeps
    // those files readable while retaining epochs after provisional discard.
    let mut epoch_watermarks = Vec::new();
    if reader.remaining() != 0 {
        let watermark_count = reader.count(40)?;
        epoch_watermarks.reserve(watermark_count);
        for _ in 0..watermark_count {
            epoch_watermarks.push((PeerId(reader.array()?), Epoch(reader.u64()?)));
        }
    }
    reader.finish()?;
    Ok(PersistedState {
        local_id,
        next_slot,
        peers,
        pairs,
        keys,
        epoch_watermarks,
    })
}

pub(crate) fn encode_directory_state(state: &DirectoryState) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(DIRECTORY_STATE_MAGIC);
    out.push(LOCAL_FORMAT_VERSION);
    push_length(&mut out, state.ads.len())?;
    for (join_code, ad) in &state.ads {
        out.extend_from_slice(&join_code.0);
        push_variable(&mut out, &encode_directory_ad(ad)?)?;
    }
    push_length(&mut out, state.watermarks.len())?;
    for watermark in &state.watermarks {
        out.extend_from_slice(&watermark.peer_id.0);
        out.extend_from_slice(&watermark.vault_id.0);
        out.extend_from_slice(&watermark.issued_at.to_be_bytes());
        push_variable(&mut out, &watermark.vk)?;
    }
    Ok(out)
}

pub(crate) fn decode_directory_state(bytes: &[u8]) -> Result<DirectoryState> {
    let mut reader = Reader::new(bytes);
    reader.require_prefix(DIRECTORY_STATE_MAGIC)?;
    reader.require_version()?;
    let ad_count = reader.count(20)?;
    if ad_count > MAX_DIRECTORY_RECORDS {
        return Err(Error::InvalidInput("directory ad count exceeds cap"));
    }
    let mut ads = Vec::with_capacity(ad_count);
    for _ in 0..ad_count {
        let join_code = JoinCode(reader.array()?);
        let ad = decode_directory_ad(reader.variable()?)?;
        ads.push((join_code, ad));
    }
    let watermark_count = reader.count(76)?;
    if watermark_count > MAX_DIRECTORY_RECORDS {
        return Err(Error::InvalidInput("directory watermark count exceeds cap"));
    }
    let mut watermarks = Vec::with_capacity(watermark_count);
    for _ in 0..watermark_count {
        watermarks.push(DirectoryWatermark {
            peer_id: PeerId(reader.array()?),
            vault_id: VaultId(reader.array()?),
            issued_at: reader.u64()?,
            vk: reader.variable()?.to_vec(),
        });
    }
    reader.finish()?;
    Ok(DirectoryState { ads, watermarks })
}

fn decode_identity_document(message: &[u8], signature: &[u8]) -> Result<IdentityDocument> {
    let mut reader = Reader::new(message);
    let peer_id = PeerId(reader.array()?);
    let ek = reader.variable()?.to_vec();
    let vk = reader.variable()?.to_vec();
    let created_at = reader.u64()?;
    reader.finish()?;
    Ok(IdentityDocument {
        peer_id,
        ek,
        vk,
        created_at,
        signature: signature.to_vec(),
    })
}

fn decode_wrap_message(message: &[u8], signature: &[u8]) -> Result<WrapMessage> {
    let mut reader = Reader::new(message);
    let kem_ct = reader.variable()?.to_vec();
    let wrap_ct = reader.variable()?.to_vec();
    let epoch = Epoch(reader.u64()?);
    let min_id = PeerId(reader.array()?);
    let max_id = PeerId(reader.array()?);
    reader.finish()?;
    require_sorted_pair(&min_id, &max_id)?;
    Ok(WrapMessage {
        kem_ct,
        wrap_ct,
        epoch,
        min_id,
        max_id,
        signature: signature.to_vec(),
    })
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(len)
            .ok_or(Error::InvalidInput("local encoding length overflow"))?;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or(Error::InvalidInput("truncated local encoding"))?;
        self.position = end;
        Ok(value)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut value = [0; N];
        value.copy_from_slice(self.take(N)?);
        Ok(value)
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    fn byte(&mut self) -> Result<u8> {
        Ok(self.array::<1>()?[0])
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn variable(&mut self) -> Result<&'a [u8]> {
        let len = usize::try_from(self.u32()?)
            .map_err(|_| Error::InvalidInput("local encoding field is too large"))?;
        self.take(len)
    }

    fn count(&mut self, minimum_record_size: usize) -> Result<usize> {
        let count = usize::try_from(self.u32()?)
            .map_err(|_| Error::InvalidInput("local encoding count is too large"))?;
        if count > self.remaining() / minimum_record_size {
            return Err(Error::InvalidInput("invalid local encoding record count"));
        }
        Ok(count)
    }

    fn require_prefix(&mut self, prefix: &[u8]) -> Result<()> {
        if self.take(prefix.len())? != prefix {
            return Err(Error::InvalidInput("invalid local encoding magic"));
        }
        Ok(())
    }

    fn require_version(&mut self) -> Result<()> {
        if self.take(1)? != [LOCAL_FORMAT_VERSION] {
            return Err(Error::InvalidInput("unsupported local encoding version"));
        }
        Ok(())
    }

    fn finish(&self) -> Result<()> {
        if self.remaining() != 0 {
            return Err(Error::InvalidInput("trailing local encoding bytes"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    use crate::{crypto::identity::LocalIdentity, ids::Seq, protocol::packet::PROTOCOL_VERSION};

    fn must_ok<T>(result: Result<T>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("unexpected error: {error}"),
        }
    }

    fn empty_state(local_id: PeerId) -> PersistedState {
        PersistedState {
            local_id,
            next_slot: 7,
            peers: Vec::new(),
            pairs: Vec::new(),
            keys: Vec::new(),
            epoch_watermarks: Vec::new(),
        }
    }

    #[test]
    fn local_identity_round_trip_preserves_document_and_independent_seeds() {
        let local = must_ok(LocalIdentity::generate());
        let encoded = must_ok(encode_local_identity(&local));
        let decoded = must_ok(decode_local_identity(&encoded));

        assert!(decoded.document == local.document);
        assert!(decoded.signing_key.seed() == local.signing_key.seed());
        assert!(decoded.decapsulation_key.seed() == local.decapsulation_key.seed());
        assert!(decoded.signing_key.seed() != decoded.decapsulation_key.seed());
    }

    #[test]
    fn local_identity_rejects_envelope_damage_and_seed_mismatch() {
        let local = must_ok(LocalIdentity::generate());
        let encoded = must_ok(encode_local_identity(&local));
        assert!(decode_local_identity(&encoded[..encoded.len() - 1]).is_err());

        let mut bad_magic = Zeroizing::new(encoded.to_vec());
        bad_magic[0] ^= 1;
        assert!(decode_local_identity(&bad_magic).is_err());

        let mut bad_version = Zeroizing::new(encoded.to_vec());
        bad_version[LOCAL_IDENTITY_MAGIC.len()] = LOCAL_FORMAT_VERSION + 1;
        assert!(decode_local_identity(&bad_version).is_err());

        let mut trailing = Zeroizing::new(encoded.to_vec());
        trailing.push(0);
        assert!(decode_local_identity(&trailing).is_err());

        let signing_seed_offset = LOCAL_IDENTITY_MAGIC.len() + 1;
        let mut wrong_signing_seed = Zeroizing::new(encoded.to_vec());
        wrong_signing_seed[signing_seed_offset] ^= 1;
        assert!(matches!(
            decode_local_identity(&wrong_signing_seed),
            Err(Error::AuthenticationFailed)
        ));

        let kem_seed_offset = signing_seed_offset + 32;
        let mut wrong_kem_seed = Zeroizing::new(encoded.to_vec());
        wrong_kem_seed[kem_seed_offset] ^= 1;
        assert!(matches!(
            decode_local_identity(&wrong_kem_seed),
            Err(Error::AuthenticationFailed)
        ));
    }

    #[test]
    fn store_state_round_trip_reuses_signed_identity_and_preserves_key_slot() {
        let local = must_ok(LocalIdentity::generate());
        let peer = must_ok(LocalIdentity::generate());
        let peer_id = peer.document.peer_id;
        let state = PersistedState {
            local_id: local.document.peer_id,
            next_slot: 12,
            peers: vec![peer.document.clone()],
            pairs: Vec::new(),
            keys: vec![PersistedKey {
                slot: 11,
                peer_id,
                epoch: Epoch(4),
                key: Zeroizing::new([0xa5; 32]),
            }],
            epoch_watermarks: vec![(peer_id, Epoch(4))],
        };

        let encoded = must_ok(encode_store_state(&state));
        let decoded = must_ok(decode_store_state(&encoded));
        assert!(decoded.local_id == state.local_id);
        assert!(decoded.next_slot == state.next_slot);
        assert!(decoded.peers == state.peers);
        assert!(decoded.pairs.is_empty());
        assert!(decoded.keys.len() == 1);
        assert!(decoded.keys[0].slot == 11);
        assert!(decoded.keys[0].peer_id == peer_id);
        assert!(decoded.keys[0].epoch == Epoch(4));
        assert!(decoded.keys[0].key.as_ref() == [0xa5; 32]);
        assert!(decoded.epoch_watermarks == vec![(peer_id, Epoch(4))]);
    }

    #[test]
    fn store_state_rejects_oversized_count_and_truncated_key_record() {
        let local_id = PeerId([0x11; 32]);
        let encoded = must_ok(encode_store_state(&empty_state(local_id)));
        let peer_count_offset = STORE_STATE_MAGIC.len() + 1 + 32 + 8;
        let mut oversized_count = Zeroizing::new(encoded.to_vec());
        oversized_count[peer_count_offset..peer_count_offset + 4]
            .copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(decode_store_state(&oversized_count).is_err());

        let mut state = empty_state(local_id);
        state.keys.push(PersistedKey {
            slot: 1,
            peer_id: PeerId([0x22; 32]),
            epoch: Epoch(2),
            key: Zeroizing::new([0x33; 32]),
        });
        let mut truncated_key = must_ok(encode_store_state(&state));
        assert!(truncated_key.pop().is_some());
        assert!(decode_store_state(&truncated_key).is_err());
    }

    #[test]
    fn store_state_reads_legacy_v1_without_epoch_watermarks() {
        let encoded = must_ok(encode_store_state(&empty_state(PeerId([0x44; 32]))));
        let legacy = &encoded[..encoded.len() - 4];
        let decoded = must_ok(decode_store_state(legacy));
        assert!(decoded.epoch_watermarks.is_empty());
    }

    #[test]
    fn decode_aad_rejects_noncanonical_length_and_version() {
        assert!(decode_aad(&[0; 80]).is_err());

        let header = PacketHeader {
            version: PROTOCOL_VERSION,
            sender_id: PeerId([1; 32]),
            receiver_id: PeerId([2; 32]),
            epoch: Epoch(3),
            seq: Seq(4),
        };
        let mut aad = packet_aad(&header);
        aad[0] = PROTOCOL_VERSION + 1;
        assert!(decode_aad(&aad).is_err());
    }

    #[test]
    fn replica_round_trip_preserves_metadata_without_plaintext_or_ciphertext() {
        let root = FileId([0x10; 32]);
        let member = PeerId([0x20; 32]);
        let file_id = FileId([0x30; 32]);
        let chunk_id = ChunkId([0x40; 32]);
        let record = ControlRecord {
            id: 1,
            update: ControlUpdate::Add(file_id),
        };
        let mut metadata = ReplicaMetadata::new(root);
        metadata.members.insert(member);
        metadata.log.push(record.clone());
        metadata.next_control = 2;
        metadata
            .mailboxes
            .insert(member, vec![QueueContent::Control(record)]);
        metadata.acked_through.insert(member, 0);
        metadata.chunk_index.insert(chunk_id, (file_id, 7));

        let encoded = must_ok(encode_replica(&metadata, 9));
        let (generation, decoded) = must_ok(decode_replica(&encoded, root));
        assert_eq!(generation, 9);
        assert!(decoded == metadata);
    }

    #[test]
    fn replica_rejects_damage_truncation_and_wrong_root() {
        let root = FileId([0x51; 32]);
        let encoded = must_ok(encode_replica(&ReplicaMetadata::new(root), 1));

        let mut damaged = encoded.clone();
        let body_offset = REPLICA_MAGIC.len() + 1 + 8 + 4;
        damaged[body_offset] ^= 1;
        assert!(matches!(
            decode_replica(&damaged, root),
            Err(Error::AuthenticationFailed)
        ));
        assert!(decode_replica(&encoded[..encoded.len() - 1], root).is_err());
        assert!(matches!(
            decode_replica(&encoded, FileId([0x52; 32])),
            Err(Error::State(_))
        ));
    }

    #[test]
    fn tree_control_kinds_round_trip_without_changing_legacy_kinds() {
        let parent = FileId([0x71; 32]);
        let child = FileId([0x72; 32]);
        let updates = [
            ControlUpdate::Link {
                parent,
                name: "é".to_owned(),
                child,
                is_dir: true,
            },
            ControlUpdate::Unlink {
                parent,
                name: "old".to_owned(),
            },
            ControlUpdate::Rename {
                src_parent: parent,
                src_name: "from".to_owned(),
                dst_parent: child,
                dst_name: "to".to_owned(),
            },
        ];
        for (offset, update) in updates.into_iter().enumerate() {
            let record = ControlRecord {
                id: offset as u64 + 8,
                update,
            };
            let encoded = must_ok(encode_control_record(&record));
            assert_eq!(encoded[8], offset as u8 + 4);
            assert!(must_ok(decode_control_record(&encoded)) == record);
        }

        let legacy = ControlRecord {
            id: 3,
            update: ControlUpdate::Add(parent),
        };
        let encoded = must_ok(encode_control_record(&legacy));
        assert_eq!(encoded.len(), 41);
        assert_eq!(encoded[8], 1);
    }

    #[test]
    fn replica_round_trip_preserves_canonical_tree() {
        let root = FileId([0x81; 32]);
        let mut tree = DirectoryTree::new(root);
        must_ok(tree.link(root, "a", FileId([0x82; 32]), true));
        must_ok(tree.link(FileId([0x82; 32]), "f", FileId([0x83; 32]), false));
        let mut metadata = ReplicaMetadata::new(root);
        metadata.dirents = tree.dirents();
        let encoded = must_ok(encode_replica(&metadata, 2));
        let (_, decoded) = must_ok(decode_replica(&encoded, root));
        assert!(decoded.dirents == metadata.dirents);
    }

    #[test]
    fn heartbeat_watermark_extends_only_the_existing_heartbeat_kind() {
        assert_eq!(must_ok(encode_net_control(&NetControl::Heartbeat)), [5]);
        let applied = NetControl::HeartbeatApplied { through: 19 };
        let encoded = must_ok(encode_net_control(&applied));
        assert_eq!(encoded.len(), 9);
        assert_eq!(encoded[0], 5);
        assert!(must_ok(decode_net_control(&encoded)) == applied);
        assert!(decode_net_control(&[5, 0]).is_err());
    }

    #[test]
    fn have_query_and_reply_use_separate_strict_magic_framing() {
        let query = must_ok(HaveQuery::new(
            FileId([0x91; 32]),
            vec![ChunkId([0xa1; 32]), ChunkId([0xa2; 32])],
        ));
        let query_bytes = must_ok(encode_have_query(&query));
        assert_eq!(&query_bytes[..HAVE_MAGIC.len()], HAVE_MAGIC);
        assert_eq!(query_bytes[HAVE_MAGIC.len()], 1);
        assert!(must_ok(decode_have_query(&query_bytes)) == query);
        assert!(decode_net_control(&query_bytes).is_err());
        assert!(decode_pull_request(&query_bytes).is_err());
        assert!(decode_control_record(&query_bytes).is_err());

        let reply = must_ok(HaveReply::new(&query, vec![0b01]));
        let reply_bytes = must_ok(encode_have_reply(&reply));
        assert_eq!(reply_bytes[HAVE_MAGIC.len()], 2);
        let decoded = must_ok(decode_have_reply(&reply_bytes));
        assert!(decoded == reply);
        assert!(decoded.validate(&query).is_ok());

        let mut truncated = reply_bytes.clone();
        truncated.pop();
        assert!(decode_have_reply(&truncated).is_err());
        let mut trailing = reply_bytes;
        trailing.push(0);
        assert!(decode_have_reply(&trailing).is_err());
    }

    #[test]
    fn have_decoders_reject_zero_oversized_and_wrong_kinds() {
        let count_offset = HAVE_MAGIC.len() + 1 + 32;
        let mut zero = Vec::from(HAVE_MAGIC.as_slice());
        zero.push(1);
        zero.extend_from_slice(&[0; 32]);
        zero.extend_from_slice(&0u32.to_be_bytes());
        assert!(decode_have_query(&zero).is_err());

        let mut oversized = zero;
        oversized[count_offset..count_offset + 4].copy_from_slice(&33u32.to_be_bytes());
        assert!(decode_have_query(&oversized).is_err());

        let mut wrong_kind = must_ok(encode_have_query(&must_ok(HaveQuery::new(
            FileId([1; 32]),
            vec![ChunkId([2; 32])],
        ))));
        wrong_kind[HAVE_MAGIC.len()] = 2;
        assert!(decode_have_query(&wrong_kind).is_err());
    }
}
