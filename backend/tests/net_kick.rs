use std::{cell::RefCell, fs, net::Ipv4Addr, path::PathBuf, rc::Rc, time::Duration};

use quantam_fs::{
    ids::{ChunkId, FileId},
    keystore::{IdentityKeyStore, KeyStore},
    net::{
        directory::{serve, DirectoryClient, DirectoryStore},
        join::{join_host, serve_host, VaultHost},
    },
    protocol::locate::HaveQuery,
    Error, Result,
};
use tokio::{net::TcpListener, task::LocalSet};

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "qfs-net-kick-{name}-{}-{}",
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

#[tokio::test(flavor = "current_thread")]
async fn historical_welcome_does_not_restore_a_kicked_peer() -> Result<()> {
    LocalSet::new()
        .run_until(async {
            let data = TestDir::new("historical-denied")?;
            let (directory, directory_task) = directory(&data).await?;
            let host_keys = KeyStore::open(&data.0.join("host.identity"))?;
            let a_keys = KeyStore::open(&data.0.join("a.identity"))?;
            let b_keys = KeyStore::open(&data.0.join("b.identity"))?;
            let b_id = b_keys.peer_id()?;
            let mut vault = VaultHost::load_or_create(host_keys, &data.0.join("vault"))?;
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
            let address = listener.local_addr()?;
            let ad = vault.publish(&directory, address).await?;
            let code = vault.join_code();
            let vault = Rc::new(RefCell::new(vault));
            let server = tokio::task::spawn_local(serve_host(listener, vault.clone()));
            let mut a = join_host(a_keys.clone(), &ad, code, None).await?;
            let _b = join_host(b_keys, &ad, code, None).await?;
            let query = HaveQuery::new(FileId([9; 32]), vec![ChunkId([10; 32])])?;

            a.have_query(&query).await?;
            assert!(a_keys.load_verified_peer(&b_id).is_ok());
            VaultHost::kick(&vault, &directory, address, b_id).await?;
            assert!(tokio::time::timeout(Duration::from_millis(200), a.run())
                .await
                .is_err());

            assert!(a_keys.load_verified_peer(&b_id).is_err());
            assert!(a_keys.current_session(b_id).is_err());

            server.abort();
            directory_task.abort();
            Ok(())
        })
        .await
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

async fn directory(path: &TestDir) -> Result<(DirectoryClient, tokio::task::JoinHandle<()>)> {
    let store = Rc::new(RefCell::new(DirectoryStore::open(
        &path.0.join("directory.bin"),
    )?));
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let client = DirectoryClient::new(listener.local_addr()?);
    let task = tokio::task::spawn_local(async move {
        let _ = serve(listener, store).await;
    });
    Ok((client, task))
}

#[tokio::test(flavor = "current_thread")]
async fn kick_closes_live_tcp_and_denies_the_rotated_code() -> Result<()> {
    LocalSet::new()
        .run_until(async {
            let data = TestDir::new("disconnect")?;
            let (directory, directory_task) = directory(&data).await?;
            let host_keys = KeyStore::open(&data.0.join("host.identity"))?;
            let member_keys = KeyStore::open(&data.0.join("member.identity"))?;
            let mut vault = VaultHost::load_or_create(host_keys, &data.0.join("vault"))?;
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
            let address = listener.local_addr()?;
            let ad = vault.publish(&directory, address).await?;
            let old_code = vault.join_code();
            let vault = Rc::new(RefCell::new(vault));
            let server = tokio::task::spawn_local(serve_host(listener, vault.clone()));
            let mut joined = join_host(member_keys.clone(), &ad, old_code, None).await?;
            let member_id = member_keys.peer_id()?;

            assert_eq!(
                VaultHost::kick(&vault, &directory, address, member_id).await?,
                member_id
            );
            // The running server drains take_disconnects itself; assert the
            // resulting TCP close rather than racing it for the queue entry.
            tokio::time::timeout(Duration::from_secs(1), joined.run())
                .await
                .map_err(|_| Error::State("kicked TCP connection remained live"))?
                .expect_err("kicked TCP connection must close");

            let kicked_code = vault.borrow().join_code();
            assert_ne!(kicked_code, old_code);
            assert!(directory.lookup(old_code).await?.is_none());
            assert!(directory.lookup(kicked_code).await?.is_some());
            let new_code = VaultHost::rotate_code(&vault, &directory, address).await?;
            assert_ne!(new_code, kicked_code);
            assert!(directory.lookup(kicked_code).await?.is_none());
            let new_ad = directory
                .lookup(new_code)
                .await?
                .ok_or(Error::State("new directory ad missing"))?;
            assert!(join_host(member_keys, &new_ad, new_code, None)
                .await
                .is_err());
            assert!(!vault.borrow().host.has_member(&member_id));

            server.abort();
            directory_task.abort();
            Ok(())
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn directory_failure_leaves_new_local_admission_authoritative() -> Result<()> {
    LocalSet::new()
        .run_until(async {
            let data = TestDir::new("directory-failure")?;
            let host_keys = KeyStore::open(&data.0.join("host.identity"))?;
            let member_keys = KeyStore::open(&data.0.join("member.identity"))?;
            let member_id = member_keys.peer_id()?;
            let vault = Rc::new(RefCell::new(VaultHost::load_or_create(
                host_keys,
                &data.0.join("vault"),
            )?));
            vault
                .borrow_mut()
                .host
                .add_member(member_keys.identity()?)?;
            let old_code = vault.borrow().join_code();
            let unused = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
            let address = unused.local_addr()?;
            drop(unused);
            let unavailable_directory = DirectoryClient::new(address);

            assert!(
                VaultHost::kick(&vault, &unavailable_directory, address, member_id)
                    .await
                    .is_err()
            );
            assert_ne!(vault.borrow().join_code(), old_code);
            assert!(!vault.borrow().host.has_member(&member_id));
            Ok(())
        })
        .await
}
