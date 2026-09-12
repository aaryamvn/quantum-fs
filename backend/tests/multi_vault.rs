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
    keystore::KeyStore,
    net::{
        directory::{serve, DirectoryAd, DirectoryClient, DirectoryStore},
        frame::{read_frame, write_frame, WRAP_ACK_KIND, WRAP_KIND},
        join::{join_host, serve_host, VaultHost},
        vaults::VaultSet,
        JoinCode, VaultId,
    },
    store::vaults::{resume_migration, vault_directory},
    Error, Result,
};
use tokio::{
    net::{TcpListener, TcpStream},
    task::LocalSet,
    time::{sleep, Duration},
};

struct TestDir(PathBuf);

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

impl TestDir {
    fn new(name: &str) -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "qfs-multi-vault-{name}-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
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

fn private_write(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn create_vault(keys: KeyStore, data_dir: &std::path::Path) -> Result<Rc<RefCell<VaultHost>>> {
    let vault_id = VaultId::generate()?;
    let directory = vault_directory(data_dir, vault_id);
    fs::create_dir_all(&directory)?;
    let metadata = quantam_fs::net::join::VaultMetadata {
        vault_id,
        join_code: JoinCode::generate()?,
        issued_at: 0,
        members: vec![keys.peer_id()?],
        denied: Default::default(),
    };
    private_write(
        &directory.join("vault"),
        &encoding::encode_vault_metadata(&metadata)?,
    )?;
    Ok(Rc::new(RefCell::new(VaultHost::open_durable(
        keys,
        &directory.join("vault"),
        &directory,
    )?)))
}

async fn directory_server(
    directory: &TestDir,
) -> Result<(DirectoryClient, tokio::task::JoinHandle<()>)> {
    let store = Rc::new(RefCell::new(DirectoryStore::open(
        &directory.0.join("directory.bin"),
    )?));
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let client = DirectoryClient::new(listener.local_addr()?);
    let task = tokio::task::spawn_local(async move {
        let _ = serve(listener, store).await;
    });
    Ok((client, task))
}

async fn publish(
    vault: &Rc<RefCell<VaultHost>>,
    directory: &DirectoryClient,
    address: std::net::SocketAddr,
) -> Result<DirectoryAd> {
    // RefCell borrows cannot cross await, so mirror VaultHost::publish's small
    // synchronous preparation around the async directory request.
    let (code, ad) = {
        let state = vault.borrow();
        let issued_at = quantam_fs::net::join::unix_time()?;
        (
            state.join_code(),
            DirectoryAd::sign(&state.keys, state.vault_id(), address, issued_at)?,
        )
    };
    directory.put(code, &ad).await?;
    Ok(ad)
}

async fn two_vault_server(
    directory: &TestDir,
) -> Result<(
    KeyStore,
    VaultSet,
    Rc<RefCell<VaultHost>>,
    Rc<RefCell<VaultHost>>,
    DirectoryAd,
    DirectoryAd,
    tokio::task::JoinHandle<()>,
    tokio::task::JoinHandle<()>,
)> {
    let keys = KeyStore::open(&directory.0.join("identity"))?;
    two_vault_server_with_keys(directory, keys).await
}

async fn two_vault_server_with_keys(
    directory: &TestDir,
    keys: KeyStore,
) -> Result<(
    KeyStore,
    VaultSet,
    Rc<RefCell<VaultHost>>,
    Rc<RefCell<VaultHost>>,
    DirectoryAd,
    DirectoryAd,
    tokio::task::JoinHandle<()>,
    tokio::task::JoinHandle<()>,
)> {
    let first = create_vault(keys.clone(), &directory.0)?;
    let second = create_vault(keys.clone(), &directory.0)?;
    let set = VaultSet::new(keys.clone())?;
    set.insert(first.clone())?;
    set.insert(second.clone())?;
    let (directory_client, directory_task) = directory_server(directory).await?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let first_ad = publish(&first, &directory_client, address).await?;
    let second_ad = publish(&second, &directory_client, address).await?;
    let served = set.clone();
    let server_task = tokio::task::spawn_local(async move {
        let _ = serve_host(listener, served).await;
    });
    Ok((
        keys,
        set,
        first,
        second,
        first_ad,
        second_ad,
        server_task,
        directory_task,
    ))
}

async fn proxy_handshake(
    listener: TcpListener,
    target: std::net::SocketAddr,
    drop_wrap_ack: bool,
    captured: Rc<RefCell<Option<Vec<u8>>>>,
) -> Result<()> {
    let (mut client, _) = listener.accept().await?;
    let mut host = TcpStream::connect(target).await?;

    let identity = read_frame(&mut client).await?;
    write_frame(&mut host, &identity).await?;
    let identity = read_frame(&mut host).await?;
    write_frame(&mut client, &identity).await?;
    let hint = read_frame(&mut client).await?;
    write_frame(&mut host, &hint).await?;
    let hint = read_frame(&mut host).await?;
    write_frame(&mut client, &hint).await?;
    let wrap = read_frame(&mut host).await?;
    if wrap.kind != WRAP_KIND {
        return Err(Error::State("expected host-initiated wrap"));
    }
    *captured.borrow_mut() = Some(wrap.payload.clone());
    write_frame(&mut client, &wrap).await?;
    let ack = read_frame(&mut client).await?;
    if ack.kind != WRAP_ACK_KIND {
        return Err(Error::State("expected member WrapAck"));
    }
    if drop_wrap_ack {
        sleep(Duration::from_millis(5_200)).await;
        return Ok(());
    }
    write_frame(&mut host, &ack).await?;
    tokio::io::copy_bidirectional(&mut client, &mut host).await?;
    Ok(())
}

#[test]
fn migration_resumes_after_chunks_move_without_guessing_from_destination() -> Result<()> {
    let directory = TestDir::new("resume-after-chunks")?;
    let keys = KeyStore::open(&directory.0.join("identity"))?;
    let vault = VaultHost::open_durable(keys.clone(), &directory.0.join("vault"), &directory.0)?;
    let vault_id = vault.vault_id();
    let code = vault.join_code();
    drop(vault);

    let destination = vault_directory(&directory.0, vault_id);
    fs::create_dir_all(&destination)?;
    fs::rename(
        directory.0.join("replica.bin"),
        destination.join("replica.bin"),
    )?;
    fs::rename(directory.0.join("chunks"), destination.join("chunks"))?;
    let mut marker = vault_id.0.to_vec();
    // Crash after the chunks rename and before advancing the phase marker.
    marker.push(1);
    let marker_path = directory.0.join(".migrate-vaults");
    fs::write(&marker_path, marker)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&marker_path, fs::Permissions::from_mode(0o600))?;
    }

