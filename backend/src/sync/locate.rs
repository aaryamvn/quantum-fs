use std::{collections::BTreeSet, future::Future, pin::Pin, time::Duration};

use crate::{
    ids::PeerId,
    keystore::KeyStore,
    protocol::locate::{HaveQuery, HaveReply},
    store::chunks::ChunkStore,
    Result,
};

/// Ephemeral holder discovery from this member's live pair sessions. The
/// directory has no membership or location role.
pub struct Locator {
    keys: KeyStore,
    members: BTreeSet<PeerId>,
    host_id: PeerId,
}

pub const LOCATE_DEADLINE: Duration = Duration::from_secs(5);

impl Locator {
    pub fn new(keys: KeyStore, members: BTreeSet<PeerId>, host_id: PeerId) -> Result<Self> {
        let local = keys.peer_id()?;
        if !members.contains(&local) || !members.contains(&host_id) {
            return Err(crate::Error::InvalidInput(
                "locator identities must be vault members",
            ));
        }
        Ok(Self {
            keys,
            members,
            host_id,
        })
    }

    /// A candidate must have both an installed current session and an open
    /// flush gate. Host presence TTL is intentionally irrelevant here.
    pub fn session_is_live(&self, peer_id: PeerId) -> bool {
        self.members.contains(&peer_id)
            && self.keys.peer_id().is_ok_and(|local| local != peer_id)
            && self.keys.current_session(peer_id).is_ok()
            && self.keys.require_live_traffic(peer_id).is_ok()
    }

    pub fn live_member_candidates(&self) -> Vec<PeerId> {
        self.members
            .iter()
            .copied()
            .filter(|peer| *peer != self.host_id && self.session_is_live(*peer))
            .collect()
    }

    /// Prefer H without a have round-trip because H owns the full replica. If
    /// H is unavailable, query live members immediately and return responders
    /// that hold at least one requested identifier. Invalid or unavailable
    /// responders are skipped rather than contributing untrusted locations.
    pub fn holders<F>(&self, query: &HaveQuery, mut ask: F) -> Result<Vec<PeerId>>
    where
        F: FnMut(PeerId, &HaveQuery) -> Result<HaveReply>,
    {
        if self.session_is_live(self.host_id) {
            return Ok(vec![self.host_id]);
        }
        let mut holders = Vec::new();
        for peer_id in self.live_member_candidates() {
            let Ok(reply) = ask(peer_id, query) else {
                continue;
            };
            if self.session_is_live(peer_id)
                && reply.validate(query).is_ok()
                && (0..query.chunk_ids().len()).any(|index| reply.has(index))
            {
                holders.push(peer_id);
            }
        }
        Ok(holders)
    }

    pub async fn holders_async<F, Fut>(&self, query: &HaveQuery, ask: F) -> Result<Vec<PeerId>>
    where
        F: FnMut(PeerId, HaveQuery) -> Fut,
        Fut: Future<Output = Result<HaveReply>>,
    {
        self.holders_async_with_timeout(query, LOCATE_DEADLINE, ask)
            .await
    }

    pub async fn holders_async_with_timeout<F, Fut>(
        &self,
        query: &HaveQuery,
        deadline: Duration,
        mut ask: F,
    ) -> Result<Vec<PeerId>>
    where
        F: FnMut(PeerId, HaveQuery) -> Fut,
        Fut: Future<Output = Result<HaveReply>>,
    {
        if self.session_is_live(self.host_id) {
            return Ok(vec![self.host_id]);
        }
        let mut pending: Vec<(PeerId, Option<Pin<Box<Fut>>>)> = self
            .live_member_candidates()
            .into_iter()
            .map(|peer_id| (peer_id, Some(Box::pin(ask(peer_id, query.clone())))))
            .collect();
        let mut holders = Vec::new();
        let queries = std::future::poll_fn(|context| {
            let mut remaining = false;
            for (peer_id, future) in &mut pending {
                let Some(candidate) = future.as_mut() else {
                    continue;
                };
                match candidate.as_mut().poll(context) {
                    std::task::Poll::Ready(result) => {
                        *future = None;
                        if let Ok(reply) = result {
                            if self.session_is_live(*peer_id)
                                && reply.validate(query).is_ok()
                                && (0..query.chunk_ids().len()).any(|index| reply.has(index))
                            {
                                holders.push(*peer_id);
                            }
                        }
                    }
                    std::task::Poll::Pending => remaining = true,
                }
            }
            if remaining {
                std::task::Poll::Pending
            } else {
                std::task::Poll::Ready(())
            }
        });
        let _ = tokio::time::timeout(deadline, queries).await;
        holders.sort_unstable();
        holders.dedup();
        Ok(holders)
    }
}

pub fn answer_have(query: &HaveQuery, chunks: &impl ChunkStore) -> Result<HaveReply> {
    HaveReply::new(query, chunks.have_bitset(query.chunk_ids()))
}
