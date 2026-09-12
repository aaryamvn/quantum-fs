use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use quantam_fs::{
    crypto::{
        sign::{PureMlDsa, RustCryptoPureMlDsa, MANIFEST_CONTEXT},
        wrap::{ConstructionBWrap, RustCryptoConstructionBWrap},
    },
    demo_log, encoding,
    ids::{Epoch, FileId},
    keystore::KeyStore,
    protocol::{manifest::Manifest, pull::PullRequest},
    store::chunks::{shared_chunk_store, ChunkStore},
    sync::{
        host::{HostService, MemberReplica},
        pull::{encrypt_at_send, InProcessPullCoordinator},
    },
};

#[test]
fn demo_log_groups_two_real_holders_and_never_completes_a_partial_pull() -> quantam_fs::Result<()> {
    let directory = TestDir::new()?;
    let host_keys = directory.keys("host")?;
    let requester_keys = directory.keys("requester")?;
    let first_holder_keys = directory.keys("first-holder")?;
    let second_holder_keys = directory.keys("second-holder")?;
    connect(&host_keys, &requester_keys)?;
    connect(&first_holder_keys, &requester_keys)?;
    connect(&second_holder_keys, &requester_keys)?;

    let host_id = host_keys.peer_id()?;
    let requester_id = requester_keys.peer_id()?;
    let members = BTreeSet::from([host_id, requester_id]);
    let file_id = FileId([0x91; 32]);
    let first_body = b"piece supplied by holder one".to_vec();
    let second_body = b"piece supplied by holder two".to_vec();
    let host_chunks = shared_chunk_store();
    let (first_id, second_id) = {
        let mut chunks = host_chunks.lock().unwrap();
        (
            chunks.put(&file_id, 0, first_body.clone())?,
            chunks.put(&file_id, 1, second_body.clone())?,
        )
    };
    let first_chunks = shared_chunk_store();
    first_chunks
        .lock()
        .unwrap()
        .put(&file_id, 0, first_body.clone())?;
    let second_chunks = shared_chunk_store();
    second_chunks
        .lock()
        .unwrap()
        .put(&file_id, 1, second_body.clone())?;

    let mut host = HostService::new(host_keys.clone(), members.clone(), host_chunks)?;
    host.heartbeat(requester_id, Duration::from_secs(60))?;
    host.commit(signed_manifest(
        &host_keys,
        file_id,
        vec![first_id, second_id],
        (first_body.len() + second_body.len()) as u64,
    )?)?;
    host.link_file(host_id, "/two-source-file", file_id)?;
    let requester_chunks = shared_chunk_store();
    let mut requester = MemberReplica::new(
        requester_keys.clone(),
        host_id,
        members,
        requester_chunks.clone(),
    )?;
    for control in host.take_online_control(requester_id)? {
        requester.apply_control(&control)?;
    }
    let trusted = requester
        .trusted_manifest(&file_id)
        .expect("host installed trusted manifest")
        .clone();

    let log_path = directory.0.join("demo-events.log");
    demo_log::start("pull transfer integration test");
    demo_log::set_log_file(&log_path)?;
    let pull = InProcessPullCoordinator::new(requester_keys, requester_chunks);
    let first_holder = InProcessPullCoordinator::new(first_holder_keys.clone(), first_chunks);
    let second_holder = InProcessPullCoordinator::new(second_holder_keys.clone(), second_chunks);

    let first_request = PullRequest::new(vec![first_id])?;
    let first_responses = first_holder.serve(&first_request, requester_id)?;
    assert_eq!(pull.accept(&first_responses, &trusted)?, 1);
    demo_log::flush();
    let partial_log = fs::read_to_string(&log_path)?;
    assert!(partial_log.contains("File transfer in progress"));
    assert!(partial_log.contains("1 / 2 pieces local"));
    assert!(!partial_log.contains("File ready locally"));

    // A fresh authenticated transmission of an already persisted piece is
    // idempotent and must not become another source contribution.
    let duplicate = first_holder.serve(&first_request, requester_id)?;
    assert_eq!(pull.accept(&duplicate, &trusted)?, 0);
    let second_responses =
        second_holder.serve(&PullRequest::new(vec![second_id])?, requester_id)?;
    assert_eq!(pull.accept(&second_responses, &trusted)?, 1);
    demo_log::flush();

    let complete_log = fs::read_to_string(&log_path)?;
    assert_eq!(complete_log.matches("File ready locally").count(), 1);
    let complete = complete_log
        .split("File ready locally")
        .nth(1)
        .expect("complete transfer block");
    assert!(complete.contains("2 / 2 pieces local | 2 contributing peers"));
    assert!(complete.contains(&format!(
        "{} -> 1 verified pieces",
        demo_log::peer(first_holder_keys.peer_id()?)
    )));
    assert!(complete.contains(&format!(
        "{} -> 1 verified pieces",
        demo_log::peer(second_holder_keys.peer_id()?)
    )));
    assert_eq!(complete.matches("verified pieces").count(), 2);
    Ok(())
}

