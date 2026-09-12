//! One member node: a joined vault, projected into the shapes the workspace draws.
//!
//! WHY a task per vault: each membership has its own identity and data directory
//! (docs/decisions/client-backend-embed.md), so each one owns a `KeyStore`, a `MemberReplica`
//! and one live `JoinedPeer`. Requests and the 100 ms heartbeat share a single `select!` loop so
//! a half-run heartbeat or a half-run command is never cancelled — the protocol's live stream is
//! request/reply over one TCP connection and a dropped future would desynchronize it.
//!
//! Change notification is that heartbeat: after every tick the replica is re-projected and the
//! diff goes out as one `fs-changed` event, which is how a folder another member created shows
//! up here without the webview asking.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time::Instant;

use quantam_fs::crypto::identity::IdentityManager;
use quantam_fs::ids::{ChunkId, FileId, PeerId};
use quantam_fs::keystore::KeyStore;
use quantam_fs::net::directory::{DirectoryAd, DirectoryClient};
use quantam_fs::net::join::{join_host, unix_time, JoinedPeer};
use quantam_fs::net::{JoinCode, VaultId};
use quantam_fs::protocol::locate::HaveQuery;
use quantam_fs::protocol::manifest::TrustedManifest;
use quantam_fs::protocol::pull::PullRequest;
use quantam_fs::store::chunks::ChunkStore;
use quantam_fs::store::tree::Dirent;
use quantam_fs::sync::host::MemberReplica;

use crate::fs_types::{
    AccessEntry, AgentReply, Availability, CreateNodeInput, DeleteNodesInput, DuplicateNodesInput,
    FolderColor, FsChange, FsChangedPayload, FsNode, HistoryEvent, HistoryKind, LastEdited, Member,
    MemberRole, MoveNodesInput, NodeAccess, NodeKind, PeerPresence, PresenceInput, RenameNodeInput,
    SetAccessInput, SetNodeColorInput, VaultIdPayload, VaultMeta, VaultMetaPatch,
};

use super::admin::{AdminMember, AdminTarget};
use super::names::{
    color_for, friendly, hex, initials, name_error, now_ms, root_node_id, unhex32, unique_name,
    vault_path,
};
use super::state::{self, VaultRecord};
use super::{Reply, Req};

/// A new remote file up to this size is fetched in the background so the first click is instant.
pub const PREFETCH_MAX_BYTES: u64 = 16 * 1024 * 1024;
/// Import chunk size. The protocol's ceiling is 1 MiB per chunk (`store::durable`).
const IMPORT_CHUNK_BYTES: usize = 512 * 1024;
/// `HaveQuery` and `PullRequest` both cap at 32 identifiers.
const PULL_BATCH: usize = 32;
/// How much of a file has to be NUL-free before it counts as text.
const TEXT_SNIFF_BYTES: usize = 4096;
/// The replicated cosmetics file, hidden from every listing.
const SIDECAR_PATH: &str = "/.qfs-meta.json";
const SIDECAR_NAME: &str = ".qfs-meta.json";
/// Reconnect backoff, measured from the end of one attempt to the start of the next: a
/// handshake the host cut in half must not be retried at once (`connect`). It only ever slows
/// the retry down; it never gives up and it never touches the local files.
const RECONNECT_BACKOFF: Duration = Duration::from_secs(1);
const MAX_RECONNECT_BACKOFF: Duration = Duration::from_secs(5);
/// The last resort for a vault whose host answers no `STATUS` at all: this many refusals the
/// host authenticated, each at least `RECONNECT_BACKOFF` apart, mean the membership is gone.
const REFUSALS_BEFORE_REVOKED: u32 = 3;
/// After this many handshakes in a row that the host cut short, the provisional pair state is
/// dropped so the next attempt negotiates a fresh one (see `attempt_join`).
const HANDSHAKES_BEFORE_RESET: u32 = 2;
/// How often `history.json`, `nodes.json` and `host.json` may be rewritten. A 100 ms
/// heartbeat must not put three atomic file writes on the runtime thread.
const SAVE_INTERVAL: Duration = Duration::from_millis(500);
/// The host is asked who committed a record at most this often, and an actor nobody can
/// name after this long stays anonymous.
const BACKFILL_INTERVAL: Duration = Duration::from_secs(2);
const BACKFILL_GIVE_UP_MS: u64 = 30_000;

/// How one reconnect attempt ended. Only `Refused` is an answer the host authenticated;
/// `Failed` covers everything a restarting server also looks like.
enum Attempt {
    Joined,
    Refused,
    Failed,
}

/* ------------------------------------------------------------- requests */

/// Everything the root runtime asks of one vault. Vault-scoped UI calls arrive here.
pub enum VaultReq {
    ListTree(Reply<Vec<FsNode>>),
    NodeSnapshot(String, Reply<Option<(FsNode, String)>>),
    CreateNode(CreateNodeInput, Reply<FsNode>),
    RenameNode(RenameNodeInput, Reply<FsNode>),
    MoveNodes(MoveNodesInput, Reply<Vec<FsNode>>),
    DeleteNodes(DeleteNodesInput, Reply<()>),
    DuplicateNodes(DuplicateNodesInput, Reply<Vec<FsNode>>),
    SetNodeColor(SetNodeColorInput, Reply<FsNode>),
    RequestDownload(String, Reply<()>),
    OpenNode(String, Reply<()>),
    ImportFiles(String, Vec<PathBuf>, Reply<Vec<FsNode>>),
    ReadTextPreview(String, usize, Reply<Option<String>>),
    GetAccess(String, Reply<NodeAccess>),
    SetAccess(SetAccessInput, Reply<NodeAccess>),
    GetHistory(String, Reply<Vec<HistoryEvent>>),
    GetVaultMeta(Reply<VaultMeta>),
    UpdateVaultMeta(VaultMetaPatch, Reply<VaultMeta>),
    ListMembers(Reply<Vec<Member>>),
    GetPresence(Reply<Vec<PeerPresence>>),
    PublishPresence(PresenceInput, Reply<()>),
    AskAgent(String, Reply<AgentReply>),
    /// The admin credentials for this vault's host, or `None` when we have no token.
    Admin(Option<AdminTarget>),
    /// The newest `STATUS` numbers for this vault.
    Status(Box<VaultSnapshot>),
    /// Stop the task: `leave`, `delete`, or a revoked membership.
    Stop(Reply<()>),
}

/// What the admin poller last saw about this vault.
#[derive(Debug, Clone, Default)]
pub struct VaultSnapshot {
    pub join_code: String,
    /// The home screen reads these two from the root loop's copy of `STATUS`; they are carried
    /// here so one snapshot type describes everything one vault's poll produced.
    #[allow(dead_code)]
    pub quota_bytes: u64,
    #[allow(dead_code)]
    pub used_bytes: u64,
    pub member_count: u32,
    pub created_ms: u64,
    pub creator: Option<String>,
    pub members: Vec<AdminMember>,
}

/* -------------------------------------------------------------- sidecar */

/// One member's cosmetics, replicated so everyone draws the same contact card.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct SidecarMember {
    name: String,
    color: String,
}

/// `/.qfs-meta.json`: everything the workspace shows that the protocol has no field for
/// (docs/decisions/client-backend-embed.md). One replicated file, one writer at a time.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Sidecar {
    v: u32,
    name: String,
    description: String,
    created_at: u64,
    created_by: String,
    auto_cleanup: bool,
    cleanup_threshold_pct: u32,
    colors: BTreeMap<String, FolderColor>,
    access: BTreeMap<String, Vec<AccessEntry>>,
    members: BTreeMap<String, SidecarMember>,
}

impl Default for Sidecar {
    fn default() -> Self {
        Sidecar {
            v: 1,
            name: String::new(),
            description: String::new(),
            created_at: 0,
            created_by: String::new(),
            auto_cleanup: true,
            cleanup_threshold_pct: 85,
            colors: BTreeMap::new(),
            access: BTreeMap::new(),
            members: BTreeMap::new(),
        }
    }
}

/* -------------------------------------------------------- internal state */

/// The shape of one node last time we projected, for classifying what changed.
#[derive(Clone, PartialEq, Eq)]
struct Shape {
    parent: Option<String>,
    name: String,
    is_dir: bool,
    version: u64,
}

/// Timestamps the protocol does not carry; first sight counts as creation. Persisted in
/// `nodes.json`, so a restart does not reset every date to "now".
use super::state::NodeRecord as Rec;

/// One file being fetched, advanced a batch per heartbeat so the loop stays responsive.
struct Download {
    node_id: String,
    chunk_ids: Vec<ChunkId>,
    trusted: TrustedManifest,
    fetched: usize,
}

/// One projected node plus the bookkeeping the UI shape does not carry.
struct Entry {
    node: FsNode,
    file_id: FileId,
    path: String,
}

/* ------------------------------------------------------------------ node */

pub struct VaultNode {
    data_dir: PathBuf,
    dir: PathBuf,
    vault_hex: String,
    root_file: FileId,
    root_id: String,
    record: VaultRecord,
    profile: state::Profile,
    emit: Arc<dyn Fn(&str, Value) + Send + Sync>,
    root_tx: UnboundedSender<Req>,
    keys: KeyStore,
    self_peer: PeerId,
    replica: Option<Rc<RefCell<MemberReplica>>>,
    conn: Option<JoinedPeer>,
    next_connect: Instant,
    backoff: Duration,
    /// One handshake at a time. A `Join` that is abandoned half-way leaves the host holding a
    /// pair it will not rotate for the next one ("active pair cannot rotate during another
    /// Join", backend/src/net/session.rs:136), so a second attempt never starts while this is set.
    joining: bool,
    /// Consecutive handshakes the host cut short, which is what a disagreement about the pair
    /// the two sides hold looks like from here.
    handshake_failures: u32,
    /// Consecutive refusals the host authenticated, and when the last one was counted.
    refusals: u32,
    last_refusal: Option<Instant>,
    /// Were we ever admitted to this vault? A `STATUS` that does not list our peer only means
    /// "removed" for a peer that was in; on a first join it is simply the truth so far.
    was_member: bool,
    /// The last transient reason we logged, so a host that is down stays one line.
    last_note: String,
    /// The host's last verified ad and the two numbers the home screen falls back to.
    cache: state::VaultCache,
    admin: Option<AdminTarget>,
    snapshot: VaultSnapshot,
    /// Written once, right after the creator's first successful join.
    initial_meta: Option<(String, String)>,
    sidecar: Sidecar,
    sidecar_version: u64,
    /// Set on every join; cleared once this member's own sidecar entry is in place. While it
    /// is set the tick loop retries the write, which is how a joiner that had to wait for the
    /// opener's `.qfs-meta.json` gets its entry out (see `sync_sidecar`).
    sidecar_pending: bool,
    entries: BTreeMap<String, Entry>,
    /// Projection order: the root first, then depth-first, which is what `list_tree` returns.
    order: Vec<String>,
    shapes: HashMap<String, Shape>,
    records: HashMap<String, Rec>,
    /// Node ids our own commands touched, so `fs-changed` can name us as the actor.
    mine: HashSet<String>,
    history: Vec<HistoryEvent>,
    history_counter: u64,
    /// History entries whose actor is still unknown: the event, the control record that made
    /// it, and when we started asking — an answer nobody has after 30 s is given up on.
    unresolved: Vec<(String, u64, u64)>,
    next_backfill: Instant,
    last_applied: u64,
    active: Option<Download>,
    /// Files a click asked for, newest first. Kept apart from `queue` so the prefetch sort
    /// can never reorder a requested download behind a background one.
    front: VecDeque<String>,
    queue: VecDeque<(u64, String)>,
    known_members: BTreeSet<PeerId>,
    /// What we last told the vault we were doing. Local only, until member-to-member pairs exist.
    own_presence: Option<PresenceInput>,
    /// The first projection after the replica opens is a rehydration, not a set of new nodes.
    hydrated: bool,
    history_dirty: bool,
    records_dirty: bool,
    cache_dirty: bool,
    next_save: Instant,
    stopped: bool,
    /// Answered as the very last thing the task does, after the final save and after the
    /// connection, the replica and the `KeyStore` lock are gone.
    stop_reply: Option<Reply<()>>,
}

