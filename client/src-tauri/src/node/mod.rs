//! The embedded backend node: one OS thread, one member node per joined vault.
//!
//! WHY this exists: the daemon has no IPC and cannot be stopped programmatically, so the desktop
//! app links the protocol core and runs the member side in process
//! (docs/decisions/client-backend-embed.md). `MemberReplica`, `JoinedPeer` and `KeyStore` are all
//! `Rc`/`RefCell`-based and deliberately not `Send`, so everything they touch lives on one thread
//! with a current-thread Tokio runtime and a `LocalSet`.
//!
//! [`Node`] is the only thing the Tauri commands see. It is a `Send + Sync` handle over an mpsc
//! channel: every method sends one request and awaits one reply, so the webview's calls never
//! block the UI thread and never reach the replica directly.
//!
//! Shape of the runtime:
//! ```text
//! Tauri commands ── Node ──mpsc──> root loop ──mpsc──> vault task (per vault)
//!                                      │  ^                 │
//!                                      │  └── admin poller ──┘ (STATUS every 1000 ms)
//!                                      └── spawn_local per long request (admin/TCP work)
//! ```
//! The root loop owns the server list, the profile and the recents; each vault task owns its
//! identity, replica, history and live host connection. Events go out through one `emit`
//! callback the app hands in at startup.
//!
//! WHY the root state is `Rc<RefCell<RootState>>`: anything that talks to a host over TCP
//! (`add_server`, `join_vault`, `create_vault`, `rotate_join_code`, `remove_member`,
//! `delete_vault`) can take seconds against a dead server. Those requests run as their own
//! `spawn_local` task and borrow the state only in short, await-free stretches; the loop itself
//! does nothing but dispatch, so `VaultTx` lookups — every tree, member and presence call the UI
//! makes — are answered immediately while a join is still in flight.

mod admin;
mod names;
mod state;
mod vault;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;

use quantam_fs::crypto::identity::IdentityManager;
use quantam_fs::keystore::KeyStore;
use quantam_fs::net::directory::DirectoryClient;
use quantam_fs::net::JoinCode;

use crate::fs_types::{
    AgentReply, AskAgentInput, CreateNodeInput, CreateVaultInput, DaemonStatus, DeleteNodesInput,
    DuplicateNodesInput, FsNode, HistoryEvent, Member, MemberRole, MoveNodesInput, NodeAccess,
    PeerPresence, PresenceInput, Recent, RenameNodeInput, Role, Server, SetAccessInput,
    SetNodeColorInput, Vault, VaultIdPayload, VaultMeta, VaultMetaPatch, VaultRemovedPayload,
};

use admin::{AdminConn, AdminStatus, AdminTarget};
use names::{hex, initials, now_ms, parse_join_input, server_id_for};
use state::{Profile, RecentRecord, ServerRecord, VaultRecord, VaultRole};
use vault::{VaultReq, VaultSnapshot};

/// Every reply is `Result<T, String>`, the `String` being the sentence the UI shows.
pub type Reply<T> = oneshot::Sender<Result<T, String>>;

/// The member heartbeat interval the protocol uses (`net::join::HEARTBEAT_INTERVAL`).
pub const TICK_INTERVAL: Duration = Duration::from_millis(100);
/// How often every admin-capable host is asked for `STATUS`.
const ADMIN_POLL_INTERVAL: Duration = Duration::from_millis(1000);
/// A single missed `STATUS` is usually one dropped packet; two in a row is an offline server.
const ADMIN_FAILURES_BEFORE_OFFLINE: u32 = 2;
/// How often the cache of a vault with no `STATUS` numbers is re-read for its own.
const LOCAL_NUMBERS_INTERVAL: Duration = Duration::from_millis(2000);
/// A vault task must answer `Stop` before its directory is moved aside.
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
/// A host's admin listener sits this far above its peer port (the `qfsd` default), which is
/// how a code join can reach the admin port of a server the user never added.
const ADMIN_PORT_OFFSET: u16 = 1000;
/// A `staging-*` directory older than this is the leftover of a crashed `create_vault`.
const STAGING_TTL: Duration = Duration::from_secs(3600);
/// Smallest vault a server will provision: 256 MiB.
const MIN_QUOTA_BYTES: u64 = 268_435_456;
/// Capacity shown for a server we cannot run admin commands against: 128 GiB.
const DEFAULT_CAPACITY_BYTES: u64 = 137_438_953_472;
/// Each online member lends the vault its own disk, discounted for churn: 0.85 * 8 GiB.
const MEMBER_CREDIT_BYTES: f64 = 0.85 * 8.0 * 1024.0 * 1024.0 * 1024.0;

/* ------------------------------------------------------------- requests */

/// One unit of work for the root loop. Vault-scoped calls only look a sender up here and then
/// talk to the vault task directly, so a slow pull in one vault cannot stall the others.
pub enum Req {
    Status(Reply<DaemonStatus>),
    Me(Reply<Member>),
    ListServers(Reply<Vec<Server>>),
    AddServer(String, String, Reply<Server>),
    CreateVault(CreateVaultInput, Reply<Vault>),
    JoinVault(String, Reply<Vault>),
    VaultTx(String, Reply<UnboundedSender<VaultReq>>),
    Recents(Reply<Vec<RecentRecord>>),
    TouchRecentAck(String, String, Reply<()>),
    RotateJoinCode(String, Reply<String>),
    RemoveMember(String, String, Reply<()>),
    DeleteVault(String, Reply<()>),
    LeaveVault(String, Reply<()>),
    /* ---- internal ---- */
    /// The poller asking what to poll.
    AdminTargets(oneshot::Sender<Vec<(String, AdminTarget)>>),
    /// One host's answer (or the reason there was none).
    AdminResult(String, Result<Box<AdminStatus>, String>),
    /// A vault task found its membership revoked.
    VaultRevoked { vault_id: String, reason: String },
    /// A vault's sidecar name changed, so the cached copy in `vault.json` is stale.
    VaultNameChanged { vault_id: String, name: String },
    /// `open_node` finished; keep the sidebar's recents current without a round trip.
    TouchRecent { vault_id: String, node_id: String },
}

/* ----------------------------------------------------------------- handle */

/// Handle to the node runtime. Cloning is cheap: it is one channel sender.
#[derive(Clone)]
pub struct Node {
    tx: UnboundedSender<Req>,
}

