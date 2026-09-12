import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { iconCategoryForName } from "@/components/icons/registry";
import { isDescendant, splitName } from "@/lib/path";
import { searchNodes } from "@/lib/search";
import type { SearchContext } from "@/lib/search";

import type {
  AskAgentInput,
  BackendClient,
  CreateNodeInput,
  DeleteNodesInput,
  DuplicateNodesInput,
  MoveNodesInput,
  PresenceInput,
  RenameNodeInput,
  SetAccessInput,
  SetNodeColorInput,
  VaultMetaPatch,
} from "./client";
import type {
  AgentReply,
  BackendEvent,
  CreateVaultInput,
  DaemonStatus,
  FsChange,
  FsNode,
  HistoryEvent,
  JoinCode,
  Member,
  MemberRole,
  NodeAccess,
  NodeId,
  OrchestrationServer,
  PeerId,
  PeerPresence,
  Recent,
  SearchHit,
  SearchQuery,
  Vault,
  VaultId,
  VaultMeta,
} from "./types";

/*
 * Every command and event name the webview knows lives in this file and nowhere
 * else, so renaming one on the Rust side is a single-file change here.
 */

/** Emitted after any mutation of the server/vault list. */
const EVENT_SERVERS_CHANGED = "backend://servers-changed";
/** Emitted after any mutation of a vault's tree; payload is {@link FsChangedPayload}. */
const EVENT_FS_CHANGED = "backend://fs-changed";
/** Emitted when a vault's member list or a member's role changes. */
const EVENT_MEMBERS_CHANGED = "backend://members-changed";
/** Emitted when a vault's settings (name, description, join code, quota) change. */
const EVENT_VAULT_CHANGED = "backend://vault-changed";
/** Emitted when the recents list is reordered or trimmed. */
const EVENT_RECENTS_CHANGED = "backend://recents-changed";
/** Emitted on every presence tick of a vault; payload is {@link PresencePayload}. */
const EVENT_PRESENCE = "backend://presence";
/** Emitted when a membership ends (deleted, kicked, unrecoverable); payload is {@link VaultRemovedPayload}. */
const EVENT_VAULT_REMOVED = "backend://vault-removed";

interface VaultScopedPayload {
  vaultId: VaultId;
}

interface VaultRemovedPayload {
  vaultId: VaultId;
  reason: string;
}

interface FsChangedPayload {
  vaultId: VaultId;
  changes: FsChange[];
  actor: PeerId;
}

interface PresencePayload {
  vaultId: VaultId;
  peers: PeerPresence[];
}

/**
 * How long a tree fetched for search stays reusable.
 *
 * Search runs on every keystroke, and the tree is replicated (small, and almost
 * never changing between two characters), so a very short window turns a burst
 * of typing into one IPC round trip without ever showing meaningfully stale
 * results. `fs-changed` drops the entry early, so a real mutation is visible at
 * once rather than after the window.
 */
const TREE_CACHE_MS = 2_000;

/**
 * Call a Rust command, normalizing its rejection into an `Error`.
 *
 * `tauri::command` rejects with whatever the handler returned — for our bridge a
 * plain `String` — so without this every `catch (e)` in the UI would have to
 * guess whether `e.message` exists. One wrapper means the whole app can rely on
 * `Error`, which is what the store's error surfaces already assume.
 */
async function invokeCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (e) {
    throw e instanceof Error ? e : new Error(String(e));
  }
}

/** Structured filters are applied after ranking, so ranking must not truncate first. */
function hasStructuredFilter(query: SearchQuery): boolean {
  return (
    query.kinds !== null ||
    query.exts !== null ||
    query.inFolderId !== null ||
    query.by !== null ||
    query.availability !== null ||
    query.modifiedAfter !== null
  );
}

/**
 * `BackendClient` backed by the Rust commands in `client/src-tauri/src/bridge.rs`
 * and `client/src-tauri/src/fs_bridge.rs`.
 *
 * Rust owns the socket to `qfsd`; this file is the only place that names its
 * commands. The one method that is not a command is `search`: the tree is already
 * replicated to every member, so ranking it here costs a map walk and keeps the
 * palette responsive on every keystroke, where a round trip per character would
 * not be (docs/decisions/client-workspace.md).
 */
