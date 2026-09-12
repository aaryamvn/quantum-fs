use std::{
    cell::RefCell,
    fs,
    net::Ipv4Addr,
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

use quantam_fs::{
    encoding,
    ids::FileId,
    keystore::{IdentityKeyStore, KeyStore},
    net::{
        directory::{serve, DirectoryClient, DirectoryStore},
        frame::{read_frame, write_frame, FLUSH_CHALLENGE_KIND},
        join::{join_host, serve_host, VaultHost},
    },
    store::chunks::{shared_chunk_store, ChunkStore},
    sync::host::MemberReplica,
    Error, Result,
};
use tokio::{
    net::{TcpListener, TcpStream},
    task::LocalSet,
    time::{sleep, Duration},
};

async fn tamper_flush_history(
    listener: TcpListener,
    target: std::net::SocketAddr,
    tamper: HistoryTamper,
) -> Result<()> {
    let (client, _) = listener.accept().await?;
    let host = TcpStream::connect(target).await?;
    let (mut client_read, mut client_write) = client.into_split();
    let (mut host_read, mut host_write) = host.into_split();
    let toward_host = async {
        loop {
            let frame = read_frame(&mut client_read).await?;
            write_frame(&mut host_write, &frame).await?;
        }
        #[allow(unreachable_code)]
        Ok::<(), Error>(())
    };
    let toward_client = async {
        let mut changed = false;
        loop {
            let mut frame = read_frame(&mut host_read).await?;
            if frame.kind == FLUSH_CHALLENGE_KIND && !changed {
                let mut offer = encoding::decode_flush_offer(&frame.payload)?;
                match &tamper {
                    HistoryTamper::Add(document) => offer.historical.push(document.clone()),
                    HistoryTamper::Remove(peer_id) => {
                        offer
                            .historical
                            .retain(|document| document.peer_id != *peer_id);
                    }
                }
                offer.historical.sort_by_key(|document| document.peer_id);
                frame.payload = encoding::encode_flush_offer(&offer)?;
                changed = true;
            }
            write_frame(&mut client_write, &frame).await?;
        }
        #[allow(unreachable_code)]
        Ok::<(), Error>(())
    };
    let _ = tokio::try_join!(toward_host, toward_client)?;
    Ok(())
}

enum HistoryTamper {
    Add(quantam_fs::crypto::identity::IdentityDocument),
    Remove(quantam_fs::ids::PeerId),
}

struct TestDir(PathBuf);

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

impl TestDir {
    fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "qfs-net-history-flush-{}-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed),
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

#[tokio::test(flavor = "current_thread")]
async fn offline_member_applies_later_writer_history_before_kick_reconciliation() -> Result<()> {
    let directory = TestDir::new()?;
    LocalSet::new()
        .run_until(async {
            let directory_store = Rc::new(RefCell::new(DirectoryStore::open(
                &directory.0.join("directory.bin"),
            )?));
            let directory_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
            let directory_client = DirectoryClient::new(directory_listener.local_addr()?);
            let directory_task = tokio::task::spawn_local(async move {
                let _ = serve(directory_listener, directory_store).await;
            });

            let host_keys = KeyStore::open(&directory.0.join("host.identity"))?;
            let mut vault =
                VaultHost::load_or_create(host_keys.clone(), &directory.0.join("vault"))?;
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
            let address = listener.local_addr()?;
            let initial_ad = vault.publish(&directory_client, address).await?;
            let initial_code = vault.join_code();
            let vault = Rc::new(RefCell::new(vault));
            let served = vault.clone();
            let server_task = tokio::task::spawn_local(async move {
                let _ = serve_host(listener, served).await;
            });

            let a_keys = KeyStore::open(&directory.0.join("a.identity"))?;
            let host_id = host_keys.peer_id()?;
            let a_id = a_keys.peer_id()?;
            let a_replica = Rc::new(RefCell::new(MemberReplica::new_in_vault(
                a_keys.clone(),
                host_id,
                [host_id, a_id].into(),
                shared_chunk_store(),
                FileId(vault.borrow().vault_id().0),
            )?));
            let joined_a = join_host(
                a_keys.clone(),
                &initial_ad,
                initial_code,
                Some(a_replica.clone()),
            )
            .await?;
            drop(joined_a);
            sleep(Duration::from_millis(100)).await;

            let b_keys = KeyStore::open(&directory.0.join("b.identity"))?;
            let b_id = b_keys.peer_id()?;
            let mut joined_b = join_host(b_keys.clone(), &initial_ad, initial_code, None).await?;
            let bodies = vec![b"written while A was offline".to_vec()];
            let file_id = joined_b.save_file("/from-b", &bodies).await?;
            let chunk_id = joined_b
                .replica
                .borrow()
                .trusted_manifest(&file_id)
                .ok_or(Error::State("B manifest missing"))?
                .manifest()
                .chunk_ids[0];

            VaultHost::kick(&vault, &directory_client, address, b_id).await?;
            sleep(Duration::from_millis(100)).await;
            let rotated_code = vault.borrow().join_code();
            let rotated_ad = directory_client
                .lookup(rotated_code)
                .await?
                .ok_or(Error::State("rotated directory ad missing"))?;
            let rejoined_a = join_host(
                a_keys.clone(),
                &rotated_ad,
                rotated_code,
                Some(a_replica.clone()),
            )
            .await?;

            let replica = a_replica.borrow();
            assert_eq!(replica.tree().resolve("/from-b")?, file_id);
            assert_eq!(
                replica
                    .trusted_manifest(&file_id)
                    .ok_or(Error::State("historical manifest missing"))?
                    .manifest()
                    .writer_id,
                b_id
            );
            assert_eq!(
                replica
                    .chunks()
                    .lock()
                    .map_err(|_| Error::State("chunk lock poisoned"))?
                    .get(&chunk_id),
                Some(bodies[0].as_slice())
            );
            assert!(!replica.members().contains(&b_id));
            drop(replica);
            assert!(a_keys.current_session(b_id).is_err());
            assert!(a_keys.load_verified_peer(&b_id).is_err());
            assert!(!vault.borrow().host.has_member(&b_id));

            drop(rejoined_a);
            drop(joined_b);
            server_task.abort();
            directory_task.abort();
            Ok(())
        })
        .await
}

async fn tampered_flush_history_scenario(remove_writer: bool) -> Result<()> {
    let directory = TestDir::new()?;
    LocalSet::new()
        .run_until(async {
            let directory_store = Rc::new(RefCell::new(DirectoryStore::open(
                &directory.0.join("directory.bin"),
            )?));
            let directory_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
            let directory_client = DirectoryClient::new(directory_listener.local_addr()?);
            let directory_task = tokio::task::spawn_local(async move {
                let _ = serve(directory_listener, directory_store).await;
            });

            let host_keys = KeyStore::open(&directory.0.join("host.identity"))?;
            let mut host =
                VaultHost::load_or_create(host_keys.clone(), &directory.0.join("vault"))?;
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
            let address = listener.local_addr()?;
            let initial_ad = host.publish(&directory_client, address).await?;
            let initial_code = host.join_code();
            let vault = Rc::new(RefCell::new(host));
            let served = vault.clone();
            let server_task = tokio::task::spawn_local(async move {
                let _ = serve_host(listener, served).await;
            });

            let a_keys = KeyStore::open(&directory.0.join("a.identity"))?;
            let host_id = host_keys.peer_id()?;
            let a_id = a_keys.peer_id()?;
            let a_replica = Rc::new(RefCell::new(MemberReplica::new_in_vault(
                a_keys.clone(),
                host_id,
                [host_id, a_id].into(),
                shared_chunk_store(),
                FileId(vault.borrow().vault_id().0),
            )?));
            let joined_a = join_host(
                a_keys.clone(),
                &initial_ad,
                initial_code,
                Some(a_replica.clone()),
            )
            .await?;
            drop(joined_a);
            sleep(Duration::from_millis(100)).await;

            let b_keys = KeyStore::open(&directory.0.join("b.identity"))?;
            let b_id = b_keys.peer_id()?;
            let mut joined_b = join_host(b_keys, &initial_ad, initial_code, None).await?;
            let file_id = joined_b
                .save_file("/from-b", &[b"queued historical body".to_vec()])
                .await?;
            VaultHost::kick(&vault, &directory_client, address, b_id).await?;
            sleep(Duration::from_millis(100)).await;
            let rotated_code = vault.borrow().join_code();
            let rotated_ad = directory_client
                .lookup(rotated_code)
                .await?
                .ok_or(Error::State("rotated directory ad missing"))?;
            let mailbox_before = vault.borrow_mut().host.mailbox(a_id)?;

            let tamper = if remove_writer {
                HistoryTamper::Remove(b_id)
            } else {
                HistoryTamper::Add(
                    KeyStore::open(&directory.0.join("unrelated.identity"))?.identity()?,
                )
            };
            let proxy = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
            let proxy_ad = quantam_fs::net::directory::DirectoryAd::sign(
                &host_keys,
                rotated_ad.vault_id,
                proxy.local_addr()?,
                quantam_fs::net::join::unix_time()?,
            )?;
            let proxy_task = tokio::task::spawn_local(async move {
                let _ = tamper_flush_history(proxy, address, tamper).await;
            });
            assert!(join_host(
                a_keys.clone(),
                &proxy_ad,
                rotated_code,
                Some(a_replica.clone()),
            )
            .await
            .is_err());
            proxy_task.abort();
            sleep(Duration::from_millis(100)).await;

            {
                let replica = a_replica.borrow();
                assert!(replica.tree().resolve("/from-b").is_err());
                assert!(replica.trusted_manifest(&file_id).is_none());
                let chunks = replica.chunks();
                let metadata = chunks
                    .lock()
                    .map_err(|_| Error::State("chunk lock poisoned"))?
                    .metadata()
                    .ok_or(Error::State("replica metadata missing"))?;
                assert!(!metadata.identity_documents.contains_key(&b_id));
            }
            assert!(vault.borrow_mut().host.mailbox(a_id)? == mailbox_before);
            assert!(a_keys.require_live_traffic(host_id).is_err());

            let rejoined = join_host(
                a_keys.clone(),
                &rotated_ad,
                rotated_code,
                Some(a_replica.clone()),
            )
            .await?;
            assert_eq!(a_replica.borrow().tree().resolve("/from-b")?, file_id);
            assert!(!a_replica.borrow().members().contains(&b_id));
            assert!(a_keys.current_session(b_id).is_err());

            drop(rejoined);
            drop(joined_b);
            server_task.abort();
            directory_task.abort();
            Ok(())
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn added_flush_history_fails_atomically_and_direct_retry_uses_same_queue() -> Result<()> {
    tampered_flush_history_scenario(false).await
}

#[tokio::test(flavor = "current_thread")]
async fn omitted_writer_history_fails_before_commit_and_direct_retry_reuses_receipts() -> Result<()>
{
    tampered_flush_history_scenario(true).await
}
