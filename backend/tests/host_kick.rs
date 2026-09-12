use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use quantam_fs::{
    crypto::{
        sign::{PureMlDsa, RustCryptoPureMlDsa, FLUSH_CONTEXT, MANIFEST_CONTEXT},
        wrap::{ConstructionBWrap, RustCryptoConstructionBWrap},
    },
    encoding,
    ids::{Epoch, FileId},
    keystore::{IdentityKeyStore, KeyStore},
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
            std::env::temp_dir().join(format!("qfs-host-kick-{}-{number}", std::process::id()));
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
fn kicked_writer_history_survives_restart_and_offline_members_revoke_pairs() -> Result<()> {
    let directory = TestDir::new()?;
    let hdir = directory.child("host")?;
    let adir = directory.child("a")?;
    let bdir = directory.child("b")?;
    let h = open_keys(&hdir)?;
    let a = open_keys(&adir)?;
    let b = open_keys(&bdir)?;
    connect(&h, &a)?;
    connect(&h, &b)?;
    connect(&a, &b)?;
    let hid = h.peer_id()?;
    let aid = a.peer_id()?;
    let bid = b.peer_id()?;
    let members = BTreeSet::from([hid, aid, bid]);
    let root = FileId([65; 32]);
    let mut host = HostService::open_durable(h.clone(), &hdir, root, members.clone())?;
    let mut member = MemberReplica::open_durable(a.clone(), &adir, root, hid, members.clone())?;
    let file = FileId([66; 32]);
    host.link_file(bid, "old-writer", file)?;
    let manifest = signed_manifest(
        &b,
        file,
        &host.chunks(),
        &[b"B's accepted bytes".to_vec()],
        1,
    )?;
    let id = manifest.chunk_ids[0];
    host.fan_out_control(bid, &ControlUpdate::NewManifest(manifest))?;
    assert!(host.kick(hid).is_err());
    assert!(host.kick(quantam_fs::ids::PeerId([99; 32])).is_err());
    assert!(host
        .fan_out_control(aid, &ControlUpdate::Kick(bid))
        .is_err());
    assert_eq!(host.kick(bid)?, bid);
    assert_eq!(host.take_disconnects(), vec![bid]);
    assert!(!host.has_member(&bid));
    assert!(host.mailbox(bid).is_err());
    assert!(host.mkdir(bid, "not-allowed").is_err());
    assert!(h.current_session(bid).is_err());
    assert!(h.load_verified_peer(&bid).is_err());
    assert!(a.current_session(bid).is_ok()); // Online consistency window closes at apply.
    assert_eq!(host.instruction_log().len(), 3);
    let challenge = host.issue_flush_challenge(aid)?;
    let signature = RustCryptoPureMlDsa.sign(
        &a.signing_key()?,
        FLUSH_CONTEXT,
        &encoding::flush_m(&challenge),
    )?;
    host.flush_mailbox(&mut member, &challenge, &signature)?;
    assert!(!member.members().contains(&bid));
    assert!(a.current_session(bid).is_err());
    assert!(a.load_verified_peer(&bid).is_err());
    assert!(host.instruction_log().is_empty()); // B no longer pins the prefix.
    drop(member);
    drop(a);
    drop(host);
    drop(h);
    let h = open_keys(&hdir)?;
    let host = HostService::open_durable(h, &hdir, root, BTreeSet::new())?;
    assert_eq!(host.tree().resolve("old-writer")?, file);
    assert_eq!(
        host.trusted_manifest(&file)
            .ok_or(Error::State("manifest lost"))?
            .manifest()
            .writer_id,
        bid
    );
    assert_eq!(
        host.chunks()
            .lock()
            .map_err(|_| Error::State("lock"))?
            .get(&id),
        Some(b"B's accepted bytes".as_slice())
    );
    let member = MemberReplica::open_durable(open_keys(&adir)?, &adir, root, hid, BTreeSet::new())?;
    assert_eq!(member.tree().resolve("old-writer")?, file);
    assert!(member.trusted_manifest(&file).is_some());
    Ok(())
}

#[test]
fn online_kick_only_reaches_remaining_members() -> Result<()> {
    let directory = TestDir::new()?;
    let h = open_keys(&directory.child("h")?)?;
    let a = open_keys(&directory.child("a")?)?;
    let b = open_keys(&directory.child("b")?)?;
    connect(&h, &a)?;
    connect(&h, &b)?;
    connect(&a, &b)?;
    let hid = h.peer_id()?;
    let aid = a.peer_id()?;
    let bid = b.peer_id()?;
    let members = BTreeSet::from([hid, aid, bid]);
    let mut host = HostService::new(
        h.clone(),
        members.clone(),
        quantam_fs::store::chunks::shared_chunk_store(),
    )?;
    let mut member = MemberReplica::new(
        a.clone(),
        hid,
        members,
        quantam_fs::store::chunks::shared_chunk_store(),
    )?;
    host.heartbeat(aid, Duration::from_secs(30))?;
    host.heartbeat(bid, Duration::from_secs(30))?;
    host.kick(bid)?;
    assert!(a.current_session(bid).is_ok());
    let controls = host.take_online_control(aid)?;
    assert_eq!(controls.len(), 1);
    member.apply_control(&controls[0])?;
    assert!(a.current_session(bid).is_err());
    assert!(!member.members().contains(&bid));
    assert!(host.take_online_control(bid).is_err());
    Ok(())
}
