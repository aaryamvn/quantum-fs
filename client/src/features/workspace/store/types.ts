/**
 * The vocabulary the workspace store and every UI unit share.
 *
 * These shapes live apart from the store itself so a component can name what it
 * accepts (a `ModalState`, a `DragState`) without importing the store and
 * subscribing to it — a tile takes a `TileDropState`, it does not read the drag.
 * Nothing here describes the file system: node, member and presence shapes are
 * the backend's (`@/lib/backend`) and are never re-declared, so the daemon swap
 * stays a one-file change.
 */

import type { PointerEvent as ReactPointerEvent } from "react";

import type { NodeId, PeerId, VaultId } from "@/lib/backend";

/** The column a folder's contents are ordered by. */
export type SortBy = "name" | "kind" | "modified" | "size";

/** Ascending or descending, applied inside each group (folders still lead). */
export type SortDir = "asc" | "desc";

/** Which pane the vault settings modal opens on. */
export type VaultSettingsTab = "general" | "members" | "sharing" | "storage" | "advanced";

/**
 * The one modal that is open, or `null`.
 *
 * A single slot rather than a flag per modal: two dialogs can never be open at
 * once, and the URL switch `?ui=info:<id>` maps onto exactly one of these.
 */
export type ModalState =
  | null
  | { kind: "info"; nodeId: NodeId }
  | { kind: "history"; nodeId: NodeId }
  | { kind: "access"; nodeId: NodeId }
  | { kind: "share"; nodeId: NodeId }
  | { kind: "confirm-delete"; nodeIds: NodeId[] }
  | { kind: "vault-settings"; vaultId: VaultId; tab: VaultSettingsTab }
  | { kind: "search" };

/** Where the context menu was summoned, and what it was summoned on. */
export interface ContextMenuState {
  x: number;
  y: number;
  /** null = canvas background */
  nodeId: NodeId | null;
}

/** One step of the back/forward history: a folder inside a vault. */
export interface NavEntry {
  vaultId: VaultId;
  folderId: NodeId;
}

/** A transient message in the bottom-left stack. */
export interface ToastItem {
  id: string;
  text: string;
  kind: "info" | "success" | "error";
  action?: { label: string; onClick(): void };
}

/** A node that has just appeared, so it can pop in instead of blinking into place. */
export interface JustCreated {
  by: PeerId;
  at: number;
}

/** The kinds of surface a drag can be dropped onto. */
export type DropKind = "tile" | "crumb" | "sidebar";

/**
 * Coarse local-drag state: start, what is under the pointer, end.
 *
 * Per-frame positions deliberately do NOT live here (docs/decisions/client-workspace.md)
 * — they are Motion values in the drag module, so a drag never re-renders the grid.
 */
export interface DragState {
  active: boolean;
  nodeIds: NodeId[];
  overTargetId: NodeId | null;
  overKind: DropKind | null;
}

/** The resting drag state; `setDrag(IDLE_DRAG)` is the reset. */
export const IDLE_DRAG: DragState = { active: false, nodeIds: [], overTargetId: null, overKind: null };

/** Implemented by the drag module next phase; the canvas calls start() from a tile pointerdown. */
export interface DragController {
  start(e: ReactPointerEvent, nodeIds: NodeId[]): void;
  cancel(): void;
}

/** What a tile receives from the canvas for the layers that overlay it. */
export type TileDropState = "none" | "over" | "near";
