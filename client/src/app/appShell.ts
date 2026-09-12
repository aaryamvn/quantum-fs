/**
 * Which screen the app is on, and the open request that carries a vault into it.
 *
 * The home list and the workspace are two full-screen worlds with a 900 ms
 * transition between them, so "where am I" cannot live inside either of them:
 * the dive has to outlive the home's unmount and precede the workspace's mount.
 * It is a store rather than App state because the sidebar, the recents list and
 * search all open *other* vaults from deep inside the workspace — threading a
 * callback up through every panel to do that would cost more than one import.
 *
 * No file-system state lives here; `useWorkspace` owns all of it. What lives
 * here is the screen plus `pending`, the request App turns into `openVault`.
 * `pending` is a fresh object per request (it carries `seq`), so an effect keyed
 * on it re-runs even when the same vault is opened twice in a row.
 */

import { create } from "zustand";

import { useWorkspace } from "@/features/workspace/store";
import type { NodeId, VaultId } from "@/lib/backend";

/** The clicked row's viewport rect — where the dive's aperture starts. */
export interface DiveOrigin {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Where inside the vault to land. Both are optional: the root with nothing selected. */
export interface VaultOpenOptions {
  folderId?: NodeId | null;
  select?: NodeId[];
}

/** One resolved open, handed to `useWorkspace.openVault` by App. */
export interface VaultOpenRequest {
  vaultId: VaultId;
  folderId: NodeId | null;
  select: NodeId[];
  /** Monotonic: makes every request a distinct object, so repeats still fire. */
  seq: number;
}

export type Screen =
  | { kind: "home" }
  /** First launch only: the app has no name for this machine's person yet. */
  | { kind: "onboarding" }
  | {
      kind: "dive";
      vaultId: VaultId;
      vaultName: string;
      origin: DiveOrigin;
      folderId: NodeId | null;
      select: NodeId[];
    }
  | { kind: "workspace"; vaultId: VaultId };

export interface AppShellState {
  screen: Screen;
  /** Consumed by App; null between opens. */
  pending: VaultOpenRequest | null;
}

export interface AppShellActions {
  /** Home → vault, through the dive. `origin` is the clicked row's rect. */
  enterVault(
    vaultId: VaultId,
    vaultName: string,
    origin: DiveOrigin,
    opts?: VaultOpenOptions,
  ): void;
  /** The dive finished: drop the transition, keep the workspace. */
  diveDone(): void;
  /** Back to the overview; also tears the vault down in the workspace store. */
  goHome(): void;
  /** Vault → vault from inside the workspace. No dive: the shell is already there. */
  switchVault(vaultId: VaultId, opts?: VaultOpenOptions): void;
  /**
   * The profile has no name yet: ask for one before the home list.
   *
   * Only ever from the overview. A deep link (`?vault=`) has already moved on,
   * and yanking someone out of a vault they asked for would be worse than a
   * missing name.
   */
  startOnboarding(): void;
  /** The name is stored: slide the home list in. */
  finishOnboarding(): void;
}

export type AppShellStore = AppShellState & AppShellActions;

let seq = 0;

function request(vaultId: VaultId, opts?: VaultOpenOptions): VaultOpenRequest {
  return {
    vaultId,
    folderId: opts?.folderId ?? null,
    select: opts?.select ?? [],
    seq: ++seq,
  };
}

export const useAppShell = create<AppShellStore>()((set, get) => ({
  screen: { kind: "home" },
  pending: null,

  enterVault(vaultId, vaultName, origin, opts) {
    const pending = request(vaultId, opts);
    set({
      screen: {
        kind: "dive",
        vaultId,
        vaultName,
        origin,
        folderId: pending.folderId,
        select: pending.select,
      },
      pending,
    });
  },

  diveDone() {
    const { screen } = get();
    // Guard: a `goHome` or a cross-vault open during the dive already moved on.
    if (screen.kind !== "dive") return;
    set({ screen: { kind: "workspace", vaultId: screen.vaultId } });
  },

  goHome() {
    set({ screen: { kind: "home" }, pending: null });
    // The workspace unmounts on the same tick, so nothing renders the cleared
    // tree; holding it would only keep a stale vault alive behind the home.
    useWorkspace.getState().closeVault();
  },

  switchVault(vaultId, opts) {
    set({ screen: { kind: "workspace", vaultId }, pending: request(vaultId, opts) });
  },

  startOnboarding() {
    if (get().screen.kind !== "home") return;
    set({ screen: { kind: "onboarding" } });
  },

  finishOnboarding() {
    if (get().screen.kind !== "onboarding") return;
    set({ screen: { kind: "home" } });
  },
}));
