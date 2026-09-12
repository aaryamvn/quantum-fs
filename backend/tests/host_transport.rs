use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
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
    protocol::{manifest::Manifest, pull::PullRequest},
    store::chunks::{shared_chunk_store, ChunkStore, SharedChunkStore},
    sync::{
        host::{HostService, MemberReplica},
        pull::InProcessPullCoordinator,
    },
};

static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> quantam_fs::Result<Self> {
        let number = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "qfs-host-transport-{}-{number}",
            std::process::id()
        ));
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
    chunks: &SharedChunkStore,
    file_id: FileId,
    plaintext: &[u8],
    version: u64,
) -> quantam_fs::Result<Manifest> {
    let id = chunks
        .lock()
        .map_err(|_| quantam_fs::Error::State("test chunk lock poisoned"))?
        .put(&file_id, 0, plaintext.to_vec())?;
    let mut manifest = Manifest {
        file_id,
        chunk_ids: vec![id],
        size: plaintext.len() as u64,
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

fn flush_signature(
    keys: &KeyStore,
    challenge: &quantam_fs::sync::host::FlushChallenge,
) -> quantam_fs::Result<Vec<u8>> {
    RustCryptoPureMlDsa.sign(
        &keys.signing_key()?,
        FLUSH_CONTEXT,
        &encoding::flush_m(challenge),
    )
}

#[test]
fn transport_flush_is_staged_and_only_exact_acknowledgement_clears_queue() -> quantam_fs::Result<()>
{
    let directory = TestDir::new()?;
    let host_keys = directory.keys("host")?;
    let member_keys = directory.keys("member")?;
    connect(&host_keys, &member_keys)?;
    let host_id = host_keys.peer_id()?;
    let member_id = member_keys.peer_id()?;
    let host_chunks = shared_chunk_store();
    let member_chunks = shared_chunk_store();
    let mut host = HostService::new(
        host_keys.clone(),
        BTreeSet::from([host_id]),
        host_chunks.clone(),
    )?;
    host.add_member(member_keys.identity()?)?;
    assert!(host.has_member(&member_id));
    assert_eq!(host.members(), &BTreeSet::from([host_id, member_id]));
    let mut replica = MemberReplica::new(
        member_keys.clone(),
        host_id,
        BTreeSet::from([host_id, member_id]),
        member_chunks.clone(),
    )?;
    assert_eq!(replica.host_id(), host_id);
    assert_eq!(replica.peer_id()?, member_id);

    let file_id = FileId([71; 32]);
    host.commit(signed_manifest(
        &host_keys,
        &host_chunks,
        file_id,
        b"first",
        1,
    )?)?;
    host.link_file(host_id, "/file", file_id)?;
    let challenge = host.issue_flush_challenge(member_id)?;
    let signature = flush_signature(&member_keys, &challenge)?;
    let first_host = host.prepare_flush(member_id, &challenge, &signature)?;
    let first_replica = replica.prepare_mailbox(first_host.envelopes())?;
    assert!(member_chunks
        .lock()
        .map_err(|_| quantam_fs::Error::State("test chunk lock poisoned"))?
        .is_empty());
    let first_report = replica.commit_prepared(first_replica)?;
    assert_eq!(first_report.controls, 2);
    assert_eq!(first_report.chunks_written, 1);

    host.commit(signed_manifest(
        &host_keys,
        &host_chunks,
        file_id,
        b"second",
        2,
    )?)?;
    assert!(host.acknowledge_flush(member_id, first_host).is_err());
    assert!(!host.mailbox(member_id)?.is_empty());
    let pull = InProcessPullCoordinator::new(host_keys, host_chunks);
    assert!(pull
        .serve(&PullRequest::new(Vec::new())?, member_id)
        .is_err());

    let retry_host = host.prepare_flush(member_id, &challenge, &signature)?;
    let retry_replica = replica.prepare_mailbox(retry_host.envelopes())?;
    let retry_report = replica.commit_prepared(retry_replica)?;
    assert_eq!(retry_report.controls, 1);
    assert_eq!(retry_report.chunks_written, 1);
    replica.finish_receipts();
    host.acknowledge_flush(member_id, retry_host)?;
    assert!(host.mailbox(member_id)?.is_empty());
    assert!(pull
        .serve(&PullRequest::new(Vec::new())?, member_id)
        .is_ok());
    Ok(())
}

#[test]
fn stopped_host_rejects_membership_admission() -> quantam_fs::Result<()> {
    let directory = TestDir::new()?;
    let host_keys = directory.keys("host")?;
    let candidate = directory.keys("candidate")?;
    let mut host = HostService::new(
        host_keys.clone(),
        BTreeSet::from([host_keys.peer_id()?]),
        shared_chunk_store(),
    )?;
    host.stop();
    assert!(host.add_member(candidate.identity()?).is_err());
    assert!(!host.has_member(&candidate.peer_id()?));
    Ok(())
}
