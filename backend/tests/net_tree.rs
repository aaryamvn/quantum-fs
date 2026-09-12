use std::{cell::RefCell, fs, path::PathBuf, rc::Rc};

use quantam_fs::{
    keystore::KeyStore,
    net::{
        directory::DirectoryAd,
        join::{join_host, serve_host, unix_time, VaultHost},
    },
    store::chunks::ChunkStore,
    Error, Result,
};
use tokio::{net::TcpListener, task::LocalSet};

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "qfs-net-tree-{}-{}",
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

#[tokio::test(flavor = "current_thread")]
async fn non_host_member_mkdir_save_rename_and_unlink_reach_h() -> Result<()> {
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

            member.mkdir("/a").await?;
            let bodies = vec![b"first half".to_vec(), b"second half".to_vec()];
            let file_id = member.save_file("/a/f", &bodies).await?;
            assert_eq!(host.borrow().host.tree().resolve("/a/f")?, file_id);
            let trusted = host
                .borrow()
                .host
                .trusted_manifest(&file_id)
                .ok_or(Error::State("host manifest missing"))?
                .clone();
            {
                let chunks = host.borrow().host.chunks();
                let chunks = chunks
                    .lock()
                    .map_err(|_| Error::State("test chunk lock poisoned"))?;
                assert_eq!(
                    chunks.get(&trusted.manifest().chunk_ids[0]),
                    Some(bodies[0].as_slice())
                );
                assert_eq!(
                    chunks.get(&trusted.manifest().chunk_ids[1]),
                    Some(bodies[1].as_slice())
                );
            }

            member.rename("/a/f", "/a/g").await?;
            assert!(host.borrow().host.tree().resolve("/a/f").is_err());
            assert_eq!(host.borrow().host.tree().resolve("/a/g")?, file_id);
            member.unlink("/a/g").await?;
            assert!(host.borrow().host.tree().resolve("/a/g").is_err());

            server.abort();
            Ok(())
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn failed_atomic_new_file_snapshot_publishes_neither_manifest_nor_link() -> Result<()> {
    LocalSet::new()
        .run_until(async {
            let directory = TestDir::new()?;
            let host_keys = KeyStore::open(&directory.0.join("host.identity"))?;
            let member_keys = KeyStore::open(&directory.0.join("member.identity"))?;
            let host = Rc::new(RefCell::new(VaultHost::open_durable(
                host_keys.clone(),
                &directory.0.join("vault.state"),
                &directory.0,
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

            let replica_path = directory.0.join("replica.bin");
            fs::remove_file(&replica_path)?;
            fs::create_dir(&replica_path)?;
            assert!(member
                .save_file("/must-not-appear", &[b"unpublished".to_vec()])
                .await
                .is_err());
            tokio::task::yield_now().await;
            assert!(host
                .borrow()
                .host
                .tree()
                .resolve("/must-not-appear")
                .is_err());
            assert!(host.borrow().host.instruction_log().is_empty());

            server.abort();
            Ok(())
        })
        .await
}
