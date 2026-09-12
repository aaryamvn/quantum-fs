use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use quantam_fs::{
    crypto::{
        sign::{PureMlDsa, RustCryptoPureMlDsa, FLUSH_CONTEXT},
        wrap::{ConstructionBWrap, RustCryptoConstructionBWrap},
    },
    encoding,
    ids::{Epoch, FileId},
    keystore::KeyStore,
    protocol::{locate::HaveQuery, pull::PullRequest},
    store::chunks::{shared_chunk_store, ChunkStore},
    sync::{
        host::{HostService, MemberReplica, QueueContent},
        locate::{answer_have, Locator},
        pull::InProcessPullCoordinator,
    },
    Error, Result,
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "qfs-eviction-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
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

fn keys(path: &Path) -> Result<KeyStore> {
    KeyStore::open(&path.join("identity"))
}

fn connect(a: &KeyStore, b: &KeyStore) -> Result<()> {
    let a_identity = a.identity()?;
    let b_identity = b.identity()?;
    a.import_peer(b_identity.clone())?;
    b.import_peer(a_identity.clone())?;
    let (sender, receiver, receiver_identity) = if a_identity.peer_id < b_identity.peer_id {
        (a, b, b_identity)
    } else {
        (b, a, a_identity)
    };
    let (_, wrap) = RustCryptoConstructionBWrap::new(sender.clone()).create(
        receiver_identity.peer_id,
        &receiver_identity.ek,
        Epoch(1),
    )?;
    RustCryptoConstructionBWrap::new(receiver.clone()).unwrap(sender.peer_id()?, &wrap)?;
    Ok(())
}

fn flush(host: &mut HostService, member: &mut MemberReplica, keys: &KeyStore) -> Result<()> {
    let challenge = host.issue_flush_challenge(keys.peer_id()?)?;
    let signature = RustCryptoPureMlDsa.sign(
        &keys.signing_key()?,
        FLUSH_CONTEXT,
        &encoding::flush_m(&challenge),
    )?;
    host.flush_mailbox(member, &challenge, &signature)?;
    Ok(())
}

struct Fixture {
    member_dir: PathBuf,
    host_keys: KeyStore,
    member_keys: KeyStore,
    host: HostService,
    member: MemberReplica,
    file_id: FileId,
    bodies: Vec<Vec<u8>>,
}

fn populated(directory: &TestDir) -> Result<Fixture> {
    let host_dir = directory.child("host")?;
    let member_dir = directory.child("member")?;
    let host_keys = keys(&host_dir)?;
    let member_keys = keys(&member_dir)?;
    connect(&host_keys, &member_keys)?;
    let host_id = host_keys.peer_id()?;
    let member_id = member_keys.peer_id()?;
    let members = BTreeSet::from([host_id, member_id]);
    let root = FileId([0x71; 32]);
    let mut host = HostService::open_durable(host_keys.clone(), &host_dir, root, members.clone())?;
    let mut member =
        MemberReplica::open_durable(member_keys.clone(), &member_dir, root, host_id, members)?;
    let bodies = vec![b"evict one".to_vec(), b"evict two".to_vec()];
    let file_id = host.save_file(&host_keys, "/cached", &bodies)?;
    flush(&mut host, &mut member, &member_keys)?;
    Ok(Fixture {
        member_dir,
        host_keys,
        member_keys,
        host,
        member,
        file_id,
        bodies,
    })
}

#[test]
fn member_evicts_cache_but_keeps_namespace_and_accepts_inflight_host_response() -> Result<()> {
    let directory = TestDir::new()?;
    let Fixture {
        member_dir,
        host_keys,
        member_keys,
        mut host,
        mut member,
        file_id,
        bodies,
    } = populated(&directory)?;
    let host_id = host_keys.peer_id()?;
    let member_id = member_keys.peer_id()?;
    let trusted = member
        .trusted_manifest(&file_id)
        .ok_or(Error::State("member manifest missing"))?
        .clone();
    let ids = trusted.manifest().chunk_ids.clone();
    let request = PullRequest::new(ids.clone())?;
    let host_pull = InProcessPullCoordinator::new(host_keys.clone(), host.chunks());
    let encrypted_before_eviction = host_pull.serve(&request, member_id)?;

    assert_eq!(
        host.evict_file(file_id).unwrap_err().to_string(),
        "invalid state: H cannot evict its availability copy"
    );
    {
        let host_chunks = host.chunks();
        let chunks = host_chunks
            .lock()
            .map_err(|_| Error::State("host chunk lock poisoned"))?;
        assert_eq!(chunks.get(&ids[0]), Some(bodies[0].as_slice()));
        assert_eq!(chunks.get(&ids[1]), Some(bodies[1].as_slice()));
    }
    assert_eq!(member.evict_file(file_id)?, ids.len());
    assert_eq!(member.tree().resolve("/cached")?, file_id);
    assert!(member.trusted_manifest(&file_id).is_some());
    let member_chunks = member.chunks();
    {
        let chunks = member_chunks
            .lock()
            .map_err(|_| Error::State("member chunk lock poisoned"))?;
        assert_eq!(chunks.have_bitset(&ids), vec![0]);
        assert!(ids.iter().all(|id| !chunks.has(id)));
    }
    assert_eq!(fs::read_dir(member_dir.join("chunks"))?.count(), 0);

    let candidate_dir = directory.child("candidate")?;
    let candidate = keys(&candidate_dir)?;
    let candidate_id = candidate.peer_id()?;
    connect(&candidate, &member_keys)?;
    let locator = Locator::new(
        candidate,
        BTreeSet::from([host_id, member_id, candidate_id]),
        host_id,
    )?;
    let query = HaveQuery::new(file_id, ids.clone())?;
    let holders = locator.holders(&query, |peer, query| {
        if peer != member_id {
            return Err(Error::AuthenticationFailed);
        }
        let chunks = member_chunks
            .lock()
            .map_err(|_| Error::State("member chunk lock poisoned"))?;
        answer_have(query, &*chunks)
    })?;
    assert!(holders.is_empty());

    let member_pull = InProcessPullCoordinator::new(member_keys, member_chunks.clone());
    assert_eq!(
        member_pull.accept(&encrypted_before_eviction, &trusted)?,
        ids.len()
    );
    let chunks = member_chunks
        .lock()
        .map_err(|_| Error::State("member chunk lock poisoned"))?;
    assert_eq!(chunks.get(&ids[0]), Some(bodies[0].as_slice()));
    assert_eq!(chunks.get(&ids[1]), Some(bodies[1].as_slice()));
    Ok(())
}

#[test]
fn member_refuses_eviction_when_host_session_is_gone_without_changing_bits() -> Result<()> {
    let directory = TestDir::new()?;
    let Fixture {
        host_keys,
        member_keys,
        mut member,
        file_id,
        ..
    } = populated(&directory)?;
    let ids = member
        .trusted_manifest(&file_id)
        .ok_or(Error::State("member manifest missing"))?
        .manifest()
        .chunk_ids
        .clone();
    let chunks = member.chunks();
    let before = chunks
        .lock()
        .map_err(|_| Error::State("member chunk lock poisoned"))?
        .have_bitset(&ids);
    member_keys.discard_pair(host_keys.peer_id()?)?;
    assert!(member.evict_file(file_id).is_err());
    let after = chunks
        .lock()
        .map_err(|_| Error::State("member chunk lock poisoned"))?
        .have_bitset(&ids);
    assert_eq!(after, before);
    Ok(())
}

#[test]
fn member_refuses_eviction_while_mailbox_metadata_pins_a_chunk() -> Result<()> {
    let directory = TestDir::new()?;
    let Fixture {
        member_keys,
        mut member,
        file_id,
        ..
    } = populated(&directory)?;
    let chunk_id = member
        .trusted_manifest(&file_id)
        .ok_or(Error::State("member manifest missing"))?
        .manifest()
        .chunk_ids[0];
    let chunks = member.chunks();
    {
        let mut store = chunks
            .lock()
            .map_err(|_| Error::State("member chunk lock poisoned"))?;
        let mut metadata = store
            .metadata()
            .ok_or(Error::State("member metadata missing"))?;
        metadata
            .mailboxes
            .entry(member_keys.peer_id()?)
            .or_default()
            .push(QueueContent::Chunk {
                file_id,
                index: 0,
                chunk_id,
            });
        store.persist_metadata(metadata)?;
    }
    assert!(member.evict_file(file_id).is_err());
    assert!(chunks
        .lock()
        .map_err(|_| Error::State("member chunk lock poisoned"))?
        .has(&chunk_id));
    Ok(())
}

#[test]
fn empty_stale_holder_falls_back_to_host_with_encrypted_requests() -> Result<()> {
    let directory = TestDir::new()?;
    let Fixture {
        host_keys,
        member_keys,
        host,
        mut member,
        file_id,
        bodies,
        ..
    } = populated(&directory)?;
    let stale_dir = directory.child("stale-holder")?;
    let stale_keys = keys(&stale_dir)?;
    connect(&member_keys, &stale_keys)?;
    let stale_id = stale_keys.peer_id()?;
    let host_id = host_keys.peer_id()?;
    let trusted = member
        .trusted_manifest(&file_id)
        .ok_or(Error::State("member manifest missing"))?
        .clone();
    let ids = trusted.manifest().chunk_ids.clone();
    let request = PullRequest::new(ids.clone())?;
    assert_eq!(member.evict_file(file_id)?, ids.len());

    let requester = InProcessPullCoordinator::new(member_keys, member.chunks());
    let stale = InProcessPullCoordinator::new(stale_keys, shared_chunk_store());
    let available = InProcessPullCoordinator::new(host_keys, host.chunks());
    let report = requester.pull_from_holders(
        &request,
        &trusted,
        &[stale_id, host_id],
        |holder, packet| {
            if holder == stale_id {
                stale.serve_packet(packet)
            } else if holder == host_id {
                available.serve_packet(packet)
            } else {
                Err(Error::AuthenticationFailed)
            }
        },
    )?;
    assert_eq!(report.holders_tried, 2);
    assert_eq!(report.chunks_written, ids.len());
    assert!(report.missing.is_empty());
    let chunks = member.chunks();
    let chunks = chunks
        .lock()
        .map_err(|_| Error::State("member chunk lock poisoned"))?;
    assert_eq!(chunks.get(&ids[0]), Some(bodies[0].as_slice()));
    assert_eq!(chunks.get(&ids[1]), Some(bodies[1].as_slice()));
    Ok(())
}
