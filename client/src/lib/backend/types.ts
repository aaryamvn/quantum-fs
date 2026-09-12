/**
 * Domain types for the daemon bridge.
 *
 * These mirror `client/src-tauri/src/bridge.rs` and `client/src-tauri/src/fs_types.rs`
 * exactly; the Rust side serializes with `#[serde(rename_all = "camelCase")]` so the
 * wire shape matches these names.
 * Terminology follows docs/decisions/net-vault-join-directory.md.
 */

/** Cryptographic member identity (crypto-identity-selfcert). Never a MAC or email. */
export type PeerId = string;

/** Identifier of an orchestration server (the designated host H of a set of vaults). */
export type ServerId = string;

/** Identifier of a vault (one shared file system with a member list). */
export type VaultId = string;

/** High-entropy base32 code that the central directory maps to (server, vault). */
export type JoinCode = string;

export interface Vault {
  id: VaultId;
  serverId: ServerId;
  name: string;
  memberCount: number;
  /** Bytes used by the vault's files. Always <= `quotaBytes`. */
  usedBytes: number;
  /** Bytes of the server's capacity allocated to this vault. */
  quotaBytes: number;
  /**
   * What the vault can actually hold: `quotaBytes` plus every member's storage
   * contribution. The host's slice is the floor; each member who joins raises it
   * by what they pledge from their own disk.
   */
  capacityBytes: number;
  role: "owner" | "member";
}

export interface OrchestrationServer {
  id: ServerId;
  name: string;
  address: string;
  peerId: PeerId;
  online: boolean;
  /** Total provisionable storage on the server; the sum of vault quotas cannot exceed it. */
  capacityBytes: number;
  vaults: Vault[];
}

/** Arguments for creating a vault: a name plus the slice of server capacity it gets. */
export interface CreateVaultInput {
  serverId: ServerId;
  name: string;
  quotaBytes: number;
}

export interface DaemonStatus {
  running: boolean;
  version: string | null;
  peerId: PeerId | null;
  dataDir: string | null;
}

/** Identifier of one node (folder or file) inside a vault's replicated tree. */
export type NodeId = string;

/** What a node is: a folder that holds children, or a leaf file. */
export type NodeKind = "folder" | "file";

/** The folder tints a member can choose; the first entry is the default look. */
export const FOLDER_COLORS = [
  "graphite",
  "coral",
  "violet",
  "blue",
  "teal",
  "green",
  "amber",
  "pink",
  "red",
] as const;

/** One of {@link FOLDER_COLORS}. */
export type FolderColor = (typeof FOLDER_COLORS)[number];

/** Files may live only on other peers; folders are always "local" because the tree is replicated to every member. */
export type Availability = "local" | "remote" | "downloading";

/** Vault-wide standing: admins manage membership, the join code and vault settings. */
export type MemberRole = "admin" | "member";

/** Per-node permission a member holds on a subtree: read-only, or allowed to mutate. */
export type AccessLevel = "viewer" | "editor";

/** One entry of a vault's replicated file tree. Folders and files share this shape. */
export interface FsNode {
  id: NodeId;
  vaultId: VaultId;
  /** null only for the vault root folder (id "root_<vaultId>", name = vault name). */
  parentId: NodeId | null;
  kind: NodeKind;
  name: string;
  /** Files: byte size. Folders: recursive total of their files. */
  sizeBytes: number;
  createdAt: number; // epoch ms
  modifiedAt: number; // epoch ms
  createdBy: PeerId;
  modifiedBy: PeerId;
  /** Folders only; null = default graphite. Always null for files. */
  color: FolderColor | null;
  availability: Availability;
  /** 0..1 while availability === "downloading", else null. */
  progress: number | null;
  /** Peers holding a full copy (files). Folders: []. */
  holders: PeerId[];
  /** Folders: number of direct children. Files: 0. */
  childCount: number;
}

/** A person on a vault's member list, as the workspace needs to draw them. */
export interface Member {
  peerId: PeerId;
  /** The member's client install (one person may run several). Stable per machine. */
  clientId: string;
  name: string;
  initials: string;
  /** Presence/cursor color as #RRGGBB. */
  color: string;
  role: MemberRole;
  online: boolean;
  lastSeenAt: number;
  lastEdited: { nodeId: NodeId; at: number } | null;
  /** Ops waiting to sync while offline. */
  queuedOps: number;
  /** Bytes of their own disk this member pledges to the vaults they belong to. */
  contributionBytes: number;
  isSelf: boolean;
}

/**
 * The local person, as they are stored on this machine.
 *
 * Lives in the daemon (the browser mock keeps it in memory), never in
 * `localStorage`: it is identity, and identity is the backend's to own.
 * `nameSet` is what the first launch turns on — false means the app has never
 * been told who this is, and the onboarding screen asks.
 */
export interface Profile {
  clientId: string;
  name: string;
  /** Presence/cursor color as #RRGGBB, same palette as {@link Member.color}. */
  color: string;
  /** false until the person has answered the onboarding question once. */
  nameSet: boolean;
  /** Bytes of local disk pledged to every vault this client joins. */
  contributionBytes: number;
}

/** Partial update of the local profile; omitted fields keep their current value. */
export interface ProfilePatch {
  name?: string;
  contributionBytes?: number;
}

/** One member's permission inside a node's access list. */
export interface AccessEntry {
  peerId: PeerId;
  level: AccessLevel;
}