impl Node {
    /// Start the runtime on its own thread and return at once. The thread lives as long as the
    /// app: `Node` is stored in Tauri's managed state and never dropped before shutdown.
    pub fn start(data_dir: PathBuf, emit: Arc<dyn Fn(&str, Value) + Send + Sync>) -> Node {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = Node { tx: tx.clone() };
        std::thread::Builder::new()
            .name("qfs-node".to_string())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(_) => return,
                };
                let local = tokio::task::LocalSet::new();
                local.block_on(&runtime, run(data_dir, emit, tx, rx));
            })
            .ok();
        handle
    }

    async fn call<T>(&self, make: impl FnOnce(Reply<T>) -> Req) -> Result<T, String> {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(make(reply))
            .map_err(|_| "The local backend is not running".to_string())?;
        answer
            .await
            .map_err(|_| "The local backend stopped responding".to_string())?
    }

    async fn vault_call<T>(
        &self,
        vault_id: &str,
        make: impl FnOnce(Reply<T>) -> VaultReq,
    ) -> Result<T, String> {
        let tx = self
            .call(|reply| Req::VaultTx(vault_id.to_string(), reply))
            .await?;
        let (reply, answer) = oneshot::channel();
        tx.send(make(reply))
            .map_err(|_| format!("Unknown vault: {vault_id}"))?;
        answer
            .await
            .map_err(|_| format!("Unknown vault: {vault_id}"))?
    }

    /* ------------------------------------------------------------ servers */

    pub async fn status(&self) -> Result<DaemonStatus, String> {
        self.call(Req::Status).await
    }

    pub async fn list_servers(&self) -> Result<Vec<Server>, String> {
        self.call(Req::ListServers).await
    }

    pub async fn add_server(&self, name: String, address: String) -> Result<Server, String> {
        self.call(|reply| Req::AddServer(name, address, reply)).await
    }

    pub async fn create_vault(&self, input: CreateVaultInput) -> Result<Vault, String> {
        self.call(|reply| Req::CreateVault(input, reply)).await
    }

    pub async fn join_vault(&self, code: String) -> Result<Vault, String> {
        self.call(|reply| Req::JoinVault(code, reply)).await
    }

    pub async fn me(&self) -> Result<Member, String> {
        self.call(Req::Me).await
    }

    /* --------------------------------------------------------------- tree */

    pub async fn list_tree(&self, vault_id: String) -> Result<Vec<FsNode>, String> {
        self.vault_call(&vault_id, VaultReq::ListTree).await
    }

    pub async fn create_node(&self, input: CreateNodeInput) -> Result<FsNode, String> {
        let vault_id = input.vault_id.clone();
        self.vault_call(&vault_id, |reply| VaultReq::CreateNode(input, reply))
            .await
    }

    pub async fn rename_node(&self, input: RenameNodeInput) -> Result<FsNode, String> {
        let vault_id = input.vault_id.clone();
        self.vault_call(&vault_id, |reply| VaultReq::RenameNode(input, reply))
            .await
    }

    pub async fn move_nodes(&self, input: MoveNodesInput) -> Result<Vec<FsNode>, String> {
        let vault_id = input.vault_id.clone();
        self.vault_call(&vault_id, |reply| VaultReq::MoveNodes(input, reply))
            .await
    }

    pub async fn delete_nodes(&self, input: DeleteNodesInput) -> Result<(), String> {
        let vault_id = input.vault_id.clone();
        self.vault_call(&vault_id, |reply| VaultReq::DeleteNodes(input, reply))
            .await
    }

    pub async fn duplicate_nodes(&self, input: DuplicateNodesInput) -> Result<Vec<FsNode>, String> {
        let vault_id = input.vault_id.clone();
        self.vault_call(&vault_id, |reply| VaultReq::DuplicateNodes(input, reply))
            .await
    }

    pub async fn set_node_color(&self, input: SetNodeColorInput) -> Result<FsNode, String> {
        let vault_id = input.vault_id.clone();
        self.vault_call(&vault_id, |reply| VaultReq::SetNodeColor(input, reply))
            .await
    }

    pub async fn request_download(&self, vault_id: String, node_id: String) -> Result<(), String> {
        self.vault_call(&vault_id, |reply| {
            VaultReq::RequestDownload(node_id, reply)
        })
        .await
    }

    pub async fn open_node(&self, vault_id: String, node_id: String) -> Result<(), String> {
        self.vault_call(&vault_id, |reply| VaultReq::OpenNode(node_id, reply))
            .await
    }

    pub async fn import_files(
        &self,
        vault_id: String,
        parent_id: String,
        paths: Vec<PathBuf>,
    ) -> Result<Vec<FsNode>, String> {
        self.vault_call(&vault_id, |reply| {
            VaultReq::ImportFiles(parent_id, paths, reply)
        })
        .await
    }

    pub async fn read_text_preview(
        &self,
        vault_id: String,
        node_id: String,
        max_bytes: usize,
    ) -> Result<Option<String>, String> {
        self.vault_call(&vault_id, |reply| {
            VaultReq::ReadTextPreview(node_id, max_bytes, reply)
        })
        .await
    }

    /* ------------------------------------------------------------- access */

    pub async fn get_access(
        &self,
        vault_id: String,
        node_id: String,
    ) -> Result<NodeAccess, String> {
        self.vault_call(&vault_id, |reply| VaultReq::GetAccess(node_id, reply))
            .await
    }

    pub async fn set_access(&self, input: SetAccessInput) -> Result<NodeAccess, String> {
        let vault_id = input.vault_id.clone();
        self.vault_call(&vault_id, |reply| VaultReq::SetAccess(input, reply))
            .await
    }

    pub async fn get_history(
        &self,
        vault_id: String,
        node_id: String,
    ) -> Result<Vec<HistoryEvent>, String> {
        self.vault_call(&vault_id, |reply| VaultReq::GetHistory(node_id, reply))
            .await
    }

    /* ------------------------------------------------------------ recents */

    /// Assembled in the handle rather than in the root loop: each node comes from its own vault
    /// task, and the root must stay free to answer everything else while they answer.
    pub async fn list_recents(&self) -> Result<Vec<Recent>, String> {
        let recents = self.call(Req::Recents).await?;
        let mut out = Vec::new();
        for entry in recents {
            let found = self
                .vault_call(&entry.vault_id, |reply| {
                    VaultReq::NodeSnapshot(entry.node_id.clone(), reply)
                })
                .await;
            if let Ok(Some((node, vault_name))) = found {
                out.push(Recent {
                    node,
                    vault_name,
                    at: entry.at,
                });
            }
        }
        Ok(out)
    }

    pub async fn touch_recent(&self, vault_id: String, node_id: String) -> Result<(), String> {
        self.call(|reply| Req::TouchRecentAck(vault_id, node_id, reply))
            .await
    }

    /* ------------------------------------------------------------- vaults */

    pub async fn get_vault_meta(&self, vault_id: String) -> Result<VaultMeta, String> {
        self.vault_call(&vault_id, VaultReq::GetVaultMeta).await
    }

    pub async fn update_vault_meta(
        &self,
        vault_id: String,
        patch: VaultMetaPatch,
    ) -> Result<VaultMeta, String> {
        self.vault_call(&vault_id, |reply| {
            VaultReq::UpdateVaultMeta(patch, reply)
        })
        .await
    }

    pub async fn rotate_join_code(&self, vault_id: String) -> Result<String, String> {
        self.call(|reply| Req::RotateJoinCode(vault_id, reply)).await
    }

    pub async fn list_members(&self, vault_id: String) -> Result<Vec<Member>, String> {
        self.vault_call(&vault_id, VaultReq::ListMembers).await
    }

    /// Standing is the host's business: it owns admission and the control log
    /// (`docs/decisions/sync-host-tcb.md`), so there is no client-side role to set.
    pub async fn set_member_role(
        &self,
        _vault_id: String,
        _peer_id: String,
        _role: MemberRole,
    ) -> Result<Member, String> {
        Err("Roles are managed by the vault server".to_string())
    }

    pub async fn remove_member(&self, vault_id: String, peer_id: String) -> Result<(), String> {
        self.call(|reply| Req::RemoveMember(vault_id, peer_id, reply))
            .await
    }

    pub async fn delete_vault(&self, vault_id: String) -> Result<(), String> {
        self.call(|reply| Req::DeleteVault(vault_id, reply)).await
    }

    pub async fn leave_vault(&self, vault_id: String) -> Result<(), String> {
        self.call(|reply| Req::LeaveVault(vault_id, reply)).await
    }

    /* ----------------------------------------------------------- presence */

    pub async fn get_presence(&self, vault_id: String) -> Result<Vec<PeerPresence>, String> {
        self.vault_call(&vault_id, VaultReq::GetPresence).await
    }

    pub async fn publish_presence(&self, input: PresenceInput) -> Result<(), String> {
        let vault_id = input.vault_id.clone();
        self.vault_call(&vault_id, |reply| {
            VaultReq::PublishPresence(input, reply)
        })
        .await
    }

    /* -------------------------------------------------------------- agent */

    pub async fn ask_agent(&self, input: AskAgentInput) -> Result<AgentReply, String> {
        let vault_id = input.vault_id.clone();
        let folder_id = input.folder_id.clone();
        self.vault_call(&vault_id, |reply| VaultReq::AskAgent(folder_id, reply))
            .await
    }
}

