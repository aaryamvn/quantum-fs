use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use quantam_fs::{
    crypto::{
        sign::{PureMlDsa, RustCryptoPureMlDsa, FLUSH_CONTEXT, MANIFEST_CONTEXT},
        wrap::{ConstructionBWrap, RustCryptoConstructionBWrap},
    },
    encoding,
    ids::{Epoch, FileId},
    keystore::KeyStore,
    protocol::manifest::Manifest,
    store::chunks::{ChunkStore, SharedChunkStore},
    sync::host::{ControlUpdate, HostService, MemberReplica},
    Error, Result,
};

static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Result<Self> {
        let number = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("qfs-host-durable-{}-{number}", std::process::id()));
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

fn signed_manifest(
    writer: &KeyStore,
    file_id: FileId,
    chunks: &SharedChunkStore,
    bodies: &[Vec<u8>],
    version: u64,
) -> Result<Manifest> {
    let mut chunk_ids = Vec::with_capacity(bodies.len());
    let mut store = chunks
        .lock()
        .map_err(|_| Error::State("test chunk lock poisoned"))?;
    for (index, body) in bodies.iter().enumerate() {
        chunk_ids.push(store.put(&file_id, index as u64, body.clone())?);
    }
    drop(store);
    let mut manifest = Manifest {
        file_id,
        chunk_ids,
        size: bodies.iter().map(|body| body.len() as u64).sum(),
        writer_id: writer.peer_id()?,
        version,
        signature: Vec::new(),
    };
    manifest.signature = RustCryptoPureMlDsa.sign(
        &writer.signing_key()?,
        MANIFEST_CONTEXT,
        &encoding::manifest_m(&manifest)?,
    )?;
    Ok(manifest)
}

#[test]
fn durable_host_recovers_chunks_members_log_and_semantic_mailbox() -> Result<()> {
    let root_dir = TestDir::new()?;
    let host_dir = root_dir.child("host")?;
    let member_dir = root_dir.child("member")?;
    let host_keys = open_keys(&host_dir)?;
    let member_keys = open_keys(&member_dir)?;
    connect(&host_keys, &member_keys)?;
    let host_id = host_keys.peer_id()?;
    let member_id = member_keys.peer_id()?;
    let members = BTreeSet::from([host_id, member_id]);
    let root = FileId([0x61; 32]);
    let mut host = HostService::open_durable(host_keys.clone(), &host_dir, root, members.clone())?;
    let mut member = MemberReplica::open_durable(
        member_keys.clone(),
        &member_dir,
        root,
        host_id,
        members.clone(),
    )?;
    let file_id = FileId([0x62; 32]);
    let bodies = vec![vec![0xa1; 512 * 1024], vec![0xb2; 512 * 1024]];
    let manifest = signed_manifest(&host_keys, file_id, &host.chunks(), &bodies, 1)?;
    let ids = manifest.chunk_ids.clone();
    host.commit(manifest)?;
    host.link_file(host_id, "/file", file_id)?;

    let replica_size = fs::metadata(host_dir.join("replica.bin"))?.len();
    assert!(replica_size < 16 * 1024);
    assert_eq!(host.mailbox(member_id)?.len(), 4);
    drop(host);
    drop(host_keys);

    let restarted_keys = open_keys(&host_dir)?;
    let pending = restarted_keys.pending_wraps()?;
    assert_eq!(pending.len(), 1);
    RustCryptoConstructionBWrap::new(member_keys.clone()).unwrap(host_id, &pending[0])?;
    let mut restarted =
        HostService::open_durable(restarted_keys, &host_dir, root, BTreeSet::new())?;
    assert_eq!(restarted.expected_root(), root);
    assert_eq!(restarted.members(), &members);
    assert_eq!(restarted.instruction_log().len(), 2);
    assert_eq!(restarted.mailbox(member_id)?.len(), 4);
    let chunks = restarted.chunks();
    let chunks = chunks
        .lock()
        .map_err(|_| Error::State("test chunk lock poisoned"))?;
    assert_eq!(chunks.get(&ids[0]), Some(bodies[0].as_slice()));
    assert_eq!(chunks.get(&ids[1]), Some(bodies[1].as_slice()));
    drop(chunks);

    let challenge = restarted.issue_flush_challenge(member_id)?;
    let signature = RustCryptoPureMlDsa.sign(
        &member_keys.signing_key()?,
        FLUSH_CONTEXT,
        &encoding::flush_m(&challenge),
    )?;
    let report = restarted.flush_mailbox(&mut member, &challenge, &signature)?;
    assert_eq!(report.controls, 2);
    assert_eq!(report.chunks_written, 2);
    drop(member);
    drop(member_keys);

    let reopened_member_keys = open_keys(&member_dir)?;
    let reopened_member = MemberReplica::open_durable(
        reopened_member_keys,
        &member_dir,
        root,
        host_id,
        BTreeSet::new(),
    )?;
    assert_eq!(reopened_member.members(), &members);
    assert_eq!(reopened_member.last_applied(), 2);
    assert_eq!(reopened_member.instruction_log().len(), 2);
    let member_chunks = reopened_member.chunks();
    let member_chunks = member_chunks
        .lock()
        .map_err(|_| Error::State("test chunk lock poisoned"))?;
    assert_eq!(member_chunks.get(&ids[0]), Some(bodies[0].as_slice()));
    assert_eq!(member_chunks.get(&ids[1]), Some(bodies[1].as_slice()));
    Ok(())
}

