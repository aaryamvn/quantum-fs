use crate::{
    encoding,
    ids::PeerId,
    keystore::KeyStore,
    net::{
        frame::{self, read_frame, write_frame, Frame},
        session,
    },
    protocol::locate::{HaveQuery, HaveReply},
    protocol::packet::PROTOCOL_VERSION,
    store::chunks::{ChunkStore, SharedChunkStore},
    Error, Result,
};
use tokio::net::TcpStream;

const QUERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Produces a point-in-time answer for exactly the requested identifiers.
/// This is live pairwise state and is never persisted or sent to a directory.
pub fn answer_have(chunks: &SharedChunkStore, query: &HaveQuery) -> Result<HaveReply> {
    let chunks = chunks
        .lock()
        .map_err(|_| Error::State("chunk store poisoned"))?;
    HaveReply::new(query, chunks.have_bitset(query.chunk_ids()))
}

/// Queries an already-authenticated live pair. Connection establishment and
/// vault admission remain the caller's responsibility.
pub async fn query_over_stream(
    stream: &mut TcpStream,
    keys: &KeyStore,
    peer: PeerId,
    query: &HaveQuery,
) -> Result<HaveReply> {
    keys.require_live_traffic(peer)?;
    let packet = session::seal_packet(keys, peer, &encoding::encode_have_query(query)?)?;
    tokio::time::timeout(
        QUERY_TIMEOUT,
        write_frame(
            stream,
            &Frame::new(
                frame::GCM_PACKET_KIND,
                encoding::encode_control_packet(&packet)?,
            )?,
        ),
    )
    .await
    .map_err(|_| Error::State("have query write timeout"))??;
    let received = tokio::time::timeout(QUERY_TIMEOUT, read_frame(stream))
        .await
        .map_err(|_| Error::State("have reply timeout"))??;
    keys.require_live_traffic(peer)?;
    if received.kind != frame::GCM_PACKET_KIND {
        return Err(Error::State("expected encrypted have reply"));
    }
    let packet = encoding::decode_control_packet(&received.payload)?;
    require_header(keys, peer, &packet.header)?;
    let plaintext = session::open_packet(keys, peer, &packet)?;
    let reply = encoding::decode_have_reply(&plaintext)?;
    reply.validate(query)?;
    Ok(reply)
}

/// Answers one have query on an already-authenticated live pair.
pub async fn serve_have_once(
    stream: &mut TcpStream,
    keys: &KeyStore,
    peer: PeerId,
    chunks: &SharedChunkStore,
) -> Result<()> {
    keys.require_live_traffic(peer)?;
    let received = tokio::time::timeout(QUERY_TIMEOUT, read_frame(stream))
        .await
        .map_err(|_| Error::State("have query timeout"))??;
    keys.require_live_traffic(peer)?;
    if received.kind != frame::GCM_PACKET_KIND {
        return Err(Error::State("expected encrypted have query"));
    }
    let packet = encoding::decode_control_packet(&received.payload)?;
    require_header(keys, peer, &packet.header)?;
    let plaintext = session::open_packet(keys, peer, &packet)?;
    let query = encoding::decode_have_query(&plaintext)?;
    let reply = answer_have(chunks, &query)?;
    let packet = session::seal_packet(keys, peer, &encoding::encode_have_reply(&reply)?)?;
    tokio::time::timeout(
        QUERY_TIMEOUT,
        write_frame(
            stream,
            &Frame::new(
                frame::GCM_PACKET_KIND,
                encoding::encode_control_packet(&packet)?,
            )?,
        ),
    )
    .await
    .map_err(|_| Error::State("have reply write timeout"))?
}

fn require_header(
    keys: &KeyStore,
    peer: PeerId,
    header: &crate::protocol::packet::PacketHeader,
) -> Result<()> {
    if header.version != PROTOCOL_VERSION
        || header.sender_id != peer
        || header.receiver_id != keys.peer_id()?
        || header.epoch != keys.current_session(peer)?.epoch
    {
        return Err(Error::AuthenticationFailed);
    }
    Ok(())
}