/* -------------------------------------------------------------- the core */

/// The numbers a vault knows about itself, for the servers we hold no admin token for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct LocalNumbers {
    member_count: u32,
    used_bytes: u64,
}

/// One live vault: its persisted record, the channel to its task, and the numbers the task
/// reported for itself (used only when `STATUS` cannot supply them).
struct Live {
    record: VaultRecord,
    tx: UnboundedSender<VaultReq>,
    local: LocalNumbers,
}

/// Root state. Single-threaded: the `RefCell` exists so the request tasks can share it, not
/// for concurrency — no borrow is ever held across an `await`.
struct RootState {
    data_dir: PathBuf,
    emit: Arc<dyn Fn(&str, Value) + Send + Sync>,
    tx: UnboundedSender<Req>,
    profile: Profile,
    servers: Vec<ServerRecord>,
    recents: Vec<RecentRecord>,
    vaults: Vec<Live>,
    statuses: HashMap<String, AdminStatus>,
    /// Consecutive failed `STATUS` rounds per server; a host goes offline on the second.
    admin_failures: HashMap<String, u32>,
    /// The last `servers-changed` payload the UI was told about, so an unchanged poll is silent.
    last_servers: Option<Value>,
}

type Root = Rc<RefCell<RootState>>;

/// Runtime entry point: load state, bring every vault back up, then serve requests forever.
async fn run(
    data_dir: PathBuf,
    emit: Arc<dyn Fn(&str, Value) + Send + Sync>,
    tx: UnboundedSender<Req>,
    mut rx: UnboundedReceiver<Req>,
) {
    quantam_fs::demo_log::start("VAULT MEMBER");
    let _ = quantam_fs::demo_log::set_log_file(&data_dir.join("demo-events.log"));
    let _ = std::fs::create_dir_all(&data_dir);
    sweep_staging(&data_dir);

    let state: Root = Rc::new(RefCell::new(RootState {
        profile: state::load_profile(&data_dir),
        servers: state::load_servers(&data_dir),
        recents: state::load_recents(&data_dir),
        vaults: Vec::new(),
        statuses: HashMap::new(),
        admin_failures: HashMap::new(),
        last_servers: None,
        data_dir: data_dir.clone(),
        emit: emit.clone(),
        tx: tx.clone(),
    }));
    // `state::load_vaults` only yields bare 64-hex directories, so `.removed-*` (a vault we
    // left) and `staging-*` (a half-made one) are never reopened.
    for record in state::load_vaults(&data_dir) {
        state.borrow_mut().start_vault(record, None);
    }
    tokio::task::spawn_local(poll_admin(tx));
    tokio::task::spawn_local(poll_local_numbers(state.clone()));
    {
        let mut root = state.borrow_mut();
        root.emit_status();
        root.emit_servers();
    }

    while let Some(req) = rx.recv().await {
        dispatch(&state, req);
    }
}

