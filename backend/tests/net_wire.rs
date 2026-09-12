use std::{
    cell::{Cell, RefCell},
    fs,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    rc::Rc,
    time::Duration,
};

use quantam_fs::{
    crypto::sign::{PureMlDsa, RustCryptoPureMlDsa, MANIFEST_CONTEXT},
    encoding,
    ids::FileId,
    keystore::KeyStore,
    net::{
        directory::DirectoryAd,
        frame::{read_frame, write_frame, Frame, FRAME_VERSION, GCM_CHUNK_KIND, GCM_PACKET_KIND},
        join::{join_host, serve_host, unix_time, VaultHost},
    },
    protocol::manifest::Manifest,
    store::chunks::ChunkStore,
    Error, Result,
};
use tokio::net::{tcp::OwnedReadHalf, tcp::OwnedWriteHalf, TcpListener, TcpStream};

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "qfs-net-wire-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| Error::State("test clock"))?
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

struct Proxy {
    addr: SocketAddr,
    captured_before_gcm: Rc<RefCell<Vec<u8>>>,
    task: tokio::task::JoinHandle<()>,
}

type IndexedPlaintexts = Vec<(u64, Vec<u8>)>;

async fn proxy(target: SocketAddr, tamper_second_chunk: bool) -> Result<Proxy> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let addr = listener.local_addr()?;
    let captured = Rc::new(RefCell::new(Vec::new()));
    let capture_task = captured.clone();
    let task = tokio::task::spawn_local(async move {
        let result = async {
            let (downstream, _) = listener.accept().await?;
            let upstream = TcpStream::connect(target).await?;
            let (down_read, down_write) = downstream.into_split();
            let (up_read, up_write) = upstream.into_split();
            let first_gcm = Rc::new(Cell::new(false));
            let chunks = Rc::new(Cell::new(0usize));
            let toward_host = relay(
                down_read,
                up_write,
                capture_task.clone(),
                first_gcm.clone(),
                false,
                false,
                chunks.clone(),
            );
            let toward_member = relay(
                up_read,
                down_write,
                capture_task,
                first_gcm,
                true,
                tamper_second_chunk,
                chunks,
            );
            let _ = tokio::try_join!(toward_host, toward_member);
            Ok::<(), Error>(())
        }
        .await;
        let _ = result;
    });
    Ok(Proxy {
        addr,
        captured_before_gcm: captured,
        task,
    })
}

async fn relay(
    mut reader: OwnedReadHalf,
    mut writer: OwnedWriteHalf,
    captured: Rc<RefCell<Vec<u8>>>,
    first_gcm: Rc<Cell<bool>>,
    server_to_member: bool,
    tamper_second_chunk: bool,
    chunks: Rc<Cell<usize>>,
) -> Result<()> {
    let mut duplicate_ack = None;
    loop {
        let mut frame = match read_frame(&mut reader).await {
            Ok(frame) => frame,
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(())
            }
            Err(error) => return Err(error),
        };
        if !first_gcm.get() {
            if frame.kind == GCM_PACKET_KIND {
                first_gcm.set(true);
            } else {
                append_wire(&mut captured.borrow_mut(), &frame)?;
            }
        }
        if server_to_member && frame.kind == GCM_CHUNK_KIND {
            let next = chunks.get() + 1;
            chunks.set(next);
            if tamper_second_chunk && next == 2 {
                let last = frame
                    .payload
                    .last_mut()
                    .ok_or(Error::State("chunk frame unexpectedly empty"))?;
                *last ^= 1;
            }
        }
        if frame.kind == quantam_fs::net::frame::WRAP_ACK_KIND {
            duplicate_ack = Some(frame.clone());
        }
        if matches!(
            frame.kind,
            GCM_PACKET_KIND | GCM_CHUNK_KIND | quantam_fs::net::frame::FLUSH_SIGNATURE_KIND
        ) {
            if let Some(ack) = &duplicate_ack {
                // Delayed duplicate acknowledgments are harmless in Join,
                // drain and live traffic phases, in either direction.
                write_frame(&mut writer, ack).await?;
            }
        }
        write_frame(&mut writer, &frame).await?;
    }
}

fn append_wire(out: &mut Vec<u8>, frame: &Frame) -> Result<()> {
    let body_len = u32::try_from(frame.payload.len() + 1)
        .map_err(|_| Error::InvalidInput("test frame too large"))?;
    out.push(FRAME_VERSION);
    out.extend_from_slice(&body_len.to_be_bytes());
    out.push(frame.kind);
    out.extend_from_slice(&frame.payload);
    Ok(())
}

fn assert_canonical_capture(mut bytes: &[u8]) {
    let mut frames = 0;
    while !bytes.is_empty() {
        assert!(bytes.len() >= 6);
        assert_eq!(bytes[0], FRAME_VERSION);
        let body_len = u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize;
        assert!(body_len >= 1);
        assert!(bytes.len() >= 5 + body_len);
        assert!((1..GCM_PACKET_KIND).contains(&bytes[5]));
        bytes = &bytes[5 + body_len..];
        frames += 1;
    }
    assert!(frames >= 4);
}

