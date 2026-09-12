use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use quantam_fs::{
    crypto::wrap::{ConstructionBWrap, RustCryptoConstructionBWrap},
    encoding,
    ids::{ChunkId, Epoch, FileId},
    keystore::KeyStore,
    protocol::{
        locate::{HaveQuery, HaveReply},
        pull::PullRequest,
    },
    store::chunks::{shared_chunk_store, ChunkStore, SharedChunkStore},
    sync::{
        host::{ControlUpdate, HostService, MemberReplica},
        locate::{answer_have, Locator},
        pull::InProcessPullCoordinator,
    },
    Error, Result,
};

static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Result<Self> {
        let number = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("qfs-locate-{}-{number}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn keys(&self, name: &str) -> Result<KeyStore> {
        KeyStore::open(&self.0.join(name))
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
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

fn copy_plaintext(chunks: &SharedChunkStore, file_id: FileId, bodies: &[Vec<u8>]) -> Result<()> {
    let mut chunks = chunks
        .lock()
        .map_err(|_| Error::State("test chunk lock poisoned"))?;
    for (index, body) in bodies.iter().enumerate() {
        chunks.put(&file_id, index as u64, body.clone())?;
    }
    Ok(())
}

#[test]
fn live_host_is_first_then_member_fallback_serves_the_trusted_manifest() -> Result<()> {
    let directory = TestDir::new()?;
    let host_keys = directory.keys("host")?;
    let requester_keys = directory.keys("requester")?;
    let holder_keys = directory.keys("holder")?;
    connect(&host_keys, &requester_keys)?;
    connect(&host_keys, &holder_keys)?;
    connect(&holder_keys, &requester_keys)?;
    let host_id = host_keys.peer_id()?;
    let requester_id = requester_keys.peer_id()?;
    let holder_id = holder_keys.peer_id()?;
    let members = BTreeSet::from([host_id, requester_id, holder_id]);
    let host_chunks = shared_chunk_store();
    let requester_chunks = shared_chunk_store();
    let holder_chunks = shared_chunk_store();
    let mut host = HostService::new(host_keys.clone(), members.clone(), host_chunks.clone())?;
    let mut requester = MemberReplica::new(
        requester_keys.clone(),
        host_id,
        members.clone(),
        requester_chunks.clone(),
    )?;
    host.heartbeat(requester_id, Duration::from_secs(60))?;
    let bodies = vec![b"from H".to_vec(), b"from nearby holder".to_vec()];
    let file_id = host.save_file(&host_keys, "/file", &bodies)?;
    for packet in host.take_online_control(requester_id)? {
        requester.apply_control(&packet)?;
    }
    let trusted = requester
        .trusted_manifest(&file_id)
        .ok_or(Error::State("trusted manifest missing"))?
        .clone();
    let chunk_ids = trusted.manifest().chunk_ids.clone();
    copy_plaintext(&holder_chunks, file_id, &bodies)?;

    let locator = Locator::new(requester_keys.clone(), members.clone(), host_id)?;
    let query = HaveQuery::new(file_id, vec![chunk_ids[0]])?;
    let mut queried = false;
    assert_eq!(
        locator.holders(&query, |_, _| {
            queried = true;
            Err(Error::State("H should not require a have query"))
        })?,
        vec![host_id]
    );
    assert!(!queried);
    let host_pull = InProcessPullCoordinator::new(host_keys, host_chunks);
    let requester_pull =
        InProcessPullCoordinator::new(requester_keys.clone(), requester_chunks.clone());
    let response = host_pull.serve(&PullRequest::new(vec![chunk_ids[0]])?, requester_id)?;
    assert_eq!(requester_pull.accept(&response, &trusted)?, 1);

    requester_keys.discard_pair(host_id)?;
    let fallback_query = HaveQuery::new(file_id, vec![chunk_ids[1]])?;
    let holders = locator.holders(&fallback_query, |peer, query| {
        if peer != holder_id {
            return Err(Error::State("unexpected holder query"));
        }
        let chunks = holder_chunks
            .lock()
            .map_err(|_| Error::State("test chunk lock poisoned"))?;
        answer_have(query, &*chunks)
    })?;
    assert_eq!(holders, vec![holder_id]);
    let holder_pull = InProcessPullCoordinator::new(holder_keys.clone(), holder_chunks.clone());
    let response = holder_pull.serve(&PullRequest::new(vec![chunk_ids[1]])?, requester_id)?;
    assert_eq!(requester_pull.accept(&response, &trusted)?, 1);
    assert_eq!(
        requester_chunks
            .lock()
            .map_err(|_| Error::State("test chunk lock poisoned"))?
            .get(&chunk_ids[1]),
        Some(bodies[1].as_slice())
    );

    let older_query = HaveQuery::new(
        file_id,
        vec![chunk_ids[0], ChunkId([0xee; 32]), chunk_ids[1]],
    )?;
    let older_reply = {
        let chunks = holder_chunks
            .lock()
            .map_err(|_| Error::State("test chunk lock poisoned"))?;
        answer_have(&older_query, &*chunks)?
    };
    assert_eq!(older_reply.have_bitset, vec![0b0000_0101]);
    assert_eq!(older_reply.chunk_ids, older_query.chunk_ids());
    assert_eq!(trusted.manifest().chunk_ids, chunk_ids);

    let mut wrong_ids = HaveReply::new(&fallback_query, vec![1])?;
    wrong_ids.chunk_ids[0] = ChunkId([0xdd; 32]);
    assert!(wrong_ids.validate(&fallback_query).is_err());
    assert!(locator
        .holders(&fallback_query, |_, _| Ok(wrong_ids.clone()))?
        .is_empty());
    let mut wrong_bits = HaveReply::new(&fallback_query, vec![1])?;
    wrong_bits.have_bitset.push(0);
    assert!(wrong_bits.validate(&fallback_query).is_err());
    Ok(())
}

#[test]
fn closed_flush_gate_removes_member_from_candidates() -> Result<()> {
    let directory = TestDir::new()?;
    let local_keys = directory.keys("local")?;
    let member_keys = directory.keys("member")?;
    connect(&local_keys, &member_keys)?;
    let local = local_keys.peer_id()?;
    let member = member_keys.peer_id()?;
    let members = BTreeSet::from([local, member]);
    let locator = Locator::new(local_keys.clone(), members.clone(), local)?;
    assert!(locator.session_is_live(member));

    let mut gate_owner = HostService::new(local_keys, members, shared_chunk_store())?;
    gate_owner.fan_out_control(local, &ControlUpdate::Add(FileId([0x71; 32])))?;
    assert!(!locator.session_is_live(member));
    assert!(locator.live_member_candidates().is_empty());
    let query = HaveQuery::new(FileId([0x72; 32]), vec![ChunkId([0x73; 32])])?;
    assert!(locator
        .holders(&query, |_, _| Err(Error::State(
            "must not query gated peer"
        )))?
        .is_empty());
    Ok(())
}

#[test]
fn have_codecs_reject_mismatched_counts_ids_and_bitset_lengths() -> Result<()> {
    let query = HaveQuery::new(
        FileId([0x81; 32]),
        vec![ChunkId([0x82; 32]), ChunkId([0x83; 32])],
    )?;
    let encoded = encoding::encode_have_query(&query)?;
    assert_eq!(encoding::decode_have_query(&encoded)?, query);
    let reply = HaveReply::new(&query, vec![0b10])?;
    let encoded = encoding::encode_have_reply(&reply)?;
    assert_eq!(encoding::decode_have_reply(&encoded)?, reply);

    let mut wrong_count = encoded.clone();
    let count_offset = quantam_fs::protocol::locate::HAVE_MAGIC.len() + 1 + 32;
    wrong_count[count_offset..count_offset + 4].copy_from_slice(&3u32.to_be_bytes());
    assert!(encoding::decode_have_reply(&wrong_count).is_err());
    let mut wrong_ids = reply.clone();
    wrong_ids.chunk_ids.swap(0, 1);
    assert!(wrong_ids.validate(&query).is_err());
    let mut wrong_bits = reply;
    wrong_bits.have_bitset.push(0);
    assert!(wrong_bits.validate(&query).is_err());
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn async_locate_returns_known_holders_at_the_overall_deadline() -> Result<()> {
    let directory = TestDir::new()?;
    let local_keys = directory.keys("async-local")?;
    let a_keys = directory.keys("async-a")?;
    let b_keys = directory.keys("async-b")?;
    connect(&local_keys, &a_keys)?;
    connect(&local_keys, &b_keys)?;
    let local = local_keys.peer_id()?;
    let a = a_keys.peer_id()?;
    let b = b_keys.peer_id()?;
    let locator = Locator::new(local_keys, BTreeSet::from([local, a, b]), local)?;
    let query = HaveQuery::new(FileId([0x91; 32]), vec![ChunkId([0x92; 32])])?;
    let slow = a.min(b);
    let responsive = a.max(b);
    let started = tokio::time::Instant::now();
    let holders = locator
        .holders_async_with_timeout(
            &query,
            Duration::from_millis(20),
            |peer, query| async move {
                if peer == slow {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                HaveReply::new(&query, vec![1])
            },
        )
        .await?;
    assert_eq!(holders, vec![responsive]);
    assert!(started.elapsed() < Duration::from_millis(500));
    Ok(())
}