/// Delete the leftovers of a `create_vault` that crashed between the keystore and the rename.
/// Vault directories are never deleted (see [`retire_vault_dir`]); a staging directory holds
/// nothing but an identity that was never admitted anywhere.
fn sweep_staging(data_dir: &Path) {
    for root in [data_dir.to_path_buf(), state::vaults_root(data_dir)] {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if !name.starts_with("staging-") {
                continue;
            }
            let stale = entry
                .metadata()
                .ok()
                .and_then(|meta| meta.modified().ok())
                .and_then(|at| at.elapsed().ok())
                .map(|age| age >= STAGING_TTL)
                .unwrap_or(false);
            if stale && entry.path().is_dir() {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
}

/// Move a vault's directory aside. It is never deleted: a leave, a delete or a revocation that
/// turns out to have been a mistake must stay recoverable by hand, and the `KeyStore` lock may
/// still be open for a moment after the task stops. `state::load_vaults` ignores the new name.
fn retire_vault_dir(data_dir: &Path, vault_hex: &str) {
    let live = state::vault_dir(data_dir, vault_hex);
    if !live.exists() {
        return;
    }
    let stamp = now_ms() / 1000;
    let root = state::vaults_root(data_dir);
    let mut aside = root.join(format!(".removed-{vault_hex}-{stamp}"));
    let mut attempt = 1u32;
    while aside.exists() {
        aside = root.join(format!(".removed-{vault_hex}-{stamp}-{attempt}"));
        attempt += 1;
    }
    let _ = std::fs::rename(&live, &aside);
}

/// One `STATUS` round per second against every host we hold a token for. Each server gets one
/// connection per round, and a server still being probed is skipped rather than dialled twice,
/// so a host that hangs cannot pile up sockets or delay the others.
async fn poll_admin(tx: UnboundedSender<Req>) {
    let inflight: Rc<RefCell<HashSet<String>>> = Rc::new(RefCell::new(HashSet::new()));
    loop {
        let (reply, answer) = oneshot::channel();
        if tx.send(Req::AdminTargets(reply)).is_err() {
            return;
        }
        let Ok(targets) = answer.await else { return };
        for (server_id, target) in targets {
            if !inflight.borrow_mut().insert(server_id.clone()) {
                continue;
            }
            let tx = tx.clone();
            let inflight = inflight.clone();
            tokio::task::spawn_local(async move {
                let result = match AdminConn::connect(&target).await {
                    Ok(mut conn) => conn.status().await.map(Box::new),
                    Err(message) => Err(message),
                };
                inflight.borrow_mut().remove(&server_id);
                let _ = tx.send(Req::AdminResult(server_id, result));
            });
        }
        tokio::time::sleep(ADMIN_POLL_INTERVAL).await;
    }
}

/// Vault cards for a server we hold no token for: there is no `STATUS` for that vault, so the
/// numbers come from what the vault task last projected into its own cache
/// (`state::VaultCache`). Zero means it has not projected anything yet, and the card keeps the
/// "one member, nothing stored" literals `servers()` falls back to.
async fn poll_local_numbers(state: Root) {
    loop {
        tokio::time::sleep(LOCAL_NUMBERS_INTERVAL).await;
        let mut moved = false;
        let mut root = state.borrow_mut();
        let pending: Vec<String> = root
            .vaults
            .iter()
            .filter(|live| {
                root.statuses
                    .get(&live.record.server_id)
                    .and_then(|status| status.vault(&live.record.vault_id))
                    .is_none()
            })
            .map(|live| live.record.vault_id.clone())
            .collect();
        for vault_id in pending {
            let cache = state::load_cache(&root.data_dir, &vault_id);
            let next = LocalNumbers {
                member_count: cache.member_count,
                used_bytes: cache.used_bytes,
            };
            if let Some(live) = root
                .vaults
                .iter_mut()
                .find(|live| live.record.vault_id == vault_id)
            {
                if live.local != next {
                    live.local = next;
                    moved = true;
                }
            }
        }
        if moved {
            root.emit_servers();
        }
    }
}

/* ---------------------------------------------------------------- dispatch */

/// The whole root loop. Everything here is synchronous: a request that has to talk to a host
/// becomes its own task, so the next `VaultTx` is answered on the very next poll of the channel.
fn dispatch(state: &Root, req: Req) {
    match req {
        Req::Status(reply) => {
            let status = state.borrow().status();
            let _ = reply.send(Ok(status));
        }
        Req::Me(reply) => {
            let me = state.borrow().me();
            let _ = reply.send(Ok(me));
        }
        Req::ListServers(reply) => {
            let servers = state.borrow().servers();
            let _ = reply.send(Ok(servers));
        }
        Req::VaultTx(vault_id, reply) => {
            let found = state
                .borrow()
                .vaults
                .iter()
                .find(|live| live.record.vault_id == vault_id)
                .map(|live| live.tx.clone())
                .ok_or_else(|| format!("Unknown vault: {vault_id}"));
            let _ = reply.send(found);
        }
        Req::Recents(reply) => {
            let recents = state.borrow().recents.clone();
            let _ = reply.send(Ok(recents));
        }
        Req::TouchRecentAck(vault_id, node_id, reply) => {
            state.borrow_mut().touch_recent(&vault_id, &node_id);
            let _ = reply.send(Ok(()));
        }
        Req::TouchRecent { vault_id, node_id } => {
            state.borrow_mut().touch_recent(&vault_id, &node_id)
        }
        Req::AdminTargets(reply) => {
            let targets = state.borrow().admin_targets();
            let _ = reply.send(targets);
        }
        Req::AdminResult(server_id, result) => apply_admin(state, &server_id, result),
        Req::VaultNameChanged { vault_id, name } => {
            let mut root = state.borrow_mut();
            if let Some(live) = root
                .vaults
                .iter_mut()
                .find(|live| live.record.vault_id == vault_id)
            {
                if live.record.name != name {
                    live.record.name = name;
                    let record = live.record.clone();
                    state::save_vault(&root.data_dir, &record);
                }
            }
            // The name is part of every vault card, so the change detection inside
            // `emit_servers` is what decides whether the UI hears about it.
            root.emit_servers();
        }
        /* ---- everything below does TCP work and must not run on the loop ---- */
        Req::AddServer(name, address, reply) => {
            let state = state.clone();
            tokio::task::spawn_local(async move {
                let result = add_server(&state, name, address).await;
                let _ = reply.send(result);
            });
        }
        Req::CreateVault(input, reply) => {
            let state = state.clone();
            tokio::task::spawn_local(async move {
                let result = create_vault(&state, input).await;
                let _ = reply.send(result);
            });
        }
        Req::JoinVault(code, reply) => {
            let state = state.clone();
            tokio::task::spawn_local(async move {
                let result = join_vault(&state, &code).await;
                let _ = reply.send(result);
            });
        }
        Req::RotateJoinCode(vault_id, reply) => {
            let state = state.clone();
            tokio::task::spawn_local(async move {
                let result = rotate_join_code(&state, &vault_id).await;
                let _ = reply.send(result);
            });
        }
        Req::RemoveMember(vault_id, peer_id, reply) => {
            let state = state.clone();
            tokio::task::spawn_local(async move {
                let result = remove_member(&state, &vault_id, &peer_id).await;
                let _ = reply.send(result);
            });
        }
        Req::DeleteVault(vault_id, reply) => {
            let state = state.clone();
            tokio::task::spawn_local(async move {
                let result = delete_vault(&state, &vault_id).await;
                let _ = reply.send(result);
            });
        }
        Req::LeaveVault(vault_id, reply) => {
            let state = state.clone();
            tokio::task::spawn_local(async move {
                drop_vault(&state, &vault_id).await;
                let _ = reply.send(Ok(()));
            });
        }
        Req::VaultRevoked { vault_id, reason } => {
            let state = state.clone();
            tokio::task::spawn_local(async move {
                state.borrow().emit(
                    "backend://vault-removed",
                    &VaultRemovedPayload {
                        vault_id: vault_id.clone(),
                        reason,
                    },
                );
                drop_vault(&state, &vault_id).await;
            });
        }
    }
}

/* ------------------------------------------------------------- read sides */

impl RootState {
    fn status(&self) -> DaemonStatus {
        DaemonStatus {
            running: true,
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            peer_id: Some(self.profile.peer_id.clone()),
            data_dir: Some(self.data_dir.display().to_string()),
        }
    }

    /// Pure read of `profile.json`, which is why `me()` is answered on the loop: it cannot be
    /// held up by an in-flight join or a dead server's timeout.
    fn me(&self) -> Member {
        Member {
            peer_id: self.profile.peer_id.clone(),
            name: self.profile.name.clone(),
            initials: initials(&self.profile.name),
            color: self.profile.color.clone(),
            role: MemberRole::Member,
            online: true,
            last_seen_at: now_ms(),
            last_edited: None,
            queued_ops: 0,
            is_self: true,
        }
    }

    /// Every known host, each with the vaults *we* belong to on it.
    fn servers(&self) -> Vec<Server> {
        self.servers
            .iter()
            .map(|record| {
                let status = self.statuses.get(&record.id);
                let capacity = match status {
                    Some(status) => {
                        status.server.capacity_bytes
                            + (status.online_members_excluding_host() as f64 * MEMBER_CREDIT_BYTES)
                                as u64
                    }
                    None if record.capacity_bytes > 0 => record.capacity_bytes,
                    None => DEFAULT_CAPACITY_BYTES,
                };
                let vaults = self
                    .vaults
                    .iter()
                    .filter(|live| live.record.server_id == record.id)
                    .map(|live| {
                        let numbers = status.and_then(|s| s.vault(&live.record.vault_id));
                        Vault {
                            id: live.record.vault_id.clone(),
                            server_id: record.id.clone(),
                            name: if live.record.name.is_empty() {
                                "Vault".to_string()
                            } else {
                                live.record.name.clone()
                            },
                            // A code-joined vault sits on a server we have no token for, so
                            // `STATUS` never mentions it: the card falls back to what the vault
                            // task itself reported (`poll_local_numbers`).
                            member_count: numbers
                                .map(|v| v.member_count)
                                .unwrap_or_else(|| live.local.member_count.max(1)),
                            used_bytes: numbers
                                .map(|v| v.used_bytes)
                                .unwrap_or(live.local.used_bytes),
                            quota_bytes: numbers.map(|v| v.quota_bytes).unwrap_or(0),
                            role: match live.record.role {
                                VaultRole::Owner => Role::Owner,
                                VaultRole::Member => Role::Member,
                            },
                        }
                    })
                    .collect();
                Server {
                    id: record.id.clone(),
                    name: record.name.clone(),
                    address: record.address.clone(),
                    peer_id: record.host_peer_id.clone().unwrap_or_default(),
                    online: record.online,
                    capacity_bytes: capacity,
                    vaults,
                }
            })
            .collect()
    }

    /// Every server, token or not: `STATUS` is read-only and needs no `AUTH`, so a host we
    /// only learned about from a join code still reports its members, usage and quota.
    fn admin_targets(&self) -> Vec<(String, AdminTarget)> {
        self.servers
            .iter()
            .map(|record| (record.id.clone(), self.admin_target(record)))
            .collect()
    }

    /// The admin endpoint of one server. An absent token becomes the empty string, which
    /// [`AdminConn::connect`] reads as "skip `AUTH`, read-only commands only".
    fn admin_target(&self, record: &ServerRecord) -> AdminTarget {
        AdminTarget {
            addr: record.address.clone(),
            token: record.token.clone().unwrap_or_default(),
        }
    }

    /// The admin credentials for one vault's host, for the commands that change the host's
    /// state and therefore do need the token. Returned rather than dialled, so the caller can
    /// connect without holding a borrow of the state.
    fn admin_target_for(&self, vault_id: &str) -> Result<(AdminTarget, String), String> {
        let server_id = self
            .vaults
            .iter()
            .find(|live| live.record.vault_id == vault_id)
            .map(|live| live.record.server_id.clone())
            .ok_or_else(|| format!("Unknown vault: {vault_id}"))?;
        let record = self
            .servers
            .iter()
            .find(|record| record.id == server_id)
            .ok_or_else(|| format!("Unknown server: {server_id}"))?;
        let token = record
            .token
            .clone()
            .ok_or("This server was added without a connect string")?;
        Ok((
            AdminTarget {
                addr: record.address.clone(),
                token,
            },
            server_id,
        ))
    }

    fn vault_shape(&self, vault_hex: &str) -> Option<Vault> {
        self.servers()
            .into_iter()
            .flat_map(|server| server.vaults)
            .find(|vault| vault.id == vault_hex)
    }

    /* ------------------------------------------------------------- vaults */

    fn start_vault(&mut self, record: VaultRecord, initial_meta: Option<(String, String)>) {
        let target = self
            .servers
            .iter()
            .find(|server| server.id == record.server_id)
            .map(|server| self.admin_target(server));
        match vault::spawn(
            self.data_dir.clone(),
            record.clone(),
            self.profile.clone(),
            self.emit.clone(),
            self.tx.clone(),
            target,
            initial_meta,
        ) {
            Ok(tx) => self.vaults.push(Live {
                record,
                tx,
                local: LocalNumbers::default(),
            }),
            Err(_) => {
                // A vault whose identity will not open (a stale lock, a half-written keystore)
                // must not take the whole node down; it simply stays absent from the lists.
            }
        }
    }

    /// Keep every vault task's admin endpoint current (a token may have just arrived).
    fn push_admin_targets(&self) {
        for live in &self.vaults {
            let target = self
                .servers
                .iter()
                .find(|record| record.id == live.record.server_id)
                .map(|record| self.admin_target(record));
            let _ = live.tx.send(VaultReq::Admin(target));
        }
    }

    /// Store a code the host just handed back. A `CREATE_VAULT`, `ROTATE_CODE` or `KICK` reply
    /// may carry either form: `vault.json` keeps the 26-character text the protocol admits
    /// with, and the six characters a human retypes go in the vault's cache.
    fn set_join_code(&mut self, vault_id: &str, code: &str) {
        let Some((short, parsed)) = state::resolve_join_code(code) else {
            return;
        };
        let long = parsed.to_string();
        if let Some(live) = self
            .vaults
            .iter_mut()
            .find(|live| live.record.vault_id == vault_id)
        {
            if live.record.join_code != long {
                live.record.join_code = long;
                let record = live.record.clone();
                state::save_vault(&self.data_dir, &record);
            }
        }
        if let Some(short) = short {
            state::remember_short_code(&self.data_dir, vault_id, &short);
        }
    }

    /* ------------------------------------------------------------ recents */

    fn touch_recent(&mut self, vault_id: &str, node_id: &str) {
        self.recents
            .retain(|entry| !(entry.vault_id == vault_id && entry.node_id == node_id));
        self.recents.insert(
            0,
            RecentRecord {
                vault_id: vault_id.to_string(),
                node_id: node_id.to_string(),
                at: now_ms(),
            },
        );
        self.recents.truncate(state::MAX_RECENTS);
        state::save_recents(&self.data_dir, &self.recents);
        self.emit("backend://recents-changed", &Value::Null);
    }

    /* ------------------------------------------------------------- events */

    fn emit<T: Serialize>(&self, name: &str, payload: &T) {
        let value = serde_json::to_value(payload).unwrap_or(Value::Null);
        (self.emit)(name, value);
    }

    /// `servers-changed` makes the home screen refetch every server and vault, so it is only
    /// sent when something the screen actually draws moved. The 1 s `STATUS` poll would
    /// otherwise re-render the whole list once a second forever.
    fn emit_servers(&mut self) {
        let next = serde_json::to_value(self.servers()).unwrap_or(Value::Null);
        if self.last_servers.as_ref() == Some(&next) {
            return;
        }
        self.last_servers = Some(next);
        (self.emit)("backend://servers-changed", Value::Null);
    }

    fn emit_status(&self) {
        self.emit("backend://daemon-status", &self.status());
    }
}

/* --------------------------------------------------------------- servers */

/// `add_server` takes the connect string the host printed: `IP:PORT/TOKEN`.
async fn add_server(state: &Root, name: String, address: String) -> Result<Server, String> {
    let raw = address.trim();
    let (addr, token) = match raw.split_once('/') {
        Some((addr, token)) if !token.trim().is_empty() => {
            (addr.trim().to_string(), Some(token.trim().to_string()))
        }
        _ => (raw.trim_end_matches('/').to_string(), None),
    };
    if addr.parse::<SocketAddr>().is_err() {
        return Err("Invalid server address".to_string());
    }
    let mut record = ServerRecord {
        id: server_id_for(&addr),
        name: if name.trim().is_empty() {
            format!("Server at {}", host_of(&addr))
        } else {
            name.trim().to_string()
        },
        address: addr.clone(),
        token: token.clone(),
        host_peer_id: None,
        directory_addr: None,
        capacity_bytes: DEFAULT_CAPACITY_BYTES,
        online: false,
    };
    let status = match &token {
        Some(token) => {
            let target = AdminTarget {
                addr: addr.clone(),
                token: token.clone(),
            };
            let mut conn = AdminConn::connect(&target).await?;
            conn.ping().await?;
            Some(conn.status().await?)
        }
        None => {
            // Without a token we cannot authenticate; a plain reachability check is all
            // this record needs, since it only ever carries vaults reached by join code.
            tokio::time::timeout(
                Duration::from_secs(3),
                tokio::net::TcpStream::connect(&addr),
            )
            .await
            .map_err(|_| "Could not reach that server".to_string())?
            .map_err(|_| "Could not reach that server".to_string())?;
            None
        }
    };

    let mut root = state.borrow_mut();
    if let Some(status) = &status {
        record.online = true;
        record.host_peer_id = Some(status.server.host_peer.clone());
        record.directory_addr = Some(status.server.directory_addr.clone());
        record.capacity_bytes = status.server.capacity_bytes;
    }
    // The same host is very likely already in the list: a code join adds it from the vault's
    // directory ad (no token). Upgrade that entry with the connect string — a second row for
    // one server would split its vaults across two cards.
    let existing = root.servers.iter().position(|previous| {
        previous.id == record.id
            || (record.host_peer_id.is_some() && previous.host_peer_id == record.host_peer_id)
    });
    match existing {
        Some(index) => {
            let previous = root.servers[index].clone();
            // Ids never change under a server: the cached `STATUS` and every vault record
            // point at the one it already has.
            record.id = previous.id;
            if name.trim().is_empty() && !previous.name.is_empty() {
                record.name = previous.name;
            }
            if record.host_peer_id.is_none() {
                record.host_peer_id = previous.host_peer_id;
            }
            if record.directory_addr.is_none() {
                record.directory_addr = previous.directory_addr;
            }
            root.servers[index] = record.clone();
        }
        None => root.servers.push(record.clone()),
    }
    if let Some(status) = status {
        if let Ok(directory) = status.server.directory_addr.trim().parse::<SocketAddr>() {
            state::remember_directory_addr(&root.data_dir, directory);
        }
        root.statuses.insert(record.id.clone(), status);
        root.admin_failures.remove(&record.id);
    }
    state::save_servers(&root.data_dir, &root.servers);
    root.push_admin_targets();
    root.emit_servers();
    root.servers()
        .into_iter()
        .find(|server| server.id == record.id)
        .ok_or_else(|| "Could not reach that server".to_string())
}

/// `STATUS` with no token, for a host the user never added: the read-only admin commands need
/// no `AUTH`, so a code-joined vault's card carries real members, usage and quota.
async fn probe_admin(addr: &str) -> Option<AdminStatus> {
    let target = AdminTarget {
        addr: addr.to_string(),
        token: String::new(),
    };
    let mut conn = AdminConn::connect(&target).await.ok()?;
    conn.status().await.ok()
}

/// Put the host that advertised a vault in the server list, or refresh the entry it already
/// has. Returns the id the vault record must carry.
fn ensure_server(
    root: &mut RootState,
    admin_addr: &str,
    host_peer: &str,
    directory_addr: &str,
    status: Option<AdminStatus>,
) -> String {
    let id = server_id_for(admin_addr);
    let existing = root.servers.iter().position(|record| {
        record.id == id || record.host_peer_id.as_deref() == Some(host_peer)
    });
    let index = match existing {
        Some(index) => index,
        None => {
            root.servers.push(ServerRecord {
                id,
                name: format!("Server {}", host_of(admin_addr)),
                address: admin_addr.to_string(),
                token: None,
                host_peer_id: Some(host_peer.to_string()),
                directory_addr: Some(directory_addr.to_string()),
                capacity_bytes: DEFAULT_CAPACITY_BYTES,
                online: false,
            });
            root.servers.len() - 1
        }
    };
    let server_id = {
        let record = &mut root.servers[index];
        record.host_peer_id = Some(host_peer.to_string());
        if record.directory_addr.as_deref().unwrap_or("").is_empty() {
            record.directory_addr = Some(directory_addr.to_string());
        }
        if let Some(status) = &status {
            // A host that answered STATUS is online by definition, and the address it
            // answered on is its admin listener — worth adopting for an entry that was
            // written before this host was ever probed (it cannot overwrite a connect
            // string, since a record with a token was added from one).
            record.online = true;
            if record.token.is_none() {
                record.address = admin_addr.to_string();
            }
            record.host_peer_id = Some(status.server.host_peer.clone());
            record.directory_addr = Some(status.server.directory_addr.clone());
            record.capacity_bytes = status.server.capacity_bytes;
        }
        record.id.clone()
    };
    if let Some(status) = status {
        root.statuses.insert(server_id.clone(), status);
        root.admin_failures.remove(&server_id);
    }
    state::save_servers(&root.data_dir, &root.servers);
    server_id
}

/// Fold one `STATUS` into the server list, the vault records and the vault tasks. Synchronous
/// on purpose: it touches no socket, so the loop can absorb a poll answer between two requests.
fn apply_admin(state: &Root, server_id: &str, result: Result<Box<AdminStatus>, String>) {
    let mut root = state.borrow_mut();
    match result {
        Ok(status) => {
            let status = *status;
            root.admin_failures.remove(server_id);
            let mut save = false;
            if let Some(record) = root.servers.iter_mut().find(|r| r.id == server_id) {
                let peer = Some(status.server.host_peer.clone());
                let directory = Some(status.server.directory_addr.clone());
                save = !record.online
                    || record.host_peer_id != peer
                    || record.directory_addr != directory
                    || record.capacity_bytes != status.server.capacity_bytes;
                record.online = true;
                record.host_peer_id = peer;
                record.directory_addr = directory;
                record.capacity_bytes = status.server.capacity_bytes;
            }
            if save {
                state::save_servers(&root.data_dir, &root.servers);
            }
            // Only hand the vault tasks a new snapshot when a number they care about moved;
            // an identical snapshot would make each of them re-emit members and presence.
            if !numbers_match(root.statuses.get(server_id), &status) {
                let mut renames: Vec<VaultRecord> = Vec::new();
                let mut shorts: Vec<(String, String)> = Vec::new();
                for live in root
                    .vaults
                    .iter_mut()
                    .filter(|live| live.record.server_id == server_id)
                {
                    let Some(numbers) = status.vault(&live.record.vault_id) else {
                        continue;
                    };
                    // `STATUS` prints the six-character short code (or `-`); `vault.json`
                    // keeps the long form, so resolve what the host said into both.
                    if let Some((short, code)) = state::resolve_join_code(&numbers.join_code) {
                        let long = code.to_string();
                        if live.record.join_code != long {
                            live.record.join_code = long;
                            renames.push(live.record.clone());
                        }
                        if let Some(short) = short {
                            shorts.push((live.record.vault_id.clone(), short));
                        }
                    }
                    let snapshot = VaultSnapshot {
                        join_code: numbers.join_code.clone(),
                        quota_bytes: numbers.quota_bytes,
                        used_bytes: numbers.used_bytes,
                        member_count: numbers.member_count,
                        created_ms: numbers.created_ms,
                        creator: numbers.creator.clone(),
                        members: status
                            .members_of(&live.record.vault_id)
                            .into_iter()
                            .cloned()
                            .collect(),
                    };
                    let _ = live.tx.send(VaultReq::Status(Box::new(snapshot)));
                }
                for record in renames {
                    state::save_vault(&root.data_dir, &record);
                }
                for (vault_id, short) in shorts {
                    state::remember_short_code(&root.data_dir, &vault_id, &short);
                }
            }
            root.statuses.insert(server_id.to_string(), status);
            root.push_admin_targets();
        }
        Err(_) => {
            // One lost round is not an outage: a host is only drawn offline after two in a
            // row, and the first success brings it straight back.
            let failures = {
                let counter = root
                    .admin_failures
                    .entry(server_id.to_string())
                    .or_insert(0);
                *counter = counter.saturating_add(1);
                *counter
            };
            if failures >= ADMIN_FAILURES_BEFORE_OFFLINE {
                if let Some(record) = root.servers.iter_mut().find(|r| r.id == server_id) {
                    record.online = false;
                }
                root.statuses.remove(server_id);
            }
        }
    }
    root.emit_servers();
}

/* ---------------------------------------------------------------- vaults */

/// Ask a host to provision a vault, then join it as its owner.
///
/// The identity comes first, in a staging directory: `CREATE_VAULT` wants the creator's peer
/// id, and the directory it finally lives in is named after the vault id the host invents.
async fn create_vault(state: &Root, input: CreateVaultInput) -> Result<Vault, String> {
    let name = input.name.trim().to_string();
    if name.is_empty() || name.chars().count() > crate::fs_state::MAX_VAULT_NAME {
        return Err("Invalid vault name".to_string());
    }
    let (data_dir, record, token, capacity, allocated) = {
        let root = state.borrow();
        let record = root
            .servers
            .iter()
            .find(|record| record.id == input.server_id)
            .cloned()
            .ok_or_else(|| format!("Unknown server: {}", input.server_id))?;
        let token = record
            .token
            .clone()
            .ok_or("This server was added without a connect string")?;
        let status = root.statuses.get(&record.id);
        let capacity = status
            .map(|status| status.server.capacity_bytes)
            .unwrap_or(record.capacity_bytes);
        let allocated: u64 = status
            .map(|status| status.vaults.iter().map(|v| v.quota_bytes).sum())
            .unwrap_or(0);
        (root.data_dir.clone(), record, token, capacity, allocated)
    };
    let free = capacity.saturating_sub(allocated);
    if input.quota_bytes < MIN_QUOTA_BYTES || input.quota_bytes > free {
        return Err("Not enough space on this server".to_string());
    }

    let staging = data_dir.join(format!("staging-{}", now_ms()));
    std::fs::create_dir_all(&staging)
        .map_err(|e| format!("Could not create the vault folder: {e}"))?;
    let creator = {
        // The keystore holds an exclusive lock, so it must be closed before the rename.
        let keys = KeyStore::open(&staging.join("identity")).map_err(|e| e.to_string())?;
        keys.load_or_create().map_err(|e| e.to_string())?;
        hex(&keys.peer_id().map_err(|e| e.to_string())?.0)
    };

    let target = AdminTarget {
        addr: record.address.clone(),
        token,
    };
    let mut conn = AdminConn::connect(&target).await?;
    let (vault_hex, reply_code) = match conn.create_vault(input.quota_bytes, Some(&creator)).await {
        Ok(pair) => pair,
        Err(message) => {
            // A staging directory is not a vault: nothing was ever admitted into it.
            let _ = std::fs::remove_dir_all(&staging);
            return Err(message);
        }
    };

    let dir = state::vault_dir(&data_dir, &vault_hex);
    let _ = std::fs::create_dir_all(state::vaults_root(&data_dir));
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err("That vault already exists on this Mac".to_string());
    }
    std::fs::rename(&staging, &dir)
        .map_err(|e| format!("Could not create the vault folder: {e}"))?;

    // The reply carries the six-character code a human types; `vault.json` keeps the long
    // form the protocol admits with, and the short form goes in the vault's cache before the
    // task starts and reads it.
    let (short, code) = state::resolve_join_code(&reply_code)
        .ok_or_else(|| "The server sent an unreadable join code".to_string())?;
    if let Some(short) = &short {
        state::remember_short_code(&data_dir, &vault_hex, short);
    }

    let mut root = state.borrow_mut();
    let directory_addr = root
        .statuses
        .get(&record.id)
        .map(|status| status.server.directory_addr.clone())
        .or_else(|| record.directory_addr.clone())
        .unwrap_or_default();
    let vault_record = VaultRecord {
        vault_id: vault_hex.clone(),
        server_id: record.id.clone(),
        join_code: code.to_string(),
        name: name.clone(),
        role: VaultRole::Owner,
        directory_addr,
    };
    state::save_vault(&root.data_dir, &vault_record);
    let profile_peer = root.profile.peer_id.clone();
    root.start_vault(vault_record, Some((name.clone(), profile_peer)));
    root.emit_servers();
    if !root
        .vaults
        .iter()
        .any(|live| live.record.vault_id == vault_hex)
    {
        return Err("The server created the vault but it could not be opened".to_string());
    }
    // The first `STATUS` is up to a second away, so answer from what we just asked for.
    Ok(Vault {
        id: vault_hex,
        server_id: record.id,
        name,
        member_count: 1,
        used_bytes: 0,
        quota_bytes: input.quota_bytes,
        role: Role::Owner,
    })
}

/// Join from a 6-character short code, the bare 26-character code, or either in the directory
/// form `"ip:port/CODE"`. No server has to be added first: the directory comes from the
/// environment, `<data_dir>/directory.txt` or a host we already know, and the vault's own host
/// is added to the server list from the ad the directory answers with.
async fn join_vault(state: &Root, raw: &str) -> Result<Vault, String> {
    let (explicit, short, code) = parse_code_input(raw)?;
    let candidates: Vec<SocketAddr> = match explicit {
        Some(addr) => vec![addr
            .parse::<SocketAddr>()
            .map_err(|_| "Invalid server address".to_string())?],
        None => {
            let known = directory_candidates(&state.borrow());
            if known.is_empty() {
                return Err(
                    "No directory to ask: set QFS_DIRECTORY_ADDR or add a server".to_string(),
                );
            }
            known
        }
    };
    let mut found = None;
    for addr in candidates {
        if let Ok(Some(ad)) = DirectoryClient::new(addr).lookup(code).await {
            found = Some((addr, ad));
            break;
        }
    }
    let (directory, ad) = found.ok_or("Invalid join code")?;
    let directory_addr = directory.to_string();
    let vault_hex = hex(&ad.vault_id.0);
    if let Some(existing) = state.borrow().vault_shape(&vault_hex) {
        return Ok(existing);
    }

    // The ad carries the host's peer address; its admin listener is that port plus 1000 and
    // answers STATUS without a token, so ask it before the entry goes in the list.
    let host_peer = hex(&ad.peer_id.0);
    let admin_addr = admin_addr_for(ad.addr);
    let status = probe_admin(&admin_addr).await;

    let mut root = state.borrow_mut();
    // A directory that answered is worth writing down: the next code-only join needs it
    // before any server or vault exists.
    state::remember_directory_addr(&root.data_dir, directory);
    let server_id = ensure_server(&mut root, &admin_addr, &host_peer, &directory_addr, status);
    if let Some(short) = &short {
        state::remember_short_code(&root.data_dir, &vault_hex, short);
    }

    let dir = state::vault_dir(&root.data_dir, &vault_hex);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create the vault folder: {e}"))?;
    let record = VaultRecord {
        vault_id: vault_hex.clone(),
        server_id,
        // The long form of the code we actually looked up, which is what `vault.json`
        // carries; the six-character form (when that is what was typed) went into the
        // vault's cache above.
        join_code: code.to_string(),
        name: String::new(),
        role: VaultRole::Member,
        directory_addr,
    };
    state::save_vault(&root.data_dir, &record);
    root.start_vault(record, None);
    root.push_admin_targets();
    root.emit_servers();
    root.vault_shape(&vault_hex)
        .ok_or_else(|| "That vault could not be opened".to_string())
}

/// Where to ask about a join code when the user has added no server: the environment, the
/// address the last successful lookup wrote down, then any host already in the list — that
/// order is `state::resolve_directory_addr`. The remaining known directories follow it, so a
/// second host can still answer when the first cannot.
fn directory_candidates(root: &RootState) -> Vec<SocketAddr> {
    let known: Vec<SocketAddr> = root
        .servers
        .iter()
        .filter_map(|record| {
            root.statuses
                .get(&record.id)
                .map(|status| status.server.directory_addr.clone())
                .or_else(|| record.directory_addr.clone())
        })
        .filter_map(|addr| addr.trim().parse::<SocketAddr>().ok())
        .collect();
    let mut out: Vec<SocketAddr> = Vec::new();
    if let Some(addr) = state::resolve_directory_addr(&root.data_dir, &known) {
        out.push(addr);
    }
    for addr in known {
        if !out.contains(&addr) {
            out.push(addr);
        }
    }
    out
}

async fn rotate_join_code(state: &Root, vault_id: &str) -> Result<String, String> {
    let (target, _) = state.borrow().admin_target_for(vault_id)?;
    let mut conn = AdminConn::connect(&target).await?;
    let code = conn.rotate_code(vault_id).await?;
    let mut root = state.borrow_mut();
    root.set_join_code(vault_id, &code);
    root.emit(
        "backend://vault-changed",
        &VaultIdPayload {
            vault_id: vault_id.to_string(),
        },
    );
    Ok(code)
}

async fn remove_member(state: &Root, vault_id: &str, peer_id: &str) -> Result<(), String> {
    let target = {
        let root = state.borrow();
        if peer_id == root.profile.peer_id {
            return Err("Leave the vault instead of removing yourself".to_string());
        }
        root.admin_target_for(vault_id)?.0
    };
    let mut conn = AdminConn::connect(&target).await?;
    let code = conn.kick(vault_id, peer_id).await?;
    let mut root = state.borrow_mut();
    root.set_join_code(vault_id, &code);
    root.emit(
        "backend://members-changed",
        &VaultIdPayload {
            vault_id: vault_id.to_string(),
        },
    );
    Ok(())
}

async fn delete_vault(state: &Root, vault_id: &str) -> Result<(), String> {
    let owner = state
        .borrow()
        .vaults
        .iter()
        .find(|live| live.record.vault_id == vault_id)
        .map(|live| live.record.role == VaultRole::Owner)
        .unwrap_or(false);
    if owner {
        let target = state.borrow().admin_target_for(vault_id).ok();
        if let Some((target, _)) = target {
            if let Ok(mut conn) = AdminConn::connect(&target).await {
                conn.forget_vault(vault_id).await?;
            }
        }
    }
    drop_vault(state, vault_id).await;
    Ok(())
}

/// Stop the task, move the local copy aside, and take the vault out of every list.
///
/// The directory is only touched after the task has answered `Stop`: that reply is its last
/// action, sent once it has dropped its `KeyStore` and replica and flushed its saves, so the
/// rename cannot race a write or a lock file. It is a rename, never a delete
/// (see [`retire_vault_dir`]).
async fn drop_vault(state: &Root, vault_id: &str) {
    let stopping = {
        let mut root = state.borrow_mut();
        root.vaults
            .iter()
            .position(|live| live.record.vault_id == vault_id)
            .map(|index| root.vaults.remove(index))
    };
    let Some(live) = stopping else {
        return;
    };
    let (reply, answer) = oneshot::channel();
    if live.tx.send(VaultReq::Stop(reply)).is_ok() {
        let _ = tokio::time::timeout(STOP_TIMEOUT, answer).await;
    }
    drop(live);

    let data_dir = {
        let mut root = state.borrow_mut();
        root.recents.retain(|entry| entry.vault_id != vault_id);
        state::save_recents(&root.data_dir, &root.recents);
        root.data_dir.clone()
    };
    retire_vault_dir(&data_dir, vault_id);
    let mut root = state.borrow_mut();
    root.emit_servers();
    root.emit("backend://recents-changed", &Value::Null);
}

/* ------------------------------------------------------------- free helpers */

/// Did any number the home screen draws actually move?
fn numbers_match(previous: Option<&AdminStatus>, next: &AdminStatus) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    if previous.vaults.len() != next.vaults.len() || previous.members.len() != next.members.len() {
        return false;
    }
    previous.vaults.iter().zip(&next.vaults).all(|(a, b)| {
        a.vault == b.vault
            && a.used_bytes == b.used_bytes
            && a.quota_bytes == b.quota_bytes
            && a.member_count == b.member_count
            && a.online_count == b.online_count
            && a.join_code == b.join_code
    }) && previous
        .members
        .iter()
        .zip(&next.members)
        .all(|(a, b)| a.peer == b.peer && a.online == b.online && a.queued_ops == b.queued_ops)
}