#[test]
fn log_truncation_waits_for_every_current_member() -> Result<()> {
    let root_dir = TestDir::new()?;
    let host_dir = root_dir.child("host")?;
    let a_dir = root_dir.child("a")?;
    let b_dir = root_dir.child("b")?;
    let host_keys = open_keys(&host_dir)?;
    let a_keys = open_keys(&a_dir)?;
    let b_keys = open_keys(&b_dir)?;
    connect(&host_keys, &a_keys)?;
    connect(&host_keys, &b_keys)?;
    let host_id = host_keys.peer_id()?;
    let a = a_keys.peer_id()?;
    let b = b_keys.peer_id()?;
    let members = BTreeSet::from([host_id, a, b]);
    let root = FileId([0x71; 32]);
    let mut host = HostService::open_durable(host_keys.clone(), &host_dir, root, members)?;
    let file_id = FileId([0x72; 32]);
    let manifest = signed_manifest(
        &host_keys,
        file_id,
        &host.chunks(),
        &[b"one record".to_vec()],
        1,
    )?;
    host.commit(manifest)?;
    assert_eq!(host.instruction_log().len(), 1);

    host.acknowledge_applied(a, 1)?;
    assert_eq!(host.instruction_log().len(), 1);
    host.acknowledge_applied(b, 1)?;
    assert!(host.instruction_log().is_empty());
    drop(host);
    drop(host_keys);

    let reopened =
        HostService::open_durable(open_keys(&host_dir)?, &host_dir, root, BTreeSet::new())?;
    assert!(reopened.instruction_log().is_empty());
    assert_eq!(reopened.acked_through(a), 1);
    assert_eq!(reopened.acked_through(b), 1);
    Ok(())
}

#[test]
fn clear_unpins_chunk_but_keeps_ordered_control_and_keystore_lock_is_shared() -> Result<()> {
    let root_dir = TestDir::new()?;
    let host_dir = root_dir.child("host")?;
    let member_dir = root_dir.child("member")?;
    let host_keys = open_keys(&host_dir)?;
    assert!(open_keys(&host_dir).is_err());
    let member_keys = open_keys(&member_dir)?;
    connect(&host_keys, &member_keys)?;
    let host_id = host_keys.peer_id()?;
    let member_id = member_keys.peer_id()?;
    let root = FileId([0x81; 32]);
    let members = BTreeSet::from([host_id, member_id]);
    let mut host = HostService::open_durable(host_keys.clone(), &host_dir, root, members)?;
    let file_id = FileId([0x82; 32]);
    let manifest = signed_manifest(
        &host_keys,
        file_id,
        &host.chunks(),
        &[b"discard after clear".to_vec()],
        1,
    )?;
    host.commit(manifest)?;
    assert_eq!(fs::read_dir(host_dir.join("chunks"))?.count(), 1);
    host.fan_out_control(host_id, &ControlUpdate::Clear(file_id))?;
    assert_eq!(fs::read_dir(host_dir.join("chunks"))?.count(), 0);
    assert!(host.trusted_manifest(&file_id).is_none());
    assert_eq!(host.instruction_log().len(), 2);
    assert_eq!(host.mailbox(member_id)?.len(), 2);
    Ok(())
}
