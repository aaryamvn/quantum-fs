/**
 * The one place the design-time file system is read in from JSON.
 *
 * `seed/fs.json` is the single source both runtimes share — the browser mock
 * imports it here, Rust `include_str!`s the same bytes — so the two builds are
 * pixel-identical without anyone hand-syncing a second fixture
 * (docs/decisions/client-workspace.md).
 *
 * Two things are deliberate. The JSON is *cloned* on every load, because the
 * engine mutates its state in place and a shared module-level object would leak
 * one screenshot run's renames into the next. And the cast happens exactly once,
 * here: `resolveJsonModule` widens every union in the file to `string`, so
 * pretending otherwise at each call site would spread the lie across the engine.
 */

import type {
  FsNode,
  HistoryEvent,
  Member,
  NodeAccess,
  NodeId,
  PeerId,
  VaultId,
  VaultMeta,
} from "../types";

import rawSeed from "../seed/fs.json";

/** One entry of the recents list as it is stored: a pointer, not a denormalized row. */
export interface SeedRecent {
  vaultId: VaultId;
  nodeId: NodeId;
  at: number;
}

/** Everything `seed/fs.json` holds, in the shape the engine wants to consume it. */
export interface FsSeed {
  /** The instant the fixture was generated; every "now" in it is relative to this. */
  generatedAt: number;
  self: PeerId;
  vaults: Record<VaultId, VaultMeta>;
  members: Record<VaultId, Member[]>;
  nodes: FsNode[];
  access: NodeAccess[];
  history: HistoryEvent[];
  recents: SeedRecent[];
  /** nodeId → text body, for the inspector's preview pane. Only text-ish files appear. */
  previews: Record<NodeId, string>;
}

/** The local user's peer id, as baked into the fixture. */
export const SEED_SELF: PeerId = (rawSeed as { self: PeerId }).self;

/**
 * A private, mutable copy of the fixture.
 *
 * `structuredClone` rather than a spread: the tree is nested four levels deep in
 * places, and a shallow copy would hand the caller arrays (`holders`,
 * `entries`) that still belong to the module.
 */
export function loadSeed(): FsSeed {
  return structuredClone(rawSeed) as unknown as FsSeed;
}
