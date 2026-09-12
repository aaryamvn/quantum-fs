//! Canonical bytes from crypto-encoding.md. These object encoders supply both AAD
//! and Pure ML-DSA M where applicable; signatures themselves are not part of M.
//! IDs are raw bytes, integers big-endian, variable fields u32-length-prefixed.

use sha2::{Digest, Sha256};

use crate::{
    crypto::{identity::IdentityDocument, wrap::WrapMessage},
    ids::{ChunkId, Epoch, FileId, PeerId},
    protocol::{
        manifest::Manifest,
        packet::{PacketHeader, PayloadType},
    },
    sync::host::FlushChallenge,
    Error, Result,
};

pub const WRAP_GCM_NONCE: [u8; 12] = [0; 12];
const PEER_ID_DOMAIN: &[u8] = b"qfs/v1/peer";
const WRAP_PAIR_DOMAIN: &[u8] = b"qfs/v1/wrap/pair/";

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
