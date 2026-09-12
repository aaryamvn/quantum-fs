use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use quantam_fs::{
    crypto::wrap::{ConstructionBWrap, RustCryptoConstructionBWrap},
    encoding,
    ids::Epoch,
    keystore::KeyStore,
    net::{
        frame::{read_frame, write_frame, Frame},
        session::{establish, open_packet, seal_packet, HandshakeProgress},
    },
    Result,
};
use tokio::io::AsyncWriteExt;
use tokio::{
    net::{TcpListener, TcpStream},
    time::{sleep, timeout, Duration},
};

struct TestDir(PathBuf);
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

impl TestDir {
    fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "qfs-net-session-{}-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| quantam_fs::Error::State("test clock"))?
                .as_nanos()
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

async fn sockets() -> Result<(TcpStream, TcpStream)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (client, accepted) = tokio::join!(TcpStream::connect(address), listener.accept());
    Ok((client?, accepted?.0))
}

async fn connect_pair(low: &KeyStore, high: &KeyStore) -> Result<(Epoch, Epoch)> {
    let (mut low_stream, mut high_stream) = sockets().await?;
    let mut low_progress = HandshakeProgress::default();
    let mut high_progress = HandshakeProgress::default();
    let (low_result, high_result) = tokio::join!(
        establish(&mut low_stream, low, None, &mut low_progress),
        establish(&mut high_stream, high, None, &mut high_progress)
    );
    let low_session = low_result?;
    let high_session = high_result?;
    assert!(low_progress.wrap_acknowledged);
    assert!(high_progress.wrap_acknowledged);
    Ok((low_session.session.epoch, high_session.session.epoch))
}

fn sorted_stores(directory: &TestDir) -> Result<(KeyStore, KeyStore)> {
    let first = KeyStore::open(&directory.0.join("first"))?;
    let second = KeyStore::open(&directory.0.join("second"))?;
    if first.peer_id()? < second.peer_id()? {
        Ok((first, second))
    } else {
        Ok((second, first))
    }
}

#[tokio::test]
async fn handshake_establishes_pair_before_gcm_packets() -> Result<()> {
    let directory = TestDir::new()?;
    let (low, high) = sorted_stores(&directory)?;
    let (low_epoch, high_epoch) = connect_pair(&low, &high).await?;
    assert_eq!(low_epoch, Epoch(1));
    assert_eq!(low_epoch, high_epoch);
    let packet = seal_packet(&low, high.peer_id()?, b"after ack")?;
    assert_eq!(open_packet(&high, low.peer_id()?, &packet)?, b"after ack");
    Ok(())
}

async fn receive_wrap_without_ack(stream: &mut TcpStream, keys: &KeyStore) -> Result<Vec<u8>> {
    receive_wrap(stream, keys, false).await
}

async fn receive_wrap(stream: &mut TcpStream, keys: &KeyStore, hold_open: bool) -> Result<Vec<u8>> {
    let identity = read_frame(stream).await?;
    if identity.kind != 1 {
        return Err(quantam_fs::Error::State("expected identity"));
    }
    write_frame(
        stream,
        &Frame::new(1, encoding::encode_identity(&keys.identity()?)?)?,
    )
    .await?;
    let hint = read_frame(stream).await?;
    if hint.kind != 2 {
        return Err(quantam_fs::Error::State("expected epoch hint"));
    }
    let peer = encoding::decode_identity(&identity.payload)?;
    keys.import_peer(peer.clone())?;
    write_frame(
        stream,
        &Frame::new(
            2,
            encoding::epoch_hint_m(&keys.peer_id()?, keys.peer_epoch(peer.peer_id)?),
        )?,
    )
    .await?;
    let wrap = read_frame(stream).await?;
    if wrap.kind != 3 {
        return Err(quantam_fs::Error::State("expected wrap"));
    }
    RustCryptoConstructionBWrap::new(keys.clone())
        .unwrap(peer.peer_id, &encoding::decode_wrap(&wrap.payload)?)?;
    if hold_open {
        sleep(Duration::from_millis(5_200)).await;
    } else {
        stream.shutdown().await?;
    }
    Ok(wrap.payload)
}

#[tokio::test]
async fn lost_wrap_ack_retries_identical_cached_ciphertext() -> Result<()> {
    let directory = TestDir::new()?;
    let (low, high) = sorted_stores(&directory)?;
    let (mut first_client, mut first_server) = sockets().await?;
    let mut first_progress = HandshakeProgress::default();
    let (first_attempt, first_wrap) = tokio::join!(
        establish(&mut first_client, &low, None, &mut first_progress),
        receive_wrap_without_ack(&mut first_server, &high)
    );
    drop(first_server);
    assert!(first_wrap.is_ok());
    assert!(first_attempt.is_err());
    assert_eq!(first_progress.peer_id, Some(high.peer_id()?));
    assert!(!first_progress.wrap_acknowledged);

    let (mut second_client, mut second_server) = sockets().await?;
    let mut second_progress = HandshakeProgress::default();
    let (second_attempt, second_wrap) = tokio::join!(
        establish(&mut second_client, &low, None, &mut second_progress),
        receive_wrap_without_ack(&mut second_server, &high)
    );
    drop(second_server);
    assert!(second_attempt.is_err());
    assert!(!second_progress.wrap_acknowledged);
    assert_eq!(first_wrap?, second_wrap?);
    Ok(())
}

