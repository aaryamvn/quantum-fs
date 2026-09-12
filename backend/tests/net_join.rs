use std::{cell::RefCell, fs, net::Ipv4Addr, path::PathBuf, rc::Rc};

use quantam_fs::{
    keystore::KeyStore,
    net::{
        directory::{serve, DirectoryAd, DirectoryClient, DirectoryStore},
        join::{join_host, serve_host, VaultHost},
        JoinCode,
    },
    Result,
};
use tokio::{net::TcpListener, task::LocalSet};

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "qfs-net-join-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| quantam_fs::Error::State("test clock"))?
                .as_nanos()
        ));
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

async fn directory(directory: &TestDir) -> Result<(DirectoryClient, tokio::task::JoinHandle<()>)> {
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

async fn vault_server(
    directory: &TestDir,
    name: &str,
    client: &DirectoryClient,
) -> Result<(
    Rc<RefCell<VaultHost>>,
    std::net::SocketAddr,
    DirectoryAd,
    tokio::task::JoinHandle<()>,
)> {
    let keys = directory.keys(&format!("{name}-identity"))?;
    let mut vault = VaultHost::load_or_create(keys, &directory.0.join(format!("{name}-vault")))?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let ad = vault.publish(client, address).await?;
    let vault = Rc::new(RefCell::new(vault));
    let served = vault.clone();
    let task = tokio::task::spawn_local(async move {
        let _ = serve_host(listener, served).await;
    });
    Ok((vault, address, ad, task))
}

#[tokio::test(flavor = "current_thread")]
async fn valid_join_adds_member_and_rotated_code_replaces_old_admission() -> Result<()> {
    let directory_state = TestDir::new("valid-rotate")?;
    LocalSet::new()
        .run_until(async {
            let (directory, directory_task) = directory(&directory_state).await?;
            let (vault, address, _, host_task) =
                vault_server(&directory_state, "host", &directory).await?;
            let old_code = vault.borrow().join_code();
            let old_ad = directory
                .lookup(old_code)
                .await?
                .ok_or(quantam_fs::Error::State("published ad missing"))?;
            let first_keys = directory_state.keys("first-member")?;
            let first_id = first_keys.peer_id()?;
            let host_id = vault.borrow().keys.peer_id()?;
            let joined = join_host(first_keys.clone(), &old_ad, old_code, None).await?;
            assert!(vault.borrow().host.has_member(&first_id));
            assert!(first_keys.current_session(host_id).is_ok());
            assert!(vault.borrow().keys.current_session(first_id).is_ok());
            drop(joined);
            tokio::task::yield_now().await;

            let new_code = VaultHost::rotate_code(&vault, &directory, address).await?;
            assert_ne!(new_code, old_code);
            assert!(directory.lookup(old_code).await?.is_none());
            let stale_keys = directory_state.keys("stale-member")?;
            assert!(join_host(stale_keys.clone(), &old_ad, old_code, None)
                .await
                .is_err());
            assert!(stale_keys.current_session(host_id).is_err());
            assert!(vault
                .borrow()
                .keys
                .current_session(stale_keys.peer_id()?)
                .is_err());
            assert!(!vault.borrow().host.has_member(&stale_keys.peer_id()?));

            let new_ad = directory
                .lookup(new_code)
                .await?
                .ok_or(quantam_fs::Error::State("rotated ad missing"))?;
            let second_keys = directory_state.keys("second-member")?;
            let second_id = second_keys.peer_id()?;
            let second = join_host(second_keys.clone(), &new_ad, new_code, None).await?;
            assert!(vault.borrow().host.has_member(&second_id));
            assert!(second_keys.current_session(host_id).is_ok());
            drop(second);
            host_task.abort();
            directory_task.abort();
            Ok(())
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn wrong_code_discards_both_provisional_pair_slots_without_admission() -> Result<()> {
    let directory_state = TestDir::new("wrong-code")?;
    LocalSet::new()
        .run_until(async {
            let (directory, directory_task) = directory(&directory_state).await?;
            let (vault, _, ad, host_task) =
                vault_server(&directory_state, "host", &directory).await?;
            let member = directory_state.keys("member")?;
            let member_id = member.peer_id()?;
            let host_id = vault.borrow().keys.peer_id()?;
            assert!(join_host(member.clone(), &ad, JoinCode([0x55; 16]), None)
                .await
                .is_err());
            tokio::task::yield_now().await;
            assert!(!vault.borrow().host.has_member(&member_id));
            assert!(member.current_session(host_id).is_err());
            assert!(vault.borrow().keys.current_session(member_id).is_err());
            host_task.abort();
            directory_task.abort();
            Ok(())
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn attacker_ad_with_stolen_code_cannot_leave_a_residual_session() -> Result<()> {
    let directory_state = TestDir::new("attacker")?;
    LocalSet::new()
        .run_until(async {
            let (directory, directory_task) = directory(&directory_state).await?;
            let real = VaultHost::load_or_create(
                directory_state.keys("real-identity")?,
                &directory_state.0.join("real-vault"),
            )?;
            let stolen = real.join_code();
            let (attacker, attacker_addr, attacker_ad, attacker_task) =
                vault_server(&directory_state, "attacker", &directory).await?;
            let stolen_code_ad = DirectoryAd::sign(
                &attacker.borrow().keys,
                attacker.borrow().vault_id(),
                attacker_addr,
                attacker_ad.issued_at + 1,
            )?;
            directory.put(stolen, &stolen_code_ad).await?;
            let looked_up = directory
                .lookup(stolen)
                .await?
                .ok_or(quantam_fs::Error::State("attacker ad missing"))?;
            let joiner = directory_state.keys("joiner")?;
            let joiner_id = joiner.peer_id()?;
            let attacker_id = attacker.borrow().keys.peer_id()?;
            assert!(join_host(joiner.clone(), &looked_up, stolen, None)
                .await
                .is_err());
            tokio::task::yield_now().await;
            assert!(joiner.current_session(attacker_id).is_err());
            assert!(attacker.borrow().keys.current_session(joiner_id).is_err());
            assert!(!attacker.borrow().host.has_member(&joiner_id));
            attacker_task.abort();
            directory_task.abort();
            Ok(())
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn tampered_directory_ad_is_rejected_before_connect_or_identity_import() -> Result<()> {
    let directory_state = TestDir::new("tampered-ad")?;
    LocalSet::new()
        .run_until(async {
            let (directory, directory_task) = directory(&directory_state).await?;
            let (vault, _, ad, host_task) =
                vault_server(&directory_state, "host", &directory).await?;
            let host_id = vault.borrow().keys.peer_id()?;
            let code = vault.borrow().join_code();

            let addr_keys = directory_state.keys("addr-member")?;
            let mut changed_addr = ad.clone();
            changed_addr
                .addr
                .set_port(changed_addr.addr.port().wrapping_add(1));
            assert!(join_host(addr_keys.clone(), &changed_addr, code, None)
                .await
                .is_err());
            assert!(addr_keys.current_session(host_id).is_err());

            let ek_keys = directory_state.keys("ek-member")?;
            let mut changed_ek = ad;
            changed_ek.ek[0] ^= 1;
            assert!(join_host(ek_keys.clone(), &changed_ek, code, None)
                .await
                .is_err());
            assert!(ek_keys.current_session(host_id).is_err());
            assert!(vault.borrow().host.members().iter().all(|peer| {
                *peer != addr_keys.peer_id().unwrap_or(host_id)
                    && *peer != ek_keys.peer_id().unwrap_or(host_id)
            }));
            host_task.abort();
            directory_task.abort();
            Ok(())
        })
        .await
}
