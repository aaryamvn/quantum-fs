use std::{
    cell::RefCell,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use quantam_fs::{
    ids::PeerId,
    keystore::KeyStore,
    net::{
        directory::{serve, DirForget, DirectoryAd, DirectoryClient, DirectoryStore},
        JoinCode, VaultId,
    },
    Error, Result,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

fn temp_directory() -> Result<PathBuf> {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "qfs-directory-test-{}-{sequence}",
        std::process::id()
    ));
    std::fs::create_dir(&path)?;
    Ok(path)
}

fn addr() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 7447)
}

fn unix_time() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| Error::State("system clock is before Unix epoch"))
}

async fn assert_raw_connection_closed(addr: SocketAddr, bytes: &[u8]) -> Result<()> {
    let mut stream = tokio::net::TcpStream::connect(addr).await?;
    stream.write_all(bytes).await?;
    stream.flush().await?;
    let mut response = [0; 1];
    let read = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut response))
        .await
        .map_err(|_| Error::State("directory did not close invalid frame"))?;
    match read {
        Ok(0) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => Ok(()),
        Ok(_) => Err(Error::State("directory replied to invalid frame")),
        Err(error) => Err(error.into()),
    }
}

#[test]
fn signed_ad_round_trips_through_atomic_owner_only_store() -> Result<()> {
    let directory = temp_directory()?;
    let result = (|| -> Result<()> {
        let keys = KeyStore::open(&directory.join("identity"))?;
        let path = directory.join("directory.bin");
        let code = JoinCode([0x31; 16]);
        let vault = VaultId([0x41; 32]);
        let now = 2_000_000;
        let ad = DirectoryAd::sign(&keys, vault, addr(), now)?;
        let mut store = DirectoryStore::open(&path)?;
        store.put(code, ad.clone(), now)?;
        assert_eq!(store.lookup(&code, now)?, Some(ad.clone()));
        drop(store);

        let mut reopened = DirectoryStore::open(&path)?;
        assert_eq!(reopened.lookup(&code, now)?, Some(ad));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(path)?.permissions().mode() & 0o077, 0);
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(directory);
    result
}

#[test]
fn tampering_bind_and_monotonic_replay_are_rejected() -> Result<()> {
    let directory = temp_directory()?;
    let result = (|| -> Result<()> {
        let keys = KeyStore::open(&directory.join("identity"))?;
        let mut store = DirectoryStore::open(&directory.join("directory.bin"))?;
        let code = JoinCode([0x51; 16]);
        let vault = VaultId([0x61; 32]);
        let now = 3_000_000;
        let ad = DirectoryAd::sign(&keys, vault, addr(), now)?;

        let mut wrong_peer = ad.clone();
        wrong_peer.peer_id = PeerId([0xff; 32]);
        assert!(matches!(
            store.put(code, wrong_peer, now),
            Err(Error::AuthenticationFailed)
        ));

        store.put(code, ad.clone(), now)?;
        assert!(matches!(
            store.put(code, ad.clone(), now),
            Err(Error::ReplayRejected)
        ));
        let forget = DirForget::sign(&keys, vault, code, now + 1)?;
        store.forget(forget, now + 1)?;
        assert!(store.lookup(&code, now + 1)?.is_none());
        assert!(matches!(
            store.put(code, ad, now + 1),
            Err(Error::ReplayRejected)
        ));
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(directory);
    result
}

#[test]
fn expiry_future_skew_and_per_peer_cap_are_enforced() -> Result<()> {
    let directory = temp_directory()?;
    let result = (|| -> Result<()> {
        let keys = KeyStore::open(&directory.join("identity"))?;
        let mut store = DirectoryStore::open(&directory.join("directory.bin"))?;
        let now = 4_000_000;

        let stale = DirectoryAd::sign(
            &keys,
            VaultId([1; 32]),
            addr(),
            now - (7 * 24 * 60 * 60) - 1,
        )?;
        assert!(store.put(JoinCode([1; 16]), stale, now).is_err());
        let future = DirectoryAd::sign(&keys, VaultId([2; 32]), addr(), now + 121)?;
        assert!(store.put(JoinCode([2; 16]), future, now).is_err());

        for value in 0u8..32 {
            let ad = DirectoryAd::sign(&keys, VaultId([value; 32]), addr(), now)?;
            store.put(JoinCode([value; 16]), ad, now)?;
        }
        let extra = DirectoryAd::sign(&keys, VaultId([0xfe; 32]), addr(), now)?;
        assert!(matches!(
            store.put(JoinCode([0xfe; 16]), extra, now),
            Err(Error::InvalidInput(_))
        ));
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(directory);
    result
}

#[tokio::test(flavor = "current_thread")]
async fn tcp_client_put_lookup_forget_and_missing_close() -> Result<()> {
    let directory = temp_directory()?;
    let local = tokio::task::LocalSet::new();
    let result = local
        .run_until(async {
            let keys = KeyStore::open(&directory.join("identity"))?;
            let store = Rc::new(RefCell::new(DirectoryStore::open(
                &directory.join("directory.bin"),
            )?));
            let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
            let server_addr = listener.local_addr()?;
            let server = tokio::task::spawn_local(serve(listener, store));
            let client = DirectoryClient::new(server_addr);
            let code = JoinCode([0x71; 16]);
            let vault = VaultId([0x72; 32]);
            let now = unix_time()?;
            let ad = DirectoryAd::sign(&keys, vault, addr(), now)?;

            assert_raw_connection_closed(server_addr, &[2, 0, 0, 0, 1, 5]).await?;
            assert_raw_connection_closed(server_addr, &[1, 0, 0, 0, 1, 15]).await?;
            assert_raw_connection_closed(server_addr, &[1, 0, 0x10, 0, 1]).await?;

            client.put(code, &ad).await?;
            assert_eq!(client.lookup(code).await?, Some(ad));
            let forget = DirForget::sign(&keys, vault, code, now + 1)?;
            client.forget(&forget).await?;
            assert!(client.lookup(code).await?.is_none());

            server.abort();
            Ok(())
        })
        .await;
    let _ = std::fs::remove_dir_all(directory);
    result
}
