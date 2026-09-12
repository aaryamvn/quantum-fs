/**
 * `withDemo(client)` — the scripted multiplayer layer, as a wrapper.
 *
 * DELETE LATER: this whole folder plus the one `withDemo(...)` call in
 * `../index.ts` is the entire demo (docs/decisions/client-workspace.md). Removing both
 * leaves a client that behaves exactly like the one underneath, with no UI change: the
 * wrapper only ever calls public `BackendClient` methods (with the mock-only `actor`
 * field) and injects `presence` / `demo-reset` events that the workspace already handles
 * for real peers.
 *
 * WHY a wrapper rather than a mock feature: the same script has to run over the Tauri
 * client once the daemon is real, and the human wants the demo gone in one commit. The
 * wrapper therefore prefers the mock's own simulation hooks when they exist — so the
 * mock's presence map stays coherent with what the screen shows — and falls back to
 * emitting straight to its own subscribers when it is wrapping the Rust bridge.
 */

import type { BackendClient, PresenceInput } from "../client";
import type { MockSimulation } from "../mock";
import { getSimulation } from "../mock";
import type { BackendEvent, PeerPresence, VaultId } from "../types";
import type { DemoRunner } from "./runner";
import { createDemoRunner } from "./runner";
import { DEMO_VAULT } from "./script";

/** Test hook: the runner behind a wrapped client, for node smokes and screenshots. */
const runners = new WeakMap<BackendClient, DemoRunner>();

/** DEV/test only. Returns null for an unwrapped client or `?demo=off`. */
export function __runnerFor(client: BackendClient): DemoRunner | null {
  return runners.get(client) ?? null;
}

function mergePeers(base: PeerPresence[], demo: PeerPresence[]): PeerPresence[] {
  if (demo.length === 0) return base;
  const merged = base.filter((peer) => !demo.some((d) => d.peerId === peer.peerId));
  return [...merged, ...demo];
}

export function withDemo(inner: BackendClient, mode: "on" | "off" | number): BackendClient {
  // `?demo=off` must leave nothing behind, not even a fan-out.
  if (mode === "off") return inner;

  const listeners = new Set<(e: BackendEvent) => void>();
  let unsubscribeInner: (() => void) | null = null;
  // The mock's side door, or null when this wraps the Tauri bridge — then the events go
  // straight to the subscribers instead, which is all the workspace actually reads.
  let simulation: MockSimulation | null = getSimulation(inner);
  let demoPeers: PeerPresence[] = [];

  const fanOut = (event: BackendEvent) => {
    for (const listener of [...listeners]) listener(event);
  };

  /**
   * Inner presence for the demo vault is re-merged on the way out: whoever produced it
   * (the mock's own `publishPresence`) only knows about the local user, and dropping the
   * scripted peers for one frame would blink the whole avatar row.
   */
  const onInnerEvent = (event: BackendEvent) => {
    if (event.type === "presence" && event.vaultId === DEMO_VAULT && runner.active()) {
      fanOut({ type: "presence", vaultId: event.vaultId, peers: mergePeers(event.peers, demoPeers) });
      return;
    }
    fanOut(event);
  };

  const emit = (event: BackendEvent) => {
    if (event.type === "presence" && event.vaultId === DEMO_VAULT) demoPeers = event.peers;
    const sim = simulation;
    if (sim !== null) {
      try {
        // Through the mock, so its presence map and the screen never disagree; the
        // resulting event comes back via `onInnerEvent`.
        if (event.type === "presence") sim.presence(event.vaultId, event.peers);
        else if (event.type === "demo-reset") sim.reset(event.vaultId);
        else fanOut(event);
        return;
      } catch (error) {
        console.warn("[demo] simulation hook failed; emitting directly", error);
        simulation = null;
      }
    }
    fanOut(event);
  };

  const runner = createDemoRunner(inner, { mode, vaultId: DEMO_VAULT, emit });

  const wrapped: BackendClient = {
    ...inner,

    subscribe(listener: (e: BackendEvent) => void) {
      listeners.add(listener);
      if (unsubscribeInner === null) unsubscribeInner = inner.subscribe(onInnerEvent);
      return () => {
        listeners.delete(listener);
        if (listeners.size === 0 && unsubscribeInner !== null) {
          unsubscribeInner();
          unsubscribeInner = null;
        }
      };
    },

    async getPresence(vaultId: VaultId) {
      const base = await inner.getPresence(vaultId);
      if (vaultId !== DEMO_VAULT || !runner.active()) return base;
      return mergePeers(base, demoPeers);
    },

    async publishPresence(input: PresenceInput) {
      await inner.publishPresence(input);
      // A workspace is listening exactly when it reports a folder inside the demo vault.
      if (input.vaultId === DEMO_VAULT && input.folderId !== null) runner.start();
      else runner.stop();
    },
  };

  runners.set(wrapped, runner);
  return wrapped;
}

export default withDemo;
export type { DemoRunner } from "./runner";
