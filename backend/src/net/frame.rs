use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{Error, Result};

pub const FRAME_VERSION: u8 = 1;
pub const MAX_FRAME_BODY_LEN: u32 = 1024 * 1024;
pub const MIN_FRAME_KIND: u8 = 1;
pub const MAX_FRAME_KIND: u8 = 14;
pub const IDENTITY_KIND: u8 = 1;
pub const EPOCH_HINT_KIND: u8 = 2;
pub const WRAP_KIND: u8 = 3;
pub const WRAP_ACK_KIND: u8 = 4;
pub const DIR_LOOKUP_KIND: u8 = 5;
pub const DIR_AD_KIND: u8 = 6;
pub const DIR_PUT_KIND: u8 = 7;
pub const DIR_FORGET_KIND: u8 = 8;
pub const JOIN_KIND: u8 = 9;
pub const GCM_PACKET_KIND: u8 = 10;
pub const GCM_CHUNK_KIND: u8 = 11;
pub const FLUSH_CHALLENGE_KIND: u8 = 12;
pub const FLUSH_SIGNATURE_KIND: u8 = 13;
pub const HEARTBEAT_KIND: u8 = 14;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub kind: u8,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(kind: u8, payload: Vec<u8>) -> Result<Self> {
        validate_kind(kind)?;
        let body_len = payload
            .len()
            .checked_add(1)
            .ok_or(Error::InvalidInput("TCP frame body length overflow"))?;
        if body_len > MAX_FRAME_BODY_LEN as usize {
            return Err(Error::InvalidInput("TCP frame body exceeds 1 MiB"));
        }
        Ok(Self { kind, payload })
    }
}

pub async fn read_frame(reader: &mut (impl AsyncRead + Unpin)) -> Result<Frame> {
    let version = reader.read_u8().await?;
    if version != FRAME_VERSION {
        return Err(Error::InvalidInput("unsupported TCP frame version"));
    }
    let body_len = reader.read_u32().await?;
    if body_len == 0 {
        return Err(Error::InvalidInput("TCP frame body omits type"));
    }
    if body_len > MAX_FRAME_BODY_LEN {
        return Err(Error::InvalidInput("TCP frame body exceeds 1 MiB"));
    }
    // The bound is checked before allocating attacker-controlled storage.
    let mut body = vec![0; body_len as usize];
    reader.read_exact(&mut body).await?;
    let kind = body[0];
    validate_kind(kind)?;
    Ok(Frame {
        kind,
        payload: body.split_off(1),
    })
}

pub async fn write_frame(writer: &mut (impl AsyncWrite + Unpin), frame: &Frame) -> Result<()> {
    validate_kind(frame.kind)?;
    let body_len = frame
        .payload
        .len()
        .checked_add(1)
        .ok_or(Error::InvalidInput("TCP frame body length overflow"))?;
    let body_len = u32::try_from(body_len)
        .map_err(|_| Error::InvalidInput("TCP frame body exceeds u32 encoding limit"))?;
    if body_len > MAX_FRAME_BODY_LEN {
        return Err(Error::InvalidInput("TCP frame body exceeds 1 MiB"));
    }
    writer.write_u8(FRAME_VERSION).await?;
    writer.write_u32(body_len).await?;
    writer.write_u8(frame.kind).await?;
    writer.write_all(&frame.payload).await?;
    writer.flush().await?;
    Ok(())
}

fn validate_kind(kind: u8) -> Result<()> {
    if !(MIN_FRAME_KIND..=MAX_FRAME_KIND).contains(&kind) {
        return Err(Error::InvalidInput("unknown TCP frame type"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn round_trip_preserves_kind_and_payload() {
        let (mut sender, mut receiver) = tokio::io::duplex(64);
        let expected = Frame::new(10, vec![0, 0x80, 0xff]).unwrap();
        write_frame(&mut sender, &expected).await.unwrap();
        assert_eq!(read_frame(&mut receiver).await.unwrap(), expected);
    }

    #[tokio::test]
    async fn rejects_version_kind_and_oversize_from_header() {
        for header in [
            [2, 0, 0, 0, 1, 1],
            [1, 0, 0, 0, 1, 0],
            [1, 0, 0x10, 0, 1, 1],
        ] {
            let (mut sender, mut receiver) = tokio::io::duplex(16);
            sender.write_all(&header).await.unwrap();
            drop(sender);
            assert!(read_frame(&mut receiver).await.is_err());
        }
    }

    #[test]
    fn constructor_counts_type_byte_in_limit() {
        assert!(Frame::new(1, vec![0; MAX_FRAME_BODY_LEN as usize - 1]).is_ok());
        assert!(Frame::new(1, vec![0; MAX_FRAME_BODY_LEN as usize]).is_err());
        assert!(Frame::new(15, Vec::new()).is_err());
    }
}
