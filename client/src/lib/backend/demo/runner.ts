/**
 * The clock behind the scripted demo.
 *
 * WHY one loop: a demo written as nested `setTimeout`s cannot be scrubbed, frozen, or
 * looped without drift, and every interruption leaves orphaned timers behind. Here a
 * single requestAnimationFrame loop keeps one number — `elapsed`, wrapped to
 * {@link LOOP_MS} — and everything else is a pure function of it: presence is *derived*
 * from the script each frame, and ops fire when `elapsed` crosses their `at`. That makes
 * `?demo=<ms>` a real screenshot switch (seek + pause reproduces the exact frame) and
 * makes stopping the demo instantaneous and complete.
 *
 * Peers have no motion: presence carries online / idle / folder and nothing else, and a
 * scripted mutation is applied to the tree the moment it fires — no announcement, no
 * flight, no pulse. The change just appears, the way a poll would deliver it.
 *
 * Every `BackendClient` call goes through one promise chain, so ops land in script order
 * even though they are async, and a rejection never escapes the demo layer.
 */

import type { BackendClient } from "../client";
import type { BackendEvent, NodeId, PeerId, PeerPresence, VaultId } from "../types";
import type { OpKey, PresenceKey } from "./script";
import { DEMO_PEERS, DEMO_ROOT, KEYS, LOOP_MS, isOpKey } from "./script";

/** Frame budget for the rAF-less fallback, and the floor between presence emits (~30/s). */
const FRAME_MS = 33;

/** Seed ids look like `root_<vault>` or `n_<vault>_<n>`; anything else is a scripted name. */
const NODE_ID = /^(?:root_|n_)/;

export interface DemoRunnerOptions {
  /** `?demo=` — live, silent, or frozen at a given elapsed time. */
  mode: "on" | "off" | number;
  vaultId: VaultId;
  /** Where presence / demo-reset events go (the wrapper decides). */
  emit: (e: BackendEvent) => void;
}

export interface DemoRunner {
  start(): void;
  stop(): void;
  seek(ms: number): void;
  pause(): void;
  play(): void;
  elapsed(): number;
  active(): boolean;
}

declare global {
  interface Window {
    /** Dev-only handle for scrubbing the demo from the console or a screenshot run. */
    __qfs?: {
      seek(ms: number): void;
      pause(): void;
      play(): void;
      elapsed(): number;
      stop(): void;
    };
  }
}

function nowMs(): number {
  return typeof performance !== "undefined" ? performance.now() : Date.now();
}

function isNodeId(value: NodeId | null): value is NodeId {
  return value !== null;
}

/** `updatedAt` is deliberately ignored: it changes every frame and means nothing to the UI. */
function samePeers(a: PeerPresence[] | null, b: PeerPresence[]): boolean {
  if (a === null || a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    const x = a[i];
    const y = b[i];
    if (x.peerId !== y.peerId) return false;
    if (x.online !== y.online || x.idle !== y.idle || x.folderId !== y.folderId) return false;
  }
  return true;
}

