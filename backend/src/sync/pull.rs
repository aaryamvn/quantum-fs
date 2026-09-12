use std::collections::BTreeSet;

use crate::{
    crypto::{
        aead::{Aes256Gcm, RustCryptoAes256Gcm},
        wrap::PairSession,
    },
    encoding,
    error::{Error, Result},
    ids::{FileId, PeerId},
    keystore::KeyStore,
    protocol::{
        manifest::TrustedManifest,
        packet::{ControlPacket, PacketHeader, PayloadType, PROTOCOL_VERSION},
        pull::{ChunkBodyFrame, PullRequest, PullResponse},
    },
    store::chunks::{ChunkStore, MemoryChunkStore, SharedChunkStore},
};

/// Common validation hook for pull coordinators.
pub trait PullCoordinator {
    fn request(&self, request: &PullRequest) -> Result<()>;
}

#[derive(Clone)]
pub struct InProcessPullCoordinator {
    keys: KeyStore,
    chunks: SharedChunkStore,
}

impl InProcessPullCoordinator {
    pub fn new(keys: KeyStore, chunks: SharedChunkStore) -> Self {
        Self { keys, chunks }
    }

    pub fn serve(&self, request: &PullRequest, requester_id: PeerId) -> Result<Vec<PullResponse>> {
        self.keys.require_live_traffic(requester_id)?;
        let session = self.keys.current_session(requester_id)?;
        let records = {
            let chunks = self
                .chunks
                .lock()
                .map_err(|_| Error::State("chunk store lock poisoned"))?;
            request
                .chunk_ids()
                .iter()
                .filter_map(|chunk_id| chunks.get_record(chunk_id).cloned())
                .collect::<Vec<_>>()
        };
        records
            .into_iter()
            .map(|record| {
                Ok(PullResponse {
                    body: encrypt_at_send(
                        &self.keys,
                        &session,
                        record.file_id,
                        record.index,
                        &record.plaintext,
                    )?,
                })
            })
            .collect()
    }

