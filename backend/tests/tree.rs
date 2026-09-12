use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use quantam_fs::{
    crypto::wrap::{ConstructionBWrap, RustCryptoConstructionBWrap},
    ids::{Epoch, FileId},
    keystore::KeyStore,
    store::chunks::ChunkStore,
    sync::host::{HostService, MemberReplica},
    Error, Result,
};

static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Result<Self> {
        let number = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("qfs-tree-{}-{number}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn child(&self, name: &str) -> Result<PathBuf> {
        let path = self.0.join(name);
        fs::create_dir(&path)?;
        Ok(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn open_keys(data_dir: &Path) -> Result<KeyStore> {
    KeyStore::open(&data_dir.join("identity"))
}

fn connect(a: &KeyStore, b: &KeyStore) -> Result<()> {
    let a_identity = a.identity()?;
    let b_identity = b.identity()?;
    a.import_peer(b_identity.clone())?;
    b.import_peer(a_identity.clone())?;
    let (sender, sender_identity, receiver, receiver_identity) =
        if a_identity.peer_id < b_identity.peer_id {
            (a, a_identity, b, b_identity)
        } else {
            (b, b_identity, a, a_identity)
        };
    let (_, message) = RustCryptoConstructionBWrap::new(sender.clone()).create(
        receiver_identity.peer_id,
        &receiver_identity.ek,
        Epoch(1),
    )?;
    RustCryptoConstructionBWrap::new(receiver.clone()).unwrap(sender_identity.peer_id, &message)?;
    Ok(())
}

fn apply_online(host: &mut HostService, member: &mut MemberReplica) -> Result<()> {
    for packet in host.take_online_control(member.peer_id()?)? {
        member.apply_control(&packet)?;
    }
    Ok(())
}

#[test]
fn member_mkdir_save_and_tree_plaintext_survive_restart() -> Result<()> {
    let root_dir = TestDir::new()?;
    let host_dir = root_dir.child("host")?;
    let writer_dir = root_dir.child("writer")?;
    let host_keys = open_keys(&host_dir)?;
    let writer_keys = open_keys(&writer_dir)?;
    connect(&host_keys, &writer_keys)?;
    let host_id = host_keys.peer_id()?;
    let writer_id = writer_keys.peer_id()?;
    let members = BTreeSet::from([host_id, writer_id]);
    let root = FileId([0x31; 32]);
    let mut host = HostService::open_durable(host_keys.clone(), &host_dir, root, members.clone())?;

    host.mkdir(writer_id, "/a")?;
    let bodies = vec![b"first chunk".to_vec(), b"second chunk".to_vec()];
    let file_id = host.save_file(&writer_keys, "/a/f", &bodies)?;
    let manifest = host
        .trusted_manifest(&file_id)
        .ok_or(Error::State("saved manifest missing"))?
        .manifest()
        .clone();
    assert_eq!(host.tree().resolve("/a/f")?, file_id);
    drop(host);
    drop(host_keys);

    let reopened_keys = open_keys(&host_dir)?;
    let reopened = HostService::open_durable(reopened_keys, &host_dir, root, BTreeSet::new())?;
    assert_eq!(reopened.members(), &members);
    assert_eq!(reopened.tree().resolve("a/f")?, file_id);
    let chunks = reopened.chunks();
    let chunks = chunks
        .lock()
        .map_err(|_| Error::State("test chunk lock poisoned"))?;
    assert_eq!(
        chunks.get(&manifest.chunk_ids[0]),
        Some(bodies[0].as_slice())
    );
    assert_eq!(
        chunks.get(&manifest.chunk_ids[1]),
        Some(bodies[1].as_slice())
    );
    Ok(())
}

#[test]
fn paths_are_byte_exact_and_directories_are_acyclic_and_nonempty() -> Result<()> {
    let directory = TestDir::new()?;
    let keys = open_keys(&directory.0)?;
    let local = keys.peer_id()?;
    let mut host = HostService::new(
        keys,
        BTreeSet::from([local]),
        quantam_fs::store::chunks::shared_chunk_store(),
    )?;

    let composed = host.mkdir(local, "/é")?;
    let decomposed = host.mkdir(local, "/e\u{301}")?;
    let upper = host.mkdir(local, "/A")?;
    let lower = host.mkdir(local, "/a")?;
    assert_ne!(composed, decomposed);
    assert_ne!(upper, lower);
    assert_eq!(host.tree().resolve("é")?, composed);
    assert_eq!(host.tree().resolve("e\u{301}")?, decomposed);

    host.mkdir(local, "/a/b")?;
    assert!(host.unlink(local, "/a").is_err());
    assert!(host.rename(local, "/a", "/a/b/a").is_err());
    for invalid in ["../x", "a//x", "a/./x", "a/../x", "a\\x", "C:/x", "a\0x"] {
        assert!(host.mkdir(local, invalid).is_err(), "accepted {invalid:?}");
    }
    Ok(())
}

#[test]
fn member_tree_updates_follow_host_arrival_order_and_reject_outsiders() -> Result<()> {
    let directory = TestDir::new()?;
    let host_dir = directory.child("host")?;
    let member_dir = directory.child("member")?;
    let outsider_dir = directory.child("outsider")?;
    let host_keys = open_keys(&host_dir)?;
    let member_keys = open_keys(&member_dir)?;
    let outsider_keys = open_keys(&outsider_dir)?;
    connect(&host_keys, &member_keys)?;
    let host_id = host_keys.peer_id()?;
    let member_id = member_keys.peer_id()?;
    let members = BTreeSet::from([host_id, member_id]);
    let mut host = HostService::new(
        host_keys.clone(),
        members.clone(),
        quantam_fs::store::chunks::shared_chunk_store(),
    )?;
    let mut member = MemberReplica::new(
        member_keys.clone(),
        host_id,
        members,
        quantam_fs::store::chunks::shared_chunk_store(),
    )?;
    host.heartbeat(member_id, Duration::from_secs(60))?;

    host.mkdir(member_id, "/a")?;
    let file_id = host.save_file(&member_keys, "/a/f", &[b"member bytes".to_vec()])?;
    apply_online(&mut host, &mut member)?;
    assert_eq!(member.tree().resolve("/a/f")?, file_id);
    assert_eq!(host.tree(), member.tree());

    host.rename(member_id, "/a/f", "/a/first")?;
    assert!(host.rename(member_id, "/a/f", "/a/second").is_err());
    apply_online(&mut host, &mut member)?;
    assert_eq!(host.tree().resolve("/a/first")?, file_id);
    assert!(host.tree().resolve("/a/second").is_err());
    assert_eq!(host.tree(), member.tree());

    assert!(host.mkdir(outsider_keys.peer_id()?, "/outsider").is_err());
    assert!(host.tree().resolve("/outsider").is_err());
    Ok(())
}
