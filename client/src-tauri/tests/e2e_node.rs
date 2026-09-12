#![cfg(unix)]
//! Headless end-to-end proof of the embedded node against real `qfsd` servers.
//!
//! WHY this test exists: every other check in the client is a unit test against shapes. This one
//! runs the whole seam the app runs (docs/decisions/client-backend-embed.md): one `qfsd`
//! directory process, one `qfsd` host process with its token-gated admin port, and two in-process
//! [`Node`] runtimes with separate data directories and separate identities. It proves the loop
//! the UI depends on — add server, create vault, join by code, mutate the tree, see the mutation
//! on the other node, import and pull real bytes, kick, leave — without a webview.
//!
//! Nothing is mocked: the two nodes speak the sealed peer protocol to the host, the join code is
//! resolved through the directory, and the file bytes are pulled chunk by chunk.

use std::io::{BufRead, BufReader, Read};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use quantamfs_lib::fs_types::{
    Availability, CreateNodeInput, CreateVaultInput, FolderColor, FsNode, Member, NodeKind,
    RenameNodeInput, Role, SetNodeColorInput,
};
use quantamfs_lib::node::Node;

type Outcome = Result<(), Box<dyn std::error::Error>>;

/// The vault this test provisions: the smallest a server will hand out.
const QUOTA_BYTES: u64 = 268_435_456;
/// The imported file's size. Big enough to be chunked (512 KiB per chunk), small enough to be
/// prefetched by the receiving node (under `vault::PREFETCH_MAX_BYTES`).
const PAYLOAD_BYTES: usize = 3 * 1024 * 1024;

/* --------------------------------------------------------------- waiting */

