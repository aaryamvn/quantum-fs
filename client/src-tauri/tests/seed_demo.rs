#![cfg(unix)]
//! Headless demo seeder: creates vaults on already-running `qfsd` servers and fills them from a
//! directory tree on disk, through the same [`Node`] the app embeds.
//!
//! The tree comes from `client/scripts/gen-seed-tree.py`, which writes real files of every type
//! the UI draws an icon for. Nothing about the content lives here any more: whatever is under
//! `QFS_SEED_ROOT` is what the vaults end up holding, names and folder structure preserved.
//!
//! ```text
//! <root>/A/01 Product Design/…   -> vault "Product Design" on the 1st connect string
//! <root>/A/02 Engineering/…      -> vault "Engineering"    on the 1st connect string
//! <root>/B/01 Research Lab/…     -> vault "Research Lab"   on the 2nd connect string
//! ```
//!
//! Server directories are matched to connect strings in sorted order, and a vault's name is its
//! directory name with the `NN ` ordering prefix removed.
//!
//! Environment:
//!
//! | variable                 | default                | meaning                                  |
//! |--------------------------|------------------------|------------------------------------------|
//! | `QFS_SEED_SERVERS`       | *(unset: test is inert)* | `A_ADDR/TOKEN,B_ADDR/TOKEN`            |
//! | `QFS_SEED_ROOT`          | `/tmp/qfs-demo/seed`   | the generated tree                       |
//! | `QFS_DATA_DIR`           | `/tmp/qfs-demo/seeder` | app data dir to provision (wiped first)  |
//! | `QFS_DIRECTORY_ADDR`     | see below              | central directory, for join codes        |
//! | `QFS_SEED_QUOTA_BYTES`   | 16 GiB                 | quota requested per vault                |
//!
//! The data dir this leaves behind is a complete client state: `vm-demo.sh provision --vm N
//! --data <dir>` installs it in a guest, which then owns every vault created here. No profile
//! name is set, so that guest still gets the onboarding name screen on first launch.
//!
//! Inert unless `QFS_SEED_SERVERS` is set, so a plain `cargo test` never touches a live server.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

use quantamfs_lib::fs_types::{Availability, CreateNodeInput, CreateVaultInput, FsNode, NodeKind};
use quantamfs_lib::node::Node;

type Outcome = Result<(), Box<dyn std::error::Error>>;

/// The directory the demo servers register with, when the environment does not say.
const DEFAULT_DIRECTORY_ADDR: &str = "172.26.28.115:7440";
const DEFAULT_SEED_ROOT: &str = "/tmp/qfs-demo/seed";
const DEFAULT_DATA_DIR: &str = "/tmp/qfs-demo/seeder";
/// 16 GiB per vault. A server advertising the default 32 GiB capacity only has room for two of
/// these: pass `QFS_SEED_QUOTA_BYTES` (or raise `--capacity-bytes` on the host) for three.
const DEFAULT_QUOTA_BYTES: u64 = 16 * 1024 * 1024 * 1024;

/* ------------------------------------------------------------------ test */

#[test]
fn seed_demo() -> Outcome {
    let Ok(servers) = std::env::var("QFS_SEED_SERVERS") else {
        return Ok(());
    };
    let connects: Vec<String> = servers
        .split(',')
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect();
    if connects.is_empty() {
        return Ok(());
    }
    if std::env::var("QFS_DIRECTORY_ADDR").is_err() {
        std::env::set_var("QFS_DIRECTORY_ADDR", DEFAULT_DIRECTORY_ADDR);
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(seed(connects))
}

fn env_path(key: &str, fallback: &str) -> PathBuf {
    match std::env::var(key) {
        Ok(value) if !value.trim().is_empty() => PathBuf::from(value.trim()),
        _ => PathBuf::from(fallback),
    }
}

async fn seed(connects: Vec<String>) -> Outcome {
    let seed_root = env_path("QFS_SEED_ROOT", DEFAULT_SEED_ROOT);
    let data_dir = env_path("QFS_DATA_DIR", DEFAULT_DATA_DIR);
    let quota = std::env::var("QFS_SEED_QUOTA_BYTES")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_QUOTA_BYTES);

    if !seed_root.is_dir() {
        return Err(format!(
            "seed tree {} does not exist — run: python3 client/scripts/gen-seed-tree.py --out {}",
            seed_root.display(),
            seed_root.display()
        )
        .into());
    }

    // A fresh data dir: this one is handed to a VM whole, so anything left over from a previous
    // run would arrive there as a vault nobody can reach.
    let _ = std::fs::remove_dir_all(&data_dir);
    std::fs::create_dir_all(&data_dir)?;

    let emit: Arc<dyn Fn(&str, Value) + Send + Sync> = Arc::new(|_name: &str, _payload: Value| {});
    let node = Node::start(data_dir.clone(), emit);

    let (server_dirs, _) = read_sorted(&seed_root)?;
    for (index, server_dir) in server_dirs.iter().enumerate() {
        let label = entry_name(server_dir);
        let Some(connect) = connects.get(index) else {
            println!("SKIP {label} — no connect string at position {index}");
            continue;
        };
        let server = match node
            .add_server(format!("Server {label}"), connect.clone())
            .await
        {
            Ok(server) => server,
            Err(error) => {
                println!("SEED ERROR {label} add_server: {error}");
                continue;
            }
        };
        let (vault_dirs, _) = read_sorted(server_dir)?;
        for vault_dir in vault_dirs {
            let name = vault_name(&vault_dir);
            match seed_vault(&node, &server.id, &vault_dir, &name, quota).await {
                Ok((hex, files, bytes)) => {
                    println!("SEEDED {label} {name} {hex} files={files} bytes={bytes}")
                }
                Err((step, error)) => println!("SEED ERROR {label} {name} {step}: {error}"),
            }
        }
    }
    println!("DATA_DIR {}", data_dir.display());
    Ok(())
}

