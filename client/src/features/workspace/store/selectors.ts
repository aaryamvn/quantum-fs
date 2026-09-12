/**
 * Reads of the workspace store, and the pure ordering rules behind them.
 *
 * Two jobs. First, every hook subscribes to the narrowest slice it can, so a
 * presence packet does not re-render a grid of 200 tiles — `useIsSelected`
 * returns a boolean, not the selection array. Second, the hooks that must return
 * a *new* array (children, path, selection) memoize it against the identity of
 * the state it derives from: zustand compares snapshots with `Object.is`, so a
 * selector that allocated on every call would re-render forever.
 *
 * The caches are module-level `WeakMap`s keyed by the `nodes` object, which the
 * store replaces wholesale on every change — so a tree mutation invalidates them
 * by construction, and an old tree is collected along with its entries.
 */

import type { FsNode, Member, NodeId, PeerId, PeerPresence } from "@/lib/backend";
import { pathOf, splitName } from "@/lib/path";

import type { JustCreated, SortBy, SortDir } from "./types";
import { useWorkspace } from "./workspaceStore";

/** One shared empty array, so "nothing here" is a stable snapshot. */
const NONE: FsNode[] = [];
const NO_NAMES: string[] = [];

/** A creation stops being "new" after this; readers ignore older entries. */
const JUST_CREATED_MS = 2000;

/** Separator for cache keys — a character no node id or sort token can contain. */
const KEY_SEP = "|";

/**
 * Finder's order: folders lead, always, whichever way the column is sorted.
 *
 * Mixing folders into a size or date ordering makes a folder impossible to find
 * by eye, which is why every file manager that tried it went back. `dir` flips
 * the order *inside* each group, never the groups themselves.
 */
export function sortNodes(nodes: FsNode[], by: SortBy, dir: SortDir): FsNode[] {
  const sign = dir === "asc" ? 1 : -1;
  const byName = (a: FsNode, b: FsNode): number =>
    a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" });

  return [...nodes].sort((a, b) => {
    if (a.kind !== b.kind) return a.kind === "folder" ? -1 : 1;

    let delta = 0;
    switch (by) {
      case "name":
        delta = byName(a, b);
        break;
      case "kind": {
        const ea = splitName(a.name).ext.toLowerCase();
        const eb = splitName(b.name).ext.toLowerCase();
        delta = ea === eb ? byName(a, b) : ea.localeCompare(eb);
        break;
      }
      case "modified":
        delta = a.modifiedAt - b.modifiedAt || byName(a, b);
        break;
      case "size":
        delta = a.sizeBytes - b.sizeBytes || byName(a, b);
        break;
    }
    return delta * sign;
  });
}

export function useNode(id: NodeId | null): FsNode | undefined {
  return useWorkspace((s) => (id === null ? undefined : s.nodes[id]));
}

const childrenCache = new WeakMap<object, Map<string, FsNode[]>>();

function childrenOf(
  nodes: Record<NodeId, FsNode>,
  folderId: NodeId | null,
  by: SortBy,
  dir: SortDir,
): FsNode[] {
  if (folderId === null) return NONE;
  let perTree = childrenCache.get(nodes);
  if (!perTree) {
    perTree = new Map();
    childrenCache.set(nodes, perTree);
  }
  const key = folderId + KEY_SEP + by + KEY_SEP + dir;
  const cached = perTree.get(key);
  if (cached) return cached;

  const list: FsNode[] = [];
  for (const id in nodes) {
    if (nodes[id].parentId === folderId) list.push(nodes[id]);
  }
  const sorted = sortNodes(list, by, dir);
  perTree.set(key, sorted);
  return sorted;
}

/** The contents of a folder, sorted the way the canvas draws them. */
export function useChildren(folderId: NodeId | null): FsNode[] {
  return useWorkspace((s) => childrenOf(s.nodes, folderId, s.sortBy, s.sortDir));
}

export function useCurrentFolder(): FsNode | undefined {
  return useWorkspace((s) => (s.folderId === null ? undefined : s.nodes[s.folderId]));
}

const pathCache = new WeakMap<object, Map<NodeId, FsNode[]>>();

/** Root first, the folder itself last — the breadcrumb, in order. */
export function usePath(folderId: NodeId | null): FsNode[] {
  return useWorkspace((s) => {
    if (folderId === null) return NONE;
    let perTree = pathCache.get(s.nodes);
    if (!perTree) {
      perTree = new Map();
      pathCache.set(s.nodes, perTree);
    }
    const cached = perTree.get(folderId);
    if (cached) return cached;
    const chain = pathOf(s.nodes, folderId);
    perTree.set(folderId, chain);
    return chain;
  });
}

/** Subscribes to one boolean, so selecting a tile re-renders two tiles, not the grid. */
export function useIsSelected(id: NodeId): boolean {
  return useWorkspace((s) => s.selection.includes(id));
}

const selectionCache = new WeakMap<object, WeakMap<object, FsNode[]>>();