export function createTauriBackend(): BackendClient {
  /** vaultId → the in-flight or recently resolved `list_tree`, for {@link TREE_CACHE_MS}. */
  const treeCache = new Map<VaultId, { at: number; nodes: Promise<FsNode[]> }>();

  const listServers = () => invokeCommand<OrchestrationServer[]>("list_servers");

  const listMembers = (vaultId: VaultId) => invokeCommand<Member[]>("list_members", { vaultId });

  /**
   * The vault's tree, reused inside the cache window.
   *
   * The *promise* is cached rather than its value so that the several searches a
   * fast typist fires before the first one resolves share a single round trip. A
   * rejection is evicted immediately, so a failure never sticks for two seconds.
   */
  const cachedTree = (vaultId: VaultId): Promise<FsNode[]> => {
    const now = Date.now();
    const hit = treeCache.get(vaultId);
    if (hit && now - hit.at < TREE_CACHE_MS) return hit.nodes;

    const nodes = invokeCommand<FsNode[]>("list_tree", { vaultId });
    treeCache.set(vaultId, { at: now, nodes });
    void nodes.catch(() => {
      if (treeCache.get(vaultId)?.nodes === nodes) treeCache.delete(vaultId);
    });
    return nodes;
  };

  const search = async (query: SearchQuery): Promise<SearchHit[]> => {
    if (query.limit <= 0) return [];

    const servers = await listServers();
    const vaults = servers.flatMap((server) => server.vaults);
    const vaultNames: Record<string, string> = {};
    for (const vault of vaults) vaultNames[vault.id] = vault.name;

    const vaultIds = query.vaultId === null ? vaults.map((vault) => vault.id) : [query.vaultId];
    if (vaultIds.length === 0) return [];

    const trees = await Promise.all(vaultIds.map((vaultId) => cachedTree(vaultId)));
    const nodesByVault: Record<string, Record<string, FsNode>> = {};
    vaultIds.forEach((vaultId, i) => {
      const byId: Record<string, FsNode> = {};
      for (const node of trees[i]) byId[node.id] = node;
      nodesByVault[vaultId] = byId;
    });

    // Best effort: `by:maya` should match a person by name, but a member list that
    // fails to load must degrade to peer-id matching rather than fail the search.
    const memberLists = await Promise.all(
      vaultIds.map((vaultId) => listMembers(vaultId).catch((): Member[] => [])),
    );
    const memberNames: Record<string, string> = {};
    for (const members of memberLists) {
      for (const member of members) memberNames[member.peerId] = member.name;
    }

    const ctx: SearchContext = {
      nodesByVault,
      vaultNames,
      memberNames,
      categoryOf: iconCategoryForName,
      now: Date.now(),
    };

    // Ranking must see every candidate when a structured filter will remove some of
    // them afterwards, or a filtered search would return fewer than `limit` hits.
    const filtered = hasStructuredFilter(query);
    const ranked = searchNodes(query.text, ctx, {
      vaultId: query.vaultId,
      limit: filtered ? Number.MAX_SAFE_INTEGER : query.limit,
    });

    const exts = query.exts ? query.exts.map((ext) => ext.replace(/^\./, "").toLowerCase()) : null;
    const hits: SearchHit[] = [];
    for (const hit of ranked) {
      const nodes = nodesByVault[hit.node.vaultId];
      const node = nodes?.[hit.node.id];
      if (!node) continue;

      if (query.kinds && !query.kinds.includes(node.kind)) continue;
      if (query.availability && node.availability !== query.availability) continue;
      if (query.modifiedAfter !== null && node.modifiedAt < query.modifiedAfter) continue;
      if (exts && !exts.includes(splitName(node.name).ext.toLowerCase())) continue;
      if (query.by && node.createdBy !== query.by && node.modifiedBy !== query.by) continue;
      if (query.inFolderId && !isDescendant(nodes, node.id, query.inFolderId)) continue;

      hits.push({
        node,
        vaultName: hit.vaultName,
        path: hit.path,
        score: hit.score,
        matches: hit.matches,
      });
      if (hits.length === query.limit) break;
    }
    return hits;
  };

  return {
    status() {
      return invokeCommand<DaemonStatus>("daemon_status");
    },

    listServers,

    addServer(input: { name: string; address: string }) {
      return invokeCommand<OrchestrationServer>("add_server", { input });
    },

    createVault(input: CreateVaultInput) {
      return invokeCommand<Vault>("create_vault", { input });
    },

    joinVault(code: JoinCode) {
      return invokeCommand<Vault>("join_vault", { code });
    },

    /**
     * Bridge every `backend://` event onto the one `BackendEvent` union.
     *
     * `listen` resolves asynchronously, so the returned unsubscribe waits for every
     * registration before detaching them — otherwise a component that mounts
     * and unmounts inside one tick would leak the listeners that had not resolved.
     */
    subscribe(listener: (e: BackendEvent) => void) {
      const unlistens = [
        listen(EVENT_SERVERS_CHANGED, () => {
          listener({ type: "servers-changed" });
        }),
        listen<FsChangedPayload>(EVENT_FS_CHANGED, (e) => {
          // The tree just changed; a cached copy would outlive its truth.
          treeCache.delete(e.payload.vaultId);
          listener({
            type: "fs-changed",
            vaultId: e.payload.vaultId,
            changes: e.payload.changes,
            actor: e.payload.actor,
          });
        }),
        listen<VaultScopedPayload>(EVENT_MEMBERS_CHANGED, (e) => {
          listener({ type: "members-changed", vaultId: e.payload.vaultId });
        }),
        listen<VaultScopedPayload>(EVENT_VAULT_CHANGED, (e) => {
          listener({ type: "vault-changed", vaultId: e.payload.vaultId });
        }),
        listen(EVENT_RECENTS_CHANGED, () => {
          listener({ type: "recents-changed" });
        }),
        listen<PresencePayload>(EVENT_PRESENCE, (e) => {
          listener({ type: "presence", vaultId: e.payload.vaultId, peers: e.payload.peers });
        }),
        listen<VaultRemovedPayload>(EVENT_VAULT_REMOVED, (e) => {
          // The vault is gone; a cached tree for it can only mislead the next search.
          treeCache.delete(e.payload.vaultId);
          listener({
            type: "vault-removed",
            vaultId: e.payload.vaultId,
            reason: e.payload.reason,
          });
        }),
      ];

      return () => {
        void Promise.all(unlistens).then((offs) => {
          for (const off of offs) off();
        });
      };
    },

    me() {
      return invokeCommand<Member>("me");
    },

    listTree(vaultId: VaultId) {
      return invokeCommand<FsNode[]>("list_tree", { vaultId });
    },

    createNode(input: CreateNodeInput) {
      return invokeCommand<FsNode>("create_node", { input });
    },

    renameNode(input: RenameNodeInput) {
      return invokeCommand<FsNode>("rename_node", { input });
    },

    moveNodes(input: MoveNodesInput) {
      return invokeCommand<FsNode[]>("move_nodes", { input });
    },

    deleteNodes(input: DeleteNodesInput) {
      return invokeCommand<void>("delete_nodes", { input });
    },

    duplicateNodes(input: DuplicateNodesInput) {
      return invokeCommand<FsNode[]>("duplicate_nodes", { input });
    },

    setNodeColor(input: SetNodeColorInput) {
      return invokeCommand<FsNode>("set_node_color", { input });
    },

    requestDownload(vaultId: VaultId, nodeId: NodeId) {
      return invokeCommand<void>("request_download", { vaultId, nodeId });
    },

    openFile(vaultId: VaultId, nodeId: NodeId) {
      return invokeCommand<void>("open_node", { vaultId, nodeId });
    },

    /**
     * `paths` is sent as an explicit `null` rather than omitted: Rust deserializes the
     * input struct as a whole, and a missing key on an `Option<Vec<String>>` field is
     * only tolerated with `#[serde(default)]` — `null` means "show the chooser" on
     * either side.
     */
    importFiles(vaultId: VaultId, parentId: NodeId, paths?: string[]) {
      return invokeCommand<FsNode[]>("import_files", {
        input: { vaultId, parentId, paths: paths ?? null },
      });
    },

    readTextPreview(vaultId: VaultId, nodeId: NodeId, maxBytes: number) {
      return invokeCommand<string | null>("read_text_preview", { vaultId, nodeId, maxBytes });
    },

    getAccess(vaultId: VaultId, nodeId: NodeId) {
      return invokeCommand<NodeAccess>("get_access", { vaultId, nodeId });
    },

    setAccess(input: SetAccessInput) {
      return invokeCommand<NodeAccess>("set_access", { input });
    },

    getHistory(vaultId: VaultId, nodeId: NodeId) {
      return invokeCommand<HistoryEvent[]>("get_history", { vaultId, nodeId });
    },

    listRecents() {
      return invokeCommand<Recent[]>("list_recents");
    },

    touchRecent(vaultId: VaultId, nodeId: NodeId) {
      return invokeCommand<void>("touch_recent", { vaultId, nodeId });
    },

    search,

    getVaultMeta(vaultId: VaultId) {
      return invokeCommand<VaultMeta>("get_vault_meta", { vaultId });
    },

    updateVaultMeta(vaultId: VaultId, patch: VaultMetaPatch) {
      return invokeCommand<VaultMeta>("update_vault_meta", { vaultId, patch });
    },

    rotateJoinCode(vaultId: VaultId) {
      return invokeCommand<JoinCode>("rotate_join_code", { vaultId });
    },

    listMembers,

    setMemberRole(vaultId: VaultId, peerId: PeerId, role: MemberRole) {
      return invokeCommand<Member>("set_member_role", { vaultId, peerId, role });
    },

    removeMember(vaultId: VaultId, peerId: PeerId) {
      return invokeCommand<void>("remove_member", { vaultId, peerId });
    },

    deleteVault(vaultId: VaultId) {
      return invokeCommand<void>("delete_vault", { vaultId });
    },

    leaveVault(vaultId: VaultId) {
      return invokeCommand<void>("leave_vault", { vaultId });
    },

    getPresence(vaultId: VaultId) {
      return invokeCommand<PeerPresence[]>("get_presence", { vaultId });
    },

    publishPresence(input: PresenceInput) {
      return invokeCommand<void>("publish_presence", { input });
    },

    askAgent(input: AskAgentInput) {
      return invokeCommand<AgentReply>("ask_agent", { input });
    },
  };
}
