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
    encoding::{self, MailboxFrame},
    ids::{Epoch, FileId},
    keystore::KeyStore,
    protocol::manifest::Manifest,
    store::chunks::{shared_chunk_store, ChunkStore, SharedChunkStore},
    sync::host::{ControlUpdate, HostService, MemberReplica},
};

static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> quantam_fs::Result<Self> {
        let number = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("qfs-host-restart-{}-{number}", std::process::id()));
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
    chunks: &SharedChunkStore,
    plaintext: &[u8],
    version: u64,
) -> quantam_fs::Result<Manifest> {
    let chunk_id = chunks
        .lock()
        .map_err(|_| quantam_fs::Error::State("test chunk lock poisoned"))?
        .put(&file_id, 0, plaintext.to_vec())?;
    let mut manifest = Manifest {
        file_id,
        chunk_ids: vec![chunk_id],
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

#[test]
fn restart_rewraps_reseals_and_flushes_memory_host_state() -> quantam_fs::Result<()> {
    let directory = TestDir::new()?;
    let host_path = directory.identity("host");
    let offline_path = directory.identity("offline");
    let host_keys = KeyStore::open(&host_path)?;
    let offline_keys = KeyStore::open(&offline_path)?;
    connect(&host_keys, &offline_keys)?;

    let host_id = host_keys.peer_id()?;
    let offline_id = offline_keys.peer_id()?;
    let members = BTreeSet::from([host_id, offline_id]);
    let host_chunks = shared_chunk_store();
    let offline_chunks = shared_chunk_store();
    let mut offline = MemberReplica::new(
        offline_keys.clone(),
        host_id,
        members.clone(),
        offline_chunks.clone(),
    )?;
    let mut host = HostService::new(host_keys.clone(), members, host_chunks.clone())?;
    let file_id = FileId([73; 32]);

    host.commit(signed_manifest(
        &host_keys,
        file_id,
        &host_chunks,
        b"before delete",
        1,
    )?)?;
    host.link_file(host_id, "/file", file_id)?;
    host.fan_out_control(host_id, &ControlUpdate::Remove(file_id))?;
    let latest = b"after recreate";
    let latest_manifest = signed_manifest(&host_keys, file_id, &host_chunks, latest, 2)?;
    let latest_id = latest_manifest.chunk_ids[0];
    host.commit(latest_manifest)?;
    host.link_file(host_id, "/file", file_id)?;

    assert_eq!(host.instruction_log().len(), 5);
    let before_restart = host.mailbox(offline_id)?;
    assert_eq!(count_frames(&before_restart)?, (5, 1));
    assert!(before_restart.iter().all(|entry| entry.epoch == Epoch(1)));

    let state = host.into_state();
    drop(host_keys);
    let restarted_keys = KeyStore::open(Path::new(&host_path))?;
    assert!(restarted_keys.session(offline_id, Epoch(1)).is_err());
    let pending = restarted_keys.pending_wraps()?;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].epoch, Epoch(2));
    RustCryptoConstructionBWrap::new(offline_keys.clone()).unwrap(host_id, &pending[0])?;

    let mut restarted = HostService::resume(restarted_keys, state)?;
    let resealed = restarted.mailbox(offline_id)?;
    assert_eq!(count_frames(&resealed)?, (5, 1));
    assert!(resealed.iter().all(|entry| entry.epoch == Epoch(2)));
    let body = resealed
        .iter()
        .find(|entry| {
            matches!(
                encoding::decode_mailbox_frame(&entry.ciphertext),
                Ok(MailboxFrame::ChunkBody { .. })
            )
        })
        .ok_or(quantam_fs::Error::State("re-sealed body missing"))?;
    assert_eq!(body.seq.0, 1);

    let challenge = restarted.issue_flush_challenge(offline_id)?;
    let signature = RustCryptoPureMlDsa.sign(
        &offline_keys.signing_key()?,
        FLUSH_CONTEXT,
        &encoding::flush_m(&challenge),
    )?;
    let report = restarted.flush_mailbox(&mut offline, &challenge, &signature)?;
    assert_eq!(report.controls, 5);
    assert_eq!(report.chunks_written, 1);
    assert_eq!(offline.instruction_log().len(), 5);
    assert_eq!(
        offline_chunks
            .lock()
            .map_err(|_| quantam_fs::Error::State("test chunk lock poisoned"))?
            .get(&latest_id),
        Some(latest.as_slice())
    );
    Ok(())
}

fn count_frames(
    envelopes: &[quantam_fs::sync::host::MailboxEnvelope],
) -> quantam_fs::Result<(usize, usize)> {
    envelopes
        .iter()
        .try_fold(
            (0, 0),
            |(controls, bodies), envelope| match encoding::decode_mailbox_frame(
                &envelope.ciphertext,
            )? {
                MailboxFrame::Control { .. } => Ok((controls + 1, bodies)),
                MailboxFrame::ChunkBody { .. } => Ok((controls, bodies + 1)),
            },
        )
}
