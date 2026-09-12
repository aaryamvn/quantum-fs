/**
 * The single source of truth for everything the vault workspace shows.
 *
 * One zustand store rather than React context (docs/decisions/client-workspace.md):
 * presence packets and drag hand-offs arrive many times a second, and a context
 * would re-render the whole grid for each of them. With a store, a tile
 * subscribes to its own id and nothing else, so a presence packet costs exactly
 * the avatar row.
 *
 * The store owns *state*, never the file system: every mutation goes out through
 * a `BackendClient` method and comes back as an `fs-changed` event through
 * {@link WorkspaceActions.applyChanges}, so a change made here and the same
 * change made by a peer take literally the same path into the UI. Actions that
 * touch the backend therefore resolve, toast on failure and hand the caller a
 * plain boolean/null — the UI never sees a rejected promise, with one exception:
 * `commitRename` returns its message so the label can shake in place.
 *
 * Per-frame values (pointer positions, ghost offsets) are deliberately absent —
 * they are Motion values in the drag module. What lives here is coarse: a drag
 * started, something is over a target, the drag ended.
 */

import { create } from "zustand";

import type {
  BackendClient,
  FolderColor,
  FsChange,
  FsNode,
  Member,
  NodeId,
  NodeKind,
  PeerId,
  PeerPresence,
  PresenceInput,
  Recent,
  VaultId,
  VaultMeta,
} from "@/lib/backend";
import { isDescendant, nextUntitled, pathOf, validateName } from "@/lib/path";

import { IDLE_DRAG } from "./types";
import type {
  ContextMenuState,
  DragState,
  JustCreated,
  ModalState,
  NavEntry,
  SortBy,
  SortDir,
  ToastItem,
} from "./types";

/** Single clock for the whole store so a test can stub time in one place. */
const now = (): number => Date.now();

/** How long a toast stands before it dismisses itself. Errors stay long enough to read twice. */
const TOAST_MS = 3200;
const TOAST_ERROR_MS = 5000;

/** One presence packet per frame-and-a-half; anything faster is invisible and costs bandwidth. */
const PRESENCE_THROTTLE_MS = 60;

/**
 * Dismiss timers, keyed by toast id.
 *
 * Module-level rather than in state because a timer handle is not data the UI
 * renders, and putting one in a zustand store would make every toast tick a
 * state change.
 */
const toastTimers = new Map<string, ReturnType<typeof setTimeout>>();
let toastSeq = 0;

/**
 * Guards against an older `openVault` resolving after a newer one.
 *
 * Two clicks in the sidebar race five parallel requests each; without the token
 * the slower vault wins and the user is looking at the wrong tree.
 */
let openToken = 0;

/** What the next presence packet will say; merged into by every `publishPresence` call. */
interface PresenceDraft {
  folderId: NodeId | null;
  hoveringNodeId: NodeId | null;
  draggingNodeIds: NodeId[];
}

let presenceDraft: PresenceDraft = {
  folderId: null,
  hoveringNodeId: null,
  draggingNodeIds: [],
};
let presenceTimer: ReturnType<typeof setTimeout> | null = null;
let presenceSentAt = 0;

function resetPresenceDraft(): void {
  if (presenceTimer !== null) {
    clearTimeout(presenceTimer);
    presenceTimer = null;
  }
  presenceDraft = { folderId: null, hoveringNodeId: null, draggingNodeIds: [] };
  presenceSentAt = 0;
}

/** Errors reach the UI as prose; anything that is not an `Error` still has to read as a sentence. */
function messageOf(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string" && error.trim() !== "") return error;
  return "Something went wrong";
}

/** The root of a loaded tree: the only node without a parent. */
function rootOf(nodes: Record<NodeId, FsNode>, vaultId: VaultId): FsNode | undefined {
  const byConvention = nodes[`root_${vaultId}`];
  if (byConvention) return byConvention;
  for (const id in nodes) {
    if (nodes[id].parentId === null) return nodes[id];
  }
  return undefined;
}

