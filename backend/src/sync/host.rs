use std::time::Duration;

use crate::error::Result;
use crate::ids::{Epoch, PeerId, Seq};

/// Compromising H compromises all shared plaintext; H is TCB for all shared files.
/// H is a configured role on an ordinary member process, using that member's identity.
pub struct HostService;

#[derive(Clone, PartialEq, Eq)]
pub struct Presence {
    pub peer_id: PeerId,
    pub ttl: Duration,
}

#[derive(Clone, PartialEq, Eq)]
pub struct MailboxEnvelope {
    pub recipient_id: PeerId,
    pub sender_id: PeerId,
    pub epoch: Epoch,
    pub seq: Seq,
    pub queued_at: u64,
    pub ciphertext: Vec<u8>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct FlushChallenge(pub [u8; 32]);

/// Canonically encoded committed control data. The concrete save, mkdir,
/// membership, and new-manifest schemas belong to the protocol layer.
pub struct ControlUpdate {
    encoded: Vec<u8>,
}

impl ControlUpdate {
    pub fn from_encoded(encoded: Vec<u8>) -> Self {
        Self { encoded }
    }

    pub fn encoded(&self) -> &[u8] {
        &self.encoded
    }
}

impl HostService {
    pub fn heartbeat(&mut self, _peer_id: PeerId, _ttl: Duration) -> Result<()> {
        Err(crate::error::Error::NotImplemented(
            "host presence heartbeat",
        ))
    }

    pub fn presence(&self, _peer_id: PeerId) -> Result<Option<Presence>> {
        Err(crate::error::Error::NotImplemented("host presence lookup"))
    }

    /// Appends an encrypted pairwise packet in recipient order. Offline mailboxes
    /// may include encrypt-at-send chunk bodies as well as control.
    pub fn append_mailbox(&mut self, _envelope: MailboxEnvelope) -> Result<()> {
        Err(crate::error::Error::NotImplemented(
            "ordered mailbox append",
        ))
    }

    pub fn issue_flush_challenge(&mut self, _recipient_id: PeerId) -> Result<FlushChallenge> {
        Err(crate::error::Error::NotImplemented(
            "flush challenge generation",
        ))
    }

    /// `signature` authenticates the exact 32 challenge bytes with Pure ML-DSA-65
    /// under qfs/v1/flush. Only the recipient may flush its ordered mailbox.
    pub fn flush_mailbox(
        &mut self,
        _recipient_id: PeerId,
        _challenge: &FlushChallenge,
        _signature: &[u8],
    ) -> Result<Vec<MailboxEnvelope>> {
        Err(crate::error::Error::NotImplemented("ordered mailbox flush"))
    }

    /// Fans out committed control updates to online members. Chunk bodies for
    /// online members are pulled; offline members get encrypt-at-send bodies
    /// via append_mailbox as the change happens.
    pub fn fan_out_control(&mut self, _sender_id: PeerId, _control: &ControlUpdate) -> Result<()> {
        Err(crate::error::Error::NotImplemented("host control fan-out"))
    }
}

// Live cursor messages use direct pairwise GCM, carry no DSA signature, and never pass through H.
// If H is unavailable, new commits and offline mailboxes wait; v1 has no automatic election.