/// Poll `$test` (an expression that may `.await`) until it is true or `$timeout` elapses.
/// A timeout dumps every event both nodes emitted before panicking, so a failure names the
/// state the runtime actually reached rather than "assertion failed".
macro_rules! wait_for {
    ($label:expr, $timeout:expr, $test:expr) => {
        wait_for!($label, $timeout, $test, ())
    };
    ($label:expr, $timeout:expr, $test:expr, $diagnose:expr) => {{
        let started = Instant::now();
        loop {
            if $test {
                break started.elapsed();
            }
            if started.elapsed() >= $timeout {
                $diagnose;
                dump_events();
                panic!("timed out after {:?} waiting for {}", $timeout, $label);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }};
}

/* ------------------------------------------------------------ event sink */

/// One node's `emit` callback, captured. `Node::start` wants an `Arc<dyn Fn + Send + Sync>`;
/// the app hands it a Tauri emit, this hands it a growing log.
#[derive(Clone)]
struct Events {
    label: &'static str,
    log: Arc<Mutex<Vec<(String, Value)>>>,
}

/// Every sink built in this process, so a timeout anywhere can print all of them.
static SINKS: Mutex<Vec<Events>> = Mutex::new(Vec::new());

impl Events {
    fn new(label: &'static str) -> (Self, Arc<dyn Fn(&str, Value) + Send + Sync>) {
        let sink = Events {
            label,
            log: Arc::new(Mutex::new(Vec::new())),
        };
        if let Ok(mut all) = SINKS.lock() {
            all.push(sink.clone());
        }
        let log = sink.log.clone();
        let emit: Arc<dyn Fn(&str, Value) + Send + Sync> =
            Arc::new(move |name: &str, payload: Value| {
                if let Ok(mut events) = log.lock() {
                    events.push((name.to_string(), payload));
                }
            });
        (sink, emit)
    }

    /// How many events have arrived; the mark a later `since` call reads from.
    fn mark(&self) -> usize {
        self.log.lock().map(|events| events.len()).unwrap_or(0)
    }

    fn since(&self, mark: usize) -> Vec<(String, Value)> {
        self.log
            .lock()
            .map(|events| events.get(mark..).map(<[_]>::to_vec).unwrap_or_default())
            .unwrap_or_default()
    }
}

fn dump_events() {
    let Ok(all) = SINKS.lock() else { return };
    for sink in all.iter() {
        let events = sink
            .log
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default();
        eprintln!("---- node {} emitted {} events:", sink.label, events.len());
        let tail = events.len().saturating_sub(30);
        for (name, payload) in &events[tail..] {
            eprintln!("     {name} {}", brief(payload));
        }
    }
}

fn brief(payload: &Value) -> String {
    let text = payload.to_string();
    if text.len() <= 360 {
        return text;
    }
    format!("{}…", &text[..360])
}

/// The nodes of every `upsert` in one `backend://fs-changed` payload.
fn upserts(payload: &Value) -> Vec<&Value> {
    payload
        .get("changes")
        .and_then(Value::as_array)
        .map(|changes| {
            changes
                .iter()
                .filter(|change| change.get("kind").and_then(Value::as_str) == Some("upsert"))
                .filter_map(|change| change.get("node"))
                .collect()
        })
        .unwrap_or_default()
}

/// Did this slice of events carry an `fs-changed` upsert of a node called `name`?
fn saw_upsert(events: &[(String, Value)], vault_id: &str, name: &str) -> bool {
    events
        .iter()
        .filter(|(event, _)| event == "backend://fs-changed")
        .filter(|(_, payload)| {
            payload.get("vaultId").and_then(Value::as_str) == Some(vault_id)
        })
        .any(|(_, payload)| {
            upserts(payload)
                .iter()
                .any(|node| node.get("name").and_then(Value::as_str) == Some(name))
        })
}

/// Did any upsert report a partial download? `progress` is only set while `downloading`.
fn saw_progress(events: &[(String, Value)], node_id: &str) -> bool {
    events
        .iter()
        .filter(|(event, _)| event == "backend://fs-changed")
        .any(|(_, payload)| {
            upserts(payload).iter().any(|node| {
                node.get("id").and_then(Value::as_str) == Some(node_id)
                    && node.get("progress").map(|value| !value.is_null()) == Some(true)
            })
        })
}

/* ------------------------------------------------------------ temp files */

/// One directory for everything this test writes: both daemons' data dirs, both nodes' data
/// dirs and the file that gets imported.
struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!("qfs-e2e-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/* --------------------------------------------------------------- daemons */

/// A `qfsd` child process whose stderr is scraped for the lines it prints on startup.
/// Same shape as `backend/tests/admin.rs`, which is the reference for driving the daemon.
struct Daemon {
    child: Child,
    lines: Vec<String>,
    receiver: Receiver<String>,
}

impl Daemon {
    fn spawn(
        exe: &PathBuf,
        data_dir: &PathBuf,
        arguments: &[&str],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut command = Command::new(exe);
        command
            .arg("--data-dir")
            .arg(data_dir)
            .args(arguments)
            .env("NO_COLOR", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        let stderr = child.stderr.take().ok_or("qfsd stderr was not piped")?;
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                match line {
                    Ok(line) => {
                        if sender.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            child,
            lines: Vec::new(),
            receiver,
        })
    }

    /// Waits for the plain stderr line that carries text after `marker`; the demo-log block
    /// prints the same headline with nothing following it.
    fn wait_for_value(&mut self, marker: &str) -> Result<String, Box<dyn std::error::Error>> {
        let found = |line: &str| {
            line.split_once(marker)
                .map(|(_, rest)| rest.trim().to_owned())
                .filter(|rest| !rest.is_empty())
        };
        if let Some(value) = self.lines.iter().find_map(|line| found(line)) {
            return Ok(value);
        }
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(format!(
                    "timed out waiting for {marker:?}; qfsd output: {:?}",
                    self.lines
                )
                .into());
            }
            let line = self.receiver.recv_timeout(remaining)?;
            self.lines.push(line.clone());
            if let Some(value) = found(&line) {
                return Ok(value);
            }
        }
    }
}

/// Both daemons die with the test, panic or not.
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The host's own answer to `STATUS`, printed when a membership assertion times out: it is the
/// only way to tell "the app never noticed" from "the host never did it". Blocking on purpose —
/// it runs from the panic path, not from the happy path.
fn print_host_status(admin_addr: &str, token: &str) {
    use std::io::Write;
    use std::net::TcpStream;

    let answer = (|| -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let stream = TcpStream::connect(admin_addr)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        let mut reader = BufReader::new(stream);
        for command in [format!("AUTH {token}"), "STATUS".to_string()] {
            reader
                .get_mut()
                .write_all(format!("{command}\n").as_bytes())?;
            reader.get_mut().flush()?;
        }
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 {
                return Ok(lines);
            }
            let line = line.trim_end().to_string();
            if line == "END" {
                return Ok(lines);
            }
            lines.push(line);
            if lines.len() > 256 {
                return Ok(lines);
            }
        }
    })();
    match answer {
        Ok(lines) => {
            eprintln!("---- host STATUS at {admin_addr}:");
            for line in lines {
                eprintln!("     {line}");
            }
        }
        Err(error) => eprintln!("---- host STATUS at {admin_addr} failed: {error}"),
    }
}