export function useSelectionNodes(): FsNode[] {
  return useWorkspace((s) => {
    let perSelection = selectionCache.get(s.nodes);
    if (!perSelection) {
      perSelection = new WeakMap();
      selectionCache.set(s.nodes, perSelection);
    }
    const cached = perSelection.get(s.selection);
    if (cached) return cached;
    const list: FsNode[] = [];
    for (const id of s.selection) {
      const node = s.nodes[id];
      if (node) list.push(node);
    }
    perSelection.set(s.selection, list);
    return list;
  });
}

export function useVaultRoot(): FsNode | undefined {
  return useWorkspace((s) => {
    if (s.vaultId === null) return undefined;
    const byConvention = s.nodes[`root_${s.vaultId}`];
    if (byConvention) return byConvention;
    for (const id in s.nodes) {
      if (s.nodes[id].parentId === null) return s.nodes[id];
    }
    return undefined;
  });
}

/**
 * What a peer id reads as when the member list cannot name it.
 *
 * A vault's history outlives its membership: a peer who left, or one whose
 * record has not arrived yet, is still the author of half the events on a node.
 * Showing 64 hex characters there tells nobody anything and showing "Unknown"
 * reads as an error, so the app admits the one true fact — somebody else did
 * this — and gives the face a neutral mark instead of initials it cannot know.
 */
export const UNKNOWN_MEMBER_NAME = "Another member";
export const UNKNOWN_MEMBER_INITIALS = "·";

/** The name and initials to draw for an actor, resolved or not. */
export function actorLabel(member: Member | null | undefined): {
  name: string;
  initials: string;
} {
  return member
    ? { name: member.name, initials: member.initials }
    : { name: UNKNOWN_MEMBER_NAME, initials: UNKNOWN_MEMBER_INITIALS };
}

export function useMember(peerId: PeerId | null | undefined): Member | undefined {
  return useWorkspace((s) =>
    peerId ? s.members.find((member) => member.peerId === peerId) : undefined,
  );
}

const onlineCache = new WeakMap<object, WeakMap<object, Member[]>>();

/**
 * Who is here right now, self last.
 *
 * Live presence outranks the member record: `member.online` is what the list
 * said when it was fetched, a `PeerPresence` is what is true this second. Self
 * goes last because the avatar row reads left-to-right as "who else is here".
 */
export function useOnlineMembers(): Member[] {
  return useWorkspace((s) => {
    let perPresence = onlineCache.get(s.members);
    if (!perPresence) {
      perPresence = new WeakMap();
      onlineCache.set(s.members, perPresence);
    }
    const cached = perPresence.get(s.presence);
    if (cached) return cached;

    const others: Member[] = [];
    const self: Member[] = [];
    for (const member of s.members) {
      const live = s.presence.find((peer) => peer.peerId === member.peerId);
      if (!(live ? live.online : member.online)) continue;
      (member.isSelf ? self : others).push(member);
    }
    const list = [...others, ...self];
    perPresence.set(s.presence, list);
    return list;
  });
}

export function usePeerPresence(peerId: PeerId): PeerPresence | undefined {
  return useWorkspace((s) => s.presence.find((peer) => peer.peerId === peerId));
}

export function useIsCreator(nodeId: NodeId | null): boolean {
  return useWorkspace((s) => {
    if (nodeId === null || s.me === null) return false;
    return s.nodes[nodeId]?.createdBy === s.me.peerId;
  });
}

/**
 * May this member change this node?
 *
 * Creators and vault admins always may. Everyone else is governed by the node's
 * access list, which lives behind `client.getAccess(vaultId, nodeId)` — an async
 * read, far too slow to gate a hover state on. TODO: resolve the real level from
 * a cached `getAccess` lookup; until then the answer is the vault default (every
 * member is an editor, see `NodeAccess.inherit`), and the Access modal stays the
 * one surface that shows the true list.
 */
export function useCanEdit(nodeId: NodeId | null): boolean {
  return useWorkspace((s) => {
    if (nodeId === null || s.me === null) return false;
    const node = s.nodes[nodeId];
    if (!node) return false;
    if (node.createdBy === s.me.peerId || s.me.role === "admin") return true;
    return true;
  });
}

const siblingCache = new WeakMap<object, Map<NodeId, string[]>>();

/** Names already taken in a folder — what a rename field checks itself against. */
export function useSiblingNames(folderId: NodeId | null): string[] {
  return useWorkspace((s) => {
    if (folderId === null) return NO_NAMES;
    let perTree = siblingCache.get(s.nodes);
    if (!perTree) {
      perTree = new Map();
      siblingCache.set(s.nodes, perTree);
    }
    const cached = perTree.get(folderId);
    if (cached) return cached;
    const names: string[] = [];
    for (const id in s.nodes) {
      if (s.nodes[id].parentId === folderId) names.push(s.nodes[id].name);
    }
    perTree.set(folderId, names);
    return names;
  });
}

/** The pop-in marker, or null once it has aged out — nothing is ever "new" twice. */
export function useJustCreated(id: NodeId): JustCreated | null {
  return useWorkspace((s) => {
    const entry = s.justCreated[id];
    if (!entry) return null;
    return Date.now() - entry.at > JUST_CREATED_MS ? null : entry;
  });
}

