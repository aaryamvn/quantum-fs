/**
 * The scripted 40-second multiplayer demo, expressed as data.
 *
 * WHY data and not code: the whole demo is a deletable layer
 * (docs/decisions/client-workspace.md). Keeping the timeline as a flat, sorted list of
 * keys means the runner is one small pure function of `elapsed` — it can be scrubbed,
 * frozen for a screenshot, or replayed backwards without any of the nested-timeout
 * bookkeeping that a hand-coded sequence would need. Nothing here knows about pixels,
 * React, or the store: every mutation is expressed as a public `BackendClient` call.
 *
 * WHAT THIS NO LONGER DOES: there is no peer presence *motion* — no cursors, no hover,
 * no live dragging, no double-click pulse. The human's follow-up removed all of it: a
 * remote change simply appears in the tree when it happens, exactly as a poll would
 * deliver it. The peers still exist as presence (online / idle / folder) so the avatar
 * row has someone to show, and they still create, move and delete on the clock.
 *
 * The beats:
 *   6.3s  Brand lands in Projects (Aaryaman)
 *  11s    "Launch" appears (Justin)
 *  17.8s  a three-file stack lands in Launch (Kenji)
 *  32s    Justin goes idle
 *  34.4s  Brand lands back in Archive (Aaryaman)
 *  39.4s  crossfade reset; the seed is put back at 39.7s and the loop starts over
 */

import type { NodeId, PeerId, VaultId } from "../types";

/** One full pass of the script. `elapsed` is always taken modulo this. */
export const LOOP_MS = 40_000;

/** The only vault the demo ever touches; every other vault is left alone. */
export const DEMO_VAULT: VaultId = "vlt_1_1";

/** Root folder of {@link DEMO_VAULT}; the scripted peers never leave it. */
export const DEMO_ROOT: NodeId = "root_vlt_1_1";

export const PEER_AARYAMAN: PeerId = "peer_aaryaman";
export const PEER_JUSTIN: PeerId = "peer_justin";
export const PEER_MAYA: PeerId = "peer_maya";
export const PEER_KENJI: PeerId = "peer_kenji";

/** Every peer the script animates, in the order the presence array is built. */
export const DEMO_PEERS: PeerId[] = [PEER_AARYAMAN, PEER_JUSTIN, PEER_MAYA, PEER_KENJI];

/**
 * Name of the folder Justin creates mid-loop. It has no id until it exists, so the
 * script refers to it by name and the runner substitutes the real id at fire time.
 */
export const LAUNCH = "Launch";

/** Seed node ids the script leans on (client/src/lib/backend/seed/fs.json). */
const PROJECTS: NodeId = "n_1_1_projects";
const BRAND: NodeId = "n_1_1_brand";
const ARCHIVE: NodeId = "n_1_1_archive";
const HERO: NodeId = "n_1_1_hero";
const MOODBOARD: NodeId = "n_1_1_moodboard";
const PALETTE: NodeId = "n_1_1_palette";

/** The three-file stack that lands in Launch. */
const STACK: NodeId[] = [HERO, MOODBOARD, PALETTE];

/**
 * One keyframe of a peer's presence.
 *
 * Only `idle` is scripted — `online` and `folderId` are constant for the whole loop, and
 * cursor / hover / drag presence no longer exists. `idle` is optional because it is
 * derived independently: a key that omits it leaves it at whatever the previous key that
 * mentioned it said.
 */
export interface PresenceKey {
  at: number;
  peer: PeerId;
  idle?: boolean;
}

/**
 * One scripted mutation, fired exactly once per loop.
 *
 * Every op applies straight to the tree through a public `BackendClient` call at its
 * `at`; nothing is announced ahead of time, so the change simply shows up. The `at` of a
 * move is therefore the moment the item *lands*.
 */
export interface OpKey {
  at: number;
  peer: PeerId;
  op: "move" | "create" | "delete" | "reset-fade";
  /** move / delete: node ids, or {@link LAUNCH} for the folder created mid-loop. */
  nodeIds?: string[];
  /** move: destination folder id, or {@link LAUNCH}. */
  toFolderId?: string;
  /** create: the folder name. */
  name?: string;
}

export type DemoKey = PresenceKey | OpKey;

/** Narrowing helper for the two key shapes. */
export function isOpKey(key: DemoKey): key is OpKey {
  return "op" in key;
}

/**
 * The timeline. Authored in ascending `at`; keys that share an `at` are applied in array
 * order, which is what keeps the reset's three restore ops in a safe sequence (empty
 * Launch before deleting it).
 */
export const KEYS: DemoKey[] = [
  // — Brand moves into Projects ————————————————————————————————————————
  { at: 6340, peer: PEER_AARYAMAN, op: "move", nodeIds: [BRAND], toFolderId: PROJECTS },

  // — Justin creates "Launch" ——————————————————————————————————————————
  { at: 11000, peer: PEER_JUSTIN, op: "create", name: LAUNCH },

  // — A three-file stack lands in Launch ————————————————————————————————
  { at: 17840, peer: PEER_KENJI, op: "move", nodeIds: STACK, toFolderId: LAUNCH },

  // — Justin idles; Archive receives Brand back ————————————————————————
  { at: 32000, peer: PEER_JUSTIN, idle: true },
  { at: 34420, peer: PEER_AARYAMAN, op: "move", nodeIds: [BRAND], toFolderId: ARCHIVE },

  // — The canvas crossfades and the seed is put back ————————————————————
  { at: 39400, peer: PEER_AARYAMAN, op: "reset-fade" },
  { at: 39700, peer: PEER_KENJI, op: "move", nodeIds: STACK, toFolderId: DEMO_ROOT },
  { at: 39700, peer: PEER_AARYAMAN, op: "move", nodeIds: [BRAND], toFolderId: DEMO_ROOT },
  { at: 39700, peer: PEER_JUSTIN, op: "delete", nodeIds: [LAUNCH] },
  { at: 39700, peer: PEER_JUSTIN, idle: false },
];
