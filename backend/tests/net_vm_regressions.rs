use std::{
    cell::RefCell,
    fs,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

use quantam_fs::{
    encoding,
    keystore::KeyStore,
    net::{
        directory::DirectoryAd,
        join::{join_host, serve_host, unix_time, VaultHost, VaultMetadata},
        vaults::VaultSet,
        JoinCode, VaultId,
    },
    store::{chunks::ChunkStore, vaults::vault_directory},
    Error, Result,
};
use tokio::{net::TcpListener, task::LocalSet};

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "qfs-net-vm-{name}-{}-{}",
            std::process::id(),
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

fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn create_vault(keys: KeyStore, root: &Path) -> Result<Rc<RefCell<VaultHost>>> {
    let vault_id = VaultId::generate()?;
    let directory = vault_directory(root, vault_id);
    fs::create_dir_all(&directory)?;
    let metadata = VaultMetadata {
        vault_id,
        join_code: JoinCode::generate()?,
        issued_at: 0,
        members: vec![keys.peer_id()?],
        denied: Default::default(),
    };
    let path = directory.join("vault");
    private_write(&path, &encoding::encode_vault_metadata(&metadata)?)?;
    Ok(Rc::new(RefCell::new(VaultHost::open_durable(
        keys, &path, &directory,
    )?)))
}

async fn start_server(
    vaults: VaultSet,
    host_keys: &KeyStore,
    vault: &Rc<RefCell<VaultHost>>,
) -> Result<(DirectoryAd, tokio::task::JoinHandle<()>)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let ad = DirectoryAd::sign(
        host_keys,
        vault.borrow().vault_id(),
        listener.local_addr()?,
        unix_time()?,
    )?;
    let task = tokio::task::spawn_local(async move {
        let _ = serve_host(listener, vaults).await;
    });
    Ok((ad, task))
}

async fn bounded_join(
    keys: KeyStore,
    ad: &DirectoryAd,
    code: JoinCode,
) -> Result<quantam_fs::net::join::JoinedPeer> {
    tokio::time::timeout(Duration::from_secs(15), join_host(keys, ad, code, None))
        .await
        .map_err(|_| Error::State("bounded localhost join timed out"))?
}