/// Reserves a loopback port by binding and dropping: the daemon then binds it at a known
/// number, which an ephemeral listener could not report back.
fn free_port() -> Result<u16, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

/// The `qfsd` binary. `CARGO_BIN_EXE_*` only covers the current package's own binaries, so the
/// backend's debug build is located relative to this manifest and built here.
///
/// The build is unconditional (it costs nothing when the binary is current): the servers and the
/// in-process nodes speak one protocol, and a `qfsd` left over from an older `backend/src` fails
/// in ways that look like client bugs — a member that never hydrates, a kick nobody hears.
fn qfsd_binary() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let backend = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("backend")
        .canonicalize()?;
    let target = backend.join("target");
    let exe = target.join("debug").join("qfsd");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let status = Command::new(cargo)
        .args(["build", "--bin", "qfsd", "--target-dir"])
        .arg(&target)
        .current_dir(&backend)
        .env_remove("CARGO_TARGET_DIR")
        .status()?;
    if !status.success() {
        return Err("cargo build --bin qfsd failed; the backend does not compile".into());
    }
    if !exe.exists() {
        return Err(format!("{} is missing after the build", exe.display()).into());
    }
    Ok(exe)
}

/* --------------------------------------------------------------- payload */

/// `PAYLOAD_BYTES` of random printable ASCII. Random so the byte comparison after the pull is
/// meaningful; printable so the assembled copy can be read back through `read_text_preview`,
/// which is the only public door onto the bytes the chunk store reassembled.
fn text_payload() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789 \n";
    let mut raw = vec![0u8; PAYLOAD_BYTES];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut raw)?;
    Ok(raw
        .iter()
        .map(|byte| ALPHABET[(*byte & 63) as usize])
        .collect())
}

