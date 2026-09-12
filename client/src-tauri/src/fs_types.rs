//! Wire shapes for the vault workspace, mirroring `client/src/lib/backend/types.ts`.
//!
//! WHY this file exists: the webview trusts exactly one seam (docs/decisions/client-workspace.md),
//! so the Tauri build must hand the UI byte-identical JSON to what the browser mock produces.
//! Every struct here serializes `camelCase` and every enum `lowercase`, which makes the TypeScript
//! declarations in `types.ts` the literal contract for this module — change one, change both.
//! These types are also the seed format: `src/lib/backend/seed/fs.json` is generated once by
//! `client/scripts/seed-fs.mjs` and read by both runtimes, so the two builds can never drift.

//! The vocabulary is shared with the node runtime (`src/node/`), so a type or a field that no
//! command happens to read is still part of the contract, not dead weight.
#![allow(dead_code)]

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/* ------------------------------------------------------------------ enums */

/// What a node is: a folder that holds children, or a leaf file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Folder,
    File,
}

/// The folder tints a member can choose; `Graphite` is the default look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FolderColor {
    Graphite,
    Coral,
    Violet,
    Blue,
    Teal,
    Green,
    Amber,
    Pink,
    Red,
}

impl FolderColor {
    /// The exact token the UI and the history summaries use.
    pub fn as_str(self) -> &'static str {
        match self {
            FolderColor::Graphite => "graphite",
            FolderColor::Coral => "coral",
            FolderColor::Violet => "violet",
            FolderColor::Blue => "blue",
            FolderColor::Teal => "teal",
            FolderColor::Green => "green",
            FolderColor::Amber => "amber",
            FolderColor::Pink => "pink",
            FolderColor::Red => "red",
        }
    }
}

/// Files may live only on other peers; folders are always `Local` because the tree is replicated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Availability {
    Local,
    Remote,
    Downloading,
}

/// Vault-wide standing: admins manage membership, the join code and vault settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemberRole {
    Admin,
    Member,
}

/// Per-node permission a member holds on a subtree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccessLevel {
    Viewer,
    Editor,
}

/// What happened to a node, for the History surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryKind {
    Created,
    Renamed,
    Moved,
    Modified,
    Duplicated,
    Colored,
    Access,
    Deleted,
    Downloaded,
}

/* ---------------------------------------------------- servers and vaults */

/// The caller's standing in a vault, as the home screen draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Owner,
    Member,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Vault {
    pub id: String,
    pub server_id: String,
    pub name: String,
    pub member_count: u32,
    /// Bytes used by the vault's files. Always <= `quota_bytes`.
    pub used_bytes: u64,
    /// Bytes of the server's capacity allocated to this vault.
    pub quota_bytes: u64,
    pub role: Role,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    pub id: String,
    pub name: String,
    pub address: String,
    pub peer_id: String,
    pub online: bool,
    /// Total provisionable storage on the server; the sum of vault quotas cannot exceed it.
    pub capacity_bytes: u64,
    pub vaults: Vec<Vault>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonStatus {
    pub running: bool,
    pub version: Option<String>,
    pub peer_id: Option<String>,
    pub data_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddServerInput {
    pub name: String,
    pub address: String,
}

/// Arguments for creating a vault: a name plus the slice of server capacity it gets.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateVaultInput {
    pub server_id: String,
    pub name: String,
    pub quota_bytes: u64,
}

/* ------------------------------------------------------------------ nodes */

/// One entry of a vault's replicated file tree. Folders and files share this shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsNode {
    pub id: String,
    pub vault_id: String,
    /// `None` only for the vault root folder (id `root_<vaultId>`, name = vault name).
    pub parent_id: Option<String>,
    pub kind: NodeKind,
    pub name: String,
    /// Files: byte size. Folders: recursive total of their files.
    pub size_bytes: u64,
    pub created_at: u64,
    pub modified_at: u64,
    pub created_by: String,
    pub modified_by: String,
    /// Folders only; `None` = default graphite. Always `None` for files.
    pub color: Option<FolderColor>,
    pub availability: Availability,
    /// 0..1 while `availability == Downloading`, else `None`.
    pub progress: Option<f64>,
    /// Peers holding a full copy (files). Folders: empty.
    pub holders: Vec<String>,
    /// Folders: number of direct children. Files: 0.
    pub child_count: u32,
}

