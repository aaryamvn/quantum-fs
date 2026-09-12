use std::{cell::RefCell, fs, net::Ipv4Addr, path::PathBuf, rc::Rc, time::Duration};

use quantam_fs::{
    keystore::KeyStore,
    net::{
        directory::DirectoryAd,
        join::{join_host, serve_host, unix_time, VaultHost, HEARTBEAT_INTERVAL},
    },
    Result,
};
use tokio::{net::TcpListener, task::LocalSet};

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "qfs-net-heartbeat-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| quantam_fs::Error::State("test clock"))?
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

#[test]
fn demo_heartbeat_defaults_to_one_hundred_milliseconds() {
    assert_eq!(HEARTBEAT_INTERVAL, Duration::from_millis(100));
}

#[tokio::test(flavor = "current_thread")]
async fn running_member_observes_host_tree_commit_on_next_poll() -> Result<()> {
    LocalSet::new()
        .run_until(async {
            let data = TestDir::new()?;
            let host_keys = KeyStore::open(&data.0.join("host.identity"))?;
            let member_keys = KeyStore::open(&data.0.join("member.identity"))?;
            let host_id = host_keys.peer_id()?;
            let vault = Rc::new(RefCell::new(VaultHost::load_or_create(
                host_keys.clone(),
                &data.0.join("vault"),
            )?));
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
            let address = listener.local_addr()?;
            let ad =
                DirectoryAd::sign(&host_keys, vault.borrow().vault_id(), address, unix_time()?)?;
            let code = vault.borrow().join_code();
            let server = tokio::task::spawn_local(serve_host(listener, vault.clone()));
            let mut joined = join_host(member_keys, &ad, code, None).await?;
            let replica = joined.replica.clone();
            let runner = tokio::task::spawn_local(async move {
                let _ = joined.run().await;
            });

            tokio::time::sleep(HEARTBEAT_INTERVAL / 2).await;
            vault
                .borrow_mut()
                .host
                .mkdir(host_id, "/heartbeat-visible")?;

            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if replica
                        .borrow()
                        .tree()
                        .resolve("/heartbeat-visible")
                        .is_ok()
                    {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .map_err(|_| quantam_fs::Error::State("heartbeat update was not observed"))?;

            runner.abort();
            server.abort();
            Ok(())
        })
        .await
}