function keyById(list: FsNode[]): Record<NodeId, FsNode> {
  const map: Record<NodeId, FsNode> = {};
  for (const node of list) map[node.id] = node;
  return map;
}

export interface WorkspaceState {
  client: BackendClient | null;
  me: Member | null;

  vaultId: VaultId | null;
  vaultName: string;
  vaultMeta: VaultMeta | null;

  /** The whole tree of the open vault, keyed by id. Small by definition: it is replicated. */
  nodes: Record<NodeId, FsNode>;
  treeLoaded: boolean;
  /** Non-null means the open failed; readers show the error regardless of `treeLoaded`. */
  treeError: string | null;

  members: Member[];
  presence: PeerPresence[];
  recents: Recent[];

  /** Current folder (the root, `root_<vaultId>`, at vault open). */
  folderId: NodeId | null;
  nav: { entries: NavEntry[]; index: number };

  selection: NodeId[];
  anchorId: NodeId | null;
  focusedId: NodeId | null;
  renamingId: NodeId | null;
  clipboard: { vaultId: VaultId; nodeIds: NodeId[] } | null;

  sortBy: SortBy;
  sortDir: SortDir;

  contextMenu: ContextMenuState | null;
  modal: ModalState;
  toasts: ToastItem[];

  /** Pop-in + violet flash; entries older than 2s are ignored by readers. */
  justCreated: Record<NodeId, JustCreated>;
  /** Folder that just received items; the tick makes two drops in a row flash twice. */
  dropFlashFolderId: NodeId | null;
  dropFlashTick: number;
  /** Canvas crossfade when the scripted demo starts over. */
  demoResetTick: number;
  drag: DragState;
}

export interface WorkspaceActions {
  attach(client: BackendClient, me: Member): void;
  openVault(vaultId: VaultId, opts?: { folderId?: NodeId | null; select?: NodeId[] }): Promise<void>;
  closeVault(): void;

  navigateTo(folderId: NodeId, opts?: { replace?: boolean }): void;
  back(): void;
  forward(): void;
  canBack(): boolean;
  canForward(): boolean;
  up(): void;

  select(ids: NodeId[], opts?: { anchor?: NodeId | null; focus?: NodeId | null }): void;
  toggleSelect(id: NodeId): void;
  rangeSelect(toId: NodeId, orderedIds: NodeId[]): void;
  selectAll(orderedIds: NodeId[]): void;
  clearSelection(): void;
  setFocused(id: NodeId | null): void;

  startRename(id: NodeId): void;
  cancelRename(): void;
  commitRename(id: NodeId, name: string): Promise<string | null>;

  createNode(kind: NodeKind): Promise<FsNode | null>;
  requestDelete(ids: NodeId[]): void;
  deleteNodes(ids: NodeId[]): Promise<void>;
  duplicateNodes(ids: NodeId[]): Promise<void>;
  moveNodes(ids: NodeId[], toFolderId: NodeId): Promise<boolean>;
  setColor(id: NodeId, color: FolderColor | null): Promise<void>;
  copy(ids: NodeId[]): void;
  paste(): Promise<void>;
  download(id: NodeId): Promise<void>;
  openNode(id: NodeId): void;

  openContextMenu(x: number, y: number, nodeId: NodeId | null): void;
  closeContextMenu(): void;
  openModal(m: Exclude<ModalState, null>): void;
  closeModal(): void;
  setSort(by: SortBy, dir?: SortDir): void;

  applyChanges(changes: FsChange[], actor: PeerId): void;
  flashDrop(folderId: NodeId): void;

  setPresence(peers: PeerPresence[]): void;
  setMembers(members: Member[]): void;
  refreshMembers(): Promise<void>;
  refreshRecents(): Promise<void>;
  refreshVaultMeta(): Promise<void>;
  refreshTree(): Promise<void>;