/* ---------------------------------------------------------------- members */

/// The node a member touched last, shown on their contact card.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastEdited {
    pub node_id: String,
    pub at: u64,
}

/// A person on a vault's member list, as the workspace needs to draw them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub peer_id: String,
    pub name: String,
    pub initials: String,
    /// Presence/cursor color as `#RRGGBB`.
    pub color: String,
    pub role: MemberRole,
    pub online: bool,
    pub last_seen_at: u64,
    pub last_edited: Option<LastEdited>,
    /// Ops waiting to sync while offline.
    pub queued_ops: u32,
    pub is_self: bool,
}

/* ----------------------------------------------------------------- access */

/// One member's permission inside a node's access list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessEntry {
    pub peer_id: String,
    pub level: AccessLevel,
}

/// The effective permission list for one node, and whether it is its own or inherited.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeAccess {
    pub node_id: String,
    /// `true` = no explicit entries here; `entries` is the list inherited from the nearest
    /// ancestor that has explicit entries (or empty = vault default: every member is an editor).
    pub inherit: bool,
    pub entries: Vec<AccessEntry>,
}

/* ---------------------------------------------------------------- history */

/// One append-only entry in a node's history.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEvent {
    pub id: String,
    pub vault_id: String,
    pub node_id: String,
    pub kind: HistoryKind,
    pub at: u64,
    pub by: String,
    /// e.g. old name / old parent name; `None` when not applicable.
    pub from: Option<String>,
    pub to: Option<String>,
    /// Human sentence without the actor, e.g. "renamed to poster-v2.hdr".
    pub summary: String,
}

/* ---------------------------------------------------------------- recents */

/// A recently touched node, denormalized with its vault name for the sidebar list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recent {
    pub node: FsNode,
    pub vault_name: String,
    pub at: u64,
}

/* ----------------------------------------------------------------- vaults */

/// Everything the vault settings modal reads and writes about one vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultMeta {
    pub id: String,
    pub server_id: String,
    pub name: String,
    pub description: String,
    pub join_code: String,
    pub created_at: u64,
    pub created_by: String,
    /// Keys rotate weekly (docs/VISION.md).
    pub key_rotated_at: u64,
    pub auto_cleanup: bool,
    /// Percent of local disk use at which auto-cleanup runs.
    pub cleanup_threshold_pct: u32,
}

/* --------------------------------------------------------------- presence */

/// Where another member's pointer is, expressed so it survives different window sizes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerCursor {
    /// `anchor == None`: fractions (0..1) of the canvas viewport.
    /// `anchor != None`: px offsets from the center of that node's tile.
    pub x: f64,
    pub y: f64,
    pub anchor: Option<String>,
    /// How long the peer's move to this target should take (ms); 0 = the follower's default pace.
    pub glide_ms: u32,
}

/// Live state of one peer in a vault: where they are and what they are touching.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerPresence {
    pub peer_id: String,
    pub online: bool,
    pub idle: bool,
    pub folder_id: Option<String>,
    pub cursor: Option<PeerCursor>,
    pub hovering_node_id: Option<String>,
    pub dragging_node_ids: Vec<String>,
    pub updated_at: u64,
}

/* ------------------------------------------------------------------ agent */

/// Answer from the agent bar at the bottom of the canvas.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentReply {
    pub id: String,
    pub text: String,
}

/* ---------------------------------------------------------------- changes */

/// One delta of an `fs-changed` event: a node to insert/replace, or a node to drop.
///
/// Internally tagged so the TypeScript union `{ kind: "upsert"; node } | { kind: "remove"; nodeId }`
/// deserializes with no adapter on the webview side.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FsChange {
    Upsert {
        node: FsNode,
    },
    Remove {
        #[serde(rename = "nodeId")]
        node_id: String,
    },
}

/* ----------------------------------------------------------------- inputs */

/// Create an empty folder or file directly under `parent_id`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateNodeInput {
    pub vault_id: String,
    pub parent_id: String,
    pub kind: NodeKind,
    pub name: String,
    /// Demo only: perform the op as another member. A real daemon ignores it.
    #[serde(default)]
    pub actor: Option<String>,
}

