use std::{collections::BTreeSet, future::Future, time::Duration};

use crate::{
    crypto::{
        aead::{Aes256Gcm, RustCryptoAes256Gcm},
        wrap::PairSession,
    },
    encoding,
    error::{Error, Result},
    ids::{ChunkId, FileId, PeerId},
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

/// A bounded pull attempt can be incomplete when all known holders are stale
/// or unreachable. Only an empty `missing` list means delivery is complete.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PullReport {
    pub chunks_written: usize,
    pub holders_tried: usize,
    pub missing: Vec<ChunkId>,
}

impl PullReport {
    pub fn is_complete(&self) -> bool {
        self.missing.is_empty()
    }
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

    /// Consume locator candidates in order. A nomination is only a hint: a
    /// holder may have evicted since its HaveReply. Fetches use the existing
    /// packet-GCM pull request and chunk-GCM responses, never holder manifests.
    pub fn pull_from_holders<F>(
        &self,
        request: &PullRequest,
        trusted: &TrustedManifest,
        holders: &[PeerId],
        mut fetch: F,
    ) -> Result<PullReport>
    where
        F: FnMut(PeerId, &ControlPacket) -> Result<Vec<PullResponse>>,
    {
        let mut report = self.begin_pull(request, trusted)?;
        let mut tried = BTreeSet::new();
        for &holder in holders {
            if report.is_complete() {
                break;
            }
            if !tried.insert(holder) {
                continue;
            }
            let remaining = PullRequest::new(report.missing.clone())?;
            let Ok(packet) = self.encrypt_request(holder, &remaining) else {
                continue;
            };
            report.holders_tried += 1;
            if let Ok(responses) = fetch(holder, &packet) {
                self.accept_holder(holder, &responses, trusted, &remaining, &mut report)?;
            }
            report.missing = self.missing_chunks(request)?;
        }
        Ok(report)
    }

    /// Network callers supply an existing live stream operation. Bound each
    /// holder attempt so a stale nomination cannot prevent fallback forever.
    /// Timeout cancels `fetch`: it must own its stream or otherwise close that
    /// stream on cancellation. Never reuse a partially consumed TCP frame.
    pub async fn pull_from_holders_async<F, Fut>(
        &self,
        request: &PullRequest,
        trusted: &TrustedManifest,
        holders: &[PeerId],
        mut fetch: F,
    ) -> Result<PullReport>
    where
        F: FnMut(PeerId, ControlPacket) -> Fut,
        Fut: Future<Output = Result<Vec<PullResponse>>>,
    {
        let mut report = self.begin_pull(request, trusted)?;
        let mut tried = BTreeSet::new();
        for &holder in holders {
            if report.is_complete() {
                break;
            }
            if !tried.insert(holder) {
                continue;
            }
            let remaining = PullRequest::new(report.missing.clone())?;
            let Ok(packet) = self.encrypt_request(holder, &remaining) else {
                continue;
            };
            report.holders_tried += 1;
            if let Ok(Ok(responses)) =
                tokio::time::timeout(Duration::from_secs(5), fetch(holder, packet)).await
            {
                self.accept_holder(holder, &responses, trusted, &remaining, &mut report)?;
            }
            report.missing = self.missing_chunks(request)?;
        }
        Ok(report)
    }

    fn begin_pull(&self, request: &PullRequest, trusted: &TrustedManifest) -> Result<PullReport> {
        if request
            .chunk_ids()
            .iter()
            .any(|id| !trusted.manifest().chunk_ids.contains(id))
        {
            return Err(Error::AuthenticationFailed);
        }
        Ok(PullReport {
            missing: self.missing_chunks(request)?,
            ..PullReport::default()
        })
    }

    fn missing_chunks(&self, request: &PullRequest) -> Result<Vec<ChunkId>> {
        let chunks = self
            .chunks
            .lock()
            .map_err(|_| Error::State("chunk store lock poisoned"))?;
        Ok(request
            .chunk_ids()
            .iter()
            .filter(|id| !chunks.has(id))
            .copied()
            .collect())
    }

    fn accept_holder(
        &self,
        holder: PeerId,
        responses: &[PullResponse],
        trusted: &TrustedManifest,
        remaining: &PullRequest,
        report: &mut PullReport,
    ) -> Result<()> {
        // Reject unsolicited frames before handing them to the existing receive
        // helper. Authenticated plaintext and final live-dirent checks stay there.
        if responses.len() > remaining.chunk_ids().len()
            || responses.iter().any(|response| {
                let body = &response.body;
                body.header.sender_id != holder
                    || body.file_id != trusted.manifest().file_id
                    || usize::try_from(body.index)
                        .ok()
                        .and_then(|index| trusted.manifest().chunk_ids.get(index))
                        .is_none_or(|id| !remaining.chunk_ids().contains(id))
            })
        {
            return Err(Error::AuthenticationFailed);
        }
        // Empty responses are deliberately not completion: the next candidate
        // still gets every missing id. An idempotent receive may write zero.
        report.chunks_written += self.accept(responses, trusted)?;
        Ok(())
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
        let mut next = chunks.clone();
        let mut count = 0;
        for (file_id, index, plaintext) in staged {
            let chunk_id = encoding::chunk_id(&file_id, index, &plaintext);
            // Control apply and pull acceptance synchronize on this store lock.
            // Re-check the current manifest at the final mutation boundary so an
            // in-flight pull cannot recreate bytes after Clear/Remove/Unlink.
            if !chunks.accepts_chunk(&file_id, index, &chunk_id) {
                continue;
            }
            if !next.has(&chunk_id) {
                next.put(&file_id, index, plaintext)?;
                count += 1;
            }
        }
        if count != 0 {
            next.persist_current()?;
            *chunks = next;
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
    if !store.accepts_chunk(&frame.file_id, frame.index, &expected) {
        return Ok(false);
    }
    if store.has(&expected) {
        return Ok(false);
    }
    let mut next = store.clone();
    next.put(&frame.file_id, frame.index, plaintext)?;
    next.persist_current()?;
    *store = next;
    Ok(true)
}
