use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use quantam_fs::{
    crypto::{
        sign::{PureMlDsa, RustCryptoPureMlDsa, FLUSH_CONTEXT, MANIFEST_CONTEXT},
        wrap::{ConstructionBWrap, RustCryptoConstructionBWrap},
    },
    encoding::{self, MailboxFrame},
    ids::{Epoch, FileId},
    keystore::KeyStore,
    protocol::{
        manifest::Manifest,
        packet::{PacketHeader, PROTOCOL_VERSION},
        pull::{ChunkBodyFrame, PullRequest},
    },
    store::chunks::{shared_chunk_store, ChunkStore, SharedChunkStore},
    sync::{
        host::{HostService, MemberReplica},
        pull::{open_chunk, InProcessPullCoordinator},
    },
};

static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> quantam_fs::Result<Self> {
        let number = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("qfs-host-delivery-{}-{number}", std::process::id()));
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

fn rotate_from(sender: &KeyStore, receiver: &KeyStore) -> quantam_fs::Result<()> {
    let receiver_identity = receiver.identity()?;
    let epoch = sender.next_epoch(&receiver_identity.peer_id)?;
    let (_, message) = RustCryptoConstructionBWrap::new(sender.clone()).create(
        receiver_identity.peer_id,
        &receiver_identity.ek,
        epoch,
    )?;
    RustCryptoConstructionBWrap::new(receiver.clone()).unwrap(sender.peer_id()?, &message)?;
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
        .put(&file_id, 0, plaintext.to_vec());
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
fn host_coalesces_offline_bodies_reseals_and_flushes_before_live_pull() -> quantam_fs::Result<()> {
    let directory = TestDir::new()?;
    let host_keys = directory.keys("host")?;
    let online_keys = directory.keys("online")?;
    let offline_keys = directory.keys("offline")?;
    connect(&host_keys, &online_keys)?;
    connect(&host_keys, &offline_keys)?;

    let host_id = host_keys.peer_id()?;
    let online_id = online_keys.peer_id()?;
    let offline_id = offline_keys.peer_id()?;
    let members = BTreeSet::from([host_id, online_id, offline_id]);
    let host_chunks = shared_chunk_store();
    let online_chunks = shared_chunk_store();
    let offline_chunks = shared_chunk_store();
    let mut host = HostService::new(host_keys.clone(), members.clone(), host_chunks.clone())?;
    let mut online = MemberReplica::new(
        online_keys.clone(),
        host_id,
        members.clone(),
        online_chunks.clone(),
    )?;
    let mut offline = MemberReplica::new(
        offline_keys.clone(),
        host_id,
        members,
        offline_chunks.clone(),
    )?;
    host.heartbeat(online_id, Duration::from_secs(60))?;

    let file_id = FileId([31; 32]);
    for (version, plaintext) in [b"first".as_slice(), b"second", b"third and final"]
        .into_iter()
        .enumerate()
    {
        host.commit(signed_manifest(
            &host_keys,
            file_id,
            &host_chunks,
            plaintext,
            version as u64 + 1,
        )?)?;
    }

    assert_eq!(host.instruction_log().len(), 3);
    let queued = host.mailbox(offline_id)?;
    let (controls, bodies) = queued.iter().try_fold(
        (0, Vec::new()),
        |(controls, mut bodies), envelope| -> quantam_fs::Result<_> {
            match encoding::decode_mailbox_frame(&envelope.ciphertext)? {
                MailboxFrame::Control { .. } => Ok((controls + 1, bodies)),
                MailboxFrame::ChunkBody { .. } => {
                    bodies.push(envelope.clone());
                    Ok((controls, bodies))
                }
            }
        },
    )?;
    assert_eq!(controls, 3);
    assert_eq!(bodies.len(), 1);

    let online_controls = host.take_online_control(online_id)?;
    assert_eq!(online_controls.len(), 3);
    for packet in &online_controls {
        online.apply_control(packet)?;
    }
    assert!(online_chunks
        .lock()
        .map_err(|_| quantam_fs::Error::State("test chunk lock poisoned"))?
        .is_empty());
    let trusted_online = online
        .trusted_manifest(&file_id)
        .ok_or(quantam_fs::Error::State("online manifest missing"))?
        .clone();
    let latest_id = trusted_online.manifest().chunk_ids[0];
    let host_pull = InProcessPullCoordinator::new(host_keys.clone(), host_chunks.clone());
    let online_pull = InProcessPullCoordinator::new(online_keys, online_chunks.clone());
    let response = host_pull.serve(&PullRequest::new(vec![latest_id])?, online_id)?;
    assert_eq!(online_pull.accept(&response, &trusted_online)?, 1);

    assert!(host_pull
        .serve(&PullRequest::new(vec![latest_id])?, offline_id)
        .is_err());
    let old_session = offline_keys.current_session(host_id)?;
    rotate_from(&host_keys, &offline_keys)?;
    let resealed = host.mailbox(offline_id)?;
    let body = resealed
        .iter()
        .find(|envelope| {
            matches!(
                encoding::decode_mailbox_frame(&envelope.ciphertext),
                Ok(MailboxFrame::ChunkBody { .. })
            )
        })
        .ok_or(quantam_fs::Error::State("resealed body missing"))?;
    assert_eq!(body.epoch, Epoch(2));
    assert_eq!(body.seq.0, 1);
    assert_ne!(body.epoch, old_session.epoch);
    let MailboxFrame::ChunkBody {
        file_id: body_file,
        index: body_index,
        ciphertext,
    } = encoding::decode_mailbox_frame(&body.ciphertext)?
    else {
        return Err(quantam_fs::Error::State("resealed frame is not a body"));
    };
    let trusted_offline = host
        .trusted_manifest(&file_id)
        .ok_or(quantam_fs::Error::State("host manifest missing"))?;
    assert!(open_chunk(
        &offline_keys,
        &old_session,
        &ChunkBodyFrame {
            header: PacketHeader {
                version: PROTOCOL_VERSION,
                sender_id: body.sender_id,
                receiver_id: body.recipient_id,
                epoch: body.epoch,
                seq: body.seq,
            },
            file_id: body_file,
            index: body_index,
            ciphertext,
        },
        trusted_offline,
    )
    .is_err());

    let challenge = host.issue_flush_challenge(offline_id)?;
    assert!(host
        .flush_mailbox(&mut offline, &challenge, &[0; 32])
        .is_err());
    let signature = RustCryptoPureMlDsa.sign(
        &offline_keys.signing_key()?,
        FLUSH_CONTEXT,
        &encoding::flush_m(&challenge),
    )?;
    let report = host.flush_mailbox(&mut offline, &challenge, &signature)?;
    assert_eq!(report.controls, 3);
    assert_eq!(report.chunks_written, 1);
    assert_eq!(offline.instruction_log().len(), 3);
    assert_eq!(
        offline_chunks
            .lock()
            .map_err(|_| quantam_fs::Error::State("test chunk lock poisoned"))?
            .get(&latest_id),
        Some(b"third and final".as_slice())
    );

    let after_flush = host_pull.serve(&PullRequest::new(vec![latest_id])?, offline_id)?;
    assert_eq!(after_flush.len(), 1);
    Ok(())
}

#[test]
fn stopped_host_rejects_commits() -> quantam_fs::Result<()> {
    let directory = TestDir::new()?;
    let host_keys = directory.keys("host")?;
    let chunks = shared_chunk_store();
    let mut host = HostService::new(
        host_keys.clone(),
        BTreeSet::from([host_keys.peer_id()?]),
        chunks.clone(),
    )?;
    let manifest = signed_manifest(&host_keys, FileId([41; 32]), &chunks, b"saved", 1)?;
    host.stop();
    assert!(host.commit(manifest).is_err());
    Ok(())
}

#[test]
fn untaken_online_control_is_resealed_in_fifo_order_after_rotation_and_offline_commit(
) -> quantam_fs::Result<()> {
    let directory = TestDir::new()?;
    let host_keys = directory.keys("host")?;
    let member_keys = directory.keys("member")?;
    connect(&host_keys, &member_keys)?;
    let host_id = host_keys.peer_id()?;
    let member_id = member_keys.peer_id()?;
    let members = BTreeSet::from([host_id, member_id]);
    let host_chunks = shared_chunk_store();
    let member_chunks = shared_chunk_store();
    let mut host = HostService::new(host_keys.clone(), members.clone(), host_chunks.clone())?;
    let mut member =
        MemberReplica::new(member_keys.clone(), host_id, members, member_chunks.clone())?;
    let file_id = FileId([51; 32]);

    host.heartbeat(member_id, Duration::from_secs(60))?;
    host.commit(signed_manifest(
        &host_keys,
        file_id,
        &host_chunks,
        b"online version",
        1,
    )?)?;
    rotate_from(&host_keys, &member_keys)?;
    host.heartbeat(member_id, Duration::ZERO)?;
    host.commit(signed_manifest(
        &host_keys,
        file_id,
        &host_chunks,
        b"offline version",
        2,
    )?)?;

    let mailbox = host.mailbox(member_id)?;
    assert_eq!(mailbox.len(), 3);
    assert!(mailbox.iter().all(|envelope| envelope.epoch == Epoch(2)));
    let control_sequences: Vec<_> = mailbox
        .iter()
        .filter_map(|envelope| {
            matches!(
                encoding::decode_mailbox_frame(&envelope.ciphertext),
                Ok(MailboxFrame::Control { .. })
            )
            .then_some(envelope.seq.0)
        })
        .collect();
    assert_eq!(control_sequences, vec![1, 2]);

    let challenge = host.issue_flush_challenge(member_id)?;
    let signature = RustCryptoPureMlDsa.sign(
        &member_keys.signing_key()?,
        FLUSH_CONTEXT,
        &encoding::flush_m(&challenge),
    )?;
    let report = host.flush_mailbox(&mut member, &challenge, &signature)?;
    assert_eq!(report.controls, 2);
    assert_eq!(report.chunks_written, 1);
    assert_eq!(member.instruction_log().len(), 2);
    let latest = member
        .trusted_manifest(&file_id)
        .ok_or(quantam_fs::Error::State("member manifest missing"))?
        .manifest()
        .chunk_ids[0];
    assert_eq!(
        member_chunks
            .lock()
            .map_err(|_| quantam_fs::Error::State("test chunk lock poisoned"))?
            .get(&latest),
        Some(b"offline version".as_slice())
    );
    Ok(())
}

#[test]
fn fifo_flush_accepts_more_chunk_bodies_than_the_replay_window() -> quantam_fs::Result<()> {
    const CHUNK_COUNT: usize = 1_030;

    let directory = TestDir::new()?;
    let host_keys = directory.keys("host")?;
    let member_keys = directory.keys("member")?;
    connect(&host_keys, &member_keys)?;
    let host_id = host_keys.peer_id()?;
    let member_id = member_keys.peer_id()?;
    let members = BTreeSet::from([host_id, member_id]);
    let host_chunks = shared_chunk_store();
    let member_chunks = shared_chunk_store();
    let file_id = FileId([61; 32]);
    let mut chunk_ids = Vec::with_capacity(CHUNK_COUNT);
    let mut total_size = 0u64;
    {
        let mut chunks = host_chunks
            .lock()
            .map_err(|_| quantam_fs::Error::State("test chunk lock poisoned"))?;
        for index in 0..CHUNK_COUNT {
            let plaintext = format!("mailbox chunk {index}").into_bytes();
            total_size += plaintext.len() as u64;
            chunk_ids.push(chunks.put(&file_id, index as u64, plaintext));
        }
    }
    let mut manifest = Manifest {
        file_id,
        chunk_ids: chunk_ids.clone(),
        size: total_size,
        writer_id: host_id,
        version: 1,
        signature: Vec::new(),
    };
    manifest.signature = RustCryptoPureMlDsa.sign(
        &host_keys.signing_key()?,
        MANIFEST_CONTEXT,
        &encoding::manifest_m(&manifest)?,
    )?;

    let mut host = HostService::new(host_keys.clone(), members.clone(), host_chunks.clone())?;
    let mut member =
        MemberReplica::new(member_keys.clone(), host_id, members, member_chunks.clone())?;
    host.commit(manifest)?;
    assert_eq!(host.mailbox(member_id)?.len(), CHUNK_COUNT + 1);
    let host_pull = InProcessPullCoordinator::new(host_keys, host_chunks);
    assert!(host_pull
        .serve(&PullRequest::new(vec![chunk_ids[0]])?, member_id)
        .is_err());

    let challenge = host.issue_flush_challenge(member_id)?;
    let signature = RustCryptoPureMlDsa.sign(
        &member_keys.signing_key()?,
        FLUSH_CONTEXT,
        &encoding::flush_m(&challenge),
    )?;
    let report = host.flush_mailbox(&mut member, &challenge, &signature)?;
    assert_eq!(report.controls, 1);
    assert_eq!(report.chunks_written, CHUNK_COUNT);
    let chunks = member_chunks
        .lock()
        .map_err(|_| quantam_fs::Error::State("test chunk lock poisoned"))?;
    assert_eq!(chunks.len(), CHUNK_COUNT);
    for chunk_id in chunk_ids {
        assert!(chunks.has(&chunk_id));
    }
    Ok(())
}