#[tokio::test]
async fn unequal_epoch_hints_reuse_the_smaller_peers_cached_wrap() -> Result<()> {
    let directory = TestDir::new()?;
    let (low, high) = sorted_stores(&directory)?;
    connect_pair(&low, &high).await?;
    let high_identity = high.identity()?;
    let (_, pending) = RustCryptoConstructionBWrap::new(low.clone()).create(
        high_identity.peer_id,
        &high_identity.ek,
        Epoch(2),
    )?;
    let (low_epoch, high_epoch) = connect_pair(&low, &high).await?;
    assert_eq!(pending.epoch, Epoch(2));
    assert_eq!(low_epoch, Epoch(2));
    assert_eq!(high_epoch, Epoch(2));
    Ok(())
}

#[tokio::test]
async fn simultaneous_epoch_wraps_converge_through_collision_retry() -> Result<()> {
    let directory = TestDir::new()?;
    let (low, high) = sorted_stores(&directory)?;
    connect_pair(&low, &high).await?;
    let low_identity = low.identity()?;
    let high_identity = high.identity()?;
    RustCryptoConstructionBWrap::new(low.clone()).create(
        high_identity.peer_id,
        &high_identity.ek,
        Epoch(2),
    )?;
    RustCryptoConstructionBWrap::new(high.clone()).create(
        low_identity.peer_id,
        &low_identity.ek,
        Epoch(2),
    )?;

    let (low_epoch, high_epoch) = connect_pair(&low, &high).await?;
    assert_eq!(low_epoch, Epoch(3));
    assert_eq!(high_epoch, Epoch(3));
    Ok(())
}

#[tokio::test]
async fn held_open_lost_ack_hits_deadline_then_retries_exact_wrap() -> Result<()> {
    let directory = TestDir::new()?;
    let (low, high) = sorted_stores(&directory)?;
    let (mut first_client, mut first_server) = sockets().await?;
    let mut first_progress = HandshakeProgress::default();
    let (deadline, first_wrap) = tokio::join!(
        timeout(
            Duration::from_secs(5),
            establish(&mut first_client, &low, None, &mut first_progress)
        ),
        receive_wrap(&mut first_server, &high, true)
    );
    assert!(deadline.is_err());
    assert!(!first_progress.wrap_acknowledged);

    let (mut retry_client, mut retry_server) = sockets().await?;
    let mut retry_progress = HandshakeProgress::default();
    let (retry_attempt, retry_wrap) = tokio::join!(
        establish(&mut retry_client, &low, None, &mut retry_progress),
        receive_wrap_without_ack(&mut retry_server, &high)
    );
    assert!(retry_attempt.is_err());
    assert_eq!(first_wrap?, retry_wrap?);
    Ok(())
}

#[tokio::test]
async fn equal_watermarks_without_active_slots_create_a_fresh_epoch() -> Result<()> {
    let directory = TestDir::new()?;
    let (low, high) = sorted_stores(&directory)?;
    connect_pair(&low, &high).await?;
    let low_session = low.current_session(high.peer_id()?)?;
    let high_session = high.current_session(low.peer_id()?)?;
    low.retire(low_session.key_handle())?;
    high.retire(high_session.key_handle())?;

    let (low_epoch, high_epoch) = connect_pair(&low, &high).await?;
    assert_eq!(low_epoch, Epoch(2));
    assert_eq!(high_epoch, Epoch(2));
    Ok(())
}

#[tokio::test]
async fn signed_cached_wrap_advances_a_reimported_receivers_floor() -> Result<()> {
    let directory = TestDir::new()?;
    let (low, high) = sorted_stores(&directory)?;
    connect_pair(&low, &high).await?;
    let low_identity = low.identity()?;
    let high_identity = high.identity()?;
    low.discard_pair(high_identity.peer_id)?;
    high.discard_pair(low_identity.peer_id)?;
    low.import_peer(high_identity.clone())?;
    high.import_peer(low_identity.clone())?;
    low.observe_remote_epoch(high_identity.peer_id, Epoch(4))?;
    RustCryptoConstructionBWrap::new(low.clone()).create(
        high_identity.peer_id,
        &high_identity.ek,
        Epoch(5),
    )?;

    let (low_epoch, high_epoch) = connect_pair(&low, &high).await?;
    assert_eq!(low_epoch, Epoch(5));
    assert_eq!(high_epoch, Epoch(5));
    Ok(())
}
