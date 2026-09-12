use std::{
    cell::RefCell,
    fs,
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

use quantam_fs::{
    crypto::wrap::{ConstructionBWrap, RustCryptoConstructionBWrap},
    encoding,
    ids::{ChunkId, Epoch, FileId},
    keystore::KeyStore,
    net::{
        directory::DirectoryAd,
        join::{join_host, serve_host, unix_time, VaultHost},
        locate::{query_over_stream, serve_have_once},
    },
    protocol::{locate::HaveQuery, pull::PullRequest},
    store::chunks::{shared_chunk_store, ChunkStore},
    sync::host::ControlUpdate,
    Error, Result,
};
use tokio::{net::TcpListener, task::LocalSet};

struct TestDir(PathBuf);

static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

impl TestDir {
    fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "qfs-net-locate-{}-{}-{}",
            std::process::id(),
            NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| Error::State("test clock"))?
                .as_nanos()
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn connect_keys(a: &KeyStore, b: &KeyStore) -> Result<()> {
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

#[tokio::test(flavor = "current_thread")]
async fn host_have_reply_follows_control_and_pull_uses_the_trusted_manifest() -> Result<()> {
    LocalSet::new()
        .run_until(async {
            let directory = TestDir::new()?;
            let host_keys = KeyStore::open(&directory.0.join("host.identity"))?;
            let member_keys = KeyStore::open(&directory.0.join("member.identity"))?;
            let host = Rc::new(RefCell::new(VaultHost::load_or_create(
                host_keys.clone(),
                &directory.0.join("vault.state"),
            )?));
            let listener = TcpListener::bind("127.0.0.1:0").await?;
            let ad = DirectoryAd::sign(
                &host_keys,
                host.borrow().vault_id(),
                listener.local_addr()?,
                unix_time()?,
            )?;
            let code = host.borrow().join_code();
            let server = tokio::task::spawn_local(serve_host(listener, host.clone()));
            let replica = Rc::new(RefCell::new(
                quantam_fs::sync::host::MemberReplica::new_in_vault(
                    member_keys.clone(),
                    host_keys.peer_id()?,
                    std::collections::BTreeSet::from([
                        member_keys.peer_id()?,
                        host_keys.peer_id()?,
                    ]),
                    shared_chunk_store(),
                    quantam_fs::ids::FileId(host.borrow().vault_id().0),
                )?,
            ));
            let mut member = join_host(member_keys, &ad, code, Some(replica.clone())).await?;

            let bodies = vec![b"nearby holder one".to_vec(), b"nearby holder two".to_vec()];
            let file_id = host
                .borrow_mut()
                .host
                .save_file(&host_keys, "/from-host", &bodies)?;
            let manifest = host
                .borrow()
                .host
                .trusted_manifest(&file_id)
                .ok_or(Error::State("host manifest missing"))?
                .manifest()
                .clone();
            let query = HaveQuery::new(file_id, manifest.chunk_ids.clone())?;
            let reply = member.have_query(&query).await?;
            assert!(reply.has(0));
            assert!(reply.has(1));
            let trusted = replica
                .borrow()
                .trusted_manifest(&file_id)
                .ok_or(Error::State("fan-out manifest was not applied"))?
                .clone();
            assert!(replica.borrow().chunks().lock().unwrap().is_empty());

            assert_eq!(
                member
                    .pull(&PullRequest::new(manifest.chunk_ids.clone())?, &trusted)
                    .await?,
                2
            );
            let chunks = replica.borrow().chunks();
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

            server.abort();
            Ok(())
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn already_live_member_pair_answers_exact_requested_ids() -> Result<()> {
    LocalSet::new()
        .run_until(async {
            let directory = TestDir::new()?;
            let requester = KeyStore::open(&directory.0.join("requester.identity"))?;
            let holder = KeyStore::open(&directory.0.join("holder.identity"))?;
            connect_keys(&requester, &holder)?;
            let file_id = FileId([73; 32]);
            let present = encoding::chunk_id(&file_id, 0, b"present");
            let absent = encoding::chunk_id(&file_id, 1, b"absent");
            let chunks = shared_chunk_store();
            chunks
                .lock()
                .map_err(|_| Error::State("test chunk lock poisoned"))?
                .put(&file_id, 0, b"present".to_vec())?;
            let query = HaveQuery::new(file_id, vec![absent, present])?;

            let listener = TcpListener::bind("127.0.0.1:0").await?;
            let connect = tokio::net::TcpStream::connect(listener.local_addr()?);
            let (request_stream, accepted) = tokio::join!(connect, listener.accept());
            let mut request_stream = request_stream?;
            let (mut holder_stream, _) = accepted?;
            let holder_id = holder.peer_id()?;
            let requester_id = requester.peer_id()?;
            let served = tokio::task::spawn_local(async move {
                serve_have_once(&mut holder_stream, &holder, requester_id, &chunks).await
            });
            let reply =
                query_over_stream(&mut request_stream, &requester, holder_id, &query).await?;
            served
                .await
                .map_err(|_| Error::State("have server task failed"))??;
            assert!(!reply.has(0));
            assert!(reply.has(1));
            Ok(())
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn have_query_opens_more_than_one_window_of_controls_before_new_welcome() -> Result<()> {
    LocalSet::new()
        .run_until(async {
            let directory = TestDir::new()?;
            let host_keys = KeyStore::open(&directory.0.join("host.identity"))?;
            let member_keys = KeyStore::open(&directory.0.join("member.identity"))?;
            let host = Rc::new(RefCell::new(VaultHost::load_or_create(
                host_keys.clone(),
                &directory.0.join("vault.state"),
            )?));
            let listener = TcpListener::bind("127.0.0.1:0").await?;
            let ad = DirectoryAd::sign(
                &host_keys,
                host.borrow().vault_id(),
                listener.local_addr()?,
                unix_time()?,
            )?;
            let code = host.borrow().join_code();
            let server = tokio::task::spawn_local(serve_host(listener, host.clone()));
            let mut member = join_host(member_keys, &ad, code, None).await?;
            let host_id = host_keys.peer_id()?;
            for value in 0..1_025u64 {
                let mut file_id = [0u8; 32];
                file_id[..8].copy_from_slice(&value.to_be_bytes());
                host.borrow_mut()
                    .host
                    .fan_out_control(host_id, &ControlUpdate::Add(FileId(file_id)))?;
            }

            let query = HaveQuery::new(FileId([81; 32]), vec![ChunkId([82; 32])])?;
            let reply = member.have_query(&query).await?;
            assert!(!reply.has(0));
            assert_eq!(member.replica.borrow().instruction_log().len(), 1_025);

            server.abort();
            Ok(())
        })
        .await
}