  toast(text: string, kind?: ToastItem["kind"], action?: ToastItem["action"]): string;
  dismissToast(id: string): void;
  publishPresence(
    patch: Partial<Pick<PresenceInput, "folderId" | "hoveringNodeId" | "draggingNodeIds">>,
  ): void;
  bumpDemoReset(): void;
  setDrag(patch: Partial<DragState>): void;
}

export type WorkspaceStore = WorkspaceState & WorkspaceActions;

/**
 * Everything a vault owns; `closeVault` and `openVault` both start from here.
 *
 * The clipboard is deliberately outside it: a copy made in one vault survives the
 * trip to another, and `paste` is the one place that decides what to do about it.
 */
const VAULT_SCOPED: Omit<WorkspaceState, "client" | "me" | "recents" | "toasts" | "clipboard"> = {
  vaultId: null,
  vaultName: "",
  vaultMeta: null,
  nodes: {},
  treeLoaded: false,
  treeError: null,
  members: [],
  presence: [],
  folderId: null,
  nav: { entries: [], index: -1 },
  selection: [],
  anchorId: null,
  focusedId: null,
  renamingId: null,
  sortBy: "name",
  sortDir: "asc",
  contextMenu: null,
  modal: null,
  justCreated: {},
  dropFlashFolderId: null,
  dropFlashTick: 0,
  demoResetTick: 0,
  drag: IDLE_DRAG,
};