/** The effective permission list for one node, and whether it is its own or inherited. */
export interface NodeAccess {
  nodeId: NodeId;
  /** true = no explicit entries here; `entries` is the effective list inherited from the nearest ancestor that has explicit entries (or empty = vault default: every member is an editor). */
  inherit: boolean;
  entries: AccessEntry[];
}

/** What happened to a node, for the History surface. */
export type HistoryKind =
  | "created"
  | "renamed"
  | "moved"
  | "modified"
  | "duplicated"
  | "colored"
  | "access"
  | "deleted"
  | "downloaded";

/** One append-only entry in a node's history. */
export interface HistoryEvent {
  id: string;
  vaultId: VaultId;
  nodeId: NodeId;
  kind: HistoryKind;
  at: number;
  by: PeerId;
  /** e.g. old name / old parent name; null when not applicable. */
  from: string | null;
  to: string | null;
  /** Human sentence without the actor, e.g. "renamed to poster-v2.hdr". */
  summary: string;
}

/** A recently touched node, denormalized with its vault name for the sidebar list. */
export interface Recent {
  node: FsNode;
  vaultName: string;
  at: number;
}

/** Everything the vault settings modal reads and writes about one vault. */
export interface VaultMeta {
  id: VaultId;
  serverId: ServerId;
  name: string;
  description: string;
  joinCode: JoinCode;
  createdAt: number;
  createdBy: PeerId;
  /** Keys rotate weekly (docs/VISION.md). */
  keyRotatedAt: number;
  autoCleanup: boolean;
  /** Percent of local disk use at which auto-cleanup runs. */
  cleanupThresholdPct: number;
}

/**
 * Where another member's pointer is, expressed so it survives different window sizes.
 *
 * Real peers never have one: the embedded daemon's presence only carries online
 * state (docs/decisions/client-backend-embed.md — live cursors through the host are
 * forbidden by `sync-host-tcb.md`), so `PeerPresence.cursor` is `null` for every real
 * peer and only the scripted demo ever fills it in.
 */
export interface PeerCursor {
  /** anchor === null: x,y are fractions (0..1) of the canvas viewport. anchor !== null: x,y are px offsets from the center of that node's tile. */
  x: number;
  y: number;
  anchor: NodeId | null;
  /** How long the peer's move to this target should take (ms); 0 = arrive at the follower's default pace. */
  glideMs: number;
}

/**
 * Live state of one peer in a vault: where they are and what they are touching.
 *
 * Against the real backend only `peerId`/`online`/`updatedAt` are meaningful —
 * presence comes from the host's peer list, which knows nothing about the webview's
 * navigation. `folderId`, `cursor` and `hoveringNodeId` are then `null` and
 * `draggingNodeIds` empty; the mock and the scripted demo are the only producers
 * that populate them.
 */
export interface PeerPresence {
  peerId: PeerId;
  online: boolean;
  idle: boolean;
  /** null for real peers (the daemon reports presence, not navigation). */
  folderId: NodeId | null;
  /** null for real peers; see {@link PeerCursor}. */
  cursor: PeerCursor | null;
  hoveringNodeId: NodeId | null;
  draggingNodeIds: NodeId[];
  updatedAt: number;
}

/** One delta of a `fs-changed` event: a node to insert/replace, or a node to drop. */
export type FsChange = { kind: "upsert"; node: FsNode } | { kind: "remove"; nodeId: NodeId };

/** A change performed by another member that the UI should animate before the tree mutates. */
export interface RemoteOp {
  id: string;
  actor: PeerId;
  vaultId: VaultId;
  kind: "move" | "pulse";
  nodeIds: NodeId[];
  /** move: destination folder. pulse: null. */
  toFolderId: NodeId | null;
  /** move: how long the flight takes before the matching fs-changed arrives. pulse: 0. */
  flightMs: number;
}

/** A fully specified search request; every filter is nullable so "no filter" is explicit. */
export interface SearchQuery {
  text: string;
  /** null = every vault the user belongs to. */
  vaultId: VaultId | null;
  kinds: NodeKind[] | null;
  exts: string[] | null;
  inFolderId: NodeId | null;
  by: PeerId | null;
  availability: Availability | null;
  modifiedAfter: number | null;
  limit: number;
}

/** One search result: the node, where it lives, and what matched in its name. */
export interface SearchHit {
  node: FsNode;
  vaultName: string;
  /** Names from the vault root (exclusive) to the parent (inclusive). */
  path: string[];
  score: number;
  /** [start, end) ranges into node.name that matched. */
  matches: [number, number][];
}

/** Answer from the agent bar at the bottom of the canvas. */
export interface AgentReply {
  id: string;
  text: string;
}

/** Push notifications from the Rust side (or the mock) to the webview. */
export type BackendEvent =
  | { type: "servers-changed" }
  | { type: "daemon-status"; status: DaemonStatus }
  | { type: "fs-changed"; vaultId: VaultId; changes: FsChange[]; actor: PeerId }
  | { type: "remote-op"; op: RemoteOp }
  | { type: "presence"; vaultId: VaultId; peers: PeerPresence[] }
  | { type: "members-changed"; vaultId: VaultId }
  | { type: "vault-changed"; vaultId: VaultId }
  | { type: "recents-changed" }
  /** The local profile changed (name or contribution); payload is the whole profile. */
  | { type: "profile-changed"; profile: Profile }
  /**
   * A vault this client belonged to is gone: the owner deleted it, the host kicked
   * this member, or the membership failed to re-establish. `reason` is a human
   * sentence for the toast; the vault has already been dropped from `listServers`.
   */
  | { type: "vault-removed"; vaultId: VaultId; reason: string }
  | { type: "demo-reset"; vaultId: VaultId };