/// A code the user typed: the six-character short code, the full 26-character Base32
/// `JoinCode`, or either behind an explicit `"ip:port/"` directory address. Returns the
/// directory the user named, the short code when that is what they typed, and the join code
/// the protocol admits with.
fn parse_code_input(raw: &str) -> Result<(Option<String>, Option<String>, JoinCode), String> {
    // The long form, and the `"ip:port/CODE"` split, are already one helper.
    if let Ok((addr, code)) = parse_join_input(raw) {
        return Ok((addr, None, code));
    }
    let trimmed = raw.trim();
    let (addr, text) = match trimmed.rsplit_once('/') {
        Some((addr, code)) if !addr.is_empty() => (Some(addr.trim().to_string()), code.trim()),
        _ => (None, trimmed),
    };
    let (short, code) =
        state::resolve_join_code(text).ok_or_else(|| "Invalid join code".to_string())?;
    Ok((addr, short, code))
}

/// The admin address of a host from the peer address in its directory ad.
fn admin_addr_for(peer_addr: SocketAddr) -> String {
    SocketAddr::new(
        peer_addr.ip(),
        peer_addr.port().saturating_add(ADMIN_PORT_OFFSET),
    )
    .to_string()
}

/// The IP half of `ip:port`, for a default server name.
fn host_of(address: &str) -> &str {
    address.rsplit_once(':').map(|(host, _)| host).unwrap_or(address)
}
