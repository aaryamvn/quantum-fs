/**
 * The in-memory file system the whole workspace is developed against.
 *
 * Outside Tauri there is no daemon, so this class *is* the vault: one mutable
 * tree per vault, plus the members, access lists, history, recents and presence
 * that hang off it. It exists so the UI can be built and screenshotted at full
 * fidelity, and — more importantly — so the behavior the UI depends on is
 * written down once, in prose the Rust mirror can be checked against
 * (docs/decisions/client-workspace.md).
 *
 * Three rules shape everything below.
 *
 * 1. **Mutations are synchronous.** No latency is simulated. A rename that waits
 *    even 80ms feels like a web app; Finder's directness is the product, so the
 *    state is consistent and the events are out before the promise resolves.
 * 2. **Every mutation emits deltas.** Callers get the affected nodes back *and*
 *    an `fs-changed` event fires, so a local edit and a peer's edit arrive
 *    through exactly one code path in the store. Ancestors whose `sizeBytes`,
 *    `childCount` or `modifiedAt` moved are in the delta too, because a
 *    breadcrumb showing a stale size is the kind of thing nobody notices until
 *    the demo.
 * 3. **Nothing shared escapes.** Reads and event payloads are `structuredClone`d.
 *    A store that mutates what it was handed would otherwise corrupt the engine
 *    invisibly, and that bug is unfindable.
 *
 * The only unusual method is {@link FsEngine.simulate}: a side door for the
 * scripted demo, kept off `BackendClient` so the real seam stays honest.
 */

import { iconCategoryForName } from "@/components/icons/registry";
import { isDescendant, splitName, uniqueName, validateName } from "@/lib/path";
import { searchNodes } from "@/lib/search";
import type { SearchContext } from "@/lib/search";

import type {
  AskAgentInput,
  CreateNodeInput,
  DeleteNodesInput,
  DuplicateNodesInput,
  MoveNodesInput,
  PresenceInput,
  RenameNodeInput,
  SetAccessInput,
  SetNodeColorInput,
  VaultMetaPatch,
} from "../client";
import type {
  AccessEntry,
  AgentReply,
  BackendEvent,
  FolderColor,
  FsChange,
  FsNode,
  HistoryEvent,
  HistoryKind,
  JoinCode,
  Member,
  MemberRole,
  NodeAccess,
  NodeId,
  OrchestrationServer,
  PeerId,
  PeerPresence,
  Recent,
  RemoteOp,
  SearchHit,
  SearchQuery,
  VaultId,
  VaultMeta,
} from "../types";

import type { FsSeed, SeedRecent } from "./seedLoader";

/** How often a simulated transfer reports progress. Matches a comfortable UI tick. */
const DOWNLOAD_TICK_MS = 100;
/** Even a tiny file takes this long, so the progress ring is legible rather than a flash. */
const DOWNLOAD_MIN_MS = 1200;
/** And even a 40 GB file finishes within a demo beat. */
const DOWNLOAD_MAX_MS = 4200;
/** Pretend LAN throughput: 50 MiB/s. Only used to make big files feel bigger. */
const DOWNLOAD_BYTES_PER_SECOND = 52_428_800;

/** An imported file is invented, so its size is drawn from a plausible range: 64 KiB… */
const IMPORT_MIN_BYTES = 65_536;
/** …up to 24 MiB, which is big enough to look like a real asset and small enough to fit a quota. */
const IMPORT_MAX_BYTES = 25_165_824;
/** With no paths to name them, the chooser "returns" this many files. */
const IMPORT_MAX_FAKE_FILES = 3;

/** Presence is pointer traffic; one packet per frame is plenty and 30ms is under a frame. */
const PRESENCE_THROTTLE_MS = 30;

/** Long enough for the agent bar's typing indicator to read as thinking, short enough to not annoy. */
const AGENT_LATENCY_MS = 650;

/** The sidebar shows a short list; more than this and it stops being "recent". */
const MAX_RECENTS = 8;

/** Same ceiling the home screen's create-vault field enforces. */
const MAX_VAULT_NAME = 40;

/** Crockford-ish base32 minus the ambiguous glyphs: what a join code is spelled with. */
const JOIN_CODE_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const JOIN_CODE_LENGTH = 6;

/**
 * The demo's side door onto the mock.
 *
 * `withDemo(client)` drives real client methods for everything it can, but two
 * things have no honest public equivalent — other people's cursors, and the
 * "this is about to happen" hint that lets a remote move animate before the tree
 * changes. They live here so `BackendClient` never grows a method the daemon
 * could not implement.
 */
export interface MockSimulation {
  /** Replace every non-self presence in a vault and publish it. */
  presence(vaultId: VaultId, peers: PeerPresence[]): void;
  /** Announce a peer's op so the canvas can animate the flight before the delta lands. */
  remoteOp(op: RemoteOp): void;
  /** Tell the workspace the loop is starting over, so it can crossfade. */
  reset(vaultId: VaultId): void;
}

/** `Math.min(Math.max(...))`, named, because the download curve reads better with it. */
function clamp(value: number, min: number, max: number): number {
  return value < min ? min : value > max ? max : value;
}

/**
 * mulberry32 — 32 bits of state, uniform enough for a join code and, unlike
 * `Math.random`, reproducible from a seed so screenshots of the settings modal
 * are stable until something actually rotates the code.
 */
function makeRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export class FsEngine {
  private readonly emit: (e: BackendEvent) => void;
  /** The live servers array owned by the mock client; vault rows here must stay in step. */
  private readonly servers: OrchestrationServer[];

  private readonly nodes = new Map<NodeId, FsNode>();
  /** Only *explicit* lists. An absent key means "inherit", which is the common case. */
  private readonly access = new Map<NodeId, AccessEntry[]>();
  private readonly vaults = new Map<VaultId, VaultMeta>();
  private readonly members = new Map<VaultId, Member[]>();
  private readonly presence = new Map<VaultId, Map<PeerId, PeerPresence>>();
  private readonly previews: Record<NodeId, string>;
  private history: HistoryEvent[];
  private recents: SeedRecent[];

  private readonly downloads = new Map<NodeId, ReturnType<typeof setInterval>>();
  private readonly presenceTimers = new Map<VaultId, ReturnType<typeof setTimeout>>();
  private readonly presenceLastAt = new Map<VaultId, number>();

  /** Per-vault counter behind the `n_<vault>_c<n>` ids new nodes get. */
  private readonly nodeCounters = new Map<VaultId, number>();
  private historyCounter = 0;
  private agentCounter = 0;
  private readonly random: () => number;

  readonly self: PeerId;

  /**
   * Everything the demo may do that the public seam cannot express.
   *
   * Arrow properties, so `getSimulation(client).presence(...)` keeps working when
   * the object is torn off the client.
   */
  readonly simulate: MockSimulation = {
    presence: (vaultId: VaultId, peers: PeerPresence[]) => {
      const byPeer = this.presenceMap(vaultId);
      for (const [peerId] of byPeer) {
        if (peerId !== this.self) byPeer.delete(peerId);
      }
      for (const peer of peers) {
        if (peer.peerId === this.self) continue;
        byPeer.set(peer.peerId, structuredClone(peer));
      }
      this.emitPresence(vaultId);
    },
    remoteOp: (op: RemoteOp) => {
      this.emit({ type: "remote-op", op: structuredClone(op) });
    },
    reset: (vaultId: VaultId) => {
      this.emit({ type: "demo-reset", vaultId });
    },
  };

  /**
   * @param servers the mock's live server list — membership counts, vault names
   * and vault removal have to be reflected there or the home screen goes stale.
   */
  constructor(seed: FsSeed, emit: (e: BackendEvent) => void, servers: OrchestrationServer[]) {
    this.emit = emit;
    this.servers = servers;
    this.self = seed.self;
    this.previews = seed.previews;
    this.history = seed.history;
    this.recents = seed.recents;
    this.random = makeRandom(seed.generatedAt);

    for (const node of seed.nodes) this.nodes.set(node.id, node);
    for (const entry of seed.access) {
      if (!entry.inherit) this.access.set(entry.nodeId, entry.entries);
    }
    for (const [vaultId, meta] of Object.entries(seed.vaults)) this.vaults.set(vaultId, meta);
    for (const [vaultId, list] of Object.entries(seed.members)) this.members.set(vaultId, list);

    // The highest seeded `h_<n>` decides where appended history starts, so ids stay unique
    // and sortable even after the fixture grows.
    for (const event of seed.history) {
      const n = Number(event.id.replace(/^h_/, ""));
      if (Number.isFinite(n) && n > this.historyCounter) this.historyCounter = n;
    }

    // Everyone the fixture says is online is already standing in the root of their vault,
    // so the avatar row and the folder badges are populated on first paint.
    for (const [vaultId, list] of this.members) {
      const root = this.rootOf(vaultId);
      const byPeer = new Map<PeerId, PeerPresence>();
      for (const member of list) {
        if (!member.online) continue;
        byPeer.set(member.peerId, {
          peerId: member.peerId,
          online: true,
          idle: false,
          folderId: root ? root.id : null,
          cursor: null,
          hoveringNodeId: null,
          draggingNodeIds: [],
          updatedAt: seed.generatedAt,
        });
      }
      this.presence.set(vaultId, byPeer);
    }
  }

  // ---------------------------------------------------------------- internals

  private node(nodeId: NodeId): FsNode {
    const node = this.nodes.get(nodeId);
    if (!node) throw new Error(`Unknown node: ${nodeId}`);
    return node;
  }

  private folder(nodeId: NodeId): FsNode {
    const node = this.node(nodeId);
    if (node.kind !== "folder") throw new Error("Can't put things inside a file");
    return node;
  }

  private meta(vaultId: VaultId): VaultMeta {
    const meta = this.vaults.get(vaultId);
    if (!meta) throw new Error(`Unknown vault: ${vaultId}`);
    return meta;
  }

  private rootOf(vaultId: VaultId): FsNode | null {
    for (const node of this.nodes.values()) {
      if (node.vaultId === vaultId && node.parentId === null) return node;
    }
    return null;
  }

  /** A fresh array every time: callers insert nodes while walking these. */
  private childrenOf(parentId: NodeId): FsNode[] {
    const out: FsNode[] = [];
    for (const node of this.nodes.values()) {
      if (node.parentId === parentId) out.push(node);
    }
    return out;
  }

  /** Every node of the vault keyed by id — what `isDescendant` and search want. */
  private nodeRecord(): Record<NodeId, FsNode> {
    const record: Record<NodeId, FsNode> = Object.create(null) as Record<NodeId, FsNode>;
    for (const [id, node] of this.nodes) record[id] = node;
    return record;
  }

  /** Ancestors of `nodeId`, nearest first, root last. Cycle-safe like `pathOf`. */
  private ancestorsOf(nodeId: NodeId): FsNode[] {
    const chain: FsNode[] = [];
    const seen = new Set<NodeId>([nodeId]);
    let parentId = this.nodes.get(nodeId)?.parentId ?? null;
    while (parentId && !seen.has(parentId)) {
      seen.add(parentId);
      const parent = this.nodes.get(parentId);
      if (!parent) break;
      chain.push(parent);
      parentId = parent.parentId;
    }
    return chain;
  }

  private subtreeIds(rootId: NodeId): NodeId[] {
    const out: NodeId[] = [];
    const seen = new Set<NodeId>();
    const stack: NodeId[] = [rootId];
    while (stack.length > 0) {
      const id = stack.pop() as NodeId;
      if (seen.has(id)) continue;
      seen.add(id);
      out.push(id);
      for (const child of this.childrenOf(id)) stack.push(child.id);
    }
    return out;
  }

  private nextNodeId(vaultId: VaultId): NodeId {
    const n = (this.nodeCounters.get(vaultId) ?? 0) + 1;
    this.nodeCounters.set(vaultId, n);
    return `n_${vaultId.replace(/^vlt_/, "")}_c${n}`;
  }

  /**
   * Reject a name that a file system would, before anything is written.
   *
   * The collision message names the *existing* sibling's kind, because that is
   * the thing in the user's way: dropping a folder called "Brand" onto a file
   * called "Brand" should say a file is there.
   */
  private assertNameFree(parentId: NodeId, name: string, excludeId?: NodeId): void {
    const message = validateName(name);
    if (message) throw new Error(message);
    const lower = name.toLowerCase();
    for (const sibling of this.childrenOf(parentId)) {
      if (sibling.id === excludeId) continue;
      if (sibling.name.toLowerCase() !== lower) continue;
      throw new Error(
        sibling.kind === "file"
          ? "A file with that name already exists"
          : "A folder with that name already exists",
      );
    }
  }

  /** Roll a size delta up the ancestor chain; every node it touches goes in `touched`. */
  private bumpSizes(fromParentId: NodeId | null, delta: number, touched: Set<NodeId>): void {
    let parentId = fromParentId;
    const seen = new Set<NodeId>();
    while (parentId && !seen.has(parentId)) {
      seen.add(parentId);
      const parent = this.nodes.get(parentId);
      if (!parent) break;
      if (delta !== 0) parent.sizeBytes += delta;
      touched.add(parent.id);
      parentId = parent.parentId;
    }
  }

  private touch(node: FsNode, actor: PeerId, at: number): void {
    node.modifiedAt = at;
    node.modifiedBy = actor;
  }

  private record(
    vaultId: VaultId,
    nodeId: NodeId,
    kind: HistoryKind,
    by: PeerId,
    at: number,
    from: string | null,
    to: string | null,
    summary: string,
  ): void {
    this.historyCounter += 1;
    this.history.push({
      id: `h_${this.historyCounter}`,
      vaultId,
      nodeId,
      kind,
      at,
      by,
      from,
      to,
      summary,
    });
  }

  /** Emit the upserts for a set of ids, dropping any that vanished in the same op. */
  private emitUpserts(vaultId: VaultId, ids: Iterable<NodeId>, actor: PeerId): void {
    const changes: FsChange[] = [];
    for (const id of ids) {
      const node = this.nodes.get(id);
      if (node) changes.push({ kind: "upsert", node: structuredClone(node) });
    }
    if (changes.length > 0) this.emit({ type: "fs-changed", vaultId, changes, actor });
  }

  private presenceMap(vaultId: VaultId): Map<PeerId, PeerPresence> {
    let byPeer = this.presence.get(vaultId);
    if (!byPeer) {
      byPeer = new Map<PeerId, PeerPresence>();
      this.presence.set(vaultId, byPeer);
    }
    return byPeer;
  }

  /**
   * The vault's presence as the rest of the app may see it.
   *
   * The local cursor is blanked on the way out. A client that echoed its own
   * pointer back would draw a second, laggier cursor chasing the real one.
   */
  private presenceList(vaultId: VaultId): PeerPresence[] {
    const out: PeerPresence[] = [];
    for (const peer of this.presenceMap(vaultId).values()) {
      const copy = structuredClone(peer);
      if (copy.peerId === this.self) copy.cursor = null;
      out.push(copy);
    }
    return out;
  }

  /** Coalesce presence bursts: one packet now, one trailing packet for the rest. */
  private emitPresence(vaultId: VaultId): void {
    const now = Date.now();
    const last = this.presenceLastAt.get(vaultId) ?? 0;
    const wait = PRESENCE_THROTTLE_MS - (now - last);
    if (wait <= 0) {
      this.presenceLastAt.set(vaultId, now);
      this.emit({ type: "presence", vaultId, peers: this.presenceList(vaultId) });
      return;
    }
    if (this.presenceTimers.has(vaultId)) return;
    this.presenceTimers.set(
      vaultId,
      setTimeout(() => {
        this.presenceTimers.delete(vaultId);
        this.presenceLastAt.set(vaultId, Date.now());
        this.emit({ type: "presence", vaultId, peers: this.presenceList(vaultId) });
      }, wait),
    );
  }

  /** The `Vault` row inside the servers list, if the vault is still on one. */
  private vaultRow(vaultId: VaultId): { server: OrchestrationServer; index: number } | null {
    for (const server of this.servers) {
      const index = server.vaults.findIndex((v) => v.id === vaultId);
      if (index !== -1) return { server, index };
    }
    return null;
  }

  private syncMemberCount(vaultId: VaultId): void {
    const row = this.vaultRow(vaultId);
    if (!row) return;
    row.server.vaults[row.index].memberCount = (this.members.get(vaultId) ?? []).length;
  }

  private memberOf(vaultId: VaultId, peerId: PeerId): Member {
    const member = (this.members.get(vaultId) ?? []).find((m) => m.peerId === peerId);
    if (!member) throw new Error("That member is not in this vault");
    return member;
  }

  private cancelDownload(nodeId: NodeId): void {
    const handle = this.downloads.get(nodeId);
    if (handle === undefined) return;
    clearInterval(handle);
    this.downloads.delete(nodeId);
  }

  // ------------------------------------------------------------------ reading

  me(): Member {
    const primary = (this.members.get("vlt_1_1") ?? []).find((m) => m.isSelf);
    if (primary) return structuredClone(primary);
    for (const list of this.members.values()) {
      const found = list.find((m) => m.isSelf);
      if (found) return structuredClone(found);
    }
    throw new Error("No local member in the seed");
  }

  listTree(vaultId: VaultId): FsNode[] {
    const out: FsNode[] = [];
    for (const node of this.nodes.values()) {
      if (node.vaultId === vaultId) out.push(structuredClone(node));
    }
    return out;
  }

  // ---------------------------------------------------------------- mutations

  createNode(input: CreateNodeInput): FsNode {
    const actor = input.actor ?? this.self;
    const parent = this.folder(input.parentId);
    const name = input.name.trim();
    this.assertNameFree(parent.id, name);

    const now = Date.now();
    const node: FsNode = {
      id: this.nextNodeId(input.vaultId),
      vaultId: input.vaultId,
      parentId: parent.id,
      kind: input.kind,
      name,
      sizeBytes: 0,
      createdAt: now,
      modifiedAt: now,
      createdBy: actor,
      modifiedBy: actor,
      color: null,
      availability: "local",
      progress: null,
      holders: input.kind === "file" ? [actor] : [],
      childCount: 0,
    };
    this.nodes.set(node.id, node);
    parent.childCount += 1;
    this.touch(parent, actor, now);

    this.record(input.vaultId, node.id, "created", actor, now, null, null, "created");
    // A new node is empty, so no ancestor's size moved: the parent is the only
    // other row on screen that changed.
    this.emitUpserts(input.vaultId, [node.id, parent.id], actor);
    return structuredClone(node);
  }

  renameNode(input: RenameNodeInput): FsNode {
    const actor = input.actor ?? this.self;
    const node = this.node(input.nodeId);
    const name = input.name.trim();
    const previous = node.name;
    if (node.parentId) this.assertNameFree(node.parentId, name, node.id);
    else {
      const message = validateName(name);
      if (message) throw new Error(message);
    }

    const now = Date.now();
    node.name = name;
    this.touch(node, actor, now);
    const parent = node.parentId ? this.nodes.get(node.parentId) : undefined;
    if (parent) this.touch(parent, actor, now);

    this.record(
      input.vaultId,
      node.id,
      "renamed",
      actor,
      now,
      previous,
      name,
      `renamed from ${previous}`,
    );
    this.emitUpserts(input.vaultId, parent ? [node.id, parent.id] : [node.id], actor);
    return structuredClone(node);
  }

  /**
   * Reparent a selection into one folder.
   *
   * Validated as a whole before anything moves: a drag of five tiles where the
   * third is the destination's own parent must fail with nothing half-applied.
   * Nodes already in the destination are silently skipped rather than rejected,
   * because dropping a mixed selection onto the folder some of it already lives
   * in is a normal gesture, not a mistake.
   */
  moveNodes(input: MoveNodesInput): FsNode[] {
    const actor = input.actor ?? this.self;
    const target = this.folder(input.toParentId);
    const record = this.nodeRecord();

    const moving: FsNode[] = [];
    for (const nodeId of input.nodeIds) {
      const node = this.node(nodeId);
      if (node.id === target.id || isDescendant(record, target.id, node.id)) {
        throw new Error("Can't move a folder into itself");
      }
      if (node.parentId === target.id) continue;
      moving.push(node);
    }
    if (moving.length === 0) return [];
    for (const node of moving) this.assertNameFree(target.id, node.name, node.id);

    const now = Date.now();
    const touched = new Set<NodeId>();
    const moved: FsNode[] = [];
    for (const node of moving) {
      const fromParent = node.parentId ? this.nodes.get(node.parentId) : undefined;
      if (fromParent) {
        fromParent.childCount -= 1;
        this.touch(fromParent, actor, now);
        this.bumpSizes(fromParent.id, -node.sizeBytes, touched);
      }
      node.parentId = target.id;
      this.touch(node, actor, now);
      target.childCount += 1;
      this.bumpSizes(target.id, node.sizeBytes, touched);

      this.record(
        input.vaultId,
        node.id,
        "moved",
        actor,
        now,
        fromParent ? fromParent.name : null,
        target.name,
        fromParent ? `moved from ${fromParent.name} to ${target.name}` : `moved to ${target.name}`,
      );
      touched.add(node.id);
      moved.push(node);
    }
    this.touch(target, actor, now);
    touched.add(target.id);

    this.emitUpserts(input.vaultId, touched, actor);
    return moved.map((node) => structuredClone(node));
  }

  deleteNodes(input: DeleteNodesInput): void {
    const actor = input.actor ?? this.self;
    const now = Date.now();
    const removed: NodeId[] = [];
    const touched = new Set<NodeId>();

    for (const nodeId of input.nodeIds) {
      const node = this.nodes.get(nodeId);
      if (!node) continue;
      if (node.parentId === null) throw new Error("Can't delete the vault root");
      const parent = this.nodes.get(node.parentId);

      for (const id of this.subtreeIds(node.id)) {
        this.cancelDownload(id);
        this.nodes.delete(id);
        this.access.delete(id);
        delete this.previews[id];
        removed.push(id);
      }
      if (parent) {
        parent.childCount -= 1;
        this.touch(parent, actor, now);
        this.bumpSizes(parent.id, -node.sizeBytes, touched);
        this.record(
          input.vaultId,
          parent.id,
          "deleted",
          actor,
          now,
          node.name,
          null,
          `deleted ${node.name}`,
        );
      }
    }
    if (removed.length === 0) return;

    const gone = new Set(removed);
    const changes: FsChange[] = removed.map((nodeId) => ({ kind: "remove", nodeId }));
    for (const id of touched) {
      if (gone.has(id)) continue;
      const node = this.nodes.get(id);
      if (node) changes.push({ kind: "upsert", node: structuredClone(node) });
    }
    this.emit({ type: "fs-changed", vaultId: input.vaultId, changes, actor });

    // A recents row pointing at a deleted node is a dead link; drop it now rather
    // than letting `listRecents` quietly shrink the list on the next read.
    const before = this.recents.length;
    this.recents = this.recents.filter((entry) => !gone.has(entry.nodeId));
    if (this.recents.length !== before) this.emit({ type: "recents-changed" });
  }

  /**
   * Copy a selection, subtrees and all.
   *
   * Only the top of each copied tree is renamed — the children inside keep their
   * names, because they are unique within their own new folder. That is what
   * Finder does, and it is why "Brand copy" does not contain "logo copy.svg".
   */
  duplicateNodes(input: DuplicateNodesInput): FsNode[] {
    const actor = input.actor ?? this.self;
    const now = Date.now();
    const touched = new Set<NodeId>();
    const tops: FsNode[] = [];

    for (const nodeId of input.nodeIds) {
      const source = this.node(nodeId);
      const parentId = input.toParentId ?? source.parentId;
      if (!parentId) throw new Error("Can't duplicate the vault root");
      const parent = this.folder(parentId);

      const name = uniqueName(
        this.childrenOf(parent.id).map((child) => child.name),
        source.name,
      );
      const copy = this.copySubtree(source, parent.id, name, actor, now, touched);

      parent.childCount += 1;
      this.touch(parent, actor, now);
      this.bumpSizes(parent.id, copy.sizeBytes, touched);

      this.record(
        input.vaultId,
        copy.id,
        "duplicated",
        actor,
        now,
        source.name,
        copy.name,
        `duplicated from ${source.name}`,
      );
      tops.push(copy);
    }

    this.emitUpserts(input.vaultId, touched, actor);
    return tops.map((node) => structuredClone(node));
  }

  /** Depth-first copy; every new node lands in `touched` so one event covers the tree. */
  private copySubtree(
    source: FsNode,
    parentId: NodeId,
    name: string,
    actor: PeerId,
    now: number,
    touched: Set<NodeId>,
  ): FsNode {
    const copy: FsNode = {
      ...structuredClone(source),
      id: this.nextNodeId(source.vaultId),
      parentId,
      name,
      createdAt: now,
      modifiedAt: now,
      createdBy: actor,
      modifiedBy: actor,
    };
    this.nodes.set(copy.id, copy);
    touched.add(copy.id);
    for (const child of this.childrenOf(source.id)) {
      this.copySubtree(child, copy.id, child.name, actor, now, touched);
    }
    return copy;
  }

  setNodeColor(input: SetNodeColorInput): FsNode {
    const actor = input.actor ?? this.self;
    const node = this.node(input.nodeId);
    if (node.kind !== "folder") throw new Error("Only folders have colors");

    const now = Date.now();
    const color: FolderColor | null = input.color;
    node.color = color;
    this.touch(node, actor, now);

    this.record(
      input.vaultId,
      node.id,
      "colored",
      actor,
      now,
      null,
      color,
      color ? `set color to ${color}` : "cleared the color",
    );
    this.emitUpserts(input.vaultId, [node.id], actor);
    return structuredClone(node);
  }

  /**
   * Pull a remote file down, with a transfer that takes plausible time.
   *
   * The curve is deliberately compressed: size matters (a 4 GB video should not
   * finish like a text file) but nothing takes longer than a demo beat, so the
   * ceiling is four seconds. Ticks emit upserts rather than a bespoke progress
   * event, so the tile, the inspector and the sidebar all update from the same
   * delta stream they already listen to.
   */
  requestDownload(vaultId: VaultId, nodeId: NodeId): void {
    const node = this.node(nodeId);
    if (node.availability !== "remote") return;

    node.availability = "downloading";
    node.progress = 0;
    this.emitUpserts(vaultId, [node.id], this.self);

    const duration = clamp(
      DOWNLOAD_MIN_MS + (node.sizeBytes / DOWNLOAD_BYTES_PER_SECOND) * 1000,
      DOWNLOAD_MIN_MS,
      DOWNLOAD_MAX_MS,
    );
    const step = DOWNLOAD_TICK_MS / duration;

    this.cancelDownload(node.id);
    const handle = setInterval(() => {
      const live = this.nodes.get(node.id);
      if (!live || live.availability !== "downloading") {
        this.cancelDownload(node.id);
        return;
      }
      const next = (live.progress ?? 0) + step;
      if (next < 1) {
        live.progress = next;
        this.emitUpserts(vaultId, [live.id], this.self);
        return;
      }
      this.cancelDownload(live.id);
      live.availability = "local";
      live.progress = null;
      if (!live.holders.includes(this.self)) live.holders = [...live.holders, this.self];
      this.record(
        vaultId,
        live.id,
        "downloaded",
        this.self,
        Date.now(),
        null,
        null,
        "downloaded to this Mac",
      );
      this.emitUpserts(vaultId, [live.id], this.self);
    }, DOWNLOAD_TICK_MS);
    this.downloads.set(node.id, handle);
  }

  /**
   * "Open in the default application", as far as a browser can honestly go.
   *
   * There is no OS to hand the bytes to, so the observable part is the part the UI
   * reacts to: a remote file starts the same simulated pull `requestDownload` runs
   * (the tile shows its ring), and either way the node becomes the newest recent.
   * Returns as soon as that is set in motion, exactly like the real bridge, which
   * resolves when the OS was *asked* to open the file.
   */
  openFile(vaultId: VaultId, nodeId: NodeId): void {
    const node = this.node(nodeId);
    if (node.kind === "file" && node.availability === "remote") {
      this.requestDownload(vaultId, nodeId);
    }
    this.touchRecent(vaultId, nodeId);
  }

  /**
   * Invent the files a native chooser or a drop would have produced.
   *
   * With `paths` the basenames are honest — a drop really did name those files — and
   * each one lands under a Finder-unique name rather than rejecting on a collision,
   * because a drop of a file that is already there should still add a copy. Without
   * them there was no chooser to show, so one to three placeholders stand in. Sizes
   * come from the engine's seeded RNG, so screenshots stay stable.
   */
  importFiles(vaultId: VaultId, parentId: NodeId, paths?: string[]): FsNode[] {
    const parent = this.folder(parentId);
    const now = Date.now();

    const basenames = (paths ?? [])
      .map((path) => path.split(/[/\\]/).pop() ?? "")
      .map((name) => name.trim())
      .filter((name) => name.length > 0);
    const count =
      basenames.length > 0
        ? basenames.length
        : 1 + Math.floor(this.random() * IMPORT_MAX_FAKE_FILES);

    const created: FsNode[] = [];
    const touched = new Set<NodeId>([parent.id]);
    for (let i = 0; i < count; i += 1) {
      const desired = basenames[i] ?? `Imported file ${i + 1}.pdf`;
      const name = uniqueName(
        this.childrenOf(parent.id).map((child) => child.name),
        desired,
      );
      const sizeBytes =
        IMPORT_MIN_BYTES + Math.round(this.random() * (IMPORT_MAX_BYTES - IMPORT_MIN_BYTES));
      const node: FsNode = {
        id: this.nextNodeId(vaultId),
        vaultId,
        parentId: parent.id,
        kind: "file",
        name,
        sizeBytes,
        createdAt: now,
        modifiedAt: now,
        createdBy: this.self,
        modifiedBy: this.self,
        color: null,
        // The bytes came from this machine, so this peer is the only holder.
        availability: "local",
        progress: null,
        holders: [this.self],
        childCount: 0,
      };
      this.nodes.set(node.id, node);
      parent.childCount += 1;
      // Rolls the new size up the whole ancestor chain, `parent` included.
      this.bumpSizes(parent.id, sizeBytes, touched);
      this.record(vaultId, node.id, "created", this.self, now, null, null, "imported from this Mac");
      touched.add(node.id);
      created.push(structuredClone(node));
    }

    this.touch(parent, this.self, now);
    this.emitUpserts(vaultId, touched, this.self);
    return created;
  }

  readTextPreview(nodeId: NodeId, maxBytes: number): string | null {
    const body = this.previews[nodeId];
    if (body === undefined) return null;
    return body.slice(0, Math.max(0, maxBytes));
  }

  // -------------------------------------------------------------------- access

  /**
   * The effective permission list for a node.
   *
   * Inheritance is resolved here rather than in the UI so the inspector can show
   * "inherited from Brand" without walking the tree itself — and so the Rust
   * mirror has exactly one rule to copy: the nearest ancestor with an explicit
   * list wins, and an empty answer means the vault default (every member edits).
   */
  getAccess(nodeId: NodeId): NodeAccess {
    const own = this.access.get(nodeId);
    if (own) return { nodeId, inherit: false, entries: structuredClone(own) };
    for (const ancestor of this.ancestorsOf(nodeId)) {
      const inherited = this.access.get(ancestor.id);
      if (inherited) return { nodeId, inherit: true, entries: structuredClone(inherited) };
    }
    return { nodeId, inherit: true, entries: [] };
  }

  setAccess(input: SetAccessInput): NodeAccess {
    const node = this.node(input.nodeId);
    const meta = this.meta(input.vaultId);
    const now = Date.now();

    if (input.inherit) {
      this.access.delete(node.id);
      this.record(
        input.vaultId,
        node.id,
        "access",
        this.self,
        now,
        null,
        null,
        "reset access to inherited",
      );
    } else {
      // The creator is an editor by construction and can never be removed, so listing
      // them would offer a control that does nothing.
      const seen = new Set<PeerId>();
      const entries: AccessEntry[] = [];
      for (const entry of input.entries) {
        if (entry.peerId === meta.createdBy) continue;
        if (seen.has(entry.peerId)) continue;
        seen.add(entry.peerId);
        entries.push({ peerId: entry.peerId, level: entry.level });
      }
      this.access.set(node.id, entries);
      this.record(
        input.vaultId,
        node.id,
        "access",
        this.self,
        now,
        null,
        null,
        `changed access for ${entries.length} ${entries.length === 1 ? "member" : "members"}`,
      );
    }

    // `modifiedAt` deliberately does not move — a permission change is not an edit —
    // but the inspector still has to re-read, so the node goes out as an upsert.
    this.emitUpserts(input.vaultId, [node.id], this.self);
    return this.getAccess(node.id);
  }

  // ------------------------------------------------------------------- history

  /**
   * A folder's history is its own events plus its children's.
   *
   * Opening History on a folder and seeing nothing because the edits all happened
   * to files inside it would be useless; one level down is where the interesting
   * changes live, and deeper than that belongs to the child's own panel.
   */
  getHistory(vaultId: VaultId, nodeId: NodeId): HistoryEvent[] {
    const out: HistoryEvent[] = [];
    for (const event of this.history) {
      if (event.vaultId !== vaultId) continue;
      if (event.nodeId === nodeId) {
        out.push(event);
        continue;
      }
      const subject = this.nodes.get(event.nodeId);
      if (subject && subject.parentId === nodeId) out.push(event);
    }
    out.sort((a, b) => b.at - a.at);
    return structuredClone(out);
  }

  // ------------------------------------------------------------------- recents

  listRecents(): Recent[] {
    const out: Recent[] = [];
    for (const entry of this.recents) {
      const node = this.nodes.get(entry.nodeId);
      if (!node) continue;
      out.push({
        node: structuredClone(node),
        vaultName: this.vaults.get(entry.vaultId)?.name ?? "",
        at: entry.at,
      });
    }
    return out;
  }

  touchRecent(vaultId: VaultId, nodeId: NodeId): void {
    this.recents = [
      { vaultId, nodeId, at: Date.now() },
      ...this.recents.filter((entry) => entry.nodeId !== nodeId),
    ].slice(0, MAX_RECENTS);
    this.emit({ type: "recents-changed" });
  }

  // -------------------------------------------------------------------- search

  /**
   * Rank the tree against a query.
   *
   * The raw text goes to `searchNodes` untouched, because the inline grammar
   * (`ext:`, `by:`, `modified:`) is part of what the user typed and only that
   * module knows it. The structured fields on {@link SearchQuery} are a *second*
   * filter applied afterwards — they come from chips and the scope the modal was
   * opened in, so they narrow the typed query rather than competing with it.
   */
  search(query: SearchQuery): SearchHit[] {
    const limit = Math.max(0, query.limit);
    if (limit === 0) return [];

    const nodesByVault: Record<VaultId, Record<NodeId, FsNode>> = {};
    const vaultNames: Record<VaultId, string> = {};
    const memberNames: Record<PeerId, string> = {};
    for (const [vaultId, list] of this.members) {
      if (!list.some((member) => member.isSelf)) continue;
      nodesByVault[vaultId] = Object.create(null) as Record<NodeId, FsNode>;
      vaultNames[vaultId] = this.vaults.get(vaultId)?.name ?? "";
      for (const member of list) memberNames[member.peerId] = member.name;
    }
    for (const node of this.nodes.values()) {
      const bucket = nodesByVault[node.vaultId];
      if (bucket) bucket[node.id] = node;
    }

    const ctx: SearchContext = {
      nodesByVault,
      vaultNames,
      memberNames,
      categoryOf: iconCategoryForName,
      now: Date.now(),
    };
    const ranked = searchNodes(query.text, ctx, {
      vaultId: query.vaultId,
      limit: Number.MAX_SAFE_INTEGER,
    });

    const exts = query.exts
      ? query.exts.map((ext) => (ext.startsWith(".") ? ext.slice(1) : ext).toLowerCase())
      : null;
    const record = query.inFolderId ? this.nodeRecord() : null;

    const hits: SearchHit[] = [];
    for (const hit of ranked) {
      const node = this.nodes.get(hit.node.id);
      if (!node) continue;
      if (query.kinds && !query.kinds.includes(node.kind)) continue;
      if (exts && !exts.includes(splitName(node.name).ext.toLowerCase())) continue;
      if (query.availability && node.availability !== query.availability) continue;
      if (query.modifiedAfter !== null && node.modifiedAt < query.modifiedAfter) continue;
      if (query.by && node.createdBy !== query.by && node.modifiedBy !== query.by) continue;
      if (record && query.inFolderId && !isDescendant(record, node.id, query.inFolderId)) continue;

      hits.push({
        node: structuredClone(node),
        vaultName: hit.vaultName,
        path: [...hit.path],
        score: hit.score,
        matches: hit.matches.map((range) => [range[0], range[1]] as [number, number]),
      });
      if (hits.length === limit) break;
    }
    return hits;
  }

  // --------------------------------------------------------------------- vault

  getVaultMeta(vaultId: VaultId): VaultMeta {
    return structuredClone(this.meta(vaultId));
  }

  /**
   * Apply a settings patch.
   *
   * A vault's name is denormalized in two other places the user is looking at —
   * the home screen's card and the tree's root folder — so renaming here fans out
   * to both rather than leaving the workspace and the sidebar disagreeing.
   */
  updateVaultMeta(vaultId: VaultId, patch: VaultMetaPatch): VaultMeta {
    const meta = this.meta(vaultId);
    let renamed = false;
    let rehomed = false;

    if (patch.name !== undefined) {
      const name = patch.name.trim();
      if (name.length === 0 || name.length > MAX_VAULT_NAME) throw new Error("Invalid vault name");
      renamed = name !== meta.name;
      meta.name = name;
    }
    if (patch.description !== undefined) meta.description = patch.description;
    if (patch.autoCleanup !== undefined) meta.autoCleanup = patch.autoCleanup;
    if (patch.cleanupThresholdPct !== undefined) {
      meta.cleanupThresholdPct = clamp(Math.round(patch.cleanupThresholdPct), 1, 100);
    }
    if (patch.serverId !== undefined && patch.serverId !== meta.serverId) {
      const destination = this.servers.find((server) => server.id === patch.serverId);
      if (!destination) throw new Error(`Unknown server: ${patch.serverId}`);
      const row = this.vaultRow(vaultId);
      if (row) {
        const [vault] = row.server.vaults.splice(row.index, 1);
        vault.serverId = destination.id;
        destination.vaults.push(vault);
      }
      meta.serverId = destination.id;
      rehomed = true;
    }

    if (renamed) {
      const root = this.rootOf(vaultId);
      if (root) {
        root.name = meta.name;
        this.touch(root, this.self, Date.now());
        this.emitUpserts(vaultId, [root.id], this.self);
      }
      const row = this.vaultRow(vaultId);
      if (row) row.server.vaults[row.index].name = meta.name;
    }

    this.emit({ type: "vault-changed", vaultId });
    if (renamed || rehomed) this.emit({ type: "servers-changed" });
    return structuredClone(meta);
  }

  rotateJoinCode(vaultId: VaultId): JoinCode {
    const meta = this.meta(vaultId);
    let code = "";
    for (let i = 0; i < JOIN_CODE_LENGTH; i++) {
      code += JOIN_CODE_ALPHABET[Math.floor(this.random() * JOIN_CODE_ALPHABET.length)];
    }
    meta.joinCode = code;
    this.emit({ type: "vault-changed", vaultId });
    return code;
  }

  // ------------------------------------------------------------------- members

  listMembers(vaultId: VaultId): Member[] {
    return structuredClone(this.members.get(vaultId) ?? []);
  }

  setMemberRole(vaultId: VaultId, peerId: PeerId, role: MemberRole): Member {
    const list = this.members.get(vaultId) ?? [];
    const member = this.memberOf(vaultId, peerId);
    // Demoting the last admin would lock everyone out of the join code and settings.
    if (member.role === "admin" && role !== "admin") {
      const admins = list.filter((m) => m.role === "admin").length;
      if (admins <= 1) throw new Error("A vault needs at least one admin");
    }
    member.role = role;
    this.syncMemberCount(vaultId);
    this.emit({ type: "members-changed", vaultId });
    this.emit({ type: "servers-changed" });
    return structuredClone(member);
  }

  removeMember(vaultId: VaultId, peerId: PeerId): void {
    if (peerId === this.self) throw new Error("You can't remove yourself");
    const list = this.members.get(vaultId) ?? [];
    const index = list.findIndex((m) => m.peerId === peerId);
    if (index === -1) throw new Error("That member is not in this vault");
    list.splice(index, 1);
    this.presenceMap(vaultId).delete(peerId);

    this.syncMemberCount(vaultId);
    this.emit({ type: "members-changed", vaultId });
    this.emit({ type: "servers-changed" });
    this.emitPresence(vaultId);
  }

  deleteVault(vaultId: VaultId): void {
    const me = this.memberOf(vaultId, this.self);
    if (me.role !== "admin") throw new Error("Only an admin can delete a vault");
    this.forgetVault(vaultId);
    this.emit({ type: "servers-changed" });
  }

  leaveVault(vaultId: VaultId): void {
    const list = this.members.get(vaultId) ?? [];
    const me = this.memberOf(vaultId, this.self);
    if (me.role === "admin" && list.filter((m) => m.role === "admin").length <= 1) {
      throw new Error("Make someone else an admin first");
    }
    this.forgetVault(vaultId);
    this.emit({ type: "servers-changed" });
  }

  /** Drop every trace of a vault this client no longer has: tree, lists, recents, timers. */
  private forgetVault(vaultId: VaultId): void {
    for (const [id, node] of [...this.nodes]) {
      if (node.vaultId !== vaultId) continue;
      this.cancelDownload(id);
      this.nodes.delete(id);
      this.access.delete(id);
      delete this.previews[id];
    }
    const timer = this.presenceTimers.get(vaultId);
    if (timer !== undefined) clearTimeout(timer);
    this.presenceTimers.delete(vaultId);
    this.presenceLastAt.delete(vaultId);
    this.presence.delete(vaultId);
    this.members.delete(vaultId);
    this.vaults.delete(vaultId);
    this.history = this.history.filter((event) => event.vaultId !== vaultId);
    this.recents = this.recents.filter((entry) => entry.vaultId !== vaultId);

    const row = this.vaultRow(vaultId);
    if (row) row.server.vaults.splice(row.index, 1);
  }

  // ------------------------------------------------------------------ presence

  getPresence(vaultId: VaultId): PeerPresence[] {
    return this.presenceList(vaultId);
  }

  publishPresence(input: PresenceInput): void {
    const byPeer = this.presenceMap(input.vaultId);
    byPeer.set(this.self, {
      peerId: this.self,
      online: true,
      idle: false,
      folderId: input.folderId,
      // Stored so a future "follow me" can read it back; never echoed to this client.
      cursor: input.cursor ? structuredClone(input.cursor) : null,
      hoveringNodeId: input.hoveringNodeId,
      draggingNodeIds: [...input.draggingNodeIds],
      updatedAt: Date.now(),
    });
    this.emitPresence(input.vaultId);
  }

  // --------------------------------------------------------------------- agent

  /**
   * The agent bar's stand-in reply.
   *
   * It answers with something only a real reader of the folder could know — the
   * number of things in it — so the bar demonstrably has context, and then says
   * plainly that the model is not wired up yet. A fake answer about file contents
   * would be a lie the moment anyone tested it.
   */
  askAgent(input: AskAgentInput): Promise<AgentReply> {
    const folder = this.node(input.folderId);
    const count = this.childrenOf(folder.id).length;
    this.agentCounter += 1;
    const id = `agent_${this.agentCounter}`;
    const text =
      `I can see ${count} items in “${folder.name}”. ` +
      "Once your local Claude or Codex is connected, I can read them and act on your behalf.";
    return new Promise<AgentReply>((resolve) => {
      setTimeout(() => resolve({ id, text }), AGENT_LATENCY_MS);
    });
  }
}