export const useWorkspace = create<WorkspaceStore>()((set, get) => {
  /** Every backend failure lands here: the UI is told in prose, the promise resolves. */
  const fail = (error: unknown): void => {
    get().toast(messageOf(error), "error");
  };

  /** Send the pending presence draft, if there is anywhere to send it. */
  const flushPresence = (): void => {
    presenceTimer = null;
    const state = get();
    const { client, vaultId } = state;
    if (!client || !vaultId) return;
    presenceSentAt = now();
    void Promise.resolve(
      client.publishPresence({
        vaultId,
        folderId: presenceDraft.folderId ?? state.folderId,
        // Nothing draws a peer's pointer any more; the field stays in the
        // backend's packet shape, so it goes out empty rather than disappearing.
        cursor: null,
        hoveringNodeId: presenceDraft.hoveringNodeId,
        draggingNodeIds: presenceDraft.draggingNodeIds,
      }),
    ).catch(() => {
      // Presence is best-effort: a dropped packet is corrected by the next one.
    });
  };

  /**
   * The nearest history entry in `step`'s direction whose folder still exists.
   *
   * A peer can delete a folder we walked through, which leaves a dead entry in
   * the trail; stepping onto it would land the user nowhere. Skipping over dead
   * entries is what keeps back/forward honest without rewriting the trail on
   * every remote change.
   */
  const liveNavIndex = (step: -1 | 1): number => {
    const { nav, nodes } = get();
    for (let i = nav.index + step; i >= 0 && i < nav.entries.length; i += step) {
      if (nodes[nav.entries[i].folderId] !== undefined) return i;
    }
    return -1;
  };

  /** Shared by `navigateTo`, `back` and `forward`: arriving in a folder always looks the same. */
  const arriveAt = (folderId: NodeId, nav: WorkspaceState["nav"]): void => {
    set({
      folderId,
      nav,
      selection: [],
      anchorId: null,
      focusedId: null,
      renamingId: null,
      contextMenu: null,
    });
    get().publishPresence({ folderId });
  };

  return {
    ...VAULT_SCOPED,
    client: null,
    me: null,
    recents: [],
    toasts: [],
    clipboard: null,

    attach(client, me) {
      set({ client, me });
    },

    async openVault(vaultId, opts) {
      const client = get().client;
      if (!client) return;

      const token = ++openToken;
      resetPresenceDraft();
      set({ ...VAULT_SCOPED, vaultId });

      try {
        const [tree, meta, members, presence, recents] = await Promise.all([
          client.listTree(vaultId),
          client.getVaultMeta(vaultId),
          client.listMembers(vaultId),
          client.getPresence(vaultId),
          client.listRecents(),
        ]);
        if (token !== openToken) return;

        const nodes = keyById(tree);
        const root = rootOf(nodes, vaultId);
        const wanted = opts?.folderId ?? null;
        const folderId = wanted && nodes[wanted] ? wanted : (root?.id ?? null);
        const selection = (opts?.select ?? []).filter((id) => nodes[id] !== undefined);

        set({
          nodes,
          vaultName: root?.name ?? meta.name,
          vaultMeta: meta,
          members,
          presence,
          recents,
          treeLoaded: true,
          folderId,
          nav: folderId ? { entries: [{ vaultId, folderId }], index: 0 } : { entries: [], index: -1 },
          selection,
          anchorId: selection[0] ?? null,
          focusedId: selection[selection.length - 1] ?? null,
        });
        get().publishPresence({ folderId });
      } catch (error) {
        if (token !== openToken) return;
        const message = messageOf(error);
        set({ treeError: message });
        get().toast(message, "error");
      }
    },

    closeVault() {
      openToken += 1;
      resetPresenceDraft();
      set({ ...VAULT_SCOPED });
    },

    navigateTo(folderId, opts) {
      const state = get();
      const vaultId = state.vaultId;
      const node = state.nodes[folderId];
      if (!vaultId || !node || node.kind !== "folder") return;
      if (folderId === state.folderId) return;

      // Navigating from the middle of the history drops everything ahead of it,
      // exactly as a browser does — the forward trail is no longer where you were.
      const kept = state.nav.entries.slice(0, state.nav.index + 1);
      const entry: NavEntry = { vaultId, folderId };
      const nav = opts?.replace
        ? { entries: [...kept.slice(0, -1), entry], index: Math.max(0, kept.length - 1) }
        : { entries: [...kept, entry], index: kept.length };

      arriveAt(folderId, nav);

      // The vault root is not a "recent": it is where the sidebar already points.
      if (node.parentId !== null) {
        void Promise.resolve(state.client?.touchRecent(vaultId, folderId)).catch(() => {});
      }
    },

    back() {
      const index = liveNavIndex(-1);
      if (index === -1) return;
      const { nav } = get();
      arriveAt(nav.entries[index].folderId, { entries: nav.entries, index });
    },

    forward() {
      const index = liveNavIndex(1);
      if (index === -1) return;
      const { nav } = get();
      arriveAt(nav.entries[index].folderId, { entries: nav.entries, index });
    },

    canBack() {
      return liveNavIndex(-1) !== -1;
    },

    canForward() {
      return liveNavIndex(1) !== -1;
    },

    up() {
      const state = get();
      const current = state.folderId ? state.nodes[state.folderId] : undefined;
      const parentId = current?.parentId ?? null;
      if (parentId) get().navigateTo(parentId);
    },

    select(ids, opts) {
      const nodes = get().nodes;
      const seen = new Set<NodeId>();
      const next: NodeId[] = [];
      for (const id of ids) {
        if (nodes[id] !== undefined && !seen.has(id)) {
          seen.add(id);
          next.push(id);
        }
      }
      set({
        selection: next,
        anchorId: opts?.anchor ?? next[0] ?? null,
        focusedId: opts?.focus ?? next[next.length - 1] ?? null,
      });
    },

    toggleSelect(id) {
      const state = get();
      if (state.nodes[id] === undefined) return;
      if (state.selection.includes(id)) {
        const selection = state.selection.filter((other) => other !== id);
        set({
          selection,
          anchorId: state.anchorId === id ? (selection[selection.length - 1] ?? null) : state.anchorId,
          focusedId: state.focusedId === id ? (selection[selection.length - 1] ?? null) : state.focusedId,
        });
        return;
      }
      set({ selection: [...state.selection, id], anchorId: id, focusedId: id });
    },

    rangeSelect(toId, orderedIds) {
      const state = get();
      const anchor = state.anchorId ?? toId;
      const to = orderedIds.indexOf(toId);
      if (to === -1) return;
      const from = orderedIds.indexOf(anchor);
      if (from === -1) {
        get().select([toId]);
        return;
      }
      const lo = Math.min(from, to);
      const hi = Math.max(from, to);
      get().select(orderedIds.slice(lo, hi + 1), { anchor, focus: toId });
    },

    selectAll(orderedIds) {
      get().select(orderedIds);
    },

    clearSelection() {
      set({ selection: [], anchorId: null, focusedId: null });
    },

    setFocused(id) {
      set({ focusedId: id });
    },

    startRename(id) {
      if (get().nodes[id] === undefined) return;
      set({ renamingId: id, selection: [id], anchorId: id, focusedId: id, contextMenu: null });
    },

    cancelRename() {
      set({ renamingId: null });
    },

    async commitRename(id, name) {
      const state = get();
      const node = state.nodes[id];
      const { client, vaultId } = state;
      const trimmed = name.trim();

      if (!node || !client || !vaultId) {
        set({ renamingId: null });
        return null;
      }
      if (trimmed === node.name) {
        set({ renamingId: null });
        return null;
      }

      // Local rules first: an empty or illegal name never reaches the daemon, and
      // the message goes back to the caller rather than to a toast — the label shakes.
      const invalid = validateName(trimmed);
      if (invalid !== null) return invalid;

      try {
        await client.renameNode({ vaultId, nodeId: id, name: trimmed });
        set({ renamingId: null });
        return null;
      } catch (error) {
        return messageOf(error);
      }
    },

    async createNode(kind) {
      const state = get();
      const { client, vaultId, folderId, me } = state;
      if (!client || !vaultId || !folderId) return null;

      const siblings: string[] = [];
      for (const id in state.nodes) {
        if (state.nodes[id].parentId === folderId) siblings.push(state.nodes[id].name);
      }

      try {
        const node = await client.createNode({
          vaultId,
          parentId: folderId,
          kind,
          name: nextUntitled(siblings, kind),
        });
        // Insert it now rather than waiting for the round trip through `fs-changed`:
        // the rename field has to open on a tile that already exists, and the early
        // insert is also what stops `applyChanges` treating the echo as a new node.
        set((s) => ({
          nodes: { ...s.nodes, [node.id]: node },
          justCreated: { ...s.justCreated, [node.id]: { by: me?.peerId ?? node.createdBy, at: now() } },
        }));
        get().select([node.id]);
        get().startRename(node.id);
        return node;
      } catch (error) {
        fail(error);
        return null;
      }
    },

    requestDelete(ids) {
      const nodes = get().nodes;
      const nodeIds = ids.filter((id) => nodes[id] !== undefined);
      if (nodeIds.length === 0) return;
      get().openModal({ kind: "confirm-delete", nodeIds });
    },

    async deleteNodes(ids) {
      const state = get();
      const { client, vaultId } = state;
      if (!client || !vaultId || ids.length === 0) return;
      const firstName = state.nodes[ids[0]]?.name ?? "item";

      try {
        await client.deleteNodes({ vaultId, nodeIds: ids });
        set({ modal: null, selection: [], anchorId: null, focusedId: null });
        get().toast(
          ids.length === 1 ? `Deleted ${firstName}` : `Deleted ${ids.length} items`,
          "success",
        );
      } catch (error) {
        fail(error);
      }
    },

    async duplicateNodes(ids) {
      const { client, vaultId, nodes } = get();
      if (!client || !vaultId) return;
      // A peer can delete something between the menu opening and the click; the
      // daemon rejects the whole batch for one dead id, so they go first.
      const nodeIds = ids.filter((id) => nodes[id] !== undefined);
      if (nodeIds.length === 0) return;
      try {
        const copies = await client.duplicateNodes({ vaultId, nodeIds, toParentId: null });
        set((s) => ({ nodes: { ...s.nodes, ...keyById(copies) } }));
        get().select(copies.map((node) => node.id));
      } catch (error) {
        fail(error);
      }
    },

    async moveNodes(ids, toFolderId) {
      const state = get();
      const { client, vaultId, nodes } = state;
      const target = nodes[toFolderId];
      if (!client || !vaultId || !target || target.kind !== "folder") return false;

      // Drop the moves that are no-ops or impossible before asking: the daemon
      // would reject the whole batch for one bad id, and a drag onto a tile
      // routinely carries the tile itself.
      const moving = ids.filter((id) => {
        const node = nodes[id];
        if (!node || id === toFolderId) return false;
        if (node.parentId === toFolderId) return false;
        if (node.kind === "folder" && isDescendant(nodes, toFolderId, id)) return false;
        return true;
      });
      if (moving.length === 0) return false;

      try {
        await client.moveNodes({ vaultId, nodeIds: moving, toParentId: toFolderId });
        get().flashDrop(toFolderId);
        set({ selection: [], anchorId: null, focusedId: null });
        return true;
      } catch (error) {
        fail(error);
        return false;
      }
    },

    async setColor(id, color) {
      const { client, vaultId } = get();
      if (!client || !vaultId) return;
      try {
        await client.setNodeColor({ vaultId, nodeId: id, color });
      } catch (error) {
        fail(error);
      }
    },

    copy(ids) {
      const state = get();
      const vaultId = state.vaultId;
      if (!vaultId) return;
      const nodeIds = ids.filter((id) => state.nodes[id] !== undefined);
      if (nodeIds.length === 0) return;
      set({ clipboard: { vaultId, nodeIds } });
      get().toast(
        nodeIds.length === 1
          ? `Copied ${state.nodes[nodeIds[0]].name}`
          : `Copied ${nodeIds.length} items`,
      );
    },

    async paste() {
      const state = get();
      const { client, vaultId, folderId, clipboard } = state;
      if (!client || !vaultId || !folderId || !clipboard || clipboard.nodeIds.length === 0) return;
      if (clipboard.vaultId !== vaultId) {
        get().toast("Can't paste across vaults yet", "error");
        return;
      }
      // The clipboard outlives the tree it was filled from: anything deleted
      // since the copy is dropped rather than failing the whole paste.
      const nodeIds = clipboard.nodeIds.filter((id) => state.nodes[id] !== undefined);
      if (nodeIds.length === 0) {
        get().toast("Nothing left to paste", "error");
        return;
      }
      try {
        const copies = await client.duplicateNodes({
          vaultId,
          nodeIds,
          toParentId: folderId,
        });
        set((s) => ({ nodes: { ...s.nodes, ...keyById(copies) } }));
        get().select(copies.map((node) => node.id));
      } catch (error) {
        fail(error);
      }
    },

    async download(id) {
      const state = get();
      const { client, vaultId } = state;
      const node = state.nodes[id];
      if (!client || !vaultId || !node) return;
      try {
        // Silent on purpose: the only report a download owes anyone is the ring
        // on the tile and the line in the inspector. There is no Download button
        // to acknowledge, so a toast would be the app talking to itself.
        await client.requestDownload(vaultId, id);
      } catch (error) {
        fail(error);
      }
    },

    openNode(id) {
      const state = get();
      const node = state.nodes[id];
      if (!node) return;

      if (node.kind === "folder") {
        get().navigateTo(id);
        return;
      }
      if (node.availability === "remote") {
        void get().download(id);
        return;
      }
      if (node.availability === "downloading") {
        // Already on its way; the tile's ring is saying so. Double-clicking
        // again is impatience, not a new instruction.
        return;
      }
      // Handing the file to the OS needs the daemon's mount point; until then
      // opening is acknowledged and recorded, which is what the Recents list reads.
      get().toast(`Opened ${node.name}`);
      if (state.vaultId) {
        void Promise.resolve(state.client?.touchRecent(state.vaultId, id)).catch(() => {});
      }
    },

    openContextMenu(x, y, nodeId) {
      const state = get();
      // Right-clicking outside the selection retargets it, the way Finder does;
      // right-clicking inside it keeps the whole selection as the menu's subject.
      if (nodeId !== null && !state.selection.includes(nodeId)) {
        get().select([nodeId]);
      }
      set({ contextMenu: { x, y, nodeId } });
    },

    closeContextMenu() {
      set({ contextMenu: null });
    },

    openModal(m) {
      set({ modal: m, contextMenu: null });
    },

    closeModal() {
      set({ modal: null });
    },

    setSort(by, dir) {
      const state = get();
      if (dir !== undefined) {
        set({ sortBy: by, sortDir: dir });
        return;
      }
      if (by === state.sortBy) {
        set({ sortDir: state.sortDir === "asc" ? "desc" : "asc" });
        return;
      }
      // A new column starts the way people expect to read it: names A→Z, but
      // dates and sizes biggest/newest first.
      set({ sortBy: by, sortDir: by === "name" || by === "kind" ? "asc" : "desc" });
    },

    applyChanges(changes, actor) {
      if (changes.length === 0) return;
      const state = get();
      const previous = state.nodes;
      const nodes: Record<NodeId, FsNode> = { ...previous };
      const justCreated = { ...state.justCreated };
      const mine = state.me?.peerId ?? null;
      let vaultName = state.vaultName;
      let touchedRemoval = false;

      for (const change of changes) {
        if (change.kind === "upsert") {
          const known = nodes[change.node.id] !== undefined;
          nodes[change.node.id] = change.node;
          // A node nobody here asked for appeared: it belongs to a peer, so it
          // pops in and flashes. Our own creations are already known (see createNode).
          if (!known && actor !== mine) justCreated[change.node.id] = { by: actor, at: now() };
          if (change.node.parentId === null) vaultName = change.node.name;
        } else {
          if (nodes[change.nodeId] !== undefined) touchedRemoval = true;
          delete nodes[change.nodeId];
          delete justCreated[change.nodeId];
        }
      }

      const folderId = state.folderId;
      const selection = state.selection.filter((id) => {
        const node = nodes[id];
        if (!node) return false;
        // Something a peer moved out from under us is no longer part of this view.
        return folderId === null || node.parentId === folderId;
      });
      // A removal can strand history entries: drop the dead ones and keep the
      // cursor on the newest survivor at or before where it stood, so back and
      // forward still walk the trail the user actually took.
      let nav = state.nav;
      if (touchedRemoval && nav.entries.length > 0) {
        const entries: NavEntry[] = [];
        let index = -1;
        for (let i = 0; i < nav.entries.length; i += 1) {
          const entry = nav.entries[i];
          if (nodes[entry.folderId] === undefined) continue;
          entries.push(entry);
          if (i <= nav.index) index = entries.length - 1;
        }
        if (entries.length !== nav.entries.length) nav = { entries, index };
      }
      const renamingId =
        state.renamingId !== null && nodes[state.renamingId] === undefined ? null : state.renamingId;

      set({
        nodes,
        justCreated,
        vaultName,
        selection,
        anchorId: state.anchorId !== null && nodes[state.anchorId] ? state.anchorId : null,
        focusedId: state.focusedId !== null && nodes[state.focusedId] ? state.focusedId : null,
        nav,
        renamingId,
      });

      // The floor was removed under us: walk up the *old* chain to the nearest
      // ancestor that survived, and replace the dead entry rather than pushing.
      if (folderId !== null && nodes[folderId] === undefined) {
        const chain = pathOf(previous, folderId);
        let survivor: NodeId | null = null;
        for (let i = chain.length - 2; i >= 0; i--) {
          if (nodes[chain[i].id] !== undefined) {
            survivor = chain[i].id;
            break;
          }
        }
        if (survivor === null && state.vaultId) survivor = rootOf(nodes, state.vaultId)?.id ?? null;
        if (survivor !== null) get().navigateTo(survivor, { replace: true });
      }
    },

    flashDrop(folderId) {
      set((s) => ({ dropFlashFolderId: folderId, dropFlashTick: s.dropFlashTick + 1 }));
    },

    setPresence(peers) {
      set({ presence: peers });
    },

    setMembers(members) {
      set({ members });
    },

    async refreshMembers() {
      const { client, vaultId } = get();
      if (!client || !vaultId) return;
      try {
        const members = await client.listMembers(vaultId);
        if (get().vaultId === vaultId) set({ members });
      } catch (error) {
        fail(error);
      }
    },

    async refreshRecents() {
      const client = get().client;
      if (!client) return;
      try {
        set({ recents: await client.listRecents() });
      } catch (error) {
        fail(error);
      }
    },

    async refreshVaultMeta() {
      const { client, vaultId } = get();
      if (!client || !vaultId) return;
      try {
        const vaultMeta = await client.getVaultMeta(vaultId);
        if (get().vaultId === vaultId) set({ vaultMeta });
      } catch (error) {
        fail(error);
      }
    },

    async refreshTree() {
      const { client, vaultId } = get();
      if (!client || !vaultId) return;
      try {
        const nodes = keyById(await client.listTree(vaultId));
        const state = get();
        if (state.vaultId !== vaultId) return;
        const root = rootOf(nodes, vaultId);
        const folderId = state.folderId && nodes[state.folderId] ? state.folderId : (root?.id ?? null);
        set({
          nodes,
          vaultName: root?.name ?? state.vaultName,
          treeLoaded: true,
          treeError: null,
          folderId,
          selection: state.selection.filter((id) => nodes[id] !== undefined),
        });
      } catch (error) {
        fail(error);
      }
    },

    toast(text, kind = "info", action) {
      toastSeq += 1;
      const id = `toast_${toastSeq}`;
      const item: ToastItem = action ? { id, text, kind, action } : { id, text, kind };
      set((s) => ({ toasts: [...s.toasts, item] }));

      if (typeof setTimeout === "function") {
        const timer = setTimeout(
          () => {
            toastTimers.delete(id);
            get().dismissToast(id);
          },
          kind === "error" ? TOAST_ERROR_MS : TOAST_MS,
        );
        toastTimers.set(id, timer);
      }
      return id;
    },

    dismissToast(id) {
      const timer = toastTimers.get(id);
      if (timer !== undefined) {
        clearTimeout(timer);
        toastTimers.delete(id);
      }
      set((s) => {
        const toasts = s.toasts.filter((t) => t.id !== id);
        return toasts.length === s.toasts.length ? s : { toasts };
      });
    },

    publishPresence(patch) {
      presenceDraft = {
        folderId: patch.folderId !== undefined ? patch.folderId : presenceDraft.folderId,
        hoveringNodeId:
          patch.hoveringNodeId !== undefined ? patch.hoveringNodeId : presenceDraft.hoveringNodeId,
        draggingNodeIds:
          patch.draggingNodeIds !== undefined ? patch.draggingNodeIds : presenceDraft.draggingNodeIds,
      };

      // Throttle with a trailing edge: the last state of a gesture must go out,
      // or a peer is left believing we are still holding what we just dropped.
      const elapsed = now() - presenceSentAt;
      if (elapsed >= PRESENCE_THROTTLE_MS) {
        flushPresence();
        return;
      }
      if (presenceTimer !== null || typeof setTimeout !== "function") return;
      presenceTimer = setTimeout(flushPresence, PRESENCE_THROTTLE_MS - elapsed);
    },

    bumpDemoReset() {
      set((s) => ({ demoResetTick: s.demoResetTick + 1 }));
    },

    setDrag(patch) {
      set((s) => ({ drag: { ...s.drag, ...patch } }));
    },
  };
});
