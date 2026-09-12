use crate::ids::{Epoch, PeerId, Seq};

pub const PROTOCOL_VERSION: u8 = 1;
pub const REPLAY_WINDOW_SIZE: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PacketHeader {
    pub version: u8,
    pub sender_id: PeerId,
    pub receiver_id: PeerId,
    pub epoch: Epoch,
    pub seq: Seq,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Direction {
    Inbound,
    Outbound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PayloadType {
    Packet,
    ChunkBody,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReplayWindowKey {
    pub peer_id: PeerId,
    pub epoch: Epoch,
    pub direction: Direction,
    pub payload_type: PayloadType,
}

/// State shape for a 1024-counter sliding window. Acceptance logic is pending.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayWindowState {
    pub highest_seen: Option<u64>,
    pub seen: [u64; REPLAY_WINDOW_SIZE / 64],
}

impl Default for ReplayWindowState {
    fn default() -> Self {
        Self {
            highest_seen: None,
            seen: [0; REPLAY_WINDOW_SIZE / 64],
        }
    }
}
