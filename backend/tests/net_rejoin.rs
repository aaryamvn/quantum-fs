use std::{cell::RefCell, fs, net::Ipv4Addr, path::PathBuf, rc::Rc, time::Duration};

use quantam_fs::{
    keystore::KeyStore,
    net::{
        directory::{serve, DirectoryClient, DirectoryStore},
        join::{join_host, serve_host, VaultHost},
    },
    Error, Result,
};
use tokio::{net::TcpListener, task::LocalSet, time::sleep};

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "qfs-net-rejoin-{name}-{}-{}",
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

/// A rotation (here caused by a kick) must not lock out the members that are
/// still in the vault: the code only controls entry of new identities.
#[tokio::test(flavor = "current_thread")]
async fn rotated_code_admits_current_members_and_still_gates_new_identities() -> Result<()> {
    LocalSet::new()
        .run_until(async {
            let data = TestDir::new("rotated-code")?;
            let (directory, directory_task) = directory(&data).await?;
            let host_keys = KeyStore::open(&data.0.join("host.identity"))?;
            let a_keys = KeyStore::open(&data.0.join("a.identity"))?;
            let b_keys = KeyStore::open(&data.0.join("b.identity"))?;
            let c_keys = KeyStore::open(&data.0.join("c.identity"))?;
            let a_id = a_keys.peer_id()?;
            let b_id = b_keys.peer_id()?;
            let c_id = c_keys.peer_id()?;
            let mut vault = VaultHost::load_or_create(host_keys, &data.0.join("vault"))?;
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
            let address = listener.local_addr()?;
            let ad = vault.publish(&directory, address).await?;
            let old_code = vault.join_code();
            let vault = Rc::new(RefCell::new(vault));
            let server = tokio::task::spawn_local(serve_host(listener, vault.clone()));

            let a = join_host(a_keys.clone(), &ad, old_code, None).await?;
            let a_replica = a.replica.clone();
            let b = join_host(b_keys.clone(), &ad, old_code, None).await?;
            assert!(vault.borrow().host.has_member(&a_id));
            assert!(vault.borrow().host.has_member(&b_id));

            // The kick rotates the join code; A joined with the old one.
            assert_eq!(
                VaultHost::kick(&vault, &directory, address, b_id).await?,
                b_id
            );
            let rotated_code = vault.borrow().join_code();
            assert_ne!(rotated_code, old_code);
            drop(b);
            drop(a);
            sleep(Duration::from_millis(150)).await;

            // A is still a member, so its stale code is accepted.
            let rejoined_a =
                join_host(a_keys.clone(), &ad, old_code, Some(a_replica.clone())).await?;
            assert!(vault.borrow().host.has_member(&a_id));

            // The kicked identity is denied permanently, under either code.
            assert!(join_host(b_keys.clone(), &ad, old_code, None)
                .await
                .is_err());
            assert!(join_host(b_keys, &ad, rotated_code, None).await.is_err());
            assert!(!vault.borrow().host.has_member(&b_id));

            // A new identity still needs the current code.
            assert!(join_host(c_keys.clone(), &ad, old_code, None)
                .await
                .is_err());
            assert!(!vault.borrow().host.has_member(&c_id));
            let joined_c = join_host(c_keys, &ad, rotated_code, None).await?;
            assert!(vault.borrow().host.has_member(&c_id));

            drop(joined_c);
            drop(rejoined_a);
            server.abort();
            directory_task.abort();
            Ok(())
        })
        .await
}