fn signed_two_chunk_manifest(
    keys: &KeyStore,
    host: &VaultHost,
    file_id: FileId,
) -> Result<(Manifest, IndexedPlaintexts)> {
    let plaintexts = vec![
        (0, b"first queued body".to_vec()),
        (1, b"second queued body".to_vec()),
    ];
    let chunks = host.host.chunks();
    let mut store = chunks
        .lock()
        .map_err(|_| Error::State("test chunk lock poisoned"))?;
    let chunk_ids = plaintexts
        .iter()
        .map(|(index, plaintext)| store.put(&file_id, *index, plaintext.clone()))
        .collect();
    drop(store);
    let mut manifest = Manifest {
        file_id,
        chunk_ids,
        size: plaintexts.iter().map(|(_, body)| body.len() as u64).sum(),
        writer_id: keys.peer_id()?,
        version: 1,
        signature: Vec::new(),
    };
    manifest.signature = RustCryptoPureMlDsa.sign(
        &keys.signing_key()?,
        MANIFEST_CONTEXT,
        &encoding::manifest_m(&manifest)?,
    )?;
    Ok((manifest, plaintexts))
}

#[tokio::test(flavor = "current_thread")]
async fn actual_handshake_keeps_raw_join_code_out_of_pre_gcm_frames() -> Result<()> {
    tokio::task::LocalSet::new()
        .run_until(async {
            let files = TestDir::new("capture")?;
            let first_keys = files.keys("host.identity")?;
            let second_keys = files.keys("member.identity")?;
            let (host_keys, member_keys) = if first_keys.peer_id()? > second_keys.peer_id()? {
                (first_keys, second_keys)
            } else {
                (second_keys, first_keys)
            };
            let host = Rc::new(RefCell::new(VaultHost::load_or_create(
                host_keys.clone(),
                &files.0.join("vault.state"),
            )?));
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
            let target = listener.local_addr()?;
            let server = tokio::task::spawn_local(serve_host(listener, host.clone()));
            let proxy = proxy(target, false).await?;
            let ad = DirectoryAd::sign(
                &host_keys,
                host.borrow().vault_id(),
                proxy.addr,
                unix_time()?,
            )?;
            let code = host.borrow().join_code();
            let mut joined = join_host(member_keys, &ad, code, None).await?;
            {
                let captured = proxy.captured_before_gcm.borrow();
                assert!(!captured
                    .windows(code.0.len())
                    .any(|window| window == code.0));
                assert_canonical_capture(&captured);
            }
            assert!(
                tokio::time::timeout(Duration::from_millis(100), joined.run())
                    .await
                    .is_err()
            );
            drop(joined);
            proxy.task.abort();
            server.abort();
            Ok(())
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
#[allow(clippy::await_holding_refcell_ref)]
async fn corrupted_second_chunk_keeps_atomic_replica_and_retries_same_mailbox() -> Result<()> {
    tokio::task::LocalSet::new()
        .run_until(async {
            let files = TestDir::new("retry")?;
            let host_keys = files.keys("host.identity")?;
            let member_keys = files.keys("member.identity")?;
            let host = Rc::new(RefCell::new(VaultHost::load_or_create(
                host_keys.clone(),
                &files.0.join("vault.state"),
            )?));
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
            let target = listener.local_addr()?;
            let direct_ad =
                DirectoryAd::sign(&host_keys, host.borrow().vault_id(), target, unix_time()?)?;
            let code = host.borrow().join_code();
            let server = tokio::task::spawn_local(serve_host(listener, host.clone()));

            let initial = join_host(member_keys.clone(), &direct_ad, code, None).await?;
            let replica = initial.replica.clone();
            let member_id = member_keys.peer_id()?;
            drop(initial);
            tokio::task::yield_now().await;
            host.borrow_mut()
                .host
                .heartbeat(member_id, Duration::ZERO)?;

            let file_id = FileId([0x51; 32]);
            let (manifest, plaintexts) =
                signed_two_chunk_manifest(&host_keys, &host.borrow(), file_id)?;
            host.borrow_mut().host.commit(manifest)?;
            assert_eq!(host.borrow_mut().host.mailbox(member_id)?.len(), 3);

            let corrupting = proxy(target, true).await?;
            let proxy_ad = DirectoryAd::sign(
                &host_keys,
                host.borrow().vault_id(),
                corrupting.addr,
                unix_time()?,
            )?;
            assert!(
                join_host(member_keys.clone(), &proxy_ad, code, Some(replica.clone()))
                    .await
                    .is_err()
            );
            tokio::task::yield_now().await;

            assert!(replica.borrow().instruction_log().is_empty());
            assert_eq!(replica.borrow().chunks().lock().unwrap().len(), 0);
            assert_eq!(host.borrow_mut().host.mailbox(member_id)?.len(), 3);
            assert!(host_keys.require_live_traffic(member_id).is_err());
            assert!(member_keys
                .require_live_traffic(host_keys.peer_id()?)
                .is_err());

            corrupting.task.abort();
            let retried =
                join_host(member_keys.clone(), &direct_ad, code, Some(replica.clone())).await?;
            assert_eq!(replica.borrow().instruction_log().len(), 1);
            let chunks = replica.borrow().chunks();
            let chunks = chunks.lock().unwrap();
            for (index, plaintext) in plaintexts {
                let id = encoding::chunk_id(&file_id, index, &plaintext);
                assert_eq!(chunks.get(&id), Some(plaintext.as_slice()));
            }
            drop(chunks);
            assert!(host.borrow_mut().host.mailbox(member_id)?.is_empty());
            host_keys.require_live_traffic(member_id)?;
            member_keys.require_live_traffic(host_keys.peer_id()?)?;

            drop(retried);
            server.abort();
            Ok(())
        })
        .await
}