/// Open the vault's identity and start its task. The `KeyStore` takes an exclusive lock on the
/// directory, so this is also what guarantees one node per vault per process.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    data_dir: PathBuf,
    record: VaultRecord,
    profile: state::Profile,
    emit: Arc<dyn Fn(&str, Value) + Send + Sync>,
    root_tx: UnboundedSender<Req>,
    admin: Option<AdminTarget>,
    initial_meta: Option<(String, String)>,
) -> Result<UnboundedSender<VaultReq>, String> {
    let dir = state::vault_dir(&data_dir, &record.vault_id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create the vault folder: {e}"))?;
    let keys = KeyStore::open(&dir.join("identity")).map_err(|e| friendly(&e))?;
    keys.load_or_create().map_err(|e| friendly(&e))?;
    let self_peer = keys.peer_id().map_err(|e| friendly(&e))?;
    let vault_bytes = unhex32(&record.vault_id).ok_or("Unknown vault")?;
    let history = state::load_history(&data_dir, &record.vault_id);
    let history_counter = history.len() as u64;
    let records = state::load_records(&data_dir, &record.vault_id);
    let cache = state::load_cache(&data_dir, &record.vault_id);
    // A vault with history behind it is one this peer was admitted to on an earlier run, which
    // is what makes a `STATUS` without our peer in it evidence rather than a first join.
    let was_member = !history.is_empty();

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let node = VaultNode {
        data_dir,
        dir,
        vault_hex: record.vault_id.clone(),
        root_file: FileId(vault_bytes),
        root_id: root_node_id(&record.vault_id),
        record,
        profile,
        emit,
        root_tx,
        keys,
        self_peer,
        replica: None,
        conn: None,
        next_connect: Instant::now(),
        backoff: RECONNECT_BACKOFF,
        joining: false,
        handshake_failures: 0,
        refusals: 0,
        last_refusal: None,
        was_member,
        last_note: String::new(),
        cache,
        admin,
        snapshot: VaultSnapshot::default(),
        initial_meta,
        sidecar: Sidecar::default(),
        sidecar_version: 0,
        sidecar_pending: false,
        entries: BTreeMap::new(),
        order: Vec::new(),
        shapes: HashMap::new(),
        records,
        mine: HashSet::new(),
        history,
        history_counter,
        unresolved: Vec::new(),
        next_backfill: Instant::now(),
        last_applied: 0,
        active: None,
        front: VecDeque::new(),
        queue: VecDeque::new(),
        known_members: BTreeSet::new(),
        own_presence: None,
        hydrated: false,
        history_dirty: false,
        records_dirty: false,
        cache_dirty: false,
        next_save: Instant::now() + SAVE_INTERVAL,
        stopped: false,
        stop_reply: None,
    };
    tokio::task::spawn_local(node.run(rx));
    Ok(tx)
}

impl VaultNode {
    /// Requests and the heartbeat in one loop. `biased` keeps user actions ahead of the tick,
    /// and each arm runs to completion so no protocol exchange is ever cut in half.
    async fn run(mut self, mut rx: UnboundedReceiver<VaultReq>) {
        self.project();
        let mut next_tick = Instant::now();
        loop {
            tokio::select! {
                biased;
                message = rx.recv() => {
                    match message {
                        Some(req) => self.handle(req).await,
                        None => break,
                    }
                }
                _ = tokio::time::sleep_until(next_tick) => {
                    self.tick().await;
                    next_tick = Instant::now() + super::TICK_INTERVAL;
                }
            }
            if self.stopped {
                break;
            }
        }
        // The final save happens before anything is dropped, and the `Stop` reply is the very
        // last thing that happens: the root loop acts on that reply at once, so the live
        // connection, the replica and the `KeyStore` lock must already be gone.
        self.save_now().await;
        let reply = self.stop_reply.take();
        drop(self);
        if let Some(reply) = reply {
            let _ = reply.send(Ok(()));
        }
    }

    /* ----------------------------------------------------------- dispatch */

    async fn handle(&mut self, req: VaultReq) {
        match req {
            VaultReq::ListTree(reply) => {
                let tree = self.tree_list();
                let _ = reply.send(Ok(tree));
            }
            VaultReq::NodeSnapshot(id, reply) => {
                let found = self
                    .entries
                    .get(&id)
                    .map(|entry| (entry.node.clone(), self.vault_name()));
                let _ = reply.send(Ok(found));
            }
            VaultReq::CreateNode(input, reply) => {
                let result = self.create_node(input).await;
                let _ = reply.send(result);
            }
            VaultReq::RenameNode(input, reply) => {
                let result = self.rename_node(input).await;
                let _ = reply.send(result);
            }
            VaultReq::MoveNodes(input, reply) => {
                let result = self.move_nodes(input).await;
                let _ = reply.send(result);
            }
            VaultReq::DeleteNodes(input, reply) => {
                let result = self.delete_nodes(input).await;
                let _ = reply.send(result);
            }
            VaultReq::DuplicateNodes(input, reply) => {
                let result = self.duplicate_nodes(input).await;
                let _ = reply.send(result);
            }
            VaultReq::SetNodeColor(input, reply) => {
                let result = self.set_node_color(input).await;
                let _ = reply.send(result);
            }
            VaultReq::RequestDownload(node_id, reply) => {
                let result = self.request_download(&node_id);
                let _ = reply.send(result);
            }
            VaultReq::OpenNode(node_id, reply) => {
                let result = self.open_node(&node_id).await;
                let _ = reply.send(result);
            }
            VaultReq::ImportFiles(parent, paths, reply) => {
                let result = self.import_files(&parent, paths).await;
                let _ = reply.send(result);
            }
            VaultReq::ReadTextPreview(node_id, max_bytes, reply) => {
                let result = self.read_text_preview(&node_id, max_bytes);
                let _ = reply.send(result);
            }
            VaultReq::GetAccess(node_id, reply) => {
                let result = self.get_access(&node_id);
                let _ = reply.send(result);
            }
            VaultReq::SetAccess(input, reply) => {
                let result = self.set_access(input).await;
                let _ = reply.send(result);
            }
            VaultReq::GetHistory(node_id, reply) => {
                let result = self.get_history(&node_id);
                let _ = reply.send(Ok(result));
            }
            VaultReq::GetVaultMeta(reply) => {
                let meta = self.vault_meta();
                let _ = reply.send(Ok(meta));
            }
            VaultReq::UpdateVaultMeta(patch, reply) => {
                let result = self.update_vault_meta(patch).await;
                let _ = reply.send(result);
            }
            VaultReq::ListMembers(reply) => {
                let members = self.members();
                let _ = reply.send(Ok(members));
            }
            VaultReq::GetPresence(reply) => {
                let presence = self.presence();
                let _ = reply.send(Ok(presence));
            }
            VaultReq::PublishPresence(input, reply) => {
                // Member-to-member pairs are not orchestrated by the backend yet and live
                // cursors through H are forbidden (docs/decisions/client-backend-embed.md), so
                // this is stored locally and nothing is broadcast. No event either: the webview
                // publishes on every pointer move, and echoing that back is pure noise.
                self.own_presence = Some(input);
                let _ = reply.send(Ok(()));
            }
            VaultReq::AskAgent(folder_id, reply) => {
                let result = self.ask_agent(&folder_id);
                let _ = reply.send(result);
            }
            VaultReq::Admin(target) => self.admin = target,
            VaultReq::Status(snapshot) => self.apply_snapshot(*snapshot),
            VaultReq::Stop(reply) => {
                // Answered in `run`, after the last save and after every handle is dropped.
                self.stopped = true;
                self.stop_reply = Some(reply);
            }
        }
    }

    /* --------------------------------------------------------------- tick */

    async fn tick(&mut self) {
        if self.conn.is_none() {
            self.connect().await;
        }
        let mut failure = None;
        if let Some(conn) = self.conn.as_mut() {
            if let Err(error) = conn.heartbeat_once().await {
                failure = Some(error);
            }
        }
        if let Some(error) = failure {
            self.drop_session(&error);
        }
        self.refresh_sidecar().await;
        if self.sidecar_pending {
            self.sync_sidecar().await;
        }
        self.advance_download().await;
        self.project();
        self.watch_members();
        self.backfill().await;
        self.flush_saves().await;
    }

    /// Batched writer. Three atomic temp+rename writes every 100 ms would be the loop's
    /// biggest cost, so dirty state waits up to `SAVE_INTERVAL` and goes out off-thread.
    async fn flush_saves(&mut self) {
        if Instant::now() < self.next_save {
            return;
        }
        self.save_now().await;
    }

    async fn save_now(&mut self) {
        self.next_save = Instant::now() + SAVE_INTERVAL;
        if !self.history_dirty && !self.records_dirty && !self.cache_dirty {
            return;
        }
        let data_dir = self.data_dir.clone();
        let vault_hex = self.vault_hex.clone();
        let history = self.history_dirty.then(|| self.history.clone());
        let records = self.records_dirty.then(|| self.records.clone());
        let cache = self.cache_dirty.then(|| self.cache.clone());
        let _ = tokio::task::spawn_blocking(move || {
            if let Some(history) = history {
                state::save_history(&data_dir, &vault_hex, &history);
            }
            if let Some(records) = records {
                state::save_records(&data_dir, &vault_hex, &records);
            }
            if let Some(cache) = cache {
                state::save_cache(&data_dir, &vault_hex, &cache);
            }
        })
        .await;
        self.history_dirty = false;
        self.records_dirty = false;
        self.cache_dirty = false;
    }

    /// The host reconciles membership through the control log, so a new member appears in the
    /// replica without any command having been called here.
    fn watch_members(&mut self) {
        let Some(replica) = self.replica.as_ref() else {
            return;
        };
        let current = replica.borrow().members().clone();
        if current == self.known_members {
            return;
        }
        let first = self.known_members.is_empty();
        self.known_members = current;
        if !first {
            self.emit_members();
        }
    }

    /// Resolve the host's ad, open the replica if this is the first connect, then join.
    /// Copied from `backend/examples/vm_probe.rs`, which is the documented in-process member
    /// sequence, except that the ad may come from our own cache instead of the directory.
    async fn connect(&mut self) {
        if self.joining || Instant::now() < self.next_connect {
            return;
        }
        // One handshake at a time, always awaited to completion: a `Join` abandoned half-way
        // is what leaves the host with a pair it will not rotate for the next attempt.
        self.joining = true;
        let outcome = self.attempt_join().await;
        self.joining = false;
        // Spacing is measured from the end of the attempt, so a handshake that ran into its
        // own deadline is followed by a full pause instead of an immediate retry.
        self.next_connect = Instant::now() + self.backoff;
        match outcome {
            Attempt::Joined => {
                self.backoff = RECONNECT_BACKOFF;
                self.next_connect = Instant::now() + RECONNECT_BACKOFF;
                self.handshake_failures = 0;
                self.refusals = 0;
                self.last_refusal = None;
                self.was_member = true;
                self.last_note.clear();
            }
            // A refusal is the one answer the host authenticated, but `admit` gives a denied
            // peer, a stale code and a wrong vault id the same `AuthenticationFailed`
            // (backend/src/net/join.rs:294), so it is never enough on its own.
            Attempt::Refused => {
                self.backoff = (self.backoff * 2).min(MAX_RECONNECT_BACKOFF);
                self.note("the vault server refused the join code we have");
                let refusals = self.count_refusal();
                let starved = self.admin_target_now().is_none();
                if self.membership_gone().await
                    || (starved && refusals >= REFUSALS_BEFORE_REVOKED && self.was_member)
                {
                    self.revoke();
                }
            }
            // An EOF, a timeout or a directory that has forgotten the code says nothing by
            // itself — and after a kick it is all a peer ever sees, because the rotated code
            // leaves the directory and the host drops the session without answering. So every
            // failed attempt asks the host who its members are before backing off.
            Attempt::Failed => {
                self.backoff = (self.backoff * 2).min(MAX_RECONNECT_BACKOFF);
                if self.membership_gone().await {
                    self.revoke();
                }
            }
        }
    }

    /// One reconnect attempt, start to finish.
    async fn attempt_join(&mut self) -> Attempt {
        // A rotation (or a kick) changes the code; the poller keeps `vault.json` current.
        // `STATUS` prints the six-character short code for an authenticated admin and `-`
        // for everyone else, so each candidate is tried in newest-first order and whichever
        // form resolves wins: a short code derives into the join code the host admits with
        // (`backend/src/net/short_code.rs`).
        let candidates = [
            self.snapshot.join_code.clone(),
            self.record.join_code.clone(),
            self.cache.short_code.clone(),
        ];
        let Some((short, code)) = candidates
            .iter()
            .find_map(|text| state::resolve_join_code(text))
        else {
            self.note("this vault's join code is not readable");
            return Attempt::Failed;
        };
        if let Some(short) = short {
            self.remember_short(&short);
        }
        let Some(ad) = self.resolve_ad(code).await else {
            return Attempt::Failed;
        };
        if self.replica.is_none() {
            let members = BTreeSet::from([self.self_peer, ad.peer_id]);
            match MemberReplica::open_durable(
                self.keys.clone(),
                &self.dir,
                self.root_file,
                ad.peer_id,
                members,
            ) {
                Ok(replica) => self.replica = Some(Rc::new(RefCell::new(replica))),
                Err(error) => {
                    self.note_failure(&error);
                    return Attempt::Failed;
                }
            }
        }
        match join_host(self.keys.clone(), &ad, code, self.replica.clone()).await {
            Ok(joined) => {
                self.conn = Some(joined);
                self.emit_members();
                self.after_join().await;
                Attempt::Joined
            }
            Err(quantam_fs::Error::AuthenticationFailed) => Attempt::Refused,
            Err(error) => {
                self.note_failure(&error);
                // Two handshakes in a row that ended early mean the two sides disagree about
                // the pair they hold ("active pair cannot rotate during another Join",
                // backend/src/net/session.rs:136) and no number of retries will settle it.
                // What is dropped here is provisional key material for this one host — no
                // vault data, no identity — so the next attempt negotiates a fresh pair.
                self.handshake_failures = self.handshake_failures.saturating_add(1);
                if self.handshake_failures >= HANDSHAKES_BEFORE_RESET {
                    self.handshake_failures = 0;
                    if self.keys.discard_pair(ad.peer_id).is_ok() {
                        self.note("starting a fresh handshake with the vault server");
                    }
                }
                Attempt::Failed
            }
        }
    }

    /// The directory first, the last ad we verified second. A cold directory, a restarted one,
    /// a rotated code or an `Ok(None)` (which `DirectoryClient::lookup` also returns for a
    /// plain EOF, backend/src/net/directory.rs:195) must not keep an existing member out: the
    /// host re-admits a peer it already knows, so the cached address is enough to get back in.
    async fn resolve_ad(&mut self, code: JoinCode) -> Option<DirectoryAd> {
        // A vault joined from a bare code may have no directory address of its own yet, so
        // the configured one (`QFS_DIRECTORY_ADDR`, then `directory.txt`) stands in.
        let directory = self
            .record
            .directory_addr
            .parse::<SocketAddr>()
            .ok()
            .or_else(|| state::resolve_directory_addr(&self.data_dir, &[]));
        if let Some(addr) = directory {
            match DirectoryClient::new(addr).lookup(code).await {
                Ok(Some(ad)) => {
                    let verified = matches!(unix_time(), Ok(now) if ad.verify(now).is_ok());
                    if verified && ad.vault_id.0 == self.root_file.0 {
                        self.remember(&ad);
                        return Some(ad);
                    }
                    self.note("the vault server's directory entry did not verify");
                }
                Ok(None) => self.note("the directory has no entry for this join code yet"),
                Err(_) => self.note("the directory did not answer"),
            }
        }
        self.cached_ad()
    }

    /// Rebuild the last ad that verified, straight out of `host.json`. The directory's own
    /// window is seven days (`MAX_AD_AGE_SECS`), so this covers a rotation, a host restart
    /// and a directory outage, and stops working exactly when the signature expires.
    fn cached_ad(&mut self) -> Option<DirectoryAd> {
        if self.cache.host_addr.is_empty() {
            return None;
        }
        let addr = self.cache.host_addr.parse::<SocketAddr>().ok()?;
        let peer = unhex32(&self.cache.host_peer_id)?;
        let ad = DirectoryAd {
            peer_id: PeerId(peer),
            vault_id: VaultId(self.root_file.0),
            addr,
            ek: self.cache.host_ek.clone(),
            vk: self.cache.host_vk.clone(),
            issued_at: self.cache.ad_issued_at,
            signature: self.cache.ad_signature.clone(),
        };
        let now = unix_time().ok()?;
        if ad.verify(now).is_err() {
            self.note("the cached address of this vault's server is too old to use");
            return None;
        }
        Some(ad)
    }

    /// Keep the newest short code, which is what the UI shows and what a human retypes.
    fn remember_short(&mut self, short: &str) {
        if self.cache.short_code == short {
            return;
        }
        self.cache.short_code = short.to_string();
        self.cache_dirty = true;
    }

    /// Keep the host's advertised address and identity, so the next connect does not depend
    /// on the directory still carrying the code we happen to know.
    fn remember(&mut self, ad: &DirectoryAd) {
        let addr = ad.addr.to_string();
        let peer = hex(&ad.peer_id.0);
        if self.cache.host_addr == addr
            && self.cache.host_peer_id == peer
            && self.cache.ad_issued_at == ad.issued_at
        {
            return;
        }
        self.cache.host_addr = addr;
        self.cache.host_peer_id = peer;
        self.cache.host_ek = ad.ek.clone();
        self.cache.host_vk = ad.vk.clone();
        self.cache.ad_issued_at = ad.issued_at;
        self.cache.ad_signature = ad.signature.clone();
        self.cache_dirty = true;
    }

    /// The creator writes the sidecar once; every member adds its own cosmetics on first join.
    async fn after_join(&mut self) {
        self.refresh_sidecar().await;
        self.sidecar_pending = true;
        self.sync_sidecar().await;
    }

    /// Publish our own member entry — and, for the member that opened the vault, its name —
    /// into `.qfs-meta.json`. Creating that file is left to the opener: a peer whose bootstrap
    /// snapshot was staged before the opener's first write does not see the file, so it would
    /// `Link` a name the host already has, and the host answers that record by closing the
    /// session (`backend/src/store/tree.rs:134`) — which wedges the join, because the stale
    /// bootstrap is re-sent unchanged on every retry. A joiner therefore waits for live
    /// catch-up to deliver the file and writes its entry as an update on a later tick.
    async fn sync_sidecar(&mut self) {
        if self.conn.is_none() {
            return;
        }
        let exists = self
            .replica
            .as_ref()
            .is_some_and(|replica| replica.borrow().tree().resolve(SIDECAR_PATH).is_ok());
        if !exists && self.initial_meta.is_none() {
            return;
        }
        let mut next = self.sidecar.clone();
        let mut dirty = false;
        let opening = self.initial_meta.clone();
        if let Some((name, creator)) = opening {
            if next.created_at == 0 {
                next.v = 1;
                next.name = name;
                next.created_at = now_ms();
                next.created_by = creator;
                dirty = true;
            } else {
                self.initial_meta = None;
            }
        }
        let own = hex(&self.self_peer.0);
        let mine = SidecarMember {
            name: self.profile.name.clone(),
            color: self.profile.color.clone(),
        };
        if next.members.get(&own) != Some(&mine) {
            next.members.insert(own, mine);
            dirty = true;
        }
        // Only a write the host took clears the pending name: a failed one is retried on the
        // next tick rather than lost.
        if !dirty {
            self.sidecar_pending = false;
            return;
        }
        if self.write_sidecar(next).await.is_ok() {
            self.initial_meta = None;
            self.sidecar_pending = false;
        }
    }

    /// Everything that is not a refused join is transient: a directory that has not heard of
    /// the code yet, an EOF, a timeout, a restarting host, even an `AuthenticationFailed` from
    /// a heartbeat on a connection the host has already forgotten. None of it means "removed",
    /// so none of it counts towards revocation and none of it touches the local files.
    fn note_failure(&mut self, error: &quantam_fs::Error) {
        let reason = friendly(error);
        self.note(&reason);
    }

    /// One line per distinct reason: a server that is down for a minute is 600 ticks.
    fn note(&mut self, reason: &str) {
        if self.last_note == reason {
            return;
        }
        self.last_note = reason.to_string();
        let short = &self.vault_hex[..8.min(self.vault_hex.len())];
        eprintln!("[vault {short}] {reason}; retrying");
    }

    /// The live session ended. What that means is decided by the next attempt; all this does
    /// is bring the retry back to one second.
    fn drop_session(&mut self, error: &quantam_fs::Error) {
        self.conn = None;
        self.note_failure(error);
        self.backoff = RECONNECT_BACKOFF;
        self.next_connect = Instant::now() + RECONNECT_BACKOFF;
    }

    /// Where to ask this vault's host about itself. `STATUS` is read-only and the host answers
    /// it without `AUTH` (backend/src/net/admin.rs), so the token may well be empty; a host we
    /// only know from a cached ad is reachable at its peer port plus the admin offset.
    fn admin_target_now(&self) -> Option<AdminTarget> {
        if let Some(target) = self.admin.clone() {
            return Some(target);
        }
        let addr = self.cache.host_addr.parse::<SocketAddr>().ok()?;
        Some(AdminTarget {
            addr: SocketAddr::new(
                addr.ip(),
                addr.port().saturating_add(super::ADMIN_PORT_OFFSET),
            )
            .to_string(),
            token: String::new(),
        })
    }

    /// The one thing that means "removed", asked for at the moment of the failure rather than
    /// taken from the last poll: the host's own `STATUS` still serves this vault, still lists
    /// members for it, and our peer id is not among them. Positive evidence only — a host that
    /// will not answer, a vault row that is gone, an empty member list, or a peer that was
    /// never admitted in the first place all keep the loop retrying. Nothing is deleted either
    /// way: the task stops and the root loop moves the vault's folder aside.
    async fn membership_gone(&mut self) -> bool {
        if !self.was_member {
            return false;
        }
        let Some(target) = self.admin_target_now() else {
            return false;
        };
        let Ok(mut conn) = super::admin::AdminConn::connect(&target).await else {
            return false;
        };
        let Ok(status) = conn.status().await else {
            return false;
        };
        if status.vault(&self.vault_hex).is_none() {
            return false;
        }
        let members = status.members_of(&self.vault_hex);
        let own = hex(&self.self_peer.0);
        !members.is_empty() && !members.iter().any(|member| member.peer == own)
    }

    /// A refusal the host authenticated, counted at most once per `RECONNECT_BACKOFF`. Only
    /// these count towards the last-resort revocation; an EOF or a timeout never does.
    fn count_refusal(&mut self) -> u32 {
        let now = Instant::now();
        if self
            .last_refusal
            .is_some_and(|last| now.duration_since(last) < RECONNECT_BACKOFF)
        {
            return self.refusals;
        }
        self.last_refusal = Some(now);
        self.refusals = self.refusals.saturating_add(1);
        self.refusals
    }

    /// Stop the task and let the root loop tell the UI. No local file is touched here.
    fn revoke(&mut self) {
        self.stopped = true;
        let _ = self.root_tx.send(Req::VaultRevoked {
            vault_id: self.vault_hex.clone(),
            reason: "You were removed from this vault".to_string(),
        });
    }

    fn apply_snapshot(&mut self, snapshot: VaultSnapshot) {
        let online_before: BTreeSet<String> = self
            .snapshot
            .members
            .iter()
            .filter(|m| m.online)
            .map(|m| m.peer.clone())
            .collect();
        let online_now: BTreeSet<String> = snapshot
            .members
            .iter()
            .filter(|m| m.online)
            .map(|m| m.peer.clone())
            .collect();
        let changed = online_before != online_now
            || self.snapshot.member_count != snapshot.member_count
            || self.snapshot.join_code != snapshot.join_code;
        self.snapshot = snapshot;
        // A poll that lists us is proof we are in, which is what makes a later poll without us
        // evidence that we were removed rather than a peer that never joined.
        let own = hex(&self.self_peer.0);
        if self.snapshot.members.iter().any(|member| member.peer == own) {
            self.was_member = true;
        }
        if let Some(short) = state::normalize_short(&self.snapshot.join_code) {
            self.remember_short(&short);
        }
        if changed {
            self.emit_members();
            let peers = self.presence();
            self.emit_presence(peers);
        }
    }

    /* ------------------------------------------------------------ sidecar */

    /// Read `/.qfs-meta.json` whenever its manifest version moves, pulling it first if the
    /// bytes are not local. It is one small chunk, so this is cheap enough for the heartbeat.
    async fn refresh_sidecar(&mut self) {
        let Some(replica) = self.replica.clone() else {
            return;
        };
        let found = {
            let replica = replica.borrow();
            replica.tree().resolve(SIDECAR_PATH).ok().and_then(|id| {
                replica
                    .trusted_manifest(&id)
                    .map(|trusted| (id, trusted.clone()))
            })
        };
        let Some((file_id, trusted)) = found else {
            return;
        };
        if trusted.manifest().version == self.sidecar_version {
            return;
        }
        if !self.is_local(&trusted) {
            let ids = trusted.manifest().chunk_ids.clone();
            if self.pull_batches(&ids, &trusted).await.is_err() {
                return;
            }
        }
        let Some(bytes) = self.assemble(&file_id) else {
            return;
        };
        if let Ok(parsed) = serde_json::from_slice::<Sidecar>(&bytes) {
            self.sidecar = parsed;
            self.sidecar_version = trusted.manifest().version;
            self.emit(
                "backend://vault-changed",
                &VaultIdPayload {
                    vault_id: self.vault_hex.clone(),
                },
            );
        }
    }

    /// Read-modify-write: the whole document goes back as one chunk. `next` only replaces the
    /// copy in memory once the host has taken it, so a failed write leaves local state exactly
    /// as it was and the caller's error is the whole story.
    async fn write_sidecar(&mut self, next: Sidecar) -> Result<(), String> {
        let bytes = serde_json::to_vec(&next)
            .map_err(|_| "Could not write the vault's settings".to_string())?;
        let conn = self
            .conn
            .as_mut()
            .ok_or("Not connected to the vault server yet")?;
        conn.save_file(SIDECAR_PATH, &[bytes])
            .await
            .map_err(|e| friendly(&e))?;
        self.sidecar = next;
        // The version we just wrote is the one we already hold in memory.
        if let Some(replica) = self.replica.clone() {
            let version = {
                let replica = replica.borrow();
                replica
                    .tree()
                    .resolve(SIDECAR_PATH)
                    .ok()
                    .and_then(|id| replica.trusted_manifest(&id))
                    .map(|trusted| trusted.manifest().version)
            };
            if let Some(version) = version {
                self.sidecar_version = version;
            }
        }
        Ok(())
    }

    /* --------------------------------------------------------- projection */

    /// Rebuild every node from the replica, diff against the last projection, and emit.
    fn project(&mut self) {
        let now = now_ms();
        let dirents = match self.replica.as_ref() {
            Some(replica) => replica.borrow().tree().dirents(),
            None => Vec::new(),
        };
        let raw = self.walk(&dirents);

        // 1. classify what changed, so history and timestamps agree with the diff.
        let mut shapes: HashMap<String, Shape> = HashMap::new();
        let mut events: Vec<(String, HistoryKind, Option<String>, Option<String>, String)> =
            Vec::new();
        let mut touched: HashSet<String> = HashSet::new();
        for item in &raw {
            let shape = Shape {
                parent: item.parent.clone(),
                name: item.name.clone(),
                is_dir: item.is_dir,
                version: item.version,
            };
            match self.shapes.get(&item.id) {
                None => {
                    if self.records.contains_key(&item.id) {
                        // Seen before this session; keep its stored timestamps.
                    } else {
                        events.push((item.id.clone(), HistoryKind::Created, None, None, "created".into()));
                        touched.insert(item.id.clone());
                    }
                }
                Some(old) if *old != shape => {
                    touched.insert(item.id.clone());
                    if old.parent != shape.parent {
                        let from = old.parent.as_ref().and_then(|id| self.name_of(id));
                        let to = shape.parent.as_ref().and_then(|id| self.name_of(id));
                        let summary = match (&from, &to) {
                            (Some(a), Some(b)) => format!("moved from {a} to {b}"),
                            (_, Some(b)) => format!("moved to {b}"),
                            _ => "moved".to_string(),
                        };
                        events.push((item.id.clone(), HistoryKind::Moved, from, to, summary));
                    } else if old.name != shape.name {
                        events.push((
                            item.id.clone(),
                            HistoryKind::Renamed,
                            Some(old.name.clone()),
                            Some(shape.name.clone()),
                            format!("renamed from {}", old.name),
                        ));
                    } else if old.version != shape.version {
                        events.push((
                            item.id.clone(),
                            HistoryKind::Modified,
                            None,
                            None,
                            "contents changed".to_string(),
                        ));
                    }
                }
                Some(_) => {}
            }
            shapes.insert(item.id.clone(), shape);
        }
        // `self.shapes` is a `HashMap`, so its iteration order would shuffle the deletions
        // this tick produces; sorted, the record ids zipped onto these events below line up
        // with insertion order run after run. (Pairing stays a best effort: the host commits
        // one record per operation, but nothing in the log names the node it touched.)
        let mut gone: Vec<(&String, &Shape)> = self
            .shapes
            .iter()
            .filter(|(id, _)| !shapes.contains_key(*id))
            .collect();
        gone.sort_by(|a, b| a.1.name.cmp(&b.1.name).then_with(|| a.0.cmp(b.0)));
        for (id, old) in gone {
            touched.insert(id.clone());
            if let Some(parent) = &old.parent {
                events.push((
                    parent.clone(),
                    HistoryKind::Deleted,
                    Some(old.name.clone()),
                    None,
                    format!("deleted {}", old.name),
                ));
                touched.insert(parent.clone());
            }
        }

        // 2. our own pending ops decide the actor; anything else is an unknown remote member.
        let ours = touched.iter().any(|id| self.mine.contains(id));
        let actor = if ours {
            self.profile.peer_id.clone()
        } else {
            String::new()
        };

        // 3. timestamps: a change touches the node and every ancestor whose totals moved.
        let parents: HashMap<String, Option<String>> = raw
            .iter()
            .map(|item| (item.id.clone(), item.parent.clone()))
            .collect();
        let mut bumped: HashSet<String> = HashSet::new();
        for id in &touched {
            let mut cursor = Some(id.clone());
            let mut guard = 0;
            while let Some(current) = cursor {
                if !bumped.insert(current.clone()) {
                    break;
                }
                cursor = parents.get(&current).cloned().flatten();
                guard += 1;
                if guard > 4096 {
                    break;
                }
            }
        }
        let mut records_changed = false;
        for item in &raw {
            let writer = item
                .writer
                .as_ref()
                .map(|peer| self.present(peer))
                .unwrap_or_default();
            let mut fresh = false;
            let entry = self.records.entry(item.id.clone()).or_insert_with(|| {
                fresh = true;
                Rec {
                    created_at: now,
                    modified_at: now,
                    created_by: if actor.is_empty() { writer.clone() } else { actor.clone() },
                    modified_by: if actor.is_empty() { writer.clone() } else { actor.clone() },
                }
            });
            if bumped.contains(&item.id) {
                entry.modified_at = now;
                entry.modified_by = if actor.is_empty() {
                    writer
                } else {
                    actor.clone()
                };
                records_changed = true;
            }
            records_changed |= fresh;
        }
        // Only once the replica is open: the very first projection of a restart has no tree
        // yet, and dropping the stored dates there is exactly how they get lost.
        if self.replica.is_some() {
            let before = self.records.len();
            self.records.retain(|id, _| shapes.contains_key(id));
            records_changed |= self.records.len() != before;
        }
        self.records_dirty |= records_changed;

        // 4. build the wire nodes.
        let mut entries: BTreeMap<String, Entry> = BTreeMap::new();
        let mut order: Vec<String> = Vec::with_capacity(raw.len());
        let mut sizes: HashMap<String, u64> = HashMap::new();
        let mut children: HashMap<String, u32> = HashMap::new();
        for item in &raw {
            if let Some(parent) = &item.parent {
                *children.entry(parent.clone()).or_insert(0) += 1;
            }
            if !item.is_dir {
                sizes.insert(item.id.clone(), item.size);
            }
        }
        // Pre-order guarantees a parent precedes its children, so reverse rolls sizes up.
        for item in raw.iter().rev() {
            let own = sizes.get(&item.id).copied().unwrap_or(0);
            if let Some(parent) = &item.parent {
                *sizes.entry(parent.clone()).or_insert(0) += own;
            }
        }
        for item in raw {
            let rec = self.records.get(&item.id).cloned().unwrap_or(Rec {
                created_at: now,
                modified_at: now,
                created_by: String::new(),
                modified_by: String::new(),
            });
            let (availability, progress) = if item.is_dir {
                (Availability::Local, None)
            } else {
                self.availability(&item)
            };
            let holders = if item.is_dir {
                Vec::new()
            } else {
                self.holders(availability)
            };
            let node = FsNode {
                id: item.id.clone(),
                vault_id: self.vault_hex.clone(),
                parent_id: item.parent.clone(),
                kind: if item.is_dir {
                    NodeKind::Folder
                } else {
                    NodeKind::File
                },
                name: item.name.clone(),
                size_bytes: sizes.get(&item.id).copied().unwrap_or(0),
                created_at: rec.created_at,
                modified_at: rec.modified_at,
                created_by: rec.created_by,
                modified_by: rec.modified_by,
                color: if item.is_dir {
                    self.sidecar.colors.get(&item.id).copied()
                } else {
                    None
                },
                availability,
                progress,
                holders,
                child_count: children.get(&item.id).copied().unwrap_or(0),
            };
            order.push(item.id.clone());
            entries.insert(
                item.id.clone(),
                Entry {
                    node,
                    file_id: item.file_id,
                    path: item.path,
                },
            );
        }

        // 4b. the two numbers the home screen falls back to for a vault whose host we have no
        // admin token for: the replica's own member count and the bytes its manifests add up
        // to (the root's rolled-up size is exactly that sum).
        let member_count = self
            .replica
            .as_ref()
            .map(|replica| replica.borrow().members().len() as u32)
            .unwrap_or(0);
        let used_bytes = sizes.get(&self.root_id).copied().unwrap_or(0);
        if member_count > 0
            && (self.cache.member_count != member_count || self.cache.used_bytes != used_bytes)
        {
            self.cache.member_count = member_count;
            self.cache.used_bytes = used_bytes;
            self.cache_dirty = true;
        }

        // 5. history, prefetch, and the one event this tick produces.
        let record_ids = self.new_record_ids();
        let mut ids = record_ids.into_iter();
        if self.hydrated {
            for (node_id, kind, from, to, summary) in events {
                let record = ids.next();
                self.log(&node_id, kind, &actor, from, to, summary, record);
            }
        }
        if self.replica.is_some() {
            self.hydrated = true;
        }
        self.enqueue_prefetch(&entries);
        self.emit_diff(entries, order, &actor);
        self.shapes = shapes;
        if ours {
            self.mine.clear();
        }
    }

    /// Depth-first from the root, skipping the hidden sidecar.
    fn walk(&self, dirents: &[Dirent]) -> Vec<Raw> {
        let mut children: HashMap<FileId, Vec<&Dirent>> = HashMap::new();
        for entry in dirents {
            if entry.name.is_empty() {
                continue;
            }
            children.entry(entry.parent).or_default().push(entry);
        }
        for list in children.values_mut() {
            list.sort_by(|a, b| a.name.cmp(&b.name));
        }
        let mut out = vec![Raw {
            file_id: self.root_file,
            id: self.root_id.clone(),
            parent: None,
            name: self.vault_name(),
            is_dir: true,
            version: 0,
            size: 0,
            writer: None,
            path: "/".to_string(),
        }];
        let mut stack = vec![(self.root_file, self.root_id.clone(), Vec::<String>::new())];
        let mut guard = 0usize;
        while let Some((file_id, node_id, prefix)) = stack.pop() {
            guard += 1;
            if guard > 100_000 {
                break;
            }
            let Some(list) = children.get(&file_id) else {
                continue;
            };
            for entry in list.iter().rev() {
                if file_id == self.root_file && entry.name == SIDECAR_NAME {
                    continue;
                }
                let id = hex(&entry.child.0);
                let mut components = prefix.clone();
                components.push(entry.name.clone());
                let (size, version, writer) = if entry.is_dir {
                    (0, 0, None)
                } else {
                    self.manifest_facts(&entry.child)
                };
                out.push(Raw {
                    file_id: entry.child,
                    id: id.clone(),
                    parent: Some(node_id.clone()),
                    name: entry.name.clone(),
                    is_dir: entry.is_dir,
                    version,
                    size,
                    writer,
                    path: vault_path(&components),
                });
                if entry.is_dir {
                    stack.push((entry.child, id, components));
                }
            }
        }
        out
    }

    fn manifest_facts(&self, file_id: &FileId) -> (u64, u64, Option<PeerId>) {
        let Some(replica) = self.replica.as_ref() else {
            return (0, 0, None);
        };
        let replica = replica.borrow();
        match replica.trusted_manifest(file_id) {
            Some(trusted) => {
                let manifest = trusted.manifest();
                (manifest.size, manifest.version, Some(manifest.writer_id))
            }
            None => (0, 0, None),
        }
    }

    fn availability(&self, item: &Raw) -> (Availability, Option<f64>) {
        let Some(replica) = self.replica.as_ref() else {
            return (Availability::Remote, None);
        };
        let local = {
            let replica = replica.borrow();
            match replica.trusted_manifest(&item.file_id) {
                Some(trusted) => self.is_local(trusted),
                None => false,
            }
        };
        if local {
            return (Availability::Local, None);
        }
        if let Some(active) = &self.active {
            if active.node_id == item.id {
                let total = active.chunk_ids.len().max(1) as f64;
                return (
                    Availability::Downloading,
                    Some((active.fetched as f64 / total).clamp(0.0, 1.0)),
                );
            }
        }
        (Availability::Remote, None)
    }

    /// H owns the full replica, so it holds every file; we appear once the bytes are here.
    fn holders(&self, availability: Availability) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(replica) = self.replica.as_ref() {
            out.push(hex(&replica.borrow().host_id().0));
        }
        if availability == Availability::Local {
            out.push(self.profile.peer_id.clone());
        }
        out
    }

    fn is_local(&self, trusted: &TrustedManifest) -> bool {
        let Some(replica) = self.replica.as_ref() else {
            return false;
        };
        let store = replica.borrow().chunks();
        let Ok(store) = store.lock() else {
            return false;
        };
        trusted
            .manifest()
            .chunk_ids
            .iter()
            .all(|chunk| store.has(chunk))
    }

    /// Whole-node upserts for anything that changed, removes for anything gone: exactly the
    /// delta contract `client/src/lib/backend/client.ts` documents.
    fn emit_diff(&mut self, entries: BTreeMap<String, Entry>, order: Vec<String>, actor: &str) {
        let mut changes = Vec::new();
        for id in &order {
            let Some(entry) = entries.get(id) else { continue };
            let changed = match self.entries.get(id) {
                Some(old) => !same_node(&old.node, &entry.node),
                None => true,
            };
            if changed {
                changes.push(FsChange::Upsert {
                    node: entry.node.clone(),
                });
            }
        }
        for id in self.entries.keys() {
            if !entries.contains_key(id) {
                changes.push(FsChange::Remove {
                    node_id: id.clone(),
                });
            }
        }
        self.entries = entries;
        self.order = order;
        if changes.is_empty() {
            return;
        }
        self.emit(
            "backend://fs-changed",
            &FsChangedPayload {
                vault_id: self.vault_hex.clone(),
                changes,
                actor: actor.to_string(),
            },
        );
    }

    fn tree_list(&self) -> Vec<FsNode> {
        self.order
            .iter()
            .filter_map(|id| self.entries.get(id).map(|entry| entry.node.clone()))
            .collect()
    }

    /* ------------------------------------------------------------ history */

    #[allow(clippy::too_many_arguments)]
    fn log(
        &mut self,
        node_id: &str,
        kind: HistoryKind,
        by: &str,
        from: Option<String>,
        to: Option<String>,
        summary: String,
        record: Option<u64>,
    ) {
        self.history_counter += 1;
        let id = format!("h_{}_{}", self.vault_hex, self.history_counter);
        self.history.push(HistoryEvent {
            id: id.clone(),
            vault_id: self.vault_hex.clone(),
            node_id: node_id.to_string(),
            kind,
            at: now_ms(),
            by: by.to_string(),
            from,
            to,
            summary,
        });
        if self.history.len() > state::MAX_HISTORY {
            let excess = self.history.len() - state::MAX_HISTORY;
            self.history.drain(..excess);
        }
        if by.is_empty() {
            if let Some(record) = record {
                self.unresolved.push((id, record, now_ms()));
            }
        }
        self.history_dirty = true;
    }

    /// Control record ids committed since the last projection, oldest first.
    fn new_record_ids(&mut self) -> Vec<u64> {
        let Some(replica) = self.replica.as_ref() else {
            return Vec::new();
        };
        let (applied, ids) = {
            let replica = replica.borrow();
            let applied = replica.last_applied();
            let ids: Vec<u64> = replica
                .instruction_log()
                .iter()
                .map(|record| record.id)
                .filter(|id| *id > self.last_applied && *id <= applied)
                .collect();
            (applied, ids)
        };
        self.last_applied = applied.max(self.last_applied);
        ids
    }

    /// Ask the host who committed the records behind our anonymous history entries.
    async fn backfill(&mut self) {
        // An unknown actor is cosmetic. Ask the host every couple of seconds, not every
        // 100 ms tick, and stop asking about a record nobody named within 30 s.
        let cutoff = now_ms().saturating_sub(BACKFILL_GIVE_UP_MS);
        self.unresolved.retain(|(_, _, asked_at)| *asked_at >= cutoff);
        if self.unresolved.is_empty() || Instant::now() < self.next_backfill {
            return;
        }
        self.next_backfill = Instant::now() + BACKFILL_INTERVAL;
        let Some(target) = self.admin.clone() else {
            return;
        };
        let since = self
            .unresolved
            .iter()
            .map(|(_, record, _)| *record)
            .min()
            .unwrap_or(0)
            .saturating_sub(1);
        let vault = self.vault_hex.clone();
        let ops = match super::admin::AdminConn::connect(&target).await {
            Ok(mut conn) => match conn.ops(&vault, since).await {
                Ok(ops) => ops,
                Err(_) => return,
            },
            Err(_) => return,
        };
        if ops.is_empty() {
            return;
        }
        let by_record: HashMap<u64, (String, u64)> = ops
            .into_iter()
            .map(|op| (op.record_id, (op.actor, op.at)))
            .collect();
        let mut resolved = false;
        let mut remaining = Vec::new();
        for (event_id, record, asked_at) in std::mem::take(&mut self.unresolved) {
            match by_record.get(&record) {
                Some((actor, at)) => {
                    let actor = self.present_hex(actor);
                    let at = *at;
                    if let Some(event) = self.history.iter_mut().find(|e| e.id == event_id) {
                        event.by = actor;
                        if at > 0 {
                            event.at = at;
                        }
                        resolved = true;
                    }
                }
                None => remaining.push((event_id, record, asked_at)),
            }
        }
        self.unresolved = remaining;
        if resolved {
            self.history_dirty = true;
            self.emit(
                "backend://vault-changed",
                &VaultIdPayload {
                    vault_id: self.vault_hex.clone(),
                },
            );
        }
    }

    /// A node's own events plus its direct children's, newest first.
    fn get_history(&self, node_id: &str) -> Vec<HistoryEvent> {
        let children: HashSet<&String> = self
            .order
            .iter()
            .filter(|id| {
                self.entries
                    .get(*id)
                    .is_some_and(|entry| entry.node.parent_id.as_deref() == Some(node_id))
            })
            .collect();
        let mut out: Vec<HistoryEvent> = self
            .history
            .iter()
            .filter(|event| event.node_id == node_id || children.contains(&event.node_id))
            .cloned()
            .collect();
        out.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| b.id.cmp(&a.id)));
        out
    }

    /* ---------------------------------------------------------- mutations */

    async fn create_node(&mut self, input: CreateNodeInput) -> Result<FsNode, String> {
        let name = input.name.trim().to_string();
        let parent = self.require_folder(&input.parent_id)?;
        self.check_name_free(&input.parent_id, &name, None)?;
        let path = child_path(&parent, &name);
        let file_id = {
            let conn = self.live()?;
            match input.kind {
                NodeKind::Folder => conn.mkdir(&path).await.map_err(|e| friendly(&e))?,
                NodeKind::File => conn.save_file(&path, &[]).await.map_err(|e| friendly(&e))?,
            }
        };
        let id = hex(&file_id.0);
        self.mine.insert(id.clone());
        self.settle().await;
        self.node_or_missing(&id)
    }

    async fn rename_node(&mut self, input: RenameNodeInput) -> Result<FsNode, String> {
        let name = input.name.trim().to_string();
        if input.node_id == self.root_id {
            // The vault root's name is cosmetic: it lives in the sidecar, not in the tree.
            if name.is_empty() || name.chars().count() > 40 {
                return Err("Invalid vault name".to_string());
            }
            let mut next = self.sidecar.clone();
            next.name = name;
            self.write_sidecar(next).await?;
            // `vault.json` caches the name for the home screen, so the root loop has to hear
            // about a rename that came in through the workspace as well as through settings.
            let _ = self.root_tx.send(Req::VaultNameChanged {
                vault_id: self.vault_hex.clone(),
                name: self.vault_name(),
            });
            self.emit_vault_changed();
            self.mine.insert(self.root_id.clone());
            self.settle().await;
            return self.node_or_missing(&self.root_id.clone());
        }
        let entry = self.require_node(&input.node_id)?;
        let parent_id = entry
            .node
            .parent_id
            .clone()
            .ok_or("Can't delete the vault root")?;
        self.check_name_free(&parent_id, &name, Some(&input.node_id))?;
        let parent = self.require_folder(&parent_id)?;
        let source = entry.path.clone();
        let destination = child_path(&parent, &name);
        {
            let conn = self.live()?;
            conn.rename(&source, &destination)
                .await
                .map_err(|e| friendly(&e))?;
        }
        self.mine.insert(input.node_id.clone());
        self.settle().await;
        self.node_or_missing(&input.node_id)
    }

    async fn move_nodes(&mut self, input: MoveNodesInput) -> Result<Vec<FsNode>, String> {
        let target = self.require_folder(&input.to_parent_id)?;
        // Validated as a whole first: a mixed drag must not half-apply.
        let mut planned: Vec<(String, String, String)> = Vec::new();
        let mut taken: Vec<String> = self.child_names(&input.to_parent_id, None);
        for node_id in &input.node_ids {
            let entry = self.require_node(node_id)?;
            let name = entry.node.name.clone();
            let source = entry.path.clone();
            if entry.node.parent_id.is_none() {
                return Err("Can't move a folder into itself".to_string());
            }
            if entry.node.parent_id.as_deref() == Some(input.to_parent_id.as_str()) {
                continue;
            }
            if *node_id == input.to_parent_id || self.is_descendant(&input.to_parent_id, node_id) {
                return Err("Can't move a folder into itself".to_string());
            }
            let lower = name.to_lowercase();
            if taken.iter().any(|existing| existing.to_lowercase() == lower) {
                return Err(collision_message(entry.node.kind));
            }
            taken.push(name.clone());
            planned.push((node_id.clone(), source, child_path(&target, &name)));
        }
        for (node_id, source, destination) in &planned {
            {
                let conn = self.live()?;
                conn.rename(source, destination)
                    .await
                    .map_err(|e| friendly(&e))?;
            }
            self.mine.insert(node_id.clone());
        }
        self.settle().await;
        Ok(planned
            .iter()
            .filter_map(|(id, _, _)| self.entries.get(id).map(|entry| entry.node.clone()))
            .collect())
    }

    async fn delete_nodes(&mut self, input: DeleteNodesInput) -> Result<(), String> {
        // A selection may hold a folder and something inside it. The folder's own subtree
        // already unlinks the child, and unlinking a path twice fails, so a selected id that
        // is a descendant of another selected id is dropped before anything is planned.
        let mut roots: Vec<&String> = Vec::new();
        for node_id in &input.node_ids {
            if *node_id == self.root_id {
                return Err("Can't delete the vault root".to_string());
            }
            let entry = self.require_node(node_id)?;
            if entry.node.parent_id.is_none() {
                return Err("Can't delete the vault root".to_string());
            }
            if input
                .node_ids
                .iter()
                .any(|other| other != node_id && self.is_descendant(node_id, other))
            {
                continue;
            }
            roots.push(node_id);
        }
        let mut paths: Vec<String> = Vec::new();
        let mut owned: Vec<String> = Vec::new();
        for node_id in roots {
            // Pre-order subtree reversed is post-order: children (and files) unlink first,
            // which is the only order `DirectoryTree::unlink` accepts for a folder.
            for id in self.subtree(node_id).iter().rev() {
                if let Some(entry) = self.entries.get(id) {
                    paths.push(entry.path.clone());
                    owned.push(id.clone());
                }
            }
        }
        for id in owned {
            self.mine.insert(id);
        }
        for path in &paths {
            let conn = self.live()?;
            conn.unlink(path).await.map_err(|e| friendly(&e))?;
        }
        self.settle().await;
        Ok(())
    }

    async fn duplicate_nodes(&mut self, input: DuplicateNodesInput) -> Result<Vec<FsNode>, String> {
        let mut made: Vec<String> = Vec::new();
        for node_id in &input.node_ids {
            let entry = self.require_node(node_id)?;
            if entry.node.parent_id.is_none() {
                return Err("Can't duplicate the vault root".to_string());
            }
            let parent_id = match &input.to_parent_id {
                Some(id) => id.clone(),
                None => entry.node.parent_id.clone().unwrap_or(self.root_id.clone()),
            };
            let parent = self.require_folder(&parent_id)?;
            let taken = self.child_names(&parent_id, None);
            let name = unique_name(&taken, &entry.node.name);
            let source = entry.node.name.clone();
            let destination = child_path(&parent, &name);
            let id = self.copy_into(node_id, &destination).await?;
            self.mine.insert(id.clone());
            made.push(id.clone());
            self.settle().await;
            let actor = self.profile.peer_id.clone();
            self.log(
                &id,
                HistoryKind::Duplicated,
                &actor,
                Some(source.clone()),
                Some(name),
                format!("duplicated from {source}"),
                None,
            );
        }
        Ok(made
            .iter()
            .filter_map(|id| self.entries.get(id).map(|entry| entry.node.clone()))
            .collect())
    }

    /// Recursive copy. Files are re-chunked under a fresh file id; folders recurse by name.
    async fn copy_into(&mut self, node_id: &str, destination: &str) -> Result<String, String> {
        let entry = self.require_node(node_id)?;
        if entry.node.kind == NodeKind::File {
            let bodies = self.file_bodies(node_id).await?;
            let conn = self.live()?;
            let file_id = conn
                .save_file(destination, &bodies)
                .await
                .map_err(|e| friendly(&e))?;
            return Ok(hex(&file_id.0));
        }
        let file_id = {
            let conn = self.live()?;
            conn.mkdir(destination).await.map_err(|e| friendly(&e))?
        };
        let children: Vec<(String, String)> = self
            .order
            .iter()
            .filter_map(|id| self.entries.get(id))
            .filter(|child| child.node.parent_id.as_deref() == Some(node_id))
            .map(|child| (child.node.id.clone(), child.node.name.clone()))
            .collect();
        for (child_id, name) in children {
            let target = format!("{}/{}", destination.trim_end_matches('/'), name);
            Box::pin(self.copy_into(&child_id, &target)).await?;
        }
        Ok(hex(&file_id.0))
    }

    async fn set_node_color(&mut self, input: SetNodeColorInput) -> Result<FsNode, String> {
        self.require_node(&input.node_id)?;
        let mut next = self.sidecar.clone();
        match input.color {
            Some(color) => {
                next.colors.insert(input.node_id.clone(), color);
            }
            None => {
                next.colors.remove(&input.node_id);
            }
        }
        self.write_sidecar(next).await?;
        let actor = self.profile.peer_id.clone();
        self.log(
            &input.node_id,
            HistoryKind::Colored,
            &actor,
            None,
            input.color.map(|c| c.as_str().to_string()),
            match input.color {
                Some(color) => format!("set color to {}", color.as_str()),
                None => "cleared the color".to_string(),
            },
            None,
        );
        self.mine.insert(input.node_id.clone());
        self.settle().await;
        self.node_or_missing(&input.node_id)
    }

    /* ------------------------------------------------------------- access */

    fn get_access(&self, node_id: &str) -> Result<NodeAccess, String> {
        self.require_node(node_id)?;
        if let Some(entries) = self.sidecar.access.get(node_id) {
            return Ok(NodeAccess {
                node_id: node_id.to_string(),
                inherit: false,
                entries: entries.clone(),
            });
        }
        let mut cursor = self
            .entries
            .get(node_id)
            .and_then(|entry| entry.node.parent_id.clone());
        while let Some(current) = cursor {
            if let Some(entries) = self.sidecar.access.get(&current) {
                return Ok(NodeAccess {
                    node_id: node_id.to_string(),
                    inherit: true,
                    entries: entries.clone(),
                });
            }
            cursor = self
                .entries
                .get(&current)
                .and_then(|entry| entry.node.parent_id.clone());
        }
        Ok(NodeAccess {
            node_id: node_id.to_string(),
            inherit: true,
            entries: Vec::new(),
        })
    }

    async fn set_access(&mut self, input: SetAccessInput) -> Result<NodeAccess, String> {
        self.require_node(&input.node_id)?;
        let mut next = self.sidecar.clone();
        let summary = if input.inherit {
            next.access.remove(&input.node_id);
            "reset access to inherited".to_string()
        } else {
            // The creator is an editor by construction and can never be removed.
            let creator = self.sidecar.created_by.clone();
            let mut seen: HashSet<String> = HashSet::new();
            let mut entries: Vec<AccessEntry> = Vec::new();
            for entry in input.entries {
                if entry.peer_id == creator || !seen.insert(entry.peer_id.clone()) {
                    continue;
                }
                entries.push(entry);
            }
            let count = entries.len();
            next.access.insert(input.node_id.clone(), entries);
            format!(
                "changed access for {count} {}",
                if count == 1 { "member" } else { "members" }
            )
        };
        self.write_sidecar(next).await?;
        let actor = self.profile.peer_id.clone();
        self.log(
            &input.node_id,
            HistoryKind::Access,
            &actor,
            None,
            None,
            summary,
            None,
        );
        self.get_access(&input.node_id)
    }

    /* -------------------------------------------------------------- files */

    /// Queue a pull and return at once; progress reaches the UI as `fs-changed` upserts.
    /// A requested file goes into `front`, which nothing reorders: the prefetch queue is
    /// sorted by size every tick and would otherwise push a big requested file to the back.
    fn request_download(&mut self, node_id: &str) -> Result<(), String> {
        let (kind, availability, size) = {
            let entry = self.require_node(node_id)?;
            (
                entry.node.kind,
                entry.node.availability,
                entry.node.size_bytes,
            )
        };
        if kind != NodeKind::File {
            return Err("Can't put things inside a file".to_string());
        }
        if availability == Availability::Local {
            return Ok(());
        }
        let _ = size;
        self.queue.retain(|(_, id)| id != node_id);
        self.front.retain(|id| id != node_id);
        self.front.push_front(node_id.to_string());
        Ok(())
    }

    /// New remote files small enough to be worth having before they are clicked.
    fn enqueue_prefetch(&mut self, entries: &BTreeMap<String, Entry>) {
        for (id, entry) in entries {
            if entry.node.kind != NodeKind::File
                || entry.node.availability != Availability::Remote
                || entry.node.size_bytes > PREFETCH_MAX_BYTES
                || self.entries.contains_key(id)
                || self.queue.iter().any(|(_, queued)| queued == id)
                || self.front.iter().any(|queued| queued == id)
                || self.active.as_ref().is_some_and(|a| a.node_id == *id)
            {
                continue;
            }
            self.queue.push_back((entry.node.size_bytes, id.clone()));
        }
        // Smallest first, so a demo's screenshots land before its video does.
        let mut sorted: Vec<(u64, String)> = self.queue.drain(..).collect();
        sorted.sort_by_key(|(size, _)| *size);
        self.queue = sorted.into();
    }

    /// One batch of 32 chunks per heartbeat: a multi-gigabyte pull never blocks a click.
    async fn advance_download(&mut self) {
        if self.active.is_none() {
            self.start_next_download();
        }
        let Some(active) = self.active.as_ref() else {
            return;
        };
        let node_id = active.node_id.clone();
        let trusted = active.trusted.clone();
        let batch: Vec<ChunkId> = active
            .chunk_ids
            .iter()
            .skip(active.fetched)
            .take(PULL_BATCH)
            .copied()
            .collect();
        if batch.is_empty() {
            self.finish_download(&node_id);
            return;
        }
        let outcome = self.pull_one(&batch, &trusted).await;
        match outcome {
            Ok(()) => {
                if let Some(active) = self.active.as_mut() {
                    active.fetched += batch.len();
                    if active.fetched >= active.chunk_ids.len() {
                        self.finish_download(&node_id);
                    }
                }
            }
            Err(_) => {
                // A failed batch drops the transfer; the file goes back to `remote` and the
                // next heartbeat may retry it from the queue.
                self.active = None;
            }
        }
    }

    /// Requested files first, then the prefetch queue smallest-first.
    fn next_queued(&mut self) -> Option<String> {
        if let Some(node_id) = self.front.pop_front() {
            return Some(node_id);
        }
        self.queue.pop_front().map(|(_, node_id)| node_id)
    }

    fn start_next_download(&mut self) {
        while let Some(node_id) = self.next_queued() {
            let Some(entry) = self.entries.get(&node_id) else {
                continue;
            };
            let file_id = entry.file_id;
            let Some(replica) = self.replica.as_ref() else {
                return;
            };
            let trusted = { replica.borrow().trusted_manifest(&file_id).cloned() };
            let Some(trusted) = trusted else { continue };
            if self.is_local(&trusted) {
                continue;
            }
            self.active = Some(Download {
                node_id,
                chunk_ids: trusted.manifest().chunk_ids.clone(),
                trusted,
                fetched: 0,
            });
            return;
        }
    }

    fn finish_download(&mut self, node_id: &str) {
        self.active = None;
        let actor = self.profile.peer_id.clone();
        self.log(
            node_id,
            HistoryKind::Downloaded,
            &actor,
            None,
            None,
            "downloaded to this Mac".to_string(),
            None,
        );
    }

    /// Every chunk of one file, pulling whatever is missing first.
    async fn file_bodies(&mut self, node_id: &str) -> Result<Vec<Vec<u8>>, String> {
        let entry = self.require_node(node_id)?;
        let file_id = entry.file_id;
        let trusted = {
            let replica = self
                .replica
                .as_ref()
                .ok_or("Not connected to the vault server yet")?;
            replica
                .borrow()
                .trusted_manifest(&file_id)
                .cloned()
                .ok_or("That file has no contents yet")?
        };
        if !self.is_local(&trusted) {
            let ids = trusted.manifest().chunk_ids.clone();
            self.pull_batches(&ids, &trusted).await?;
        }
        let replica = self
            .replica
            .as_ref()
            .ok_or("Not connected to the vault server yet")?;
        let replica = replica.borrow();
        let store = replica.chunks();
        let store = store
            .lock()
            .map_err(|_| "The local chunk store is unavailable".to_string())?;
        let mut bodies = Vec::with_capacity(trusted.manifest().chunk_ids.len());
        for chunk in &trusted.manifest().chunk_ids {
            let body = store
                .get(chunk)
                .ok_or("That file's contents are not local yet")?;
            bodies.push(body.to_vec());
        }
        Ok(bodies)
    }

    /// Pull to completion, emitting per-batch progress. Used when the caller must wait —
    /// `open_node`, a duplicate, a sidecar refresh. The transfer is published as the active
    /// download for the length of the pull, so the projection says `downloading` with real
    /// progress instead of `remote`; any background transfer is put back afterwards.
    async fn pull_batches(
        &mut self,
        chunk_ids: &[ChunkId],
        trusted: &TrustedManifest,
    ) -> Result<(), String> {
        let batches: Vec<Vec<ChunkId>> = chunk_ids
            .chunks(PULL_BATCH)
            .map(|batch| batch.to_vec())
            .collect();
        let background = self.active.take();
        self.active = Some(Download {
            node_id: hex(&trusted.manifest().file_id.0),
            chunk_ids: chunk_ids.to_vec(),
            trusted: trusted.clone(),
            fetched: 0,
        });
        let mut fetched = 0usize;
        let mut outcome = Ok(());
        for batch in batches {
            if let Err(error) = self.pull_one(&batch, trusted).await {
                outcome = Err(error);
                break;
            }
            fetched += batch.len();
            if let Some(active) = self.active.as_mut() {
                active.fetched = fetched;
            }
            self.project();
        }
        self.active = background;
        outcome
    }

    /// One `have_query` + `pull` against H, exactly the sequence `vm_probe`'s `pull_verify` runs.
    /// H owns the full replica, so a live host session is always the best holder
    /// (`sync/locate.rs`: `Locator::holders` short-circuits on it).
    async fn pull_one(
        &mut self,
        batch: &[ChunkId],
        trusted: &TrustedManifest,
    ) -> Result<(), String> {
        if batch.is_empty() {
            return Ok(());
        }
        let file_id = trusted.manifest().file_id;
        let conn = self.live()?;
        let query = HaveQuery::new(file_id, batch.to_vec()).map_err(|e| friendly(&e))?;
        let reply = conn.have_query(&query).await.map_err(|e| friendly(&e))?;
        if !(0..batch.len()).any(|index| reply.has(index)) {
            return Err("No member online has those bytes yet".to_string());
        }
        let request = PullRequest::new(batch.to_vec()).map_err(|e| friendly(&e))?;
        let conn = self.live()?;
        conn.pull(&request, trusted)
            .await
            .map(|_| ())
            .map_err(|e| friendly(&e))
    }

    /// Make sure the bytes are here, assemble them under `open/`, and hand the copy to the OS.
    async fn open_node(&mut self, node_id: &str) -> Result<(), String> {
        let entry = self.require_node(node_id)?;
        if entry.node.kind != NodeKind::File {
            return Err("Can't put things inside a file".to_string());
        }
        let name = entry.node.name.clone();
        let file_id = entry.file_id;
        let bodies = self.file_bodies(node_id).await?;
        let version = {
            let replica = self
                .replica
                .as_ref()
                .ok_or("Not connected to the vault server yet")?;
            replica
                .borrow()
                .trusted_manifest(&file_id)
                .map(|trusted| trusted.manifest().version)
                .unwrap_or(0)
        };
        let dir = state::open_dir(&self.data_dir, &self.vault_hex).join(node_id);
        let target = dir.join(&name);
        // Concatenating and writing a multi-gigabyte file on the runtime thread would freeze
        // every vault that shares it, so the whole staging step goes to the blocking pool.
        let path = tokio::task::spawn_blocking(move || -> Result<PathBuf, String> {
            std::fs::create_dir_all(&dir)
                .map_err(|e| format!("Could not assemble the file: {e}"))?;
            let index = dir.join("version.json");
            let stale = std::fs::read(&index)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<u64>(&bytes).ok())
                != Some(version)
                || !target.exists();
            if stale {
                let mut assembled = Vec::new();
                for body in &bodies {
                    assembled.extend_from_slice(body);
                }
                std::fs::write(&target, &assembled)
                    .map_err(|e| format!("Could not assemble the file: {e}"))?;
                let _ = std::fs::write(&index, serde_json::to_vec(&version).unwrap_or_default());
            }
            Ok(target)
        })
        .await
        .map_err(|_| "Could not assemble the file".to_string())??;
        #[cfg(target_os = "macos")]
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("Could not open that file: {e}"))?;
        #[cfg(not(target_os = "macos"))]
        return Err("Opening files isn't supported on this platform yet".to_string());
        #[cfg(target_os = "macos")]
        {
            let _ = self.root_tx.send(Req::TouchRecent {
                vault_id: self.vault_hex.clone(),
                node_id: node_id.to_string(),
            });
            Ok(())
        }
    }

    async fn import_files(
        &mut self,
        parent_id: &str,
        paths: Vec<PathBuf>,
    ) -> Result<Vec<FsNode>, String> {
        let parent = self.require_folder(parent_id)?;
        let mut made = Vec::new();
        for path in paths {
            let desired = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Imported file")
                .to_string();
            if let Some(message) = name_error(&desired) {
                return Err(message.to_string());
            }
            let taken = self.child_names(parent_id, None);
            let name = unique_name(&taken, &desired);
            // Off the runtime thread: importing from a slow disk must not stop the heartbeat.
            let source = path.clone();
            let bodies: Vec<Vec<u8>> = tokio::task::spawn_blocking(move || {
                std::fs::read(&source)
                    .map(|bytes| {
                        bytes
                            .chunks(IMPORT_CHUNK_BYTES)
                            .map(|chunk| chunk.to_vec())
                            .collect::<Vec<Vec<u8>>>()
                    })
                    .map_err(|e| format!("Could not read {}: {e}", source.display()))
            })
            .await
            .map_err(|_| format!("Could not read {}", path.display()))??;
            let destination = child_path(&parent, &name);
            let file_id = {
                let conn = self.live()?;
                conn.save_file(&destination, &bodies)
                    .await
                    .map_err(|e| friendly(&e))?
            };
            let id = hex(&file_id.0);
            self.mine.insert(id.clone());
            self.settle().await;
            let actor = self.profile.peer_id.clone();
            self.log(
                &id,
                HistoryKind::Created,
                &actor,
                None,
                None,
                "imported from this Mac".to_string(),
                None,
            );
            made.push(id);
        }
        Ok(made
            .iter()
            .filter_map(|id| self.entries.get(id).map(|entry| entry.node.clone()))
            .collect())
    }

    fn read_text_preview(
        &self,
        node_id: &str,
        max_bytes: usize,
    ) -> Result<Option<String>, String> {
        let entry = self.require_node(node_id)?;
        if entry.node.kind != NodeKind::File || entry.node.availability != Availability::Local {
            return Ok(None);
        }
        let Some(bytes) = self.assemble(&entry.file_id) else {
            return Ok(None);
        };
        let sniff = bytes.len().min(TEXT_SNIFF_BYTES);
        if bytes[..sniff].contains(&0) {
            return Ok(None);
        }
        let cut = bytes.len().min(max_bytes);
        Ok(Some(String::from_utf8_lossy(&bytes[..cut]).to_string()))
    }

    /// Concatenate one file's local chunks in manifest order; `None` if any is missing.
    fn assemble(&self, file_id: &FileId) -> Option<Vec<u8>> {
        let replica = self.replica.as_ref()?;
        let replica = replica.borrow();
        let trusted = replica.trusted_manifest(file_id)?;
        let store = replica.chunks();
        let store = store.lock().ok()?;
        let mut out = Vec::new();
        for chunk in &trusted.manifest().chunk_ids {
            out.extend_from_slice(store.get(chunk)?);
        }
        Some(out)
    }

    /* ------------------------------------------------------------ members */

    fn members(&self) -> Vec<Member> {
        let host = self.replica.as_ref().map(|r| r.borrow().host_id());
        let mut peers: BTreeSet<PeerId> = self
            .replica
            .as_ref()
            .map(|r| r.borrow().members().clone())
            .unwrap_or_default();
        peers.insert(self.self_peer);
        if let Some(host) = host {
            peers.insert(host);
        }
        peers
            .iter()
            .map(|peer| {
                let peer_hex = hex(&peer.0);
                let is_self = *peer == self.self_peer;
                let is_host = host == Some(*peer);
                let cosmetics = self.sidecar.members.get(&peer_hex);
                let name = match cosmetics {
                    Some(entry) if !entry.name.is_empty() => entry.name.clone(),
                    _ if is_self => self.profile.name.clone(),
                    _ if is_host => "Vault server".to_string(),
                    _ => format!("Member {}", &peer_hex[..4.min(peer_hex.len())]),
                };
                let color = match cosmetics {
                    Some(entry) if !entry.color.is_empty() => entry.color.clone(),
                    _ if is_self => self.profile.color.clone(),
                    _ => color_for(&peer_hex),
                };
                let status = self
                    .snapshot
                    .members
                    .iter()
                    .find(|member| member.peer == peer_hex);
                let presented = if is_self {
                    self.profile.peer_id.clone()
                } else {
                    peer_hex.clone()
                };
                let role = if is_host || self.sidecar.created_by == presented {
                    MemberRole::Admin
                } else {
                    MemberRole::Member
                };
                Member {
                    peer_id: presented.clone(),
                    name: name.clone(),
                    initials: initials(&name),
                    color,
                    role,
                    online: is_self || is_host || status.is_some_and(|m| m.online),
                    last_seen_at: status.map(|m| m.last_seen_at).unwrap_or_else(now_ms),
                    last_edited: self.last_edited(&presented),
                    queued_ops: status.map(|m| m.queued_ops).unwrap_or(0),
                    is_self,
                }
            })
            .collect()
    }

    fn last_edited(&self, peer: &str) -> Option<LastEdited> {
        self.history
            .iter()
            .rev()
            .find(|event| event.by == peer)
            .map(|event| LastEdited {
                node_id: event.node_id.clone(),
                at: event.at,
            })
    }

    fn presence(&self) -> Vec<PeerPresence> {
        let now = now_ms();
        self.members()
            .into_iter()
            .filter(|member| member.online)
            .map(|member| {
                let own = if member.is_self {
                    self.own_presence.as_ref()
                } else {
                    None
                };
                PeerPresence {
                    peer_id: member.peer_id,
                    online: true,
                    idle: false,
                    folder_id: own.and_then(|state| state.folder_id.clone()),
                    cursor: own.and_then(|state| state.cursor.clone()),
                    hovering_node_id: own.and_then(|state| state.hovering_node_id.clone()),
                    dragging_node_ids: own
                        .map(|state| state.dragging_node_ids.clone())
                        .unwrap_or_default(),
                    updated_at: now,
                }
            })
            .collect()
    }

    /* ------------------------------------------------------------- vaults */

    fn vault_name(&self) -> String {
        if !self.sidecar.name.is_empty() {
            return self.sidecar.name.clone();
        }
        if !self.record.name.is_empty() {
            return self.record.name.clone();
        }
        "Vault".to_string()
    }

    /// What a human types, preferred over the 26-character form the protocol uses: the
    /// newest short code we have seen, and only the long code when there is no short one.
    fn display_join_code(&self) -> String {
        for text in [&self.snapshot.join_code, &self.record.join_code] {
            if let Some(short) = state::normalize_short(text) {
                return short;
            }
        }
        if !self.cache.short_code.is_empty() {
            return self.cache.short_code.clone();
        }
        if !self.snapshot.join_code.is_empty() && self.snapshot.join_code != "-" {
            return self.snapshot.join_code.clone();
        }
        self.record.join_code.clone()
    }

    fn vault_meta(&self) -> VaultMeta {
        let created_at = if self.sidecar.created_at > 0 {
            self.sidecar.created_at
        } else {
            self.snapshot.created_ms
        };
        let created_by = if !self.sidecar.created_by.is_empty() {
            self.sidecar.created_by.clone()
        } else {
            self.snapshot
                .creator
                .as_ref()
                .map(|hex| self.present_hex(hex))
                .unwrap_or_default()
        };
        VaultMeta {
            id: self.vault_hex.clone(),
            server_id: self.record.server_id.clone(),
            name: self.vault_name(),
            description: self.sidecar.description.clone(),
            join_code: self.display_join_code(),
            created_at,
            created_by,
            // Pair epochs roll forward with the vault's own life; the UI shows one date.
            key_rotated_at: created_at,
            auto_cleanup: self.sidecar.auto_cleanup,
            cleanup_threshold_pct: self.sidecar.cleanup_threshold_pct,
        }
    }

    async fn update_vault_meta(&mut self, patch: VaultMetaPatch) -> Result<VaultMeta, String> {
        if let Some(server_id) = &patch.server_id {
            if *server_id != self.record.server_id {
                return Err("Vaults cannot move between servers".to_string());
            }
        }
        let mut next = self.sidecar.clone();
        let mut renamed = false;
        if let Some(name) = patch.name {
            let name = name.trim().to_string();
            if name.is_empty() || name.chars().count() > 40 {
                return Err("Invalid vault name".to_string());
            }
            renamed = name != next.name;
            next.name = name;
        }
        if let Some(description) = patch.description {
            next.description = description;
        }
        if let Some(auto_cleanup) = patch.auto_cleanup {
            next.auto_cleanup = auto_cleanup;
        }
        if let Some(pct) = patch.cleanup_threshold_pct {
            next.cleanup_threshold_pct = pct.clamp(1, 100);
        }
        self.write_sidecar(next).await?;
        self.emit_vault_changed();
        if renamed {
            self.mine.insert(self.root_id.clone());
        }
        self.settle().await;
        let _ = self.root_tx.send(Req::VaultNameChanged {
            vault_id: self.vault_hex.clone(),
            name: self.vault_name(),
        });
        Ok(self.vault_meta())
    }

    fn ask_agent(&self, folder_id: &str) -> Result<AgentReply, String> {
        let entry = self.require_node(folder_id)?;
        let children: Vec<&Entry> = self
            .order
            .iter()
            .filter_map(|id| self.entries.get(id))
            .filter(|child| child.node.parent_id.as_deref() == Some(folder_id))
            .collect();
        let local = children
            .iter()
            .filter(|child| child.node.availability == Availability::Local)
            .count();
        let bytes: u64 = children.iter().map(|child| child.node.size_bytes).sum();
        Ok(AgentReply {
            id: format!("agent_{}", now_ms()),
            text: format!(
                "I can see {} items in \"{}\", {} of them local ({}).",
                children.len(),
                entry.node.name,
                local,
                human_bytes(bytes)
            ),
        })
    }

    /* ------------------------------------------------------------- lookup */

    fn require_node(&self, node_id: &str) -> Result<&Entry, String> {
        self.entries
            .get(node_id)
            .ok_or_else(|| format!("Unknown node: {node_id}"))
    }

    /// The path of a folder, rejecting a file destination the way the UI contract requires.
    fn require_folder(&self, node_id: &str) -> Result<String, String> {
        let entry = self.require_node(node_id)?;
        if entry.node.kind != NodeKind::Folder {
            return Err("Can't put things inside a file".to_string());
        }
        Ok(entry.path.clone())
    }

    fn node_or_missing(&self, node_id: &str) -> Result<FsNode, String> {
        self.entries
            .get(node_id)
            .map(|entry| entry.node.clone())
            .ok_or_else(|| "The vault server has not applied that change yet".to_string())
    }

    fn name_of(&self, node_id: &str) -> Option<String> {
        self.entries
            .get(node_id)
            .map(|entry| entry.node.name.clone())
            .or_else(|| self.shapes.get(node_id).map(|shape| shape.name.clone()))
    }

    fn child_names(&self, parent_id: &str, exclude: Option<&str>) -> Vec<String> {
        self.order
            .iter()
            .filter_map(|id| self.entries.get(id))
            .filter(|entry| entry.node.parent_id.as_deref() == Some(parent_id))
            .filter(|entry| Some(entry.node.id.as_str()) != exclude)
            .map(|entry| entry.node.name.clone())
            .collect()
    }

    /// The UI compares names case-insensitively; the backend tree does not, so check here.
    fn check_name_free(
        &self,
        parent_id: &str,
        name: &str,
        exclude: Option<&str>,
    ) -> Result<(), String> {
        if let Some(message) = name_error(name) {
            return Err(message.to_string());
        }
        let lower = name.to_lowercase();
        for id in self.order.iter() {
            let Some(entry) = self.entries.get(id) else {
                continue;
            };
            if entry.node.parent_id.as_deref() != Some(parent_id) {
                continue;
            }
            if Some(entry.node.id.as_str()) == exclude {
                continue;
            }
            if entry.node.name.to_lowercase() == lower {
                return Err(collision_message(entry.node.kind));
            }
        }
        Ok(())
    }

    fn subtree(&self, root: &str) -> Vec<String> {
        let mut out = vec![root.to_string()];
        let mut index = 0;
        while index < out.len() {
            let current = out[index].clone();
            for id in &self.order {
                if self
                    .entries
                    .get(id)
                    .is_some_and(|entry| entry.node.parent_id.as_deref() == Some(current.as_str()))
                {
                    out.push(id.clone());
                }
            }
            index += 1;
        }
        out
    }

    fn is_descendant(&self, id: &str, ancestor: &str) -> bool {
        let mut cursor = self
            .entries
            .get(id)
            .and_then(|entry| entry.node.parent_id.clone());
        let mut guard = 0;
        while let Some(current) = cursor {
            if current == ancestor {
                return true;
            }
            guard += 1;
            if guard > 4096 {
                break;
            }
            cursor = self
                .entries
                .get(&current)
                .and_then(|entry| entry.node.parent_id.clone());
        }
        false
    }

    fn live(&mut self) -> Result<&mut JoinedPeer, String> {
        self.conn
            .as_mut()
            .ok_or_else(|| "Not connected to the vault server yet".to_string())
    }

    /// One heartbeat plus a projection, so a command's reply and its `fs-changed` agree.
    async fn settle(&mut self) {
        let mut failure = None;
        if let Some(conn) = self.conn.as_mut() {
            if let Err(error) = conn.heartbeat_once().await {
                failure = Some(error);
            }
        }
        if let Some(error) = failure {
            self.drop_session(&error);
        }
        self.project();
    }

    /* -------------------------------------------------------------- peers */

    /// Our own per-vault identity is always presented as the one canonical profile id.
    fn present(&self, peer: &PeerId) -> String {
        if *peer == self.self_peer {
            self.profile.peer_id.clone()
        } else {
            hex(&peer.0)
        }
    }

    fn present_hex(&self, peer_hex: &str) -> String {
        match unhex32(peer_hex) {
            Some(bytes) => self.present(&PeerId(bytes)),
            None => peer_hex.to_string(),
        }
    }

    /* ------------------------------------------------------------- events */

    fn emit<T: Serialize>(&self, name: &str, payload: &T) {
        let value = serde_json::to_value(payload).unwrap_or(Value::Null);
        (self.emit)(name, value);
    }

    fn emit_members(&self) {
        self.emit(
            "backend://members-changed",
            &VaultIdPayload {
                vault_id: self.vault_hex.clone(),
            },
        );
    }

    fn emit_vault_changed(&self) {
        self.emit(
            "backend://vault-changed",
            &VaultIdPayload {
                vault_id: self.vault_hex.clone(),
            },
        );
    }

    fn emit_presence(&self, peers: Vec<PeerPresence>) {
        self.emit(
            "backend://presence",
            &crate::fs_types::PresencePayload {
                vault_id: self.vault_hex.clone(),
                peers,
            },
        );
    }
}

