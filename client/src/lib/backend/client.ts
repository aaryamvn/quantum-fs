import type {
  AccessEntry,
  AgentReply,
  BackendEvent,
  CreateVaultInput,
  DaemonStatus,
  FolderColor,
  FsNode,
  HistoryEvent,
  JoinCode,
  Member,
  MemberRole,
  NodeAccess,
  NodeId,
  NodeKind,
  OrchestrationServer,
  PeerCursor,
  PeerId,
  PeerPresence,
  Recent,
  SearchHit,
  SearchQuery,
  ServerId,
  Vault,
  VaultId,
  VaultMeta,
} from "./types";

/**
 * Mock/demo escape hatch shared by every mutation input.
 *
 * The scripted multiplayer demo (docs/decisions/client-workspace.md) drives the same
 * public methods as the UI, so it needs a way to say "Justin did this".
 */
export interface ActorInput {
  /** Mock/demo only: perform the op as another member. The real daemon ignores it. */
  actor?: PeerId;
}

/** Create an empty folder or file directly under `parentId`. */
export interface CreateNodeInput extends ActorInput {
  vaultId: VaultId;
  parentId: NodeId;
  kind: NodeKind;
  name: string;
}

/** Rename one node in place; its parent and children are untouched. */
export interface RenameNodeInput extends ActorInput {
  vaultId: VaultId;
  nodeId: NodeId;
  name: string;
}

/** Reparent a selection into one destination folder. */
export interface MoveNodesInput extends ActorInput {
  vaultId: VaultId;
  nodeIds: NodeId[];
  toParentId: NodeId;
}

/** Delete a selection and, for folders, everything beneath it. */
export interface DeleteNodesInput extends ActorInput {
  vaultId: VaultId;
  nodeIds: NodeId[];
}

/** Copy a selection, subtrees included, with Finder-style naming. */
export interface DuplicateNodesInput extends ActorInput {
  vaultId: VaultId;
  nodeIds: NodeId[];
  /** null = beside the originals */
  toParentId: NodeId | null;
}

/** Tint one folder, or clear the tint back to the default graphite. */
export interface SetNodeColorInput extends ActorInput {
  vaultId: VaultId;
  nodeId: NodeId;
  color: FolderColor | null;
}

/** Replace one node's access list, or hand it back to inheritance. */
export interface SetAccessInput {
  vaultId: VaultId;
  nodeId: NodeId;
  inherit: boolean;
  entries: AccessEntry[];
}

/** This client's own live state, broadcast to the other members of a vault. */
export interface PresenceInput {
  vaultId: VaultId;
  folderId: NodeId | null;
  cursor: PeerCursor | null;
  hoveringNodeId: NodeId | null;
  draggingNodeIds: NodeId[];
}

/** Partial update of a vault's settings; omitted fields keep their current value. */
export interface VaultMetaPatch {
  name?: string;
  description?: string;
  serverId?: ServerId;
  autoCleanup?: boolean;
  cleanupThresholdPct?: number;
}

/** One question for the agent bar, scoped to the folder the user is looking at. */
export interface AskAgentInput {
  vaultId: VaultId;
  folderId: NodeId;
  prompt: string;
}

/**
 * The single seam between the webview and the local `qfsd` daemon.
 *
 * The webview never speaks to the daemon directly (docs/decisions/client-stack.md):
 * inside Tauri this is implemented by Rust commands that own the daemon socket,
 * outside Tauri by an in-memory mock with seeded data.
 *
 * Rules every implementation must honor, so the UI can trust one behavior
 * (docs/decisions/client-workspace.md):
 *
 * - Names are unique per parent, compared case-insensitively. An op that would collide
 *   rejects with `Error("A file with that name already exists")` or
 *   `Error("A folder with that name already exists")`.
 * - Moving a folder into itself or into one of its descendants rejects with
 *   `Error("Can't move a folder into itself")`.
 * - Duplicates get Finder naming — `"name copy"`, then `"name copy 2"`, `"name copy 3"` —
 *   with the extension preserved (`poster.hdr` -> `poster copy.hdr`).
 * - Every mutation ALSO emits an `fs-changed` event: an `upsert` for every touched node,
 *   including ancestors whose `sizeBytes` / `childCount` / `modifiedAt` changed, and a
 *   `remove` for every node of a deleted subtree. Mutations also append `HistoryEvent`s.
 * - `requestDownload` flips `availability` from `"remote"` to `"downloading"` (with
 *   `progress` ticking 0..1) and finally to `"local"`, emitting upserts as it goes.
 * - `touchRecent` keeps at most 8 recents, newest first, and emits `recents-changed`.
 */
export interface BackendClient {
  status(): Promise<DaemonStatus>;
  listServers(): Promise<OrchestrationServer[]>;
  addServer(input: { name: string; address: string }): Promise<OrchestrationServer>;
  createVault(input: CreateVaultInput): Promise<Vault>;
  joinVault(code: JoinCode): Promise<Vault>;
  /** Returns an unsubscribe function. */
  subscribe(listener: (e: BackendEvent) => void): () => void;

  /** The local user, as a `Member` (so avatars and presence share one shape). */
  me(): Promise<Member>;
  /** Every node of a vault, root included; the tree is small because it is replicated. */
  listTree(vaultId: VaultId): Promise<FsNode[]>;
  createNode(input: CreateNodeInput): Promise<FsNode>;
  renameNode(input: RenameNodeInput): Promise<FsNode>;
  moveNodes(input: MoveNodesInput): Promise<FsNode[]>;
  deleteNodes(input: DeleteNodesInput): Promise<void>;
  duplicateNodes(input: DuplicateNodesInput): Promise<FsNode[]>;
  setNodeColor(input: SetNodeColorInput): Promise<FsNode>;
  /** Starts a fetch of a `"remote"` file; resolves once the transfer has been queued. */
  requestDownload(vaultId: VaultId, nodeId: NodeId): Promise<void>;
  /** First `maxBytes` of a text-ish file for the inspector; null when not previewable. */
  readTextPreview(vaultId: VaultId, nodeId: NodeId, maxBytes: number): Promise<string | null>;
  getAccess(vaultId: VaultId, nodeId: NodeId): Promise<NodeAccess>;
  setAccess(input: SetAccessInput): Promise<NodeAccess>;
  /** Newest first. */
  getHistory(vaultId: VaultId, nodeId: NodeId): Promise<HistoryEvent[]>;
  listRecents(): Promise<Recent[]>;
  touchRecent(vaultId: VaultId, nodeId: NodeId): Promise<void>;
  search(query: SearchQuery): Promise<SearchHit[]>;
  getVaultMeta(vaultId: VaultId): Promise<VaultMeta>;
  updateVaultMeta(vaultId: VaultId, patch: VaultMetaPatch): Promise<VaultMeta>;
  /** Mints a fresh code and invalidates the old one at once. */
  rotateJoinCode(vaultId: VaultId): Promise<JoinCode>;
  listMembers(vaultId: VaultId): Promise<Member[]>;
  setMemberRole(vaultId: VaultId, peerId: PeerId, role: MemberRole): Promise<Member>;
  removeMember(vaultId: VaultId, peerId: PeerId): Promise<void>;
  deleteVault(vaultId: VaultId): Promise<void>;
  leaveVault(vaultId: VaultId): Promise<void>;
  getPresence(vaultId: VaultId): Promise<PeerPresence[]>;
  publishPresence(input: PresenceInput): Promise<void>;
  askAgent(input: AskAgentInput): Promise<AgentReply>;
}