    pub fn accept(
        &self,
        responses: &[PullResponse],
        trusted_manifest: &TrustedManifest,
    ) -> Result<usize> {
        let mut staged = Vec::new();
        let mut staged_ids = BTreeSet::new();
        {
            let chunks = self
                .chunks
                .lock()
                .map_err(|_| Error::State("chunk store lock poisoned"))?;
            for response in responses {
                let body = &response.body;
                self.keys.require_live_traffic(body.header.sender_id)?;
                let session = self.keys.current_session(body.header.sender_id)?;
                let plaintext = open_chunk(&self.keys, &session, body, trusted_manifest)?;
                let expected = encoding::chunk_id(&body.file_id, body.index, &plaintext);
                if chunks.has(&expected) || staged_ids.contains(&expected) {
                    continue;
                }
                staged_ids.insert(expected);
                staged.push((body.file_id, body.index, plaintext));
            }
        }

        let mut chunks = self
            .chunks
            .lock()
            .map_err(|_| Error::State("chunk store lock poisoned"))?;
        let mut count = 0;
        for (file_id, index, plaintext) in staged {
            let chunk_id = encoding::chunk_id(&file_id, index, &plaintext);
            if !chunks.has(&chunk_id) {
                chunks.put(&file_id, index, plaintext);
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn encrypt_request(
        &self,
        holder_id: PeerId,
        request: &PullRequest,
    ) -> Result<ControlPacket> {
        self.keys.require_live_traffic(holder_id)?;
        let session = self.keys.current_session(holder_id)?;
        let header = PacketHeader {
            version: PROTOCOL_VERSION,
            sender_id: self.keys.peer_id()?,
            receiver_id: holder_id,
            epoch: session.epoch,
            seq: self
                .keys
                .next_outbound_seq(session.key_handle(), PayloadType::Packet)?,
        };
        let aad = encoding::packet_aad(&header);
        let nonce = encoding::nonce(&header, PayloadType::Packet);
        let plaintext = encoding::encode_pull_request(request)?;
        let ciphertext = RustCryptoAes256Gcm::new(self.keys.clone()).seal(
            session.key_handle(),
            &nonce,
            &aad,
            &plaintext,
        )?;
        Ok(ControlPacket { header, ciphertext })
    }

    pub fn serve_packet(&self, packet: &ControlPacket) -> Result<Vec<PullResponse>> {
        self.keys.require_live_traffic(packet.header.sender_id)?;
        let session = self.keys.current_session(packet.header.sender_id)?;
        let aad = encoding::packet_aad(&packet.header);
        let nonce = encoding::nonce(&packet.header, PayloadType::Packet);
        let plaintext = RustCryptoAes256Gcm::new(self.keys.clone()).open(
            session.key_handle(),
            &nonce,
            &aad,
            &packet.ciphertext,
        )?;
        let request = encoding::decode_pull_request(&plaintext)?;
        self.serve(&request, packet.header.sender_id)
    }
}

impl PullCoordinator for InProcessPullCoordinator {
    fn request(&self, request: &PullRequest) -> Result<()> {
        PullRequest::new(request.chunk_ids().to_vec()).map(|_| ())
    }
}

pub fn encrypt_at_send(
    keys: &KeyStore,
    session: &PairSession,
    file_id: FileId,
    index: u64,
    plaintext: &[u8],
) -> Result<ChunkBodyFrame> {
    let header = PacketHeader {
        version: PROTOCOL_VERSION,
        sender_id: keys.peer_id()?,
        receiver_id: session.peer_id,
        epoch: session.epoch,
        seq: keys.next_outbound_seq(session.key_handle(), PayloadType::ChunkBody)?,
    };
    let aad = encoding::chunk_aad(&header, &file_id, index);
    let nonce = encoding::nonce(&header, PayloadType::ChunkBody);
    let ciphertext = RustCryptoAes256Gcm::new(keys.clone()).seal(
        session.key_handle(),
        &nonce,
        &aad,
        plaintext,
    )?;
    Ok(ChunkBodyFrame {
        header,
        file_id,
        index,
        ciphertext,
    })
}

pub fn open_chunk(
    keys: &KeyStore,
    session: &PairSession,
    frame: &ChunkBodyFrame,
    trusted: &TrustedManifest,
) -> Result<Vec<u8>> {
    let manifest = trusted.manifest();
    let aad = encoding::chunk_aad(&frame.header, &frame.file_id, frame.index);
    let nonce = encoding::nonce(&frame.header, PayloadType::ChunkBody);
    let plaintext = RustCryptoAes256Gcm::new(keys.clone()).open(
        session.key_handle(),
        &nonce,
        &aad,
        &frame.ciphertext,
    )?;
    if frame.file_id != manifest.file_id {
        return Err(Error::AuthenticationFailed);
    }
    let index = usize::try_from(frame.index)
        .map_err(|_| Error::InvalidInput("chunk index exceeds platform limit"))?;
    let expected = manifest
        .chunk_ids
        .get(index)
        .ok_or(Error::AuthenticationFailed)?;
    if encoding::chunk_id(&frame.file_id, frame.index, &plaintext) != *expected {
        return Err(Error::AuthenticationFailed);
    }
    Ok(plaintext)
}

pub fn receive_chunk(
    keys: &KeyStore,
    session: &PairSession,
    frame: &ChunkBodyFrame,
    trusted: &TrustedManifest,
    store: &mut MemoryChunkStore,
) -> Result<bool> {
    let plaintext = open_chunk(keys, session, frame, trusted)?;
    let expected = encoding::chunk_id(&frame.file_id, frame.index, &plaintext);
    if store.has(&expected) {
        return Ok(false);
    }
    store.put(&frame.file_id, frame.index, plaintext);
    Ok(true)
}