/* ------------------------------------------------------------- free helpers */

/// One node as the walk found it, before availability and timestamps are attached.
struct Raw {
    file_id: FileId,
    id: String,
    parent: Option<String>,
    name: String,
    is_dir: bool,
    version: u64,
    size: u64,
    writer: Option<PeerId>,
    path: String,
}

fn child_path(parent: &str, name: &str) -> String {
    if parent == "/" {
        format!("/{name}")
    } else {
        format!("{}/{}", parent.trim_end_matches('/'), name)
    }
}

fn collision_message(kind: NodeKind) -> String {
    match kind {
        NodeKind::File => "A file with that name already exists".to_string(),
        NodeKind::Folder => "A folder with that name already exists".to_string(),
    }
}

/// Field-by-field, because `FsNode` is a wire shape and deriving `PartialEq` on it would
/// invite comparing nodes elsewhere by value.
fn same_node(a: &FsNode, b: &FsNode) -> bool {
    a.parent_id == b.parent_id
        && a.kind == b.kind
        && a.name == b.name
        && a.size_bytes == b.size_bytes
        && a.created_at == b.created_at
        && a.modified_at == b.modified_at
        && a.created_by == b.created_by
        && a.modified_by == b.modified_by
        && a.color == b.color
        && a.availability == b.availability
        && a.progress == b.progress
        && a.holders == b.holders
        && a.child_count == b.child_count
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