static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> quantam_fs::Result<Self> {
        let number = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("qfs-pull-transfer-{}-{number}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn keys(&self, name: &str) -> quantam_fs::Result<KeyStore> {
        KeyStore::open(&self.0.join(name))
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn connect(a: &KeyStore, b: &KeyStore) -> quantam_fs::Result<()> {
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
    chunk_ids: Vec<quantam_fs::ids::ChunkId>,
    size: u64,
) -> quantam_fs::Result<Manifest> {
    let mut manifest = Manifest {
        file_id,
        chunk_ids,
        size,
        writer_id: writer.peer_id()?,
        version: 1,
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
fn pull_uses_host_trust_and_encrypts_the_same_chunk_differently_per_recipient(
) -> quantam_fs::Result<()> {
    let directory = TestDir::new()?;
    let host_keys = directory.keys("host")?;
    let holder_keys = directory.keys("holder")?;
    let requester_keys = directory.keys("requester")?;
    let other_keys = directory.keys("other")?;
    connect(&host_keys, &requester_keys)?;
    connect(&holder_keys, &requester_keys)?;
    connect(&holder_keys, &other_keys)?;

    let host_id = host_keys.peer_id()?;
    let requester_id = requester_keys.peer_id()?;
    let members = BTreeSet::from([host_id, requester_id]);
    let host_chunks = shared_chunk_store();
    let requester_chunks = shared_chunk_store();
    let holder_chunks = shared_chunk_store();
    let file_id = FileId([9; 32]);
    let plaintext = b"same plaintext for each pair".to_vec();
    let host_plaintext = b"second holder plaintext".to_vec();
    let (chunk_id, host_chunk_id) = {
        let mut chunks = host_chunks.lock().unwrap();
        (
            chunks.put(&file_id, 0, plaintext.clone())?,
            chunks.put(&file_id, 1, host_plaintext.clone())?,
        )
    };
    holder_chunks
        .lock()
        .unwrap()
        .put(&file_id, 0, plaintext.clone())?;

    let mut host = HostService::new(host_keys.clone(), members.clone(), host_chunks.clone())?;
    host.heartbeat(requester_id, Duration::from_secs(60))?;
    host.commit(signed_manifest(
        &host_keys,
        file_id,
        vec![chunk_id, host_chunk_id],
        (plaintext.len() + host_plaintext.len()) as u64,
    )?)?;
    host.link_file(host_id, "/file", file_id)?;
    let mut requester = MemberReplica::new(
        requester_keys.clone(),
        host_id,
        members,
        requester_chunks.clone(),
    )?;
    for control in host.take_online_control(requester_id)? {
        requester.apply_control(&control)?;
    }
    let trusted = requester.trusted_manifest(&file_id).unwrap().clone();

    let holder = InProcessPullCoordinator::new(holder_keys.clone(), holder_chunks);
    let responses = holder.serve(&PullRequest::new(vec![chunk_id])?, requester_id)?;
    let requester_pull = InProcessPullCoordinator::new(requester_keys.clone(), requester_chunks);
    assert_eq!(requester_pull.accept(&responses, &trusted)?, 1);

    let host_holder = InProcessPullCoordinator::new(host_keys.clone(), host_chunks);
    let missing = encoding::chunk_id(&file_id, 99, b"missing");
    let request = PullRequest::new(vec![missing, host_chunk_id])?;
    let packet = requester_pull.encrypt_request(host_id, &request)?;
    let host_responses = host_holder.serve_packet(&packet)?;
    assert_eq!(host_responses.len(), 1);
    assert_eq!(requester_pull.accept(&host_responses, &trusted)?, 1);

    let repeated_packet = requester_pull.encrypt_request(host_id, &request)?;
    let repeated = host_holder.serve_packet(&repeated_packet)?;
    assert_eq!(requester_pull.accept(&repeated, &trusted)?, 0);
    assert_eq!(
        requester.chunks().lock().unwrap().get(&chunk_id),
        Some(plaintext.as_slice())
    );
    assert_eq!(
        requester.chunks().lock().unwrap().get(&host_chunk_id),
        Some(host_plaintext.as_slice())
    );

    let to_requester = encrypt_at_send(
        &holder_keys,
        &holder_keys.current_session(requester_id)?,
        file_id,
        0,
        &plaintext,
    )?;
    let to_other = encrypt_at_send(
        &holder_keys,
        &holder_keys.current_session(other_keys.peer_id()?)?,
        file_id,
        0,
        &plaintext,
    )?;
    assert_ne!(to_requester.ciphertext, to_other.ciphertext);
    Ok(())
}

#[test]
fn tampered_body_causes_no_store_write() -> quantam_fs::Result<()> {
    let directory = TestDir::new()?;
    let sender = directory.keys("sender")?;
    let receiver = directory.keys("receiver")?;
    connect(&sender, &receiver)?;
    let file_id = FileId([4; 32]);
    let plaintext = b"authenticated chunk";
    let chunk_id = encoding::chunk_id(&file_id, 0, plaintext);
    let members = BTreeSet::from([sender.peer_id()?, receiver.peer_id()?]);
    let manifest = signed_manifest(&sender, file_id, vec![chunk_id], plaintext.len() as u64)?;

    // The recipient's control path is the only public source of trusted manifests.
    let host_chunks = shared_chunk_store();
    host_chunks
        .lock()
        .unwrap()
        .put(&file_id, 0, plaintext.to_vec())?;
    let mut host = HostService::new(sender.clone(), members.clone(), host_chunks)?;
    host.heartbeat(receiver.peer_id()?, Duration::from_secs(60))?;
    host.commit(manifest)?;
    host.link_file(sender.peer_id()?, "/file", file_id)?;
    let destination = shared_chunk_store();
    let mut replica = MemberReplica::new(
        receiver.clone(),
        sender.peer_id()?,
        members,
        destination.clone(),
    )?;
    for control in host.take_online_control(receiver.peer_id()?)? {
        replica.apply_control(&control)?;
    }
    let trusted = replica.trusted_manifest(&file_id).unwrap().clone();
    let mut frame = encrypt_at_send(
        &sender,
        &sender.current_session(receiver.peer_id()?)?,
        file_id,
        0,
        plaintext,
    )?;
    frame.ciphertext[0] ^= 1;
    let pull = InProcessPullCoordinator::new(receiver, destination.clone());
    assert!(pull
        .accept(
            &[quantam_fs::protocol::pull::PullResponse { body: frame }],
            &trusted
        )
        .is_err());
    assert!(destination.lock().unwrap().is_empty());
    Ok(())
}

#[test]
fn authenticated_wrong_index_and_plaintext_hash_cause_no_store_write() -> quantam_fs::Result<()> {
    let directory = TestDir::new()?;
    let sender = directory.keys("sender")?;
    let receiver = directory.keys("receiver")?;
    connect(&sender, &receiver)?;
    let file_id = FileId([5; 32]);
    let expected_plaintext = b"manifest plaintext";
    let expected_id = encoding::chunk_id(&file_id, 0, expected_plaintext);
    let members = BTreeSet::from([sender.peer_id()?, receiver.peer_id()?]);

    let host_chunks = shared_chunk_store();
    host_chunks
        .lock()
        .unwrap()
        .put(&file_id, 0, expected_plaintext.to_vec())?;
    let mut host = HostService::new(sender.clone(), members.clone(), host_chunks)?;
    host.heartbeat(receiver.peer_id()?, Duration::from_secs(60))?;
    host.commit(signed_manifest(
        &sender,
        file_id,
        vec![expected_id],
        expected_plaintext.len() as u64,
    )?)?;
    host.link_file(sender.peer_id()?, "/file", file_id)?;
    let destination = shared_chunk_store();
    let mut replica = MemberReplica::new(
        receiver.clone(),
        sender.peer_id()?,
        members,
        destination.clone(),
    )?;
    for control in host.take_online_control(receiver.peer_id()?)? {
        replica.apply_control(&control)?;
    }
    let trusted = replica.trusted_manifest(&file_id).unwrap().clone();
    let session = sender.current_session(receiver.peer_id()?)?;

    let wrong_index = encrypt_at_send(&sender, &session, file_id, 1, expected_plaintext)?;
    let pull = InProcessPullCoordinator::new(receiver.clone(), destination.clone());
    assert!(pull
        .accept(
            &[quantam_fs::protocol::pull::PullResponse { body: wrong_index }],
            &trusted,
        )
        .is_err());
    assert!(destination.lock().unwrap().is_empty());

    let wrong_plaintext = encrypt_at_send(&sender, &session, file_id, 0, b"other plaintext")?;
    assert!(pull
        .accept(
            &[quantam_fs::protocol::pull::PullResponse {
                body: wrong_plaintext,
            }],
            &trusted,
        )
        .is_err());
    assert!(destination.lock().unwrap().is_empty());
    Ok(())
}

#[test]
fn host_rejects_bad_signature_and_signed_nonmember_before_manifest_apply() -> quantam_fs::Result<()>
{
    let directory = TestDir::new()?;
    let host_keys = directory.keys("host")?;
    let member = directory.keys("member")?;
    let nonmember = directory.keys("nonmember")?;
    connect(&host_keys, &member)?;
    host_keys.import_peer(nonmember.identity()?)?;
    let members = BTreeSet::from([host_keys.peer_id()?, member.peer_id()?]);
    let chunks = shared_chunk_store();
    let mut host = HostService::new(host_keys.clone(), members, chunks)?;
    host.heartbeat(member.peer_id()?, Duration::from_secs(60))?;

    let bad_file = FileId([6; 32]);
    let bad_id = encoding::chunk_id(&bad_file, 0, b"bad signature body");
    let mut bad_signature = signed_manifest(
        &host_keys,
        bad_file,
        vec![bad_id],
        b"bad signature body".len() as u64,
    )?;
    bad_signature.signature[0] ^= 1;
    assert!(host.commit(bad_signature).is_err());
    assert!(host.trusted_manifest(&bad_file).is_none());
    assert!(host.take_online_control(member.peer_id()?)?.is_empty());

    let outsider_file = FileId([7; 32]);
    let outsider_id = encoding::chunk_id(&outsider_file, 0, b"outsider body");
    let outsider_manifest = signed_manifest(
        &nonmember,
        outsider_file,
        vec![outsider_id],
        b"outsider body".len() as u64,
    )?;
    assert!(host.commit(outsider_manifest).is_err());
    assert!(host.trusted_manifest(&outsider_file).is_none());
    assert!(host.take_online_control(member.peer_id()?)?.is_empty());
    assert!(host.chunks().lock().unwrap().is_empty());
    Ok(())
}

#[test]
fn pull_started_before_remove_cannot_resurrect_the_chunk() -> quantam_fs::Result<()> {
    let directory = TestDir::new()?;
    let host_keys = directory.keys("host")?;
    let member_keys = directory.keys("member")?;
    connect(&host_keys, &member_keys)?;
    let host_id = host_keys.peer_id()?;
    let member_id = member_keys.peer_id()?;
    let members = BTreeSet::from([host_id, member_id]);
    let host_chunks = shared_chunk_store();
    let member_chunks = shared_chunk_store();
    let file_id = FileId([42; 32]);
    let plaintext = b"removed while pull is in flight";
    let chunk_id = host_chunks
        .lock()
        .map_err(|_| quantam_fs::Error::State("test chunk lock poisoned"))?
        .put(&file_id, 0, plaintext.to_vec())?;

    let mut host = HostService::new(host_keys.clone(), members.clone(), host_chunks.clone())?;
    host.heartbeat(member_id, Duration::from_secs(60))?;
    host.commit(signed_manifest(
        &host_keys,
        file_id,
        vec![chunk_id],
        plaintext.len() as u64,
    )?)?;
    host.link_file(host_id, "/file", file_id)?;
    let mut member =
        MemberReplica::new(member_keys.clone(), host_id, members, member_chunks.clone())?;
    for control in host.take_online_control(member_id)? {
        member.apply_control(&control)?;
    }
    let stale_trust = member.trusted_manifest(&file_id).unwrap().clone();
    let holder = InProcessPullCoordinator::new(host_keys.clone(), host_chunks);
    let response = holder.serve(&PullRequest::new(vec![chunk_id])?, member_id)?;

    host.unlink(host_id, "/file")?;
    for control in host.take_online_control(member_id)? {
        member.apply_control(&control)?;
    }
    assert!(member.trusted_manifest(&file_id).is_none());

    let pull = InProcessPullCoordinator::new(member_keys, member_chunks.clone());
    assert_eq!(pull.accept(&response, &stale_trust)?, 0);
    assert!(member_chunks
        .lock()
        .map_err(|_| quantam_fs::Error::State("test chunk lock poisoned"))?
        .is_empty());
    Ok(())
}
