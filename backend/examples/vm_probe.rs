use std::{
    cell::RefCell,
    collections::BTreeSet,
    io::{self, BufRead},
    net::SocketAddr,
    path::{Path, PathBuf},
    rc::Rc,
    str::FromStr,
    time::{Duration, Instant},
};

use clap::{Parser, Subcommand};
use quantam_fs::{
    crypto::identity::IdentityManager,
    encoding,
    ids::{ChunkId, FileId, PeerId},
    keystore::KeyStore,
    net::{
        directory::DirectoryClient,
        join::{join_host, serve_host, VaultHost, HEARTBEAT_INTERVAL},
        JoinCode,
    },
    protocol::{locate::HaveQuery, pull::PullRequest},
    store::chunks::ChunkStore,
    store::tree::DirectoryTree,
    store::vaults::discover_vaults,
    sync::host::MemberReplica,
    Error, Result,
};
use tokio::{net::TcpListener, task::LocalSet};

#[derive(Parser)]
#[command(about = "Cross-machine integration probe for quantam-fs")]
struct Args {
    #[command(subcommand)]
    mode: Mode,
}

#[derive(Subcommand)]
enum Mode {
    Member {
        #[arg(long)]
        data_dir: PathBuf,
        #[arg(long)]
        directory_addr: SocketAddr,
        #[arg(long)]
        join_code: JoinCode,
    },
    Inspect {
        #[arg(long)]
        data_dir: PathBuf,
        #[arg(long)]
        root_id: String,
        #[arg(long)]
        path: String,
        #[arg(long, default_value_t = 0)]
        wait_ms: u64,
    },
    Host {
        #[arg(long)]
        data_dir: PathBuf,
        #[arg(long)]
        listen_addr: SocketAddr,
        #[arg(long)]
        advertise_addr: Option<SocketAddr>,
        #[arg(long)]
        directory_addr: SocketAddr,
    },
}

fn main() {
    if let Err(error) = run() {
        emit_err("startup", &error.to_string());
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    LocalSet::new().block_on(&runtime, async {
        match Args::parse().mode {
            Mode::Member {
                data_dir,
                directory_addr,
                join_code,
            } => member(data_dir, directory_addr, join_code).await,
            Mode::Host {
                data_dir,
                listen_addr,
                advertise_addr,
                directory_addr,
            } => host(data_dir, listen_addr, advertise_addr, directory_addr).await,
            Mode::Inspect {
                data_dir,
                root_id,
                path,
                wait_ms,
            } => inspect(&data_dir, FileId(parse_id(&root_id)?), &path, wait_ms).await,
        }
    })
}

fn command_receiver() -> std::sync::mpsc::Receiver<String> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(32);
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    receiver
}