/// FNV-1a, purely so the log line proves the two byte strings were compared.
fn checksum(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/* ----------------------------------------------------------------- steps */

fn step(clock: &mut Instant, message: String) {
    println!("[{:>6} ms] {message}", clock.elapsed().as_millis());
    *clock = Instant::now();
}

async fn tree(node: &Node, vault_id: &str) -> Vec<FsNode> {
    node.list_tree(vault_id.to_string())
        .await
        .unwrap_or_default()
}

async fn by_id(node: &Node, vault_id: &str, node_id: &str) -> Option<FsNode> {
    tree(node, vault_id)
        .await
        .into_iter()
        .find(|item| item.id == node_id)
}

/* ------------------------------------------------------------------ test */

#[test]
fn two_nodes_and_real_servers_share_one_vault() -> Outcome {
    // The node runtimes own their own threads; the test only needs a runtime to await replies
    // and to sleep between polls, so a current-thread runtime is enough (and the crate does not
    // depend on tokio's multi-thread feature).
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(scenario())
}

async fn scenario() -> Outcome {
    let mut clock = Instant::now();
    let root = TestDir::new()?;
    let exe = qfsd_binary()?;
    step(&mut clock, format!("qfsd binary at {}", exe.display()));

    /* -- 1. a real directory and a real host, both on loopback ------------ */

    let directory_addr = format!("127.0.0.1:{}", free_port()?);
    let host_addr = format!("127.0.0.1:{}", free_port()?);
    let admin_addr = format!("127.0.0.1:{}", free_port()?);

    let mut directory = Daemon::spawn(
        &exe,
        &root.join("directory"),
        &["--directory", "--listen-addr", &directory_addr],
    )?;
    let printed = directory.wait_for_value("qfsd: directory address ")?;
    assert_eq!(printed, directory_addr);

    let mut host = Daemon::spawn(
        &exe,
        &root.join("host"),
        &[
            "--listen-addr",
            &host_addr,
            "--admin-addr",
            &admin_addr,
            "--directory-addr",
            &directory_addr,
        ],
    )?;
    let connect = host.wait_for_value("qfsd: app connect string ")?;
    let (printed_admin, token) = connect
        .split_once('/')
        .ok_or("connect string is not ADDRESS/TOKEN")?;
    assert_eq!(printed_admin, admin_addr);
    assert_eq!(token.len(), 20);
    step(
        &mut clock,
        format!("step 1: directory {directory_addr} + host {host_addr}, connect string {connect}"),
    );

    /* -- 2. node A adds the server and creates a vault -------------------- */

    let (a_events, a_emit) = Events::new("A");
    let (b_events, b_emit) = Events::new("B");
    let node_a = Node::start(root.join("a"), a_emit);
    let node_b = Node::start(root.join("b"), b_emit);

    let server_a = node_a
        .add_server("Server A".to_string(), connect.clone())
        .await
        .map_err(|error| format!("A.add_server: {error}"))?;
    assert!(server_a.online, "the server A just added is offline");
    assert!(
        server_a.capacity_bytes > 0,
        "server capacity was {}",
        server_a.capacity_bytes
    );
    assert!(server_a.vaults.is_empty(), "a fresh host has no vaults yet");

    let vault = node_a
        .create_vault(CreateVaultInput {
            server_id: server_a.id.clone(),
            name: "E2E".to_string(),
            quota_bytes: QUOTA_BYTES,
        })
        .await
        .map_err(|error| format!("A.create_vault: {error}"))?;
    assert_eq!(vault.role, Role::Owner);
    assert_eq!(vault.name, "E2E");
    assert_eq!(vault.quota_bytes, QUOTA_BYTES);

    let root_id = format!("root_{}", vault.id);
    let waited = wait_for!("A's replica to expose the vault root", Duration::from_secs(15), {
        by_id(&node_a, &vault.id, &root_id)
            .await
            .is_some_and(|node| node.name == "E2E" && node.parent_id.is_none())
    });
    step(
        &mut clock,
        format!(
            "step 2: vault {} created; A's root projected in {} ms",
            &vault.id[..16],
            waited.as_millis()
        ),
    );

    /* -- 3. node B joins with the code A's host printed ------------------- */

    let meta = node_a
        .get_vault_meta(vault.id.clone())
        .await
        .map_err(|error| format!("A.get_vault_meta: {error}"))?;
    // Length is deliberately not pinned: the host has issued both the 26-character Base32 code
    // and the 6-character short code during this task. What has to hold is that B joins with it.
    assert!(
        !meta.join_code.trim().is_empty(),
        "the vault has no join code"
    );

    let server_b = node_b
        .add_server("Server A".to_string(), connect.clone())
        .await
        .map_err(|error| format!("B.add_server: {error}"))?;
    assert!(server_b.online);
    assert_eq!(server_b.peer_id, server_a.peer_id);

    let joined = node_b
        .join_vault(meta.join_code.clone())
        .await
        .map_err(|error| format!("B.join_vault: {error}"))?;
    assert_eq!(joined.id, vault.id);
    assert_eq!(joined.role, Role::Member);

    // The name proves more than the id does: B's record carries no name, so "E2E" can only come
    // from the replicated `.qfs-meta.json` A wrote — that is, from a real hydration.
    let waited = wait_for!(
        "B's replica to expose the vault root",
        Duration::from_secs(30),
        {
            by_id(&node_b, &vault.id, &root_id)
                .await
                .is_some_and(|node| node.name == "E2E" && node.parent_id.is_none())
        },
        print_host_status(&admin_addr, token)
    );
    step(
        &mut clock,
        format!(
            "step 3: B joined with the {}-character code {:?} and hydrated in {} ms",
            meta.join_code.chars().count(),
            meta.join_code,
            waited.as_millis()
        ),
    );

    /* -- 4. a folder A creates appears on B ------------------------------- */

    let mark = b_events.mark();
    let created_at = Instant::now();
    let docs = node_a
        .create_node(CreateNodeInput {
            vault_id: vault.id.clone(),
            parent_id: root_id.clone(),
            kind: NodeKind::Folder,
            name: "Docs".to_string(),
            actor: None,
        })
        .await
        .map_err(|error| format!("A.create_node(Docs): {error}"))?;
    assert_eq!(docs.parent_id.as_deref(), Some(root_id.as_str()));
    assert_eq!(docs.kind, NodeKind::Folder);

    wait_for!("B's fs-changed upsert of Docs", Duration::from_secs(10), {
        saw_upsert(&b_events.since(mark), &vault.id, "Docs")
    });
    let latency = created_at.elapsed();
    let mirrored = by_id(&node_b, &vault.id, &docs.id)
        .await
        .ok_or("B's tree has no node with the id A created")?;
    assert_eq!(mirrored.name, "Docs");
    assert_eq!(mirrored.parent_id.as_deref(), Some(root_id.as_str()));
    assert_eq!(mirrored.kind, NodeKind::Folder);
    step(
        &mut clock,
        format!("step 4: A created Docs, B saw it {} ms later", latency.as_millis()),
    );

    /* -- 5. rename, colour and an empty file, each observed on B ---------- */

    let mark = b_events.mark();
    let renamed = node_a
        .rename_node(RenameNodeInput {
            vault_id: vault.id.clone(),
            node_id: docs.id.clone(),
            name: "Papers".to_string(),
            actor: None,
        })
        .await
        .map_err(|error| format!("A.rename_node: {error}"))?;
    assert_eq!(renamed.name, "Papers");
    let papers_id = renamed.id.clone();
    let waited = wait_for!("B to see the rename to Papers", Duration::from_secs(10), {
        saw_upsert(&b_events.since(mark), &vault.id, "Papers")
            && by_id(&node_b, &vault.id, &papers_id)
                .await
                .is_some_and(|node| node.name == "Papers")
    });
    step(&mut clock, format!("step 5a: rename seen by B in {} ms", waited.as_millis()));

    let coloured = node_a
        .set_node_color(SetNodeColorInput {
            vault_id: vault.id.clone(),
            node_id: papers_id.clone(),
            color: Some(FolderColor::Coral),
            actor: None,
        })
        .await
        .map_err(|error| format!("A.set_node_color: {error}"))?;
    assert_eq!(coloured.color, Some(FolderColor::Coral));
    let waited = wait_for!(
        "B to see the coral colour through the replicated sidecar",
        Duration::from_secs(20),
        {
            by_id(&node_b, &vault.id, &papers_id)
                .await
                .is_some_and(|node| node.color == Some(FolderColor::Coral))
        }
    );
    step(&mut clock, format!("step 5b: colour seen by B in {} ms", waited.as_millis()));

    let mark = b_events.mark();
    let notes = node_a
        .create_node(CreateNodeInput {
            vault_id: vault.id.clone(),
            parent_id: papers_id.clone(),
            kind: NodeKind::File,
            name: "notes.txt".to_string(),
            actor: None,
        })
        .await
        .map_err(|error| format!("A.create_node(notes.txt): {error}"))?;
    assert_eq!(notes.kind, NodeKind::File);
    assert_eq!(notes.size_bytes, 0);
    assert_eq!(notes.parent_id.as_deref(), Some(papers_id.as_str()));
    let notes_id = notes.id.clone();
    let waited = wait_for!("B to see notes.txt", Duration::from_secs(10), {
        saw_upsert(&b_events.since(mark), &vault.id, "notes.txt")
            && by_id(&node_b, &vault.id, &notes_id).await.is_some()
    });
    step(
        &mut clock,
        format!("step 5c: empty file seen by B in {} ms", waited.as_millis()),
    );

    /* -- 6. import 3 MiB on A, pull the bytes on B ------------------------ */

    let payload = text_payload()?;
    let payload_path = root.join("payload.txt");
    std::fs::write(&payload_path, &payload)?;

    let mark = b_events.mark();
    let imported = node_a
        .import_files(
            vault.id.clone(),
            papers_id.clone(),
            vec![payload_path.clone()],
        )
        .await
        .map_err(|error| format!("A.import_files: {error}"))?;
    assert_eq!(imported.len(), 1, "import produced {} nodes", imported.len());
    let file = imported[0].clone();
    assert_eq!(file.name, "payload.txt");
    assert_eq!(file.size_bytes, PAYLOAD_BYTES as u64);
    assert_eq!(file.availability, Availability::Local);
    let file_id = file.id.clone();

    wait_for!("B to see payload.txt", Duration::from_secs(20), {
        saw_upsert(&b_events.since(mark), &vault.id, "payload.txt")
            && by_id(&node_b, &vault.id, &file_id).await.is_some()
    });
    let seen = by_id(&node_b, &vault.id, &file_id)
        .await
        .ok_or("B's tree lost payload.txt")?;
    assert_eq!(seen.size_bytes, PAYLOAD_BYTES as u64);
    assert!(
        matches!(
            seen.availability,
            Availability::Remote | Availability::Downloading | Availability::Local
        ),
        "unexpected availability {:?}",
        seen.availability
    );
    let was_remote = seen.availability != Availability::Local;
    step(
        &mut clock,
        format!(
            "step 6a: A imported {} bytes (checksum {:016x}); B sees it as {:?}",
            PAYLOAD_BYTES,
            checksum(&payload),
            seen.availability
        ),
    );

    node_b
        .request_download(vault.id.clone(), file_id.clone())
        .await
        .map_err(|error| format!("B.request_download: {error}"))?;
    let waited = wait_for!("B's copy to become local", Duration::from_secs(60), {
        by_id(&node_b, &vault.id, &file_id)
            .await
            .is_some_and(|node| node.availability == Availability::Local)
    });
    let progressed = saw_progress(&b_events.since(mark), &file_id);
    step(
        &mut clock,
        format!(
            "step 6b: B pulled the bytes in {} ms (started {}; progress upserts observed: {})",
            waited.as_millis(),
            if was_remote { "remote" } else { "already local" },
            progressed
        ),
    );

    // The chunk store is the proof the bytes really crossed: B's vault directory now holds them.
    let chunks = root.join("b").join("vaults").join(&vault.id).join("chunks");
    let stored = std::fs::read_dir(&chunks)
        .map_err(|error| format!("B has no chunk store at {}: {error}", chunks.display()))?
        .filter_map(Result::ok)
        .count();
    assert!(stored > 0, "B's chunk store at {} is empty", chunks.display());

    // `open_node` would hand the assembled copy to the OS (`open`), which a headless test must
    // not do; `read_text_preview` runs the same `assemble()` over the same chunk store, so the
    // bytes are verified without launching an application.
    let assembled = node_b
        .read_text_preview(vault.id.clone(), file_id.clone(), PAYLOAD_BYTES)
        .await
        .map_err(|error| format!("B.read_text_preview: {error}"))?
        .ok_or("B could not assemble payload.txt")?;
    assert_eq!(
        assembled.len(),
        PAYLOAD_BYTES,
        "assembled {} bytes, imported {PAYLOAD_BYTES}",
        assembled.len()
    );
    assert_eq!(
        checksum(assembled.as_bytes()),
        checksum(&payload),
        "the bytes B assembled differ from the file A imported"
    );
    assert!(assembled.as_bytes() == payload.as_slice());
    step(
        &mut clock,
        format!(
            "step 6c: B assembled {stored} chunks into {} bytes, checksum {:016x} — identical",
            assembled.len(),
            checksum(assembled.as_bytes())
        ),
    );

    /* -- 7. members, presence, and the kick ------------------------------- */

    let waited = wait_for!(
        "A to list B as an online member",
        Duration::from_secs(20),
        {
            let members = node_a
                .list_members(vault.id.clone())
                .await
                .unwrap_or_default();
            others(&members, &server_a.peer_id)
                .iter()
                .any(|member| member.online)
        },
        print_host_status(&admin_addr, token)
    );
    let members = node_a
        .list_members(vault.id.clone())
        .await
        .map_err(|error| format!("A.list_members: {error}"))?;
    assert_eq!(
        members.iter().filter(|member| member.is_self).count(),
        1,
        "exactly one member is self: {members:#?}"
    );
    let peers = others(&members, &server_a.peer_id);
    assert_eq!(
        peers.len(),
        1,
        "A should see exactly one other member besides the host: {members:#?}"
    );
    let b_peer = peers[0].peer_id.clone();
    assert!(peers[0].online, "B is offline in A's member list");
    step(
        &mut clock,
        format!(
            "step 7a: A lists {} members ({} incl. the host); B is {} and online after {} ms",
            members.len(),
            members
                .iter()
                .map(|member| member.name.clone())
                .collect::<Vec<_>>()
                .join(", "),
            &b_peer[..8],
            waited.as_millis()
        ),
    );

    let mark = b_events.mark();
    node_a
        .remove_member(vault.id.clone(), b_peer.clone())
        .await
        .map_err(|error| format!("A.remove_member: {error}"))?;
    let waited = wait_for!(
        "B to be told its membership is gone",
        Duration::from_secs(30),
        {
            b_events.since(mark).iter().any(|(event, payload)| {
                event == "backend://vault-removed"
                    && payload.get("vaultId").and_then(Value::as_str) == Some(vault.id.as_str())
            })
        },
        print_host_status(&admin_addr, token)
    );
    let shrank = wait_for!(
        "B to leave A's member list",
        Duration::from_secs(30),
        {
            let members = node_a
                .list_members(vault.id.clone())
                .await
                .unwrap_or_default();
            !members.iter().any(|member| member.peer_id == b_peer)
        },
        print_host_status(&admin_addr, token)
    );
    assert!(
        node_b.list_tree(vault.id.clone()).await.is_err(),
        "B still has a task for a vault it was removed from"
    );
    step(
        &mut clock,
        format!(
            "step 7b: kick — B notified in {} ms, A's list dropped it in {} ms",
            waited.as_millis(),
            shrank.as_millis()
        ),
    );

    /* -- 8. A leaves; the server stays, its vault list empties ------------ */

    node_a
        .leave_vault(vault.id.clone())
        .await
        .map_err(|error| format!("A.leave_vault: {error}"))?;
    let servers = node_a
        .list_servers()
        .await
        .map_err(|error| format!("A.list_servers: {error}"))?;
    let listed = servers
        .iter()
        .find(|server| server.id == server_a.id)
        .ok_or("A forgot the server when it left the vault")?;
    assert!(
        listed.vaults.is_empty(),
        "A still lists {} vault(s) on the server",
        listed.vaults.len()
    );
    assert!(node_a.list_tree(vault.id.clone()).await.is_err());
    step(
        &mut clock,
        format!(
            "step 8: A left; server {:?} online={} with {} vaults",
            listed.name,
            listed.online,
            listed.vaults.len()
        ),
    );

    println!(
        "A emitted {} events, B emitted {} events",
        a_events.mark(),
        b_events.mark()
    );
    Ok(())
}

/// Everyone in a member list who is neither this node nor the vault server. `members()` always
/// includes H (it owns the replica) and self, so the other peers are what is left.
fn others<'a>(members: &'a [Member], host_peer: &str) -> Vec<&'a Member> {
    members
        .iter()
        .filter(|member| !member.is_self && member.peer_id != host_peer)
        .collect()
}
