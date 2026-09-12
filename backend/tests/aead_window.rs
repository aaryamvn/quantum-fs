use quantam_fs::{
    crypto::{
        aead::{Aes256Gcm, RustCryptoAes256Gcm},
        wrap::{ConstructionBWrap, PairSession, RustCryptoConstructionBWrap},
    },
    encoding,
    ids::{Epoch, FileId, PeerId, Seq},
    keystore::KeyStore,
    protocol::packet::{PacketHeader, PayloadType, ReplayWindowState, PROTOCOL_VERSION},
    Error,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> quantam_fs::Result<Self> {
        let number = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("qfs-aead-{}-{number}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn identity(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Pair {
    _directory: TestDir,
    sender: KeyStore,
    receiver: KeyStore,
    sender_session: PairSession,
    receiver_session: PairSession,
    sender_id: PeerId,
    receiver_id: PeerId,
}

fn pair() -> quantam_fs::Result<Pair> {
    let directory = TestDir::new()?;
    let first = KeyStore::open(&directory.identity("first"))?;
    let second = KeyStore::open(&directory.identity("second"))?;
    let first_identity = first.identity()?;
    let second_identity = second.identity()?;
    let (sender, sender_identity, receiver, receiver_identity) =
        if first_identity.peer_id < second_identity.peer_id {
            (first, first_identity, second, second_identity)
        } else {
            (second, second_identity, first, first_identity)
        };
    sender.import_peer(receiver_identity.clone())?;
    receiver.import_peer(sender_identity.clone())?;
    let (sender_session, message) = RustCryptoConstructionBWrap::new(sender.clone()).create(
        receiver_identity.peer_id,
        &receiver_identity.ek,
        Epoch(1),
    )?;
    let receiver_session = RustCryptoConstructionBWrap::new(receiver.clone())
        .unwrap(sender_identity.peer_id, &message)?;
    Ok(Pair {
        _directory: directory,
        sender,
        receiver,
        sender_session,
        receiver_session,
        sender_id: sender_identity.peer_id,
        receiver_id: receiver_identity.peer_id,
    })
}

fn header(pair: &Pair, seq: u64) -> PacketHeader {
    PacketHeader {
        version: PROTOCOL_VERSION,
        sender_id: pair.sender_id,
        receiver_id: pair.receiver_id,
        epoch: Epoch(1),
        seq: Seq(seq),
    }
}

#[test]
fn replay_window_accepts_reordered_holes_once() {
    let mut window = ReplayWindowState::default();
    assert!(window.accept(Seq(7)).is_ok());
    assert!(window.accept(Seq(10)).is_ok());
    assert!(window.accept(Seq(8)).is_ok());
    assert!(window.accept(Seq(9)).is_ok());
    assert!(matches!(window.accept(Seq(8)), Err(Error::ReplayRejected)));
}

#[test]
fn replay_window_enforces_full_1024_counter_boundary() {
    let mut window = ReplayWindowState::default();
    assert!(window.accept(Seq(2_000)).is_ok());
    assert!(window.accept(Seq(977)).is_ok());
    assert!(matches!(
        window.accept(Seq(976)),
        Err(Error::ReplayRejected)
    ));
}

#[test]
fn packet_chunk_and_direction_domains_have_distinct_nonces() {
    let low = PeerId([1; 32]);
    let high = PeerId([2; 32]);
    let forward = PacketHeader {
        version: PROTOCOL_VERSION,
        sender_id: low,
        receiver_id: high,
        epoch: Epoch(3),
        seq: Seq(4),
    };
    let reverse = PacketHeader {
        sender_id: high,
        receiver_id: low,
        ..forward
    };

    let packet_forward = encoding::nonce(&forward, PayloadType::Packet);
    let chunk_forward = encoding::nonce(&forward, PayloadType::ChunkBody);
    let packet_reverse = encoding::nonce(&reverse, PayloadType::Packet);
    assert_ne!(packet_forward, chunk_forward);
    assert_ne!(packet_forward, packet_reverse);
    assert_ne!(chunk_forward, packet_reverse);
}

#[test]
fn one_pair_handle_seals_packets_and_chunks_and_counters_survive_provider_recreation(
) -> quantam_fs::Result<()> {
    let pair = pair()?;
    let sender_aead = RustCryptoAes256Gcm::new(pair.sender.clone());
    let receiver_aead = RustCryptoAes256Gcm::new(pair.receiver.clone());

    let packet_header = header(&pair, 1);
    let packet_aad = encoding::packet_aad(&packet_header);
    let packet_nonce = encoding::nonce(&packet_header, PayloadType::Packet);
    let packet_ct = sender_aead.seal(
        pair.sender_session.key_handle(),
        &packet_nonce,
        &packet_aad,
        b"packet",
    )?;
    assert_eq!(
        receiver_aead.open(
            pair.receiver_session.key_handle(),
            &packet_nonce,
            &packet_aad,
            &packet_ct,
        )?,
        b"packet"
    );

    let chunk_header = header(&pair, 1);
    let chunk_aad = encoding::chunk_aad(&chunk_header, &FileId([9; 32]), 3);
    let chunk_nonce = encoding::nonce(&chunk_header, PayloadType::ChunkBody);
    let chunk_ct = sender_aead.seal(
        pair.sender_session.key_handle(),
        &chunk_nonce,
        &chunk_aad,
        b"chunk",
    )?;
    assert_eq!(
        receiver_aead.open(
            pair.receiver_session.key_handle(),
            &chunk_nonce,
            &chunk_aad,
            &chunk_ct,
        )?,
        b"chunk"
    );

    let recreated = RustCryptoAes256Gcm::new(pair.sender.clone());
    assert!(matches!(
        recreated.seal(
            pair.sender_session.key_handle(),
            &packet_nonce,
            &packet_aad,
            b"reuse"
        ),
        Err(Error::State(_))
    ));
    Ok(())
}

#[test]
fn failed_authentication_does_not_poison_the_receive_window() -> quantam_fs::Result<()> {
    let pair = pair()?;
    let sender = RustCryptoAes256Gcm::new(pair.sender.clone());
    let receiver = RustCryptoAes256Gcm::new(pair.receiver.clone());
    let header = header(&pair, 19);
    let aad = encoding::packet_aad(&header);
    let nonce = encoding::nonce(&header, PayloadType::Packet);
    let ciphertext = sender.seal(pair.sender_session.key_handle(), &nonce, &aad, b"valid")?;
    let mut corrupted = ciphertext.clone();
    corrupted[0] ^= 1;
    assert!(matches!(
        receiver.open(pair.receiver_session.key_handle(), &nonce, &aad, &corrupted),
        Err(Error::AuthenticationFailed)
    ));
    assert_eq!(
        receiver.open(
            pair.receiver_session.key_handle(),
            &nonce,
            &aad,
            &ciphertext,
        )?,
        b"valid"
    );
    Ok(())
}

#[test]
fn receive_window_accepts_authenticated_holes_and_rejects_replay() -> quantam_fs::Result<()> {
    let pair = pair()?;
    let sender = RustCryptoAes256Gcm::new(pair.sender.clone());
    let receiver = RustCryptoAes256Gcm::new(pair.receiver.clone());
    let mut messages = Vec::new();
    for seq in [7, 8, 10] {
        let header = header(&pair, seq);
        let aad = encoding::packet_aad(&header);
        let nonce = encoding::nonce(&header, PayloadType::Packet);
        let ciphertext =
            sender.seal(pair.sender_session.key_handle(), &nonce, &aad, &[seq as u8])?;
        messages.push((nonce, aad, ciphertext));
    }
    for index in [2, 0, 1] {
        let (nonce, aad, ciphertext) = &messages[index];
        receiver.open(pair.receiver_session.key_handle(), nonce, aad, ciphertext)?;
    }
    let (nonce, aad, ciphertext) = &messages[0];
    assert!(matches!(
        receiver.open(pair.receiver_session.key_handle(), nonce, aad, ciphertext),
        Err(Error::ReplayRejected)
    ));
    Ok(())
}

#[test]
fn metadata_mismatches_and_retired_handles_are_rejected() -> quantam_fs::Result<()> {
    let pair = pair()?;
    let sender = RustCryptoAes256Gcm::new(pair.sender.clone());
    let valid = header(&pair, 1);
    let packet_aad = encoding::packet_aad(&valid);
    let chunk_nonce = encoding::nonce(&valid, PayloadType::ChunkBody);
    assert!(matches!(
        sender.seal(
            pair.sender_session.key_handle(),
            &chunk_nonce,
            &packet_aad,
            b"wrong type"
        ),
        Err(Error::InvalidInput(_))
    ));

    let wrong_epoch = PacketHeader {
        epoch: Epoch(2),
        ..valid
    };
    assert!(matches!(
        sender.seal(
            pair.sender_session.key_handle(),
            &encoding::nonce(&wrong_epoch, PayloadType::Packet),
            &encoding::packet_aad(&wrong_epoch),
            b"wrong epoch"
        ),
        Err(Error::InvalidInput(_))
    ));
    let wrong_peer = PacketHeader {
        receiver_id: PeerId([0xff; 32]),
        ..valid
    };
    assert!(matches!(
        sender.seal(
            pair.sender_session.key_handle(),
            &encoding::nonce(&wrong_peer, PayloadType::Packet),
            &encoding::packet_aad(&wrong_peer),
            b"wrong peer"
        ),
        Err(Error::InvalidInput(_))
    ));
    let wrong_direction = PacketHeader {
        sender_id: pair.receiver_id,
        receiver_id: pair.sender_id,
        ..valid
    };
    assert!(matches!(
        sender.seal(
            pair.sender_session.key_handle(),
            &encoding::nonce(&wrong_direction, PayloadType::Packet),
            &encoding::packet_aad(&wrong_direction),
            b"wrong direction"
        ),
        Err(Error::InvalidInput(_))
    ));

    pair.sender.retire(pair.sender_session.key_handle())?;
    assert!(matches!(
        sender.seal(
            pair.sender_session.key_handle(),
            &encoding::nonce(&valid, PayloadType::Packet),
            &packet_aad,
            b"retired"
        ),
        Err(Error::KeyUnavailable)
    ));
    Ok(())
}