    resume_migration(&keys, &directory.0)?;
    assert!(!directory.0.join(".migrate-vaults").exists());
    assert!(!directory.0.join("vault").exists());
    assert!(destination.join("vault").is_file());
    assert!(destination.join("replica.bin").is_file());
    assert!(destination.join("chunks").is_dir());

    let reopened = VaultHost::open_durable(keys, &destination.join("vault"), &destination)?;
    assert_eq!(reopened.vault_id(), vault_id);
    assert_eq!(reopened.join_code(), code);
    assert_eq!(reopened.host.expected_root().0, vault_id.0);
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn two_hosted_vaults_keep_members_trees_and_restart_state_disjoint() -> Result<()> {
    let directory = TestDir::new("two-live-vaults")?;
    LocalSet::new()
        .run_until(async {
            let (host_keys, set, first, second, first_ad, second_ad, server, directory_task) =
                two_vault_server(&directory).await?;
            let first_id = first.borrow().vault_id();
            let second_id = second.borrow().vault_id();
            let first_code = first.borrow().join_code();
            let second_code = second.borrow().join_code();
            let first_member = KeyStore::open(&directory.0.join("first-member"))?;
            let second_member = KeyStore::open(&directory.0.join("second-member"))?;
            let first_peer = first_member.peer_id()?;
            let second_peer = second_member.peer_id()?;

            let mut joined_first = join_host(first_member, &first_ad, first_code, None).await?;
            let mut joined_second = join_host(second_member, &second_ad, second_code, None).await?;
            joined_first.mkdir("/only-first").await?;
            joined_second.mkdir("/only-second").await?;

            assert!(first.borrow().host.has_member(&first_peer));
            assert!(!first.borrow().host.has_member(&second_peer));
            assert!(second.borrow().host.has_member(&second_peer));
            assert!(!second.borrow().host.has_member(&first_peer));
            assert!(first.borrow().host.tree().resolve("/only-first").is_ok());
            assert!(first.borrow().host.tree().resolve("/only-second").is_err());
            assert!(second.borrow().host.tree().resolve("/only-second").is_ok());
            assert!(second.borrow().host.tree().resolve("/only-first").is_err());

            drop(joined_first);
            drop(joined_second);
            server.abort();
            directory_task.abort();
            let _ = server.await;
            let _ = directory_task.await;
            drop(set);
            drop(first);
            drop(second);

            let first_directory = vault_directory(&directory.0, first_id);
            let second_directory = vault_directory(&directory.0, second_id);
            let reopened_first = VaultHost::open_durable(
                host_keys.clone(),
                &first_directory.join("vault"),
                &first_directory,
            )?;
            let reopened_second = VaultHost::open_durable(
                host_keys,
                &second_directory.join("vault"),
                &second_directory,
            )?;
            assert_eq!(reopened_first.join_code(), first_code);
            assert_eq!(reopened_second.join_code(), second_code);
            assert_eq!(reopened_first.host.expected_root().0, first_id.0);
            assert_eq!(reopened_second.host.expected_root().0, second_id.0);
            assert!(reopened_first.host.tree().resolve("/only-first").is_ok());
            assert!(reopened_second.host.tree().resolve("/only-second").is_ok());
            Ok(())
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn cross_vault_join_preserves_existing_binding_pair_and_live_session() -> Result<()> {
    let directory = TestDir::new("session-conflict")?;
    LocalSet::new()
        .run_until(async {
            let (host_keys, set, first, second, first_ad, second_ad, server, directory_task) =
                two_vault_server(&directory).await?;
            let first_code = first.borrow().join_code();
            let second_code = second.borrow().join_code();
            let first_id = first.borrow().vault_id();
            let second_id = second.borrow().vault_id();
            let member = KeyStore::open(&directory.0.join("member"))?;
            let member_id = member.peer_id()?;
            let host_id = host_keys.peer_id()?;
            let joined = join_host(member.clone(), &first_ad, first_code, None).await?;
            let epoch = member.current_session(host_id)?.epoch;
            let mailbox_before = first.borrow_mut().host.mailbox(member_id)?;
            assert_eq!(set.bound_vault(member_id), Some(first_id));
            assert!(matches!(
                set.check_binding(member_id, second_id),
                Err(Error::VaultSessionConflict {
                    peer_id,
                    bound,
                    requested,
                }) if peer_id == member_id && bound == first_id && requested == second_id
            ));
            drop(joined);
            tokio::task::yield_now().await;
            assert_eq!(set.bound_vault(member_id), Some(first_id));

            assert!(matches!(
                join_host(member.clone(), &second_ad, second_code, None).await,
                Err(Error::VaultSessionConflict {
                    peer_id,
                    bound,
                    requested,
                }) if peer_id == member_id && bound == first_id && requested == second_id
            ));
            tokio::task::yield_now().await;
            assert_eq!(set.bound_vault(member_id), Some(first_id));
            assert_eq!(member.current_session(host_id)?.epoch, epoch);
            assert_eq!(host_keys.current_session(member_id)?.epoch, epoch);
            assert!(first.borrow_mut().host.mailbox(member_id)? == mailbox_before);
            assert!(first.borrow().host.has_member(&member_id));
            assert!(!second.borrow().host.has_member(&member_id));
            let mut joined = join_host(member, &first_ad, first_code, None).await?;
            joined.mkdir("/still-live").await?;
            assert!(first.borrow().host.tree().resolve("/still-live").is_ok());

            drop(joined);
            server.abort();
            directory_task.abort();
            Ok(())
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn brand_new_peer_wrong_code_loses_only_its_provisional_pair() -> Result<()> {
    let directory = TestDir::new("provisional-cleanup")?;
    LocalSet::new()
        .run_until(async {
            let (host_keys, _set, first, _second, first_ad, _second_ad, server, directory_task) =
                two_vault_server(&directory).await?;
            let peer = KeyStore::open(&directory.0.join("new-peer"))?;
            let peer_id = peer.peer_id()?;
            let host_id = host_keys.peer_id()?;
            assert!(
                join_host(peer.clone(), &first_ad, JoinCode([0x55; 16]), None)
                    .await
                    .is_err()
            );
            tokio::task::yield_now().await;
            assert!(peer.current_session(host_id).is_err());
            assert!(host_keys.current_session(peer_id).is_err());
            assert!(!first.borrow().host.has_member(&peer_id));
            server.abort();
            directory_task.abort();
            Ok(())
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn serve_host_retries_identical_wrap_after_lost_ack() -> Result<()> {
    let directory = TestDir::new("host-wrap-retry")?;
    LocalSet::new()
        .run_until(async {
            let first_identity = KeyStore::open(&directory.0.join("retry-identity-a"))?;
            let second_identity = KeyStore::open(&directory.0.join("retry-identity-b"))?;
            let (lower, member) = if first_identity.peer_id()? < second_identity.peer_id()? {
                (first_identity, second_identity)
            } else {
                (second_identity, first_identity)
            };
            let (host_keys, _set, first, _second, first_ad, _second_ad, server, directory_task) =
                two_vault_server_with_keys(&directory, lower).await?;
            let host_id = host_keys.peer_id()?;
            assert!(host_id < member.peer_id()?);
            let member_id = member.peer_id()?;
            let code = first.borrow().join_code();
            let target = first_ad.addr;

            let first_proxy = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
            let first_capture = Rc::new(RefCell::new(None));
            let first_ad = DirectoryAd::sign(
                &host_keys,
                first_ad.vault_id,
                first_proxy.local_addr()?,
                quantam_fs::net::join::unix_time()?,
            )?;
            let capture = first_capture.clone();
            let first_proxy_task = tokio::task::spawn_local(async move {
                let _ = proxy_handshake(first_proxy, target, true, capture).await;
            });
            assert!(join_host(member.clone(), &first_ad, code, None)
                .await
                .is_err());
            tokio::time::timeout(Duration::from_secs(7), first_proxy_task)
                .await
                .map_err(|_| Error::State("first lost-ack proxy did not stop"))?
                .map_err(|_| Error::State("first lost-ack proxy task failed"))?;
            assert!(host_keys.current_session(member_id).is_ok());
            assert!(member.current_session(host_id).is_ok());

            let second_proxy = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
            let second_capture = Rc::new(RefCell::new(None));
            let second_ad = DirectoryAd::sign(
                &host_keys,
                first.borrow().vault_id(),
                second_proxy.local_addr()?,
                quantam_fs::net::join::unix_time()?,
            )?;
            let capture = second_capture.clone();
            let second_proxy_task = tokio::task::spawn_local(async move {
                let _ = proxy_handshake(second_proxy, target, false, capture).await;
            });
            let joined = join_host(member, &second_ad, code, None).await?;
            assert!(first_capture.borrow().is_some());
            assert!(*first_capture.borrow() == *second_capture.borrow());
            drop(joined);
            second_proxy_task.abort();
            let stopped = tokio::time::timeout(Duration::from_secs(7), second_proxy_task)
                .await
                .map_err(|_| Error::State("second lost-ack proxy did not stop"))?;
            if stopped.is_err_and(|error| !error.is_cancelled()) {
                return Err(Error::State("second lost-ack proxy task failed"));
            }
            server.abort();
            directory_task.abort();
            Ok(())
        })
        .await
}