async fn member_restart_case(host_is_lower: bool) -> Result<()> {
    let directory = TestDir::new(if host_is_lower {
        "host-lower"
    } else {
        "host-higher"
    })?;
    let left_path = directory.0.join("left");
    let right_path = directory.0.join("right");
    let left = KeyStore::open(&left_path)?;
    let right = KeyStore::open(&right_path)?;
    let left_is_lower = left.peer_id()? < right.peer_id()?;
    let (host_keys, member_keys, member_path) = if left_is_lower == host_is_lower {
        (left, right, right_path)
    } else {
        (right, left, left_path)
    };
    let host_id = host_keys.peer_id()?;
    let member_id = member_keys.peer_id()?;
    let vault = create_vault(host_keys.clone(), &directory.0)?;
    let code = vault.borrow().join_code();
    let set = VaultSet::new(host_keys.clone())?;
    set.insert(vault.clone())?;
    let (ad, server) = start_server(set, &host_keys, &vault).await?;

    let joined = bounded_join(member_keys.clone(), &ad, code).await?;
    let first_epoch = member_keys.current_session(host_id)?.epoch;
    drop(joined);
    tokio::task::yield_now().await;
    vault
        .borrow_mut()
        .host
        .heartbeat(member_id, Duration::ZERO)?;
    drop(member_keys);

    let restarted = KeyStore::open(&member_path)?;
    let mut rejoined = bounded_join(restarted.clone(), &ad, code).await?;
    let restarted_epoch = restarted.current_session(host_id)?.epoch;
    assert!(restarted_epoch > first_epoch);
    assert_eq!(host_keys.current_session(member_id)?.epoch, restarted_epoch);
    rejoined.mkdir("/member-restarted-and-live").await?;
    assert!(vault
        .borrow()
        .host
        .tree()
        .resolve("/member-restarted-and-live")
        .is_ok());
    server.abort();
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn member_only_restart_stays_live_in_both_peer_id_orders() -> Result<()> {
    LocalSet::new()
        .run_until(async {
            member_restart_case(true).await?;
            member_restart_case(false).await
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn fresh_member_bootstraps_nested_renamed_file_after_log_truncation() -> Result<()> {
    LocalSet::new()
        .run_until(async {
            let directory = TestDir::new("bootstrap-truncated")?;
            let host_keys = KeyStore::open(&directory.0.join("host"))?;
            let member_keys = KeyStore::open(&directory.0.join("member"))?;
            let vault = create_vault(host_keys.clone(), &directory.0)?;
            let host_id = host_keys.peer_id()?;
            vault.borrow_mut().host.mkdir(host_id, "/docs")?;
            vault.borrow_mut().host.mkdir(host_id, "/docs/old")?;
            let body = b"durable bootstrap body".to_vec();
            let file_id = vault.borrow_mut().host.save_file(
                &host_keys,
                "/docs/old/note",
                std::slice::from_ref(&body),
            )?;
            vault
                .borrow_mut()
                .host
                .rename(host_id, "/docs/old", "/docs/current")?;
            assert!(vault.borrow().host.instruction_log().is_empty());

            let code = vault.borrow().join_code();
            let set = VaultSet::new(host_keys.clone())?;
            set.insert(vault.clone())?;
            let (ad, server) = start_server(set, &host_keys, &vault).await?;
            let joined = bounded_join(member_keys, &ad, code).await?;
            let replica = joined.replica.borrow();
            assert_eq!(replica.tree().resolve("/docs/current/note")?, file_id);
            assert!(replica.tree().resolve("/docs/old").is_err());
            let trusted = replica
                .trusted_manifest(&file_id)
                .ok_or(Error::State("bootstrapped manifest missing"))?;
            let chunk_id = trusted.manifest().chunk_ids[0];
            let chunks = replica.chunks();
            assert_eq!(
                chunks
                    .lock()
                    .map_err(|_| Error::State("test chunk lock poisoned"))?
                    .get(&chunk_id),
                Some(body.as_slice())
            );
            drop(replica);
            drop(joined);
            server.abort();
            Ok(())
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn other_vault_candidate_rejection_preserves_confirmed_session_and_code() -> Result<()> {
    LocalSet::new()
        .run_until(async {
            let directory = TestDir::new("wrong-vault-candidate")?;
            let host_keys = KeyStore::open(&directory.0.join("host"))?;
            let member_path = directory.0.join("member");
            let member_keys = KeyStore::open(&member_path)?;
            let member_id = member_keys.peer_id()?;
            let host_id = host_keys.peer_id()?;
            let first = create_vault(host_keys.clone(), &directory.0)?;
            let second = create_vault(host_keys.clone(), &directory.0)?;
            let first_code = first.borrow().join_code();
            let second_code = second.borrow().join_code();
            let first_id = first.borrow().vault_id();
            let second_id = second.borrow().vault_id();
            let set = VaultSet::new(host_keys.clone())?;
            set.insert(first.clone())?;
            set.insert(second.clone())?;
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
            let address: SocketAddr = listener.local_addr()?;
            let first_ad =
                DirectoryAd::sign(&host_keys, first.borrow().vault_id(), address, unix_time()?)?;
            let second_ad = DirectoryAd::sign(
                &host_keys,
                second.borrow().vault_id(),
                address,
                unix_time()?,
            )?;
            let served = set.clone();
            let server = tokio::task::spawn_local(async move {
                let _ = serve_host(listener, served).await;
            });

            let joined = bounded_join(member_keys.clone(), &first_ad, first_code).await?;
            let confirmed_host_epoch = host_keys.current_session(member_id)?.epoch;
            let code_before = first.borrow().join_code();
            drop(joined);
            tokio::task::yield_now().await;
            drop(member_keys);
            let restarted_member = KeyStore::open(&member_path)?;
            assert!(restarted_member.current_session(host_id)?.epoch > confirmed_host_epoch);

            assert!(matches!(
                bounded_join(restarted_member.clone(), &second_ad, second_code).await,
                Err(Error::VaultSessionConflict {
                    peer_id,
                    bound,
                    requested,
                }) if peer_id == member_id && bound == first_id && requested == second_id
            ));
            assert_eq!(first.borrow().join_code(), code_before);
            assert!(restarted_member.current_session(host_id).is_err());
            assert_eq!(
                host_keys.current_session(member_id)?.epoch,
                confirmed_host_epoch
            );
            assert!(host_keys.cached_candidate(member_id)?.is_none());
            assert!(first.borrow().host.has_member(&member_id));
            assert!(!second.borrow().host.has_member(&member_id));

            let mut rejoined = bounded_join(restarted_member, &first_ad, first_code).await?;
            rejoined.mkdir("/confirmed-session-survived").await?;
            assert!(first
                .borrow()
                .host
                .tree()
                .resolve("/confirmed-session-survived")
                .is_ok());
            server.abort();
            Ok(())
        })
        .await
}
