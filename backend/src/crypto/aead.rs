use std::collections::BTreeMap;

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm as Aes256GcmCipher, Nonce,
};
use zeroize::Zeroizing;

use crate::{
    encoding,
    error::{Error, Result},
    ids::{Epoch, PeerId, Seq},
    keystore::KeyStore,
    protocol::packet::{
        Direction, PacketHeader, PayloadType, ReplayWindowKey, ReplayWindowState, PROTOCOL_VERSION,
    },
};

#[derive(Clone)]
pub struct PairKeyHandle {
    pub(crate) slot: u64,
    pub(crate) store_id: [u8; 32],
}

pub(crate) struct PairKeyState {
    pub(crate) key: Zeroizing<[u8; 32]>,
    pub(crate) peer_id: PeerId,
    pub(crate) epoch: Epoch,
    send_packet: Option<u64>,
    send_chunk: Option<u64>,
    replay: BTreeMap<ReplayWindowKey, ReplayWindowState>,
}

impl PairKeyState {
    pub(crate) fn new(key: Zeroizing<[u8; 32]>, peer_id: PeerId, epoch: Epoch) -> Self {
        Self {
            key,
            peer_id,
            epoch,
            send_packet: None,
            send_chunk: None,
            replay: BTreeMap::new(),
        }
    }

    fn reserve_send(&mut self, payload_type: PayloadType, seq: Seq) -> Result<()> {
        let last = match payload_type {
            PayloadType::Packet => &mut self.send_packet,
            PayloadType::ChunkBody => &mut self.send_chunk,
        };
        if last.is_some_and(|last| seq.0 <= last) {
            return Err(Error::State("AEAD send counter must strictly increase"));
        }
        *last = Some(seq.0);
        Ok(())
    }
}

pub trait Aes256Gcm {
    fn seal(
        &self,
        key: &PairKeyHandle,
        nonce: &[u8; 12],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>>;
    fn open(
        &self,
        key: &PairKeyHandle,
        nonce: &[u8; 12],
        aad: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>>;
}

#[derive(Clone)]
pub struct RustCryptoAes256Gcm {
    store: KeyStore,
}

impl RustCryptoAes256Gcm {
    pub fn new(store: KeyStore) -> Self {
        Self { store }
    }

    fn validate_header(
        local_id: PeerId,
        state: &PairKeyState,
        header: &PacketHeader,
        payload_type: PayloadType,
        nonce: &[u8; 12],
        direction: Direction,
    ) -> Result<ReplayWindowKey> {
        if header.version != PROTOCOL_VERSION {
            return Err(Error::InvalidInput("unsupported packet version"));
        }
        let endpoints_match = match direction {
            Direction::Outbound => {
                header.sender_id == local_id && header.receiver_id == state.peer_id
            }
            Direction::Inbound => {
                header.sender_id == state.peer_id && header.receiver_id == local_id
            }
        };
        if !endpoints_match || header.epoch != state.epoch {
            return Err(Error::InvalidInput("AAD does not match pair key"));
        }
        if nonce != &encoding::nonce(header, payload_type) {
            return Err(Error::InvalidInput("nonce does not match AAD"));
        }
        Ok(ReplayWindowKey {
            peer_id: state.peer_id,
            epoch: state.epoch,
            direction,
            payload_type,
        })
    }
}

impl Aes256Gcm for RustCryptoAes256Gcm {
    fn seal(
        &self,
        handle: &PairKeyHandle,
        nonce: &[u8; 12],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>> {
        let local_id = self.store.peer_id()?;
        let (header, payload_type) = encoding::decode_aad(aad)?;
        self.store.with_pair_state(handle, |state| {
            Self::validate_header(
                local_id,
                state,
                &header,
                payload_type,
                nonce,
                Direction::Outbound,
            )?;
            state.reserve_send(payload_type, header.seq)?;
            let cipher = Aes256GcmCipher::new_from_slice(state.key.as_ref())
                .map_err(|_| Error::State("invalid pair key length"))?;
            let nonce = Nonce::from(*nonce);
            cipher
                .encrypt(
                    &nonce,
                    Payload {
                        msg: plaintext,
                        aad,
                    },
                )
                .map_err(|_| Error::AuthenticationFailed)
        })
    }

    fn open(
        &self,
        handle: &PairKeyHandle,
        nonce: &[u8; 12],
        aad: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>> {
        let local_id = self.store.peer_id()?;
        let (header, payload_type) = encoding::decode_aad(aad)?;
        self.store.with_pair_state(handle, |state| {
            let replay_key = Self::validate_header(
                local_id,
                state,
                &header,
                payload_type,
                nonce,
                Direction::Inbound,
            )?;
            state
                .replay
                .entry(replay_key)
                .or_default()
                .check(header.seq)?;
            let cipher = Aes256GcmCipher::new_from_slice(state.key.as_ref())
                .map_err(|_| Error::State("invalid pair key length"))?;
            let nonce = Nonce::from(*nonce);
            let plaintext = cipher
                .decrypt(
                    &nonce,
                    Payload {
                        msg: ciphertext,
                        aad,
                    },
                )
                .map_err(|_| Error::AuthenticationFailed)?;
            state
                .replay
                .entry(replay_key)
                .or_default()
                .accept(header.seq)?;
            Ok(plaintext)
        })
    }
}
