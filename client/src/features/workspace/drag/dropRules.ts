/**
 * The one answer to "can this land here?", shared by the hit test and the drop.
 *
 * Three surfaces accept a drop — a folder tile, a breadcrumb, a sidebar vault
 * row — and all three name a folder by id, so one predicate covers them. Keeping
 * it pure (nodes in, boolean out) is what lets the pointer loop ask it sixty
 * times a second and the drop ask it once more, with no chance of the highlight
 * promising a move the store would then refuse.
 */

import type { FsNode, NodeId } from "@/lib/backend";
import { isDescendant } from "@/lib/path";

import { hitTestDropTarget, useWorkspace } from "../store";
import type { DropKind } from "../store";

/**
 * Whether `targetId` is a legal home for `dragIds`.
 *
 * Four refusals, in the order they are cheapest to check: the target has to be a
 * folder that exists; a thing cannot be dropped into itself; a folder cannot be
 * dropped into its own subtree (the move would orphan the tree); and a drag that
 * is already entirely inside the target is a no-op, which should not light up.
 * A mixed drag where only *some* items already live there still counts, because
 * the rest of them do move.
 */
export function canDropInto(
  nodes: Record<NodeId, FsNode>,
  dragIds: NodeId[],
  targetId: NodeId,
): boolean {
  const target = nodes[targetId];
  if (!target || target.kind !== "folder") return false;
  if (dragIds.length === 0) return false;

  let everyoneHome = true;
  for (const id of dragIds) {
    if (id === targetId) return false;
    const node = nodes[id];
    if (!node) continue;
    if (node.kind === "folder" && isDescendant(nodes, targetId, id)) return false;
    if (node.parentId !== targetId) everyoneHome = false;
  }
  return !everyoneHome;
}

/**
 * What the pointer is over, if it is over anything droppable.
 *
 * The dragged nodes are excluded from the hit test itself rather than filtered
 * afterwards: a tile sitting under its own ghost would otherwise shadow the
 * folder behind it and the drag would find no target at all.
 */
export function resolveDrop(
  x: number,
  y: number,
  dragIds: NodeId[],
): { id: NodeId; kind: DropKind } | null {
  const hit = hitTestDropTarget(x, y, new Set(dragIds));
  if (!hit) return null;
  const { nodes } = useWorkspace.getState();
  if (!canDropInto(nodes, dragIds, hit.id)) return null;
  return { id: hit.id, kind: hit.kind };
}
