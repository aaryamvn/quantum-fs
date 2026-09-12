use std::{cell::RefCell, fs, path::PathBuf, rc::Rc, time::Duration};

use quantam_fs::{
    crypto::sign::{PureMlDsa, RustCryptoPureMlDsa, MANIFEST_CONTEXT},
    encoding,
    ids::FileId,
    keystore::KeyStore,
    net::{
        directory::DirectoryAd,
        join::{join_host, serve_host, unix_time, VaultHost},
    },
    protocol::manifest::Manifest,
    store::chunks::ChunkStore,
    sync::host::ControlUpdate,
    Error, Result,
};
use tokio::{net::TcpListener, task::LocalSet};

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "qfs-net-reconnect-{}-{}",
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

fn signed_manifest(
    keys: &KeyStore,
    host: &VaultHost,
    file_id: FileId,
    plaintext: &[u8],
) -> Result<Manifest> {
    let chunks = host.host.chunks();
    let chunk_id = chunks
        .lock()
        .map_err(|_| Error::State("test chunk lock poisoned"))?
        .put(&file_id, 0, plaintext.to_vec());
    let mut manifest = Manifest {
        file_id,
        chunk_ids: vec![chunk_id],
        size: plaintext.len() as u64,
        writer_id: keys.peer_id()?,
        version: 1,
        signature: Vec::new(),
    };
    manifest.signature = RustCryptoPureMlDsa.sign(
        &keys.signing_key()?,
        MANIFEST_CONTEXT,
        &encoding::manifest_m(&manifest)?,
    )?;
    Ok(manifest)
}

#[tokio::test(flavor = "current_thread")]
async fn restart_rejoins_reseals_large_queue_and_flushes_before_live() -> Result<()> {
    LocalSet::new()
        .run_until(async {
            let directory = TestDir::new()?;
            let host_path = directory.0.join("host.identity");
            let member_path = directory.0.join("member.identity");
            let vault_path = directory.0.join("vault.state");
            let host_keys = KeyStore::open(&host_path)?;
            let member_keys = KeyStore::open(&member_path)?;
            let host = Rc::new(RefCell::new(VaultHost::load_or_create(
                host_keys.clone(),
                &vault_path,
            )?));
            let listener = TcpListener::bind("127.0.0.1:0").await?;
            let address = listener.local_addr()?;
            let ad =
                DirectoryAd::sign(&host_keys, host.borrow().vault_id(), address, unix_time()?)?;
            let code = host.borrow().join_code();
            let server = tokio::task::spawn_local(serve_host(listener, host.clone()));
            let joined = join_host(member_keys.clone(), &ad, code, None).await?;
            let replica = joined.replica.clone();
            let host_id = host_keys.peer_id()?;
            let member_id = member_keys.peer_id()?;
            let first_epoch = member_keys.current_session(host_id)?.epoch;
            drop(joined);
            host.borrow_mut()
                .host
                .heartbeat(member_id, Duration::ZERO)?;

            for _ in 0..1_025 {
                host.borrow_mut()
                    .host
                    .fan_out_control(host_id, &ControlUpdate::Remove(FileId([31; 32])))?;
            }
            let plaintext = b"queued across host restart";
            let file_id = FileId([77; 32]);
            let manifest = signed_manifest(&host_keys, &host.borrow(), file_id, plaintext)?;
            host.borrow_mut().host.commit(manifest)?;
            assert_eq!(host.borrow_mut().host.mailbox(member_id)?.len(), 1_027);
            assert!(host_keys.require_live_traffic(member_id).is_err());

            server.abort();
            let _ = server.await;
            tokio::time::timeout(Duration::from_secs(1), async {
                while Rc::strong_count(&host) != 1 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .map_err(|_| Error::State("host connection did not close after EOF"))?;
            let old = Rc::try_unwrap(host)
                .map_err(|_| Error::State("host server retained state after abort"))?
                .into_inner();
            let state = old.host.into_state();
            drop(old.keys);
            drop(host_keys);

            let reopened_keys = KeyStore::open(&host_path)?;
            let resumed = Rc::new(RefCell::new(VaultHost::resume(
                reopened_keys.clone(),
                &vault_path,
                state,
            )?));
            let listener = TcpListener::bind("127.0.0.1:0").await?;
            let address = listener.local_addr()?;
            let new_ad = DirectoryAd::sign(
                &reopened_keys,
                resumed.borrow().vault_id(),
                address,
                unix_time()?,
            )?;
            let server = tokio::task::spawn_local(serve_host(listener, resumed.clone()));
            assert!(reopened_keys.require_live_traffic(member_id).is_err());

            let rejoined =
                join_host(member_keys.clone(), &new_ad, code, Some(replica.clone())).await?;
            let new_epoch = member_keys.current_session(host_id)?.epoch;
            assert!(new_epoch > first_epoch);
            assert_eq!(reopened_keys.current_session(member_id)?.epoch, new_epoch);
            reopened_keys.require_live_traffic(member_id)?;
            assert!(resumed.borrow_mut().host.mailbox(member_id)?.is_empty());
            assert_eq!(replica.borrow().instruction_log().len(), 1_026);
            let chunks = replica.borrow().chunks();
            let chunk_id = encoding::chunk_id(&file_id, 0, plaintext);
            assert_eq!(
                chunks
                    .lock()
                    .map_err(|_| Error::State("test chunk lock poisoned"))?
                    .get(&chunk_id),
                Some(plaintext.as_slice())
            );
            drop(rejoined);
            server.abort();
            Ok(())
        })
        .await
}
