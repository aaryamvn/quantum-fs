use quantam_fs::{
    crypto::{
        aead::{Aes256Gcm, RustCryptoAes256Gcm},
        wrap::{ConstructionBWrap, RustCryptoConstructionBWrap},
    },
    encoding,
    ids::{Epoch, Seq},
    keystore::KeyStore,
    protocol::packet::{PacketHeader, PayloadType, PROTOCOL_VERSION},
    Error,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

struct Directory(PathBuf);
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
impl Directory {
    fn new() -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let number = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "qfs-wrap-{}-{number}-{timestamp}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn peers(
    dir: &Directory,
) -> std::result::Result<(KeyStore, KeyStore, PathBuf), Box<dyn std::error::Error>> {
    let a_path = dir.0.join("a");
    let b_path = dir.0.join("b");
    let a = KeyStore::open(&a_path)?;
    let b = KeyStore::open(&b_path)?;
    a.import_peer(b.identity()?)?;
    b.import_peer(a.identity()?)?;
    if a.peer_id()? < b.peer_id()? {
        Ok((a, b, a_path))
    } else {
        Ok((b, a, b_path))
    }
}

#[test]
fn first_contact_bind_tampering_and_duplicate_wrap_do_not_reset_replay() -> TestResult {
    let dir = Directory::new()?;
    let (low, high, _) = peers(&dir)?;
    let low_id = low.identity()?;
    let high_id = high.identity()?;
    let sender = RustCryptoConstructionBWrap::new(low.clone());
    let receiver = RustCryptoConstructionBWrap::new(high.clone());
    assert!(receiver
        .create(low_id.peer_id, &low_id.ek, Epoch(1))
        .is_err());
    let mut wrong_ek = high_id.ek.clone();
    wrong_ek[0] ^= 1;
    assert!(sender.create(high_id.peer_id, &wrong_ek, Epoch(1)).is_err());
    let (local, message) = sender.create(high_id.peer_id, &high_id.ek, Epoch(1))?;
    let mut tampered = message.clone();
    tampered.wrap_ct[0] ^= 1;
    assert!(receiver.unwrap(low_id.peer_id, &tampered).is_err());
    let remote = receiver.unwrap(low_id.peer_id, &message)?;
    let header = PacketHeader {
        version: PROTOCOL_VERSION,
        sender_id: low_id.peer_id,
        receiver_id: high_id.peer_id,
        epoch: Epoch(1),
        seq: Seq(0),
    };
    let nonce = encoding::nonce(&header, PayloadType::Packet);
    let aad = encoding::packet_aad(&header);
    let ciphertext =
        RustCryptoAes256Gcm::new(low.clone()).seal(local.key_handle(), &nonce, &aad, b"hello")?;
    let cipher = RustCryptoAes256Gcm::new(high.clone());
    assert_eq!(
        cipher.open(remote.key_handle(), &nonce, &aad, &ciphertext)?,
        b"hello"
    );
    let duplicate = receiver.unwrap(low_id.peer_id, &message)?;
    assert!(matches!(
        cipher.open(duplicate.key_handle(), &nonce, &aad, &ciphertext),
        Err(Error::ReplayRejected)
    ));
    assert!(sender
        .create(high_id.peer_id, &high_id.ek, Epoch(1))
        .is_err());
    Ok(())
}

#[test]
fn simultaneous_epoch_collision_keeps_smaller_then_loser_retries_at_last_plus_two() -> TestResult {
    let dir = Directory::new()?;
    let (low, high, _) = peers(&dir)?;
    let low_id = low.identity()?;
    let high_id = high.identity()?;
    let a = RustCryptoConstructionBWrap::new(low.clone());
    let b = RustCryptoConstructionBWrap::new(high.clone());
    let (_, initial) = a.create(high_id.peer_id, &high_id.ek, Epoch(1))?;
    b.unwrap(low_id.peer_id, &initial)?;
    let (_, low_wrap) = a.create(high_id.peer_id, &high_id.ek, Epoch(2))?;
    let (losing_session, high_wrap) = b.create(low_id.peer_id, &low_id.ek, Epoch(2))?;
    assert!(matches!(
        a.unwrap(high_id.peer_id, &high_wrap),
        Err(Error::EpochConflict {
            retry_epoch: Epoch(3)
        })
    ));
    b.unwrap(low_id.peer_id, &low_wrap)?;
    assert!(high.retire(losing_session.key_handle()).is_err());
    assert_eq!(high.retry_epoch(&low_id.peer_id)?, Some(Epoch(3)));
    let (retried, retry_wrap) = b.retry_collision(low_id.peer_id)?;
    assert_eq!(retried.epoch, Epoch(3));
    a.unwrap(high_id.peer_id, &retry_wrap)?;
    assert_eq!(high.retry_epoch(&low_id.peer_id)?, None);
    assert!(a.unwrap(high_id.peer_id, &high_wrap).is_err());
    assert!(high.session(low_id.peer_id, Epoch(1)).is_ok());
    assert!(high.session(low_id.peer_id, Epoch(2)).is_ok());
    Ok(())
}

#[test]
fn restart_discards_old_slots_and_prepares_a_fresh_epoch_with_zero_counters() -> TestResult {
    let dir = Directory::new()?;
    let (low, high, low_path) = peers(&dir)?;
    let low_id = low.identity()?;
    let high_id = high.identity()?;
    let sender = RustCryptoConstructionBWrap::new(low.clone());
    let receiver = RustCryptoConstructionBWrap::new(high.clone());
    let (session, old_wrap) = sender.create(high_id.peer_id, &high_id.ek, Epoch(1))?;
    receiver.unwrap(low_id.peer_id, &old_wrap)?;
    let old_handle = session.key_handle().clone();
    drop(sender);
    drop(low);
    let restarted = KeyStore::open(&low_path)?;
    assert!(restarted.identity()? == low_id);
    let pending = restarted.pending_wraps()?;
    assert_eq!(pending.len(), 1);
    let fresh = &pending[0];
    assert_eq!(fresh.epoch, Epoch(2));
    assert!(fresh.kem_ct != old_wrap.kem_ct);
    assert!(fresh.wrap_ct != old_wrap.wrap_ct);
    assert!(restarted.session(high_id.peer_id, Epoch(1)).is_err());
    assert!(restarted.retire(&old_handle).is_err());
    let local = restarted.session(high_id.peer_id, Epoch(2))?;
    let remote = receiver.unwrap(low_id.peer_id, fresh)?;
    let header = PacketHeader {
        version: 1,
        sender_id: low_id.peer_id,
        receiver_id: high_id.peer_id,
        epoch: Epoch(2),
        seq: Seq(0),
    };
    let nonce = encoding::nonce(&header, PayloadType::Packet);
    let aad = encoding::packet_aad(&header);
    let ciphertext =
        RustCryptoAes256Gcm::new(restarted).seal(local.key_handle(), &nonce, &aad, b"restarted")?;
    assert_eq!(
        RustCryptoAes256Gcm::new(high).open(remote.key_handle(), &nonce, &aad, &ciphertext)?,
        b"restarted"
    );
    Ok(())
}

#[test]
fn newer_peer_ek_replaces_pending_wrap_and_preserves_principal() -> TestResult {
    let dir = Directory::new()?;
    let (low, high, _) = peers(&dir)?;
    let low_id = low.identity()?;
    let old_high = high.identity()?;
    let sender = RustCryptoConstructionBWrap::new(low.clone());
    let receiver = RustCryptoConstructionBWrap::new(high.clone());
    let (_, old_wrap) = sender.create(old_high.peer_id, &old_high.ek, Epoch(1))?;
    receiver.unwrap(low_id.peer_id, &old_wrap)?;
    let rotated = high.rotate_identity_ek()?;
    assert!(rotated.peer_id == old_high.peer_id && rotated.vk == old_high.vk);
    low.import_peer(rotated)?;
    let pending = low.pending_wraps()?;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].epoch, Epoch(2));
    receiver.unwrap(low_id.peer_id, &pending[0])?;
    assert!(sender.retry(&old_wrap).is_err());
    assert!(low.import_peer(old_high).is_err());
    Ok(())
}

#[test]
fn empty_scaffold_identity_is_upgraded_to_private_real_keys() -> TestResult {
    let dir = Directory::new()?;
    let path = dir.0.join("identity");
    fs::write(&path, [])?;
    let store = KeyStore::open(&path)?;
    store.identity()?.verify()?;
    assert!(!fs::read(&path)?.is_empty());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
    }
    Ok(())
}

#[test]
fn missing_identity_does_not_replace_the_principal_of_existing_pair_state() -> TestResult {
    let dir = Directory::new()?;
    let path = dir.0.join("identity");
    let store = KeyStore::open(&path)?;
    drop(store);
    fs::remove_file(&path)?;
    assert!(KeyStore::open(&path).is_err());
    assert!(!path.exists());
    Ok(())
}