/// Rename one node in place; its parent and children are untouched.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameNodeInput {
    pub vault_id: String,
    pub node_id: String,
    pub name: String,
    #[serde(default)]
    pub actor: Option<String>,
}

/// Reparent a selection into one destination folder.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveNodesInput {
    pub vault_id: String,
    pub node_ids: Vec<String>,
    pub to_parent_id: String,
    #[serde(default)]
    pub actor: Option<String>,
}

/// Delete a selection and, for folders, everything beneath it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteNodesInput {
    pub vault_id: String,
    pub node_ids: Vec<String>,
    #[serde(default)]
    pub actor: Option<String>,
}

/// Copy a selection, subtrees included, with Finder-style naming.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateNodesInput {
    pub vault_id: String,
    pub node_ids: Vec<String>,
    /// `None` = beside the originals.
    #[serde(default)]
    pub to_parent_id: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
}

/// Tint one folder, or clear the tint back to the default graphite.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetNodeColorInput {
    pub vault_id: String,
    pub node_id: String,
    #[serde(default)]
    pub color: Option<FolderColor>,
    #[serde(default)]
    pub actor: Option<String>,
}

/// Replace one node's access list, or hand it back to inheritance.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAccessInput {
    pub vault_id: String,
    pub node_id: String,
    pub inherit: bool,
    pub entries: Vec<AccessEntry>,
}

/// This client's own live state, broadcast to the other members of a vault.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceInput {
    pub vault_id: String,
    pub folder_id: Option<String>,
    pub cursor: Option<PeerCursor>,
    pub hovering_node_id: Option<String>,
    pub dragging_node_ids: Vec<String>,
}

/// Partial update of a vault's settings; omitted fields keep their current value.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultMetaPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub server_id: Option<String>,
    #[serde(default)]
    pub auto_cleanup: Option<bool>,
    #[serde(default)]
    pub cleanup_threshold_pct: Option<u32>,
}

/// One question for the agent bar, scoped to the folder the user is looking at.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskAgentInput {
    /// Part of the wire contract and of every real implementation; the canned reply
    /// answers from the folder alone, so neither field is read here yet.
    #[allow(dead_code)]
    pub vault_id: String,
    pub folder_id: String,
    #[allow(dead_code)]
    pub prompt: String,
}

/// Import OS files into a vault folder. `paths` absent = ask the OS for a file selection.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportFilesInput {
    pub vault_id: String,
    pub parent_id: String,
    #[serde(default)]
    pub paths: Option<Vec<String>>,
}

/* --------------------------------------------------------- event payloads */

/// Body of `backend://fs-changed`: the deltas one mutation produced, and who caused them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsChangedPayload {
    pub vault_id: String,
    pub changes: Vec<FsChange>,
    pub actor: String,
}

/// Body of `backend://members-changed` and `backend://vault-changed`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultIdPayload {
    pub vault_id: String,
}

/// Body of `backend://presence`: the full peer list of one vault after a publish.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresencePayload {
    pub vault_id: String,
    pub peers: Vec<PeerPresence>,
}

/// Body of `backend://vault-removed`: a vault the client no longer has, and why.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultRemovedPayload {
    pub vault_id: String,
    pub reason: String,
}

/* ------------------------------------------------------------------- seed */

/// One entry of the seed's recents list; the node itself is looked up at read time.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeedRecent {
    pub vault_id: String,
    pub node_id: String,
    pub at: u64,
}

/// `src/lib/backend/seed/fs.json` — the single artifact both runtimes boot from.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsSeed {
    /// Fixed clock the generator measured every timestamp back from.
    #[allow(dead_code)]
    pub generated_at: u64,
    #[serde(rename = "self")]
    pub self_: String,
    pub vaults: HashMap<String, VaultMeta>,
    pub members: HashMap<String, Vec<Member>>,
    pub nodes: Vec<FsNode>,
    pub access: Vec<NodeAccess>,
    pub history: Vec<HistoryEvent>,
    pub recents: Vec<SeedRecent>,
    pub previews: HashMap<String, String>,
}
