use crate::{
    error::{Error, Result},
    ids::{Epoch, PeerId, Seq},
};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlPacket {
    pub header: PacketHeader,
    pub ciphertext: Vec<u8>,
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

impl ReplayWindowState {
    /// Checks whether `seq` is fresh without changing the window.
    pub fn check(&self, seq: Seq) -> Result<()> {
        let Some(highest) = self.highest_seen else {
            return Ok(());
        };
        if seq.0 > highest {
            return Ok(());
        }

        let distance = highest - seq.0;
        if distance >= REPLAY_WINDOW_SIZE as u64 || self.bit_is_set(distance as usize) {
            return Err(Error::ReplayRejected);
        }
        Ok(())
    }

    /// Records `seq` after authentication has succeeded.
    pub fn accept(&mut self, seq: Seq) -> Result<()> {
        self.check(seq)?;
        match self.highest_seen {
            None => {
                self.highest_seen = Some(seq.0);
                self.set_bit(0);
            }
            Some(highest) if seq.0 > highest => {
                let advance = seq.0 - highest;
                self.shift(advance);
                self.highest_seen = Some(seq.0);
                self.set_bit(0);
            }
            Some(highest) => self.set_bit((highest - seq.0) as usize),
        }
        Ok(())
    }

    fn bit_is_set(&self, distance: usize) -> bool {
        self.seen[distance / 64] & (1u64 << (distance % 64)) != 0
    }

    fn set_bit(&mut self, distance: usize) {
        self.seen[distance / 64] |= 1u64 << (distance % 64);
    }

    fn shift(&mut self, advance: u64) {
        if advance >= REPLAY_WINDOW_SIZE as u64 {
            self.seen.fill(0);
            return;
        }
        let advance = advance as usize;
        let mut shifted = [0u64; REPLAY_WINDOW_SIZE / 64];
        for old_distance in 0..(REPLAY_WINDOW_SIZE - advance) {
            if self.bit_is_set(old_distance) {
                let new_distance = old_distance + advance;
                shifted[new_distance / 64] |= 1u64 << (new_distance % 64);
            }
        }
        self.seen = shifted;
    }
}

#[cfg(test)]
mod tests {
    use super::ReplayWindowState;
    use crate::{ids::Seq, Error};

    #[test]
    fn window_accepts_holes_and_rejects_replays_and_old_boundary() {
        let mut window = ReplayWindowState::default();
        assert!(window.accept(Seq(2_000)).is_ok());
        assert!(window.accept(Seq(1_998)).is_ok());
        assert!(matches!(
            window.check(Seq(1_998)),
            Err(Error::ReplayRejected)
        ));
        assert!(window.accept(Seq(977)).is_ok());
        assert!(matches!(window.check(Seq(976)), Err(Error::ReplayRejected)));
    }

    #[test]
    fn large_jump_discards_old_window_without_overflow() {
        let mut window = ReplayWindowState::default();
        assert!(window.accept(Seq(0)).is_ok());
        assert!(window.accept(Seq(u64::MAX)).is_ok());
        assert!(matches!(window.check(Seq(0)), Err(Error::ReplayRejected)));
    }
}