/// One vault, end to end. `Err((step, message))` names the step that failed so the caller can
/// report it and carry on with the next vault.
async fn seed_vault(
    node: &Node,
    server_id: &str,
    source: &Path,
    name: &str,
    quota: u64,
) -> Result<(String, usize, u64), (String, String)> {
    let vault = node
        .create_vault(CreateVaultInput {
            server_id: server_id.to_string(),
            name: name.to_string(),
            quota_bytes: quota,
        })
        .await
        .map_err(|error| ("create_vault".to_string(), error))?;
    let vault_id = vault.id.clone();
    let root_id = format!("root_{vault_id}");

    wait_for(Duration::from_secs(30), || async {
        tree(node, &vault_id)
            .await
            .iter()
            .any(|item| item.id == root_id && item.parent_id.is_none())
    })
    .await
    .map_err(|error| ("hydrate".to_string(), error))?;

    // Depth first, parents before children: `create_node` needs its parent's id, and every call
    // is awaited before the next so the vault never sees an import for a folder it has not made.
    let mut folders = 0usize;
    let mut files = 0usize;
    let mut bytes = 0u64;
    let mut stack: Vec<(PathBuf, String)> = vec![(source.to_path_buf(), root_id)];

    while let Some((dir, parent_id)) = stack.pop() {
        let (child_dirs, child_files) =
            read_sorted(&dir).map_err(|error| ("read_dir".to_string(), error.to_string()))?;

        if !child_files.is_empty() {
            for path in &child_files {
                bytes += std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
            }
            files += child_files.len();
            node.import_files(vault_id.clone(), parent_id.clone(), child_files)
                .await
                .map_err(|error| (format!("import_files({})", dir.display()), error))?;
        }

        for child in child_dirs {
            let folder = entry_name(&child);
            let made = node
                .create_node(CreateNodeInput {
                    vault_id: vault_id.clone(),
                    parent_id: parent_id.clone(),
                    kind: NodeKind::Folder,
                    name: folder.clone(),
                    actor: None,
                })
                .await
                .map_err(|error| (format!("create_node({folder})"), error))?;
            folders += 1;
            stack.push((child, made.id));
        }
    }

    // Every file has to reach `Local` before the data dir is worth copying anywhere: a chunk
    // still in flight is a file the provisioned VM would show as unavailable.
    let want_nodes = 1 + folders + files;
    wait_for(Duration::from_secs(300), || async {
        let items = tree(node, &vault_id).await;
        let local = items
            .iter()
            .filter(|item| item.kind == NodeKind::File && item.availability == Availability::Local)
            .count();
        items.len() >= want_nodes && local >= files
    })
    .await
    .map_err(|error| ("settle".to_string(), error))?;

    Ok((vault_id, files, bytes))
}

/* --------------------------------------------------------------- helpers */

fn entry_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// `01 Product Design` -> `Product Design`. The prefix only fixes the order on disk.
fn vault_name(path: &Path) -> String {
    let raw = entry_name(path);
    match raw.split_once(' ') {
        Some((head, rest)) if !head.is_empty() && head.chars().all(|c| c.is_ascii_digit()) => {
            rest.to_string()
        }
        _ => raw,
    }
}

/// `(directories, files)` under `dir`, each sorted by name, dot-entries dropped so a stray
/// `.DS_Store` never lands in a vault.
fn read_sorted(dir: &Path) -> std::io::Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry_name(&path).starts_with('.') {
            continue;
        }
        if path.is_dir() {
            dirs.push(path);
        } else if path.is_file() {
            files.push(path);
        }
    }
    dirs.sort();
    files.sort();
    Ok((dirs, files))
}

async fn tree(node: &Node, vault_id: &str) -> Vec<FsNode> {
    node.list_tree(vault_id.to_string())
        .await
        .unwrap_or_default()
}

/// Poll until `test` is true or `timeout` elapses. Never panics: a timeout is an error the
/// caller reports and moves past.
async fn wait_for<F, Fut>(timeout: Duration, mut test: F) -> Result<Duration, String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let started = Instant::now();
    loop {
        if test().await {
            return Ok(started.elapsed());
        }
        if started.elapsed() >= timeout {
            return Err(format!("timed out after {timeout:?}"));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