export function createDemoRunner(client: BackendClient, opts: DemoRunnerOptions): DemoRunner {
  const { mode, vaultId, emit } = opts;

  /** Script keys per peer, newest last, so derivation walks backwards from `elapsed`. */
  const presenceKeys = new Map<PeerId, PresenceKey[]>();
  for (const peer of DEMO_PEERS) presenceKeys.set(peer, []);
  for (const key of KEYS) {
    if (isOpKey(key)) continue;
    presenceKeys.get(key.peer)?.push(key);
  }

  /** Nodes the script created this loop, by scripted name (`"Launch"` → real id). */
  const createdIds = new Map<string, NodeId>();

  let chain: Promise<void> = Promise.resolve();
  let running = false;
  let playing = false;
  let origin = 0;
  let frozen = 0;
  let lastElapsed = -1;
  let lastPeers: PeerPresence[] | null = null;
  let lastEmitAt = 0;
  let rafId: number | null = null;
  let timerId: ReturnType<typeof setInterval> | null = null;

  function enqueue(step: () => Promise<unknown>): void {
    chain = chain
      .then(step)
      .then(
        () => undefined,
        (error: unknown) => {
          console.warn("[demo] op failed", error);
        },
      );
  }

  function resolveId(ref: string): NodeId | null {
    const created = createdIds.get(ref);
    if (created !== undefined) return created;
    return NODE_ID.test(ref) ? ref : null;
  }

  function resolveAll(refs: string[] | undefined): NodeId[] {
    if (refs === undefined) return [];
    return refs.map(resolveId).filter(isNodeId);
  }

  function derive(elapsed: number): PeerPresence[] {
    const updatedAt = Date.now();
    return DEMO_PEERS.map((peerId) => {
      const keys = presenceKeys.get(peerId) ?? [];
      let idle = false;

      for (let i = keys.length - 1; i >= 0; i -= 1) {
        const key = keys[i];
        if (key.at > elapsed) continue;
        if (key.idle !== undefined) {
          idle = key.idle;
          break;
        }
      }

      return {
        peerId,
        online: true,
        idle,
        folderId: DEMO_ROOT,
        cursor: null,
        hoveringNodeId: null,
        draggingNodeIds: [],
        updatedAt,
      };
    });
  }

  function emitPresence(elapsed: number, force: boolean): void {
    const peers = derive(elapsed);
    const at = nowMs();
    if (!force) {
      if (at - lastEmitAt < FRAME_MS) return;
      if (samePeers(lastPeers, peers)) return;
    }
    lastPeers = peers;
    lastEmitAt = at;
    emit({ type: "presence", vaultId, peers });
  }

  /** What the workspace sees while the demo is not running: the four peers, not idle. */
  function emitEmptyPresence(): void {
    const updatedAt = Date.now();
    const peers: PeerPresence[] = DEMO_PEERS.map((peerId) => ({
      peerId,
      online: true,
      idle: false,
      folderId: DEMO_ROOT,
      cursor: null,
      hoveringNodeId: null,
      draggingNodeIds: [],
      updatedAt,
    }));
    lastPeers = peers;
    lastEmitAt = nowMs();
    emit({ type: "presence", vaultId, peers });
  }

  function fireMove(key: OpKey): void {
    // Resolved inside the chain, not here: a seek that crosses both Justin's create and
    // Kenji's move fires them in the same frame, and "Launch" only has an id once the
    // queued create has actually run.
    enqueue(async () => {
      const nodeIds = resolveAll(key.nodeIds);
      const toFolderId = key.toFolderId === undefined ? null : resolveId(key.toFolderId);
      if (nodeIds.length === 0 || toFolderId === null) {
        console.warn("[demo] skipped a move with unresolved nodes", key.nodeIds, key.toFolderId);
        return;
      }
      await client.moveNodes({ vaultId, nodeIds, toParentId: toFolderId, actor: key.peer });
    });
  }

  function fireCreate(key: OpKey): void {
    const name = key.name;
    if (name === undefined) return;
    enqueue(async () => {
      try {
        const node = await client.createNode({
          vaultId,
          parentId: DEMO_ROOT,
          kind: "folder",
          name,
          actor: key.peer,
        });
        createdIds.set(name, node.id);
      } catch {
        // A leftover folder from an interrupted loop: adopt it, so the move that follows
        // still has somewhere to land.
        const existing = (await client.listTree(vaultId)).find(
          (node) => node.parentId === DEMO_ROOT && node.name === name,
        );
        if (existing) createdIds.set(name, existing.id);
        else console.warn(`[demo] could not create "${name}"`);
      }
    });
  }

  function fireDelete(key: OpKey): void {
    enqueue(async () => {
      const nodeIds = resolveAll(key.nodeIds);
      if (nodeIds.length === 0) return;
      await client.deleteNodes({ vaultId, nodeIds, actor: key.peer });
      for (const [name, id] of [...createdIds]) {
        if (nodeIds.includes(id)) createdIds.delete(name);
      }
    });
  }

  function fireOp(key: OpKey, immediate: boolean): void {
    switch (key.op) {
      case "move":
        fireMove(key);
        return;
      case "create":
        fireCreate(key);
        return;
      case "delete":
        fireDelete(key);
        return;
      case "reset-fade":
        // A seek only reconstructs the tree; the crossfade would be a lie mid-scrub.
        if (!immediate) emit({ type: "demo-reset", vaultId });
        return;
    }
  }

  function fireWindow(from: number, to: number, immediate: boolean): void {
    for (const key of KEYS) {
      if (!isOpKey(key)) continue;
      if (key.at > from && key.at <= to) fireOp(key, immediate);
    }
  }

  /** Half-open `(from, to]`, splitting at the loop seam so a wrap fires tail then head. */
  function fireRange(from: number, to: number, immediate: boolean): void {
    if (to >= from) {
      fireWindow(from, to, immediate);
      return;
    }
    fireWindow(from, LOOP_MS, immediate);
    fireWindow(-1, to, immediate);
  }

  function elapsedNow(): number {
    if (!running) return 0;
    if (!playing) return frozen;
    const raw = (nowMs() - origin) % LOOP_MS;
    return raw < 0 ? raw + LOOP_MS : raw;
  }

  function tick(): void {
    if (!running || !playing) return;
    const elapsed = elapsedNow();
    fireRange(lastElapsed, elapsed, false);
    lastElapsed = elapsed;
    emitPresence(elapsed, false);
  }

  function schedule(): void {
    if (typeof requestAnimationFrame === "function") {
      if (rafId !== null) return;
      rafId = requestAnimationFrame(() => {
        rafId = null;
        tick();
        if (running && playing) schedule();
      });
      return;
    }
    if (timerId === null) timerId = setInterval(tick, FRAME_MS);
  }

  function unschedule(): void {
    if (rafId !== null && typeof cancelAnimationFrame === "function") cancelAnimationFrame(rafId);
    rafId = null;
    if (timerId !== null) clearInterval(timerId);
    timerId = null;
  }

  function seek(ms: number): void {
    if (mode === "off") return;
    if (!running) start();
    const target = ((ms % LOOP_MS) + LOOP_MS) % LOOP_MS;
    // A seek reconstructs the tree, so every op in the crossed window is applied at once
    // (a backwards seek crosses the reset first, which is what puts the seed back).
    fireRange(lastElapsed, target, true);
    lastElapsed = target;
    frozen = target;
    if (playing) origin = nowMs() - target;
    enqueue(async () => {
      emitPresence(target, true);
    });
  }

  function pause(): void {
    if (!running || !playing) return;
    frozen = elapsedNow();
    playing = false;
    unschedule();
  }

  function play(): void {
    if (!running || playing || mode === "off") return;
    origin = nowMs() - frozen;
    playing = true;
    schedule();
  }

  function start(): void {
    if (mode === "off" || running) return;
    running = true;
    playing = true;
    origin = nowMs();
    lastElapsed = -1;
    lastPeers = null;
    if (typeof mode === "number") {
      // Freeze before seeking, so the frame is exactly the requested elapsed time and not
      // that plus however long the seek itself took — screenshots have to be repeatable.
      pause();
      seek(mode);
      return;
    }
    schedule();
  }

  function stop(): void {
    if (!running) return;
    running = false;
    playing = false;
    unschedule();
    lastElapsed = -1;
    frozen = 0;
    emitEmptyPresence();
  }

  const runner: DemoRunner = {
    start,
    stop,
    seek,
    pause,
    play,
    elapsed: elapsedNow,
    active: () => running,
  };

  if (import.meta.env?.DEV && typeof window !== "undefined") {
    window.__qfs = { seek, pause, play, elapsed: elapsedNow, stop };
  }

  return runner;
}
