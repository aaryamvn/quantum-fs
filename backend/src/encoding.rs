//! Canonical bytes from crypto-encoding.md. These object encoders supply both AAD
//! and Pure ML-DSA M where applicable; signatures themselves are not part of M.
//! IDs are raw bytes, integers big-endian, variable fields u32-length-prefixed.

use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::{
    crypto::{
        identity::{IdentityDocument, LocalIdentity},
        wrap::WrapMessage,
    },
    ids::{ChunkId, Epoch, FileId, PeerId},
    keystore::{PersistedKey, PersistedPair, PersistedState},
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
const LOCAL_IDENTITY_MAGIC: &[u8] = b"qfs/local/identity/";
const STORE_STATE_MAGIC: &[u8] = b"qfs/local/keys/";
const LOCAL_FORMAT_VERSION: u8 = 1;

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
    reader.finish()?;
    Ok(PersistedState {
        local_id,
        next_slot,
        peers,
        pairs,
        keys,
    })
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
}
