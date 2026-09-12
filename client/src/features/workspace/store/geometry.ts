/**
 * Where things actually are on screen, shared by every layer that has to agree.
 *
 * The drag layer draws *outside* the grid but has to land on it exactly: a
 * dropped stack has to finish on the target's center, and the hit test has to
 * answer for tiles, breadcrumbs and sidebar rows alike. Reading that from a
 * React tree would mean threading refs through four components and re-rendering
 * on every measurement, so the registries are module singletons — deliberately
 * not state (docs/decisions/client-workspace.md).
 *
 * Every register* function returns its own unregister, so a component's effect
 * cleanup is the whole story and StrictMode's double mount is harmless.
 */

import type { NodeId } from "@/lib/backend";

import type { DropKind } from "./types";

const tiles = new Map<NodeId, HTMLElement>();

interface DropTarget {
  id: NodeId;
  el: HTMLElement;
  kind: DropKind;
}

/**
 * Keyed by `kind:id`, not by id alone.
 *
 * The same node can legitimately be two targets at once — a vault root is both
 * the sidebar row and the first breadcrumb — and an id-only key let whichever
 * registered last silently evict the other, which made the root crumb a dead
 * drop target from the first drag onward. The key carries the kind so both
 * entries coexist; `hitTestDropTarget` still answers with the node id.
 */
const dropTargets = new Map<string, DropTarget>();

const dropKey = (id: NodeId, kind: DropKind): string => `${kind}:${id}`;

let canvasEl: HTMLElement | null = null;

/** Priority order for a hit test: a tile sitting over the sidebar wins the drop. */
const KIND_ORDER: DropKind[] = ["tile", "crumb", "sidebar"];

/** Register a tile's element. Returns the unregister, which only removes its own entry. */
export function registerTile(id: NodeId, el: HTMLElement): () => void {
  tiles.set(id, el);
  return () => {
    if (tiles.get(id) === el) tiles.delete(id);
  };
}

export function getTileEl(id: NodeId): HTMLElement | null {
  return tiles.get(id) ?? null;
}

export function getTileRect(id: NodeId): DOMRect | null {
  const el = tiles.get(id);
  return el ? el.getBoundingClientRect() : null;
}

export function setCanvasEl(el: HTMLElement | null): void {
  canvasEl = el;
}

export function getCanvasEl(): HTMLElement | null {
  return canvasEl;
}

export function getCanvasRect(): DOMRect | null {
  return canvasEl ? canvasEl.getBoundingClientRect() : null;
}

/**
 * Register anything a drag can be dropped on: a folder tile, a breadcrumb, a
 * sidebar row. One registry rather than three because the hit test has to resolve
 * overlaps *between* kinds, which three separate lists could not do — and it is
 * keyed by kind+id so the same node registered as two kinds keeps both entries.
 */
export function registerDropTarget(id: NodeId, el: HTMLElement, kind: DropKind): () => void {
  const key = dropKey(id, kind);
  dropTargets.set(key, { id, el, kind });
  return () => {
    if (dropTargets.get(key)?.el === el) dropTargets.delete(key);
  };
}

/** The registered rect for one target, or null when nothing of that kind is registered. */
export function getDropTargetRect(id: NodeId, kind: DropKind): DOMRect | null {
  const target = dropTargets.get(dropKey(id, kind));
  return target ? target.el.getBoundingClientRect() : null;
}

function contains(rect: DOMRect, x: number, y: number): boolean {
  return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
}

/**
 * What is under the pointer, excluding the nodes being dragged.
 *
 * Kind decides first (a tile over a crumb is still the tile you aimed at); ties
 * inside a kind go to the smallest rect, which is the innermost — and therefore
 * topmost — target under the point.
 */
export function hitTestDropTarget(
  x: number,
  y: number,
  exclude: Set<NodeId>,
): { id: NodeId; kind: DropKind; el: HTMLElement } | null {
  let best: { id: NodeId; kind: DropKind; el: HTMLElement } | null = null;
  let bestRank = KIND_ORDER.length;
  let bestArea = Number.POSITIVE_INFINITY;

  for (const target of dropTargets.values()) {
    if (exclude.has(target.id)) continue;
    const rank = KIND_ORDER.indexOf(target.kind);
    if (rank > bestRank) continue;

    const rect = target.el.getBoundingClientRect();
    if (!contains(rect, x, y)) continue;

    const area = rect.width * rect.height;
    if (rank < bestRank || area < bestArea) {
      best = { id: target.id, kind: target.kind, el: target.el };
      bestRank = rank;
      bestArea = area;
    }
  }
  return best;
}

/** Viewport center of a tile: where a dropped stack has to land. */
export function tileCenter(id: NodeId): { x: number; y: number } | null {
  const rect = getTileRect(id);
  if (!rect) return null;
  return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
}