async fn member(data_dir: PathBuf, directory_addr: SocketAddr, code: JoinCode) -> Result<()> {
    std::fs::create_dir_all(&data_dir)?;
    let keys = KeyStore::open(&data_dir.join("identity"))?;
    keys.load_or_create()?;
    let directory = DirectoryClient::new(directory_addr);
    let ad = directory
        .lookup(code)
        .await?
        .ok_or(Error::InvalidInput("join code not found"))?;
    let replica = Rc::new(RefCell::new(MemberReplica::open_durable(
        keys.clone(),
        &data_dir,
        FileId(ad.vault_id.0),
        ad.peer_id,
        BTreeSet::from([keys.peer_id()?, ad.peer_id]),
    )?));
    let mut joined = join_host(keys.clone(), &ad, code, Some(replica)).await?;
    emit_ok(
        "ready",
        &format!(
            "\"peer_id\":\"{}\",\"vault_id\":\"{}\"",
            hex(&keys.peer_id()?.0),
            ad.vault_id
        ),
    );

    let commands = command_receiver();
    let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        match commands.try_recv() {
            Ok(line) => {
                if line.trim() == "quit" {
                    emit_ok("quit", "");
                    return Ok(());
                }
                let op = line.split_whitespace().next().unwrap_or("command");
                if let Err(error) = member_command(&mut joined, &keys, &line).await {
                    emit_err(op, &error.to_string());
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                interval.tick().await;
                if let Err(error) = heartbeat(&mut joined).await {
                    emit_err("heartbeat", &error.to_string());
                    return Err(error);
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => return Ok(()),
        }
    }
}

async fn heartbeat(joined: &mut quantam_fs::net::join::JoinedPeer) -> Result<()> {
    let query = HaveQuery::new(FileId([0; 32]), vec![ChunkId([0; 32])])?;
    joined.have_query(&query).await.map(|_| ())
}

async fn member_command(
    joined: &mut quantam_fs::net::join::JoinedPeer,
    keys: &KeyStore,
    line: &str,
) -> Result<()> {
    let fields: Vec<_> = line.split_whitespace().collect();
    let op = fields.first().copied().unwrap_or("");
    match (op, fields.as_slice()) {
        ("id", [_]) => {
            let replica = joined.replica.borrow();
            emit_ok(
                op,
                &format!(
                    "\"peer_id\":\"{}\",\"root_id\":\"{}\",\"last_applied\":{}",
                    hex(&replica.peer_id()?.0),
                    hex(&replica.expected_root().0),
                    replica.last_applied()
                ),
            );
        }
        ("heartbeat", [_]) => {
            heartbeat(joined).await?;
            emit_ok(op, "");
        }
        ("session", [_]) => {
            let host = joined.replica.borrow().host_id();
            emit_ok(
                op,
                &format!("\"epoch\":{}", keys.current_session(host)?.epoch.0),
            );
        }
        ("members", [_]) => {
            let replica = joined.replica.borrow();
            let ids = replica
                .members()
                .iter()
                .map(|id| format!("\"{}\"", hex(&id.0)))
                .collect::<Vec<_>>()
                .join(",");
            emit_ok(
                op,
                &format!(
                    "\"count\":{},\"peer_ids\":[{}]",
                    replica.members().len(),
                    ids
                ),
            );
        }
        ("mkdir", [_, path]) => {
            let id = joined.mkdir(path).await?;
            emit_ok(op, &format!("\"file_id\":\"{}\"", hex(&id.0)));
        }
        ("save", [_, path, chunks, bytes, seed]) => {
            let bodies = payloads(parse(chunks)?, parse(bytes)?, parse(seed)?)?;
            let id = joined.save_file(path, &bodies).await?;
            emit_ok(
                op,
                &format!(
                    "\"file_id\":\"{}\",\"bytes\":{}",
                    hex(&id.0),
                    total(&bodies)
                ),
            );
        }
        ("rename", [_, source, destination]) => {
            joined.rename(source, destination).await?;
            emit_ok(op, "");
        }
        ("unlink", [_, path]) => {
            joined.unlink(path).await?;
            emit_ok(op, "");
        }
        ("status", [_, path]) => status(joined, path, op)?,
        ("evict", [_, path]) => {
            let id = joined.replica.borrow().tree().resolve(path)?;
            let removed = joined.replica.borrow_mut().evict_file(id)?;
            emit_ok(
                op,
                &format!("\"file_id\":\"{}\",\"removed\":{}", hex(&id.0), removed),
            );
        }
        ("restore", [_, path]) => {
            let count = restore(joined, path).await?;
            emit_ok(op, &format!("\"restored\":{}", count));
        }
        ("pull_verify", [_, path, chunks, bytes, seed]) => {
            restore(joined, path).await?;
            verify(joined, path, parse(chunks)?, parse(bytes)?, parse(seed)?)?;
            emit_ok(
                op,
                &format!(
                    "\"bytes\":{}",
                    parse::<usize>(chunks)? * parse::<usize>(bytes)?
                ),
            );
        }
        ("verify_local", [_, path, chunks, bytes, seed]) => {
            verify(joined, path, parse(chunks)?, parse(bytes)?, parse(seed)?)?;
            emit_ok(
                op,
                &format!(
                    "\"bytes\":{}",
                    parse::<usize>(chunks)? * parse::<usize>(bytes)?
                ),
            );
        }
        ("wait", [_, path, wanted, timeout]) if *wanted == "present" || *wanted == "absent" => {
            let present = *wanted == "present";
            let deadline = Instant::now() + Duration::from_millis(parse(timeout)?);
            loop {
                heartbeat(joined).await?;
                if joined.replica.borrow().tree().resolve(path).is_ok() == present {
                    emit_ok(op, &format!("\"present\":{}", present));
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(Error::State("wait deadline exceeded"));
                }
                tokio::time::sleep(HEARTBEAT_INTERVAL).await;
            }
        }
        _ => return Err(Error::InvalidInput("unknown or malformed member command")),
    }
    Ok(())
}

fn status(joined: &quantam_fs::net::join::JoinedPeer, path: &str, op: &str) -> Result<()> {
    let replica = joined.replica.borrow();
    let id = replica.tree().resolve(path)?;
    let is_dir = replica.tree().is_dir(&id);
    let (manifest_chunks, local_chunks) = if let Some(trusted) = replica.trusted_manifest(&id) {
        let ids = &trusted.manifest().chunk_ids;
        let store = replica.chunks();
        let store = store
            .lock()
            .map_err(|_| Error::State("chunk store poisoned"))?;
        (
            ids.len(),
            ids.iter().filter(|chunk| store.has(chunk)).count(),
        )
    } else {
        (0, 0)
    };
    emit_ok(
        op,
        &format!(
            "\"file_id\":\"{}\",\"is_dir\":{},\"manifest_chunks\":{},\"local_chunks\":{}",
            hex(&id.0),
            is_dir,
            manifest_chunks,
            local_chunks
        ),
    );
    Ok(())
}

async fn restore(joined: &mut quantam_fs::net::join::JoinedPeer, path: &str) -> Result<usize> {
    let (ids, trusted) = {
        let replica = joined.replica.borrow();
        let id = replica.tree().resolve(path)?;
        let trusted = replica
            .trusted_manifest(&id)
            .ok_or(Error::State("file has no trusted manifest"))?
            .clone();
        (trusted.manifest().chunk_ids.clone(), trusted)
    };
    let mut restored = 0;
    for batch in ids.chunks(32) {
        restored += joined
            .pull(&PullRequest::new(batch.to_vec())?, &trusted)
            .await?;
    }
    Ok(restored)
}

fn verify(
    joined: &quantam_fs::net::join::JoinedPeer,
    path: &str,
    chunks: usize,
    bytes: usize,
    seed: u64,
) -> Result<()> {
    let expected = payloads(chunks, bytes, seed)?;
    let replica = joined.replica.borrow();
    let id = replica.tree().resolve(path)?;
    let trusted = replica
        .trusted_manifest(&id)
        .ok_or(Error::State("file has no trusted manifest"))?;
    if trusted.manifest().chunk_ids.len() != expected.len() {
        return Err(Error::AuthenticationFailed);
    }
    let store = replica.chunks();
    let store = store
        .lock()
        .map_err(|_| Error::State("chunk store poisoned"))?;
    for (chunk_id, expected) in trusted.manifest().chunk_ids.iter().zip(&expected) {
        if store.get(chunk_id) != Some(expected.as_slice()) {
            return Err(Error::AuthenticationFailed);
        }
    }
    Ok(())
}

async fn host(
    data_dir: PathBuf,
    listen: SocketAddr,
    advertise: Option<SocketAddr>,
    directory: SocketAddr,
) -> Result<()> {
    std::fs::create_dir_all(&data_dir)?;
    let keys = KeyStore::open(&data_dir.join("identity"))?;
    keys.load_or_create()?;
    let listener = TcpListener::bind(listen).await?;
    let address = advertise.unwrap_or(listener.local_addr()?);
    let discovered = discover_vaults(&keys, &data_dir)?;
    let vault_dir = match discovered.as_slice() {
        [] => data_dir.clone(),
        [(_, path)] => path.clone(),
        _ => return Err(Error::InvalidInput("host probe requires exactly one vault")),
    };
    let mut opened = VaultHost::open_durable(keys.clone(), &vault_dir.join("vault"), &vault_dir)?;
    opened
        .publish(&DirectoryClient::new(directory), address)
        .await?;
    let vault = Rc::new(RefCell::new(opened));
    emit_ok(
        "ready",
        &format!(
            "\"peer_id\":\"{}\",\"join_code\":\"{}\",\"listen_addr\":\"{}\"",
            hex(&keys.peer_id()?.0),
            vault.borrow().join_code(),
            address
        ),
    );
    let server = tokio::task::spawn_local(serve_host(listener, vault.clone()));
    let commands = command_receiver();
    loop {
        let line = match commands.try_recv() {
            Ok(line) => line,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                tokio::time::sleep(Duration::from_millis(25)).await;
                continue;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
        };
        let fields: Vec<_> = line.split_whitespace().collect();
        let op = fields.first().copied().unwrap_or("command");
        let result = match fields.as_slice() {
            ["quit"] => {
                emit_ok("quit", "");
                break;
            }
            ["id"] => {
                emit_ok(op, &format!("\"peer_id\":\"{}\"", hex(&keys.peer_id()?.0)));
                Ok(())
            }
            ["code"] => {
                emit_ok(
                    op,
                    &format!("\"join_code\":\"{}\"", vault.borrow().join_code()),
                );
                Ok(())
            }
            ["status", path] => host_status(&vault, path, op),
            ["kick", peer] => match parse_peer(peer) {
                Ok(peer) => {
                    VaultHost::kick(&vault, &DirectoryClient::new(directory), address, peer)
                        .await
                        .map(|id| emit_ok(op, &format!("\"peer_id\":\"{}\"", hex(&id.0))))
                }
                Err(error) => Err(error),
            },
            _ => Err(Error::InvalidInput("unknown or malformed host command")),
        };
        if let Err(error) = result {
            emit_err(op, &error.to_string());
        }
    }
    server.abort();
    Ok(())
}

fn host_status(vault: &Rc<RefCell<VaultHost>>, path: &str, op: &str) -> Result<()> {
    let state = vault.borrow();
    let id = state.host.tree().resolve(path)?;
    emit_ok(
        op,
        &format!(
            "\"file_id\":\"{}\",\"is_dir\":{}",
            hex(&id.0),
            state.host.tree().is_dir(&id)
        ),
    );
    Ok(())
}

async fn inspect(data_dir: &Path, root: FileId, path: &str, wait_ms: u64) -> Result<()> {
    let deadline = Instant::now() + Duration::from_millis(wait_ms);
    loop {
        let bytes = std::fs::read(data_dir.join("replica.bin"))?;
        let (generation, metadata) = encoding::decode_replica(&bytes, root)?;
        let tree = DirectoryTree::from_dirents(root, metadata.dirents.clone())?;
        match tree.resolve(path) {
            Ok(file_id) => {
                let is_dir = tree.is_dir(&file_id);
                let manifest = metadata.manifests.get(&file_id);
                let manifest_chunks = manifest.map_or(0, |value| value.chunk_ids.len());
                let mut local_chunks = 0;
                for chunk_id in manifest.into_iter().flat_map(|value| &value.chunk_ids) {
                    let Some((indexed_file, index)) = metadata.chunk_index.get(chunk_id) else {
                        continue;
                    };
                    let Ok(plaintext) =
                        std::fs::read(data_dir.join("chunks").join(hex(&chunk_id.0)))
                    else {
                        continue;
                    };
                    if *indexed_file == file_id
                        && encoding::chunk_id(indexed_file, *index, &plaintext) == *chunk_id
                    {
                        local_chunks += 1;
                    }
                }
                emit_ok(
                    "inspect",
                    &format!(
                        "\"generation\":{generation},\"name_present\":true,\"file_id\":\"{}\",\"is_dir\":{is_dir},\"manifest_chunks\":{manifest_chunks},\"local_chunks\":{local_chunks},\"members\":{}",
                        hex(&file_id.0),
                        metadata.members.len()
                    ),
                );
                return Ok(());
            }
            Err(_) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(_) => {
                emit_ok(
                    "inspect",
                    &format!(
                        "\"generation\":{generation},\"name_present\":false,\"members\":{}",
                        metadata.members.len()
                    ),
                );
                return Ok(());
            }
        }
    }
}

fn payloads(chunks: usize, bytes: usize, seed: u64) -> Result<Vec<Vec<u8>>> {
    if chunks == 0
        || bytes == 0
        || bytes > 1024 * 1024
        || chunks
            .checked_mul(bytes)
            .is_none_or(|total| total > 64 * 1024 * 1024)
    {
        return Err(Error::InvalidInput(
            "payload requires positive chunks, 1..=1048576 bytes each, and at most 64 MiB total",
        ));
    }
    let mut state = seed;
    Ok((0..chunks)
        .map(|index| {
            let mut body = vec![0; bytes];
            state ^= (index as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
            for byte in &mut body {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *byte = state as u8;
            }
            body
        })
        .collect())
}

fn total(bodies: &[Vec<u8>]) -> usize {
    bodies.iter().map(Vec::len).sum()
}
fn parse<T: FromStr>(value: &str) -> Result<T> {
    value
        .parse()
        .map_err(|_| Error::InvalidInput("invalid numeric argument"))
}
fn parse_peer(value: &str) -> Result<PeerId> {
    Ok(PeerId(parse_id(value)?))
}
fn parse_id(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::InvalidInput(
            "peer id must be 64 lowercase hex characters",
        ));
    }
    let mut bytes = [0; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        let pair = &value.as_bytes()[index * 2..index * 2 + 2];
        let text = std::str::from_utf8(pair).map_err(|_| Error::InvalidInput("invalid peer id"))?;
        *byte = u8::from_str_radix(text, 16).map_err(|_| Error::InvalidInput("invalid peer id"))?;
    }
    Ok(bytes)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn escaped(value: &str) -> String {
    let mut output = String::new();
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character <= '\u{1f}' => {
                output.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => output.push(character),
        }
    }
    output
}
fn emit_ok(op: &str, fields: &str) {
    println!(
        "{{\"ok\":true,\"op\":\"{}\"{}}}",
        escaped(op),
        if fields.is_empty() {
            String::new()
        } else {
            format!(",{fields}")
        }
    );
}
fn emit_err(op: &str, error: &str) {
    println!(
        "{{\"ok\":false,\"op\":\"{}\",\"error\":\"{}\"}}",
        escaped(op),
        escaped(error)
    );
}
