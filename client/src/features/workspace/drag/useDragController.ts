/**
 * The gesture half of dragging: threshold, capture, velocity, drop.
 *
 * Pointer events rather than HTML5 drag-and-drop, because HTML5 gives up the
 * things that make a drag feel native — it owns the cursor, it cannot animate a
 * ghost, it fires no useful velocity, and it cancels the moment the pointer
 * leaves the window.
 *
 * Two rules keep the gesture from being dropped on the floor. The listeners live
 * on `window`, not on the grabbed tile, so a release over the sidebar, over a
 * menu, or after the tile itself has re-rendered still ends the drag we started.
 * And pointer capture is taken at the *threshold*, never at pointerdown: a press
 * that never travels stays a plain click, which is what leaves the browser free
 * to pair two of them into the double-click that opens a folder.
 *
 * Nothing here re-renders React per move. The pointer goes into Motion values;
 * the store hears from us only when a drag begins, when the target under the
 * pointer changes, and when it ends.
 */

import { useEffect, useRef } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";

import type { NodeId } from "@/lib/backend";

import {
  getCanvasEl,
  getTileEl,
  getTileRect,
  hitTestDropTarget,
  IDLE_DRAG,
  tileCenter,
  useWorkspace,
} from "../store";
import type { DragController, DropKind } from "../store";
import { dragMV, notifySession, playOutro, session } from "./dragState";
import { canDropInto, resolveDrop } from "./dropRules";

/** Travel, in pixels, before a press becomes a drag. Below it the canvas still owns the click. */
const MOVE_THRESHOLD = 4;
/** Smoothing window for velocity, in ms: long enough to be steady, short enough to feel live. */
const VELOCITY_TAU = 80;
/** The stack never leans further than this, however fast the pointer moves. */
const MAX_TILT_DEG = 1.5;
/** Pointer speed (px/s) that earns a full lean. */
const TILT_FULL_SPEED = 600;
/** How close to the canvas edge the pointer must be before the canvas scrolls itself. */
const AUTO_SCROLL_EDGE = 56;
/** Hard cap on auto-scroll speed, in px per frame. */
const AUTO_SCROLL_MAX = 18;
/** Below this idle time the pointer is still considered moving, so velocity is not decayed. */
const IDLE_MS = 24;

/**
 * What counts as a drop target in the DOM, when the registry cannot answer.
 *
 * The registry is the primary answer — it knows each target's kind and resolves
 * overlaps between them — but it is a snapshot of rects, and a target that
 * scrolled, re-rendered or registered late between the last frame and the
 * release would be missed. So the release asks the DOM under the pointer as
 * well: a folder tile, a breadcrumb, or a sidebar vault row. The drag layer is
 * `pointer-events: none`, so the carried ghosts never shadow the answer.
 */
const FALLBACK_DROP_SELECTOR =
  '[data-node-id][data-kind="folder"], [data-testid="breadcrumbs"] [data-node-id], [data-vault-id]';

/**
 * The node a DOM drop target names, or null when it names nothing droppable.
 *
 * A sidebar row carries its vault id rather than a node id, and the folder it
 * stands for is that vault's root — the same id `ServerTree` registers.
 */
function fallbackTarget(x: number, y: number, dragIds: NodeId[]): NodeId | null {
  if (typeof document === "undefined") return null;
  const el = document.elementFromPoint(x, y)?.closest<HTMLElement>(FALLBACK_DROP_SELECTOR) ?? null;
  if (!el) return null;
  const vaultId = el.getAttribute("data-vault-id");
  const id = el.getAttribute("data-node-id") ?? (vaultId === null ? null : `root_${vaultId}`);
  if (id === null) return null;
  return canDropInto(useWorkspace.getState().nodes, dragIds, id) ? id : null;
}

/** Registry first, DOM second: the two agree everywhere except at the edges. */
function resolveTarget(x: number, y: number, dragIds: NodeId[]): NodeId | null {
  return resolveDrop(x, y, dragIds)?.id ?? fallbackTarget(x, y, dragIds);
}

function clamp(value: number, min: number, max: number): number {
  return value < min ? min : value > max ? max : value;
}

/** The controller, plus the teardown the hook needs on unmount. */
interface OwnedController extends DragController {
  dispose(): void;
}

/**
 * One drag machine. Built outside React so every handler is a plain closure over
 * mutable locals — the alternative, refs on refs, reads far worse for the same
 * behavior.
 */
function createController(): OwnedController {
  let el: HTMLElement | null = null;
  let pointerId = -1;
  let startX = 0;
  let startY = 0;
  let lastX = 0;
  let lastY = 0;
  let lastMoveAt = 0;
  let lastFrameAt = 0;
  let vx = 0;
  let vy = 0;
  let overId: NodeId | null = null;
  let overKind: DropKind | null = null;
  /** True once the threshold is passed: the only state that licenses a store write. */
  let begun = false;
  /** True from pointerup/Escape until the outro finishes; blocks a second ending. */
  let finishing = false;
  let rafId: number | null = null;

  /** Re-hit-test, and tell the store only when the answer actually changed. */
  function updateTarget(): void {
    const current = session.current;
    if (!current) return;
    const hit = resolveDrop(dragMV.x.get(), dragMV.y.get(), current.nodeIds);
    const id = hit?.id ?? null;
    const kind = hit?.kind ?? null;
    if (id === overId && kind === overKind) return;
    overId = id;
    overKind = kind;
    useWorkspace.getState().setDrag({ overTargetId: id, overKind: kind });
  }

  /** Exponentially smoothed velocity, and the lean that follows from it. */
  function sample(x: number, y: number, at: number): void {
    const dt = Math.max(1, at - lastMoveAt);
    const alpha = 1 - Math.exp(-dt / VELOCITY_TAU);
    vx += (((x - lastX) / dt) * 1000 - vx) * alpha;
    vy += (((y - lastY) / dt) * 1000 - vy) * alpha;
    lastX = x;
    lastY = y;
    lastMoveAt = at;
    publishMotion();
  }

  function publishMotion(): void {
    dragMV.vx.set(vx);
    dragMV.vy.set(vy);
    dragMV.tilt.set(clamp(vx / TILT_FULL_SPEED, -1, 1) * MAX_TILT_DEG);
  }

  /**
   * Scroll the canvas when the pointer hovers its top or bottom edge, so a drag
   * can reach a folder that is off screen. Returns true when it actually moved,
   * because every drop target under the pointer has just shifted.
   */
  function autoScroll(): boolean {
    const canvas = getCanvasEl();
    if (!canvas) return false;
    const rect = canvas.getBoundingClientRect();
    const x = dragMV.x.get();
    const y = dragMV.y.get();
    if (x < rect.left || x > rect.right) return false;

    let delta = 0;
    if (y < rect.top + AUTO_SCROLL_EDGE) delta = -(rect.top + AUTO_SCROLL_EDGE - y) / AUTO_SCROLL_EDGE;
    else if (y > rect.bottom - AUTO_SCROLL_EDGE) delta = (y - (rect.bottom - AUTO_SCROLL_EDGE)) / AUTO_SCROLL_EDGE;
    if (delta === 0) return false;

    const before = canvas.scrollTop;
    const max = Math.max(0, canvas.scrollHeight - canvas.clientHeight);
    canvas.scrollTop = clamp(before + clamp(delta, -1, 1) * AUTO_SCROLL_MAX, 0, max);
    return canvas.scrollTop !== before;
  }

  /**
   * The only per-frame work: bleed velocity away when the pointer stops (so the
   * stack straightens instead of staying frozen mid-lean) and run the edge scroll.
   */
  function frame(at: number): void {
    rafId = requestAnimationFrame(frame);
    if (!begun) return;
    const dt = Math.max(1, at - lastFrameAt);
    lastFrameAt = at;
    if (at - lastMoveAt > IDLE_MS) {
      const decay = Math.exp(-dt / VELOCITY_TAU);
      vx *= decay;
      vy *= decay;
      publishMotion();
    }
    if (autoScroll()) updateTarget();
  }

  function onMove(event: PointerEvent): void {
    const current = session.current;
    if (!current || finishing || event.pointerId !== pointerId) return;
    dragMV.x.set(event.clientX);
    dragMV.y.set(event.clientY);

    if (!begun) {
      if (Math.hypot(event.clientX - startX, event.clientY - startY) < MOVE_THRESHOLD) return;
      begin();
    }
    sample(event.clientX, event.clientY, performance.now());
    updateTarget();
  }

  /** Threshold passed: the press is a drag. This is the first store write of the gesture. */
  function begin(): void {
    const current = session.current;
    if (!current) return;
    current.moved = true;
    begun = true;
    // Now that the press is certainly a drag, take the pointer: the tile under
    // it can re-render or scroll away and the gesture still belongs to us.
    // Best effort — a synthetic pointer id (some test harnesses) throws, and the
    // window listeners carry the drag on their own.
    try {
      current.sourceEl.setPointerCapture(current.pointerId);
    } catch {
      /* no capture available */
    }
    const store = useWorkspace.getState();
    store.setDrag({ active: true, nodeIds: current.nodeIds, overTargetId: null, overKind: null });
    store.publishPresence({ draggingNodeIds: current.nodeIds });
    notifySession();
  }

  /** Where the ghosts should converge for a drop: the tile's center, or the target's. */
  function dropCenter(targetId: NodeId, dragIds: NodeId[]): { x: number; y: number } {
    const center = tileCenter(targetId);
    if (center) return center;
    const hit = hitTestDropTarget(dragMV.x.get(), dragMV.y.get(), new Set(dragIds));
    if (hit) {
      const rect = hit.el.getBoundingClientRect();
      return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
    }
    return { x: dragMV.x.get(), y: dragMV.y.get() };
  }

  async function onUp(event: PointerEvent): Promise<void> {
    const current = session.current;
    if (!current || finishing || event.pointerId !== pointerId) return;
    finishing = true;
    detach();

    // A press that never travelled is a click; the canvas has already handled it.
    if (!begun) {
      end();
      return;
    }

    dragMV.x.set(event.clientX);
    dragMV.y.set(event.clientY);

    // Resolved from the release point rather than trusted from the last frame:
    // an auto-scroll, a re-render or a late registration can have moved the grid
    // under the pointer since the last hit test.
    const targetId = resolveTarget(event.clientX, event.clientY, current.nodeIds);
    if (targetId === null) {
      await playOutro({ kind: "return" });
      end();
      return;
    }

    const center = dropCenter(targetId, current.nodeIds);
    await playOutro({ kind: "suck", x: center.x, y: center.y });
    // Awaited: the tiles stay carried until the daemon has taken them, so a
    // refusal can still put them back on the squares they came from.
    const moved = await useWorkspace.getState().moveNodes(current.nodeIds, targetId);
    // The move was refused (a stale target, a daemon error): put them back rather
    // than leaving a hole where the tiles used to be.
    if (!moved) await playOutro({ kind: "return" });
    end();
  }

  function onKeyDown(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;
    if (!session.current || finishing) return;
    event.preventDefault();
    cancel();
  }

  function cancel(): void {
    if (!session.current || finishing) return;
    finishing = true;
    detach();
    if (!begun) {
      end();
      return;
    }
    void playOutro({ kind: "return" }).then(end);
  }

  /**
   * Listen for the rest of the gesture on the window.
   *
   * On the window rather than on the grabbed tile: capture is not taken until
   * the threshold, the tile can re-render out from under a listener bound to it,
   * and a release anywhere — over the sidebar, over a menu, past the last frame
   * the tile was drawn in — still has to end the drag we started.
   */
  function attach(): void {
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUpHandler);
    window.addEventListener("pointercancel", onPointerCancel);
    window.addEventListener("lostpointercapture", onLostCapture);
    window.addEventListener("keydown", onKeyDown);
  }

  /** Stop listening and stop the frame loop; the session stays alive for the outro. */
  function detach(): void {
    if (rafId !== null) {
      cancelAnimationFrame(rafId);
      rafId = null;
    }
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUpHandler);
    window.removeEventListener("pointercancel", onPointerCancel);
    window.removeEventListener("lostpointercapture", onLostCapture);
    window.removeEventListener("keydown", onKeyDown);
  }

  function onUpHandler(event: PointerEvent): void {
    void onUp(event);
  }

  function onPointerCancel(event: PointerEvent): void {
    if (event.pointerId !== pointerId) return;
    cancel();
  }

  /**
   * The pointer was taken away from us mid-drag — the captured tile was removed
   * from the DOM, or the browser handed the pointer to something else. There is
   * no release coming, so the drag ends the only honest way it can.
   */
  function onLostCapture(event: PointerEvent): void {
    if (event.pointerId !== pointerId || finishing || !begun) return;
    cancel();
  }

  /** Release everything and hand the store back its resting state. */
  function end(): void {
    detach();
    try {
      if (el && pointerId >= 0 && el.hasPointerCapture(pointerId)) {
        el.releasePointerCapture(pointerId);
      }
    } catch {
      /* the pointer is already gone */
    }
    if (begun) {
      const store = useWorkspace.getState();
      store.setDrag(IDLE_DRAG);
      store.publishPresence({ draggingNodeIds: [] });
    }
    session.current = null;
    notifySession();

    el = null;
    pointerId = -1;
    begun = false;
    finishing = false;
    overId = null;
    overKind = null;
    vx = 0;
    vy = 0;
    dragMV.vx.set(0);
    dragMV.vy.set(0);
    dragMV.tilt.set(0);
  }

  /**
   * Which of the dragged nodes the pointer actually went down on. It leads the
   * stack and owns the grip point, so a five-item drag still hangs off the tile
   * the hand is on.
   */
  function grabbedId(target: HTMLElement, ids: NodeId[]): NodeId {
    const attr = target.closest("[data-node-id]")?.getAttribute("data-node-id");
    if (attr && ids.includes(attr)) return attr;
    for (const id of ids) {
      const tile = getTileEl(id);
      if (tile && (tile === target || tile.contains(target) || target.contains(tile))) return id;
    }
    return ids[0];
  }

  return {
    start(event: ReactPointerEvent, nodeIds: NodeId[]): void {
      if (session.current || finishing) return;
      if (event.button !== 0) return;
      const target = event.currentTarget as HTMLElement | null;
      if (!target || nodeIds.length === 0) return;

      const lead = grabbedId(target, nodeIds);
      const ordered = [lead, ...nodeIds.filter((id) => id !== lead)];
      const originRects: Record<NodeId, DOMRect> = {};
      for (const id of ordered) {
        const rect = getTileRect(id);
        if (rect) originRects[id] = rect;
      }
      const anchor = originRects[lead] ?? target.getBoundingClientRect();

      el = target;
      pointerId = event.pointerId;
      startX = lastX = event.clientX;
      startY = lastY = event.clientY;
      lastMoveAt = lastFrameAt = performance.now();
      vx = 0;
      vy = 0;
      overId = null;
      overKind = null;
      dragMV.x.jump(event.clientX);
      dragMV.y.jump(event.clientY);
      dragMV.vx.jump(0);
      dragMV.vy.jump(0);
      dragMV.tilt.jump(0);

      session.current = {
        nodeIds: ordered,
        originRects,
        grabOffset: { x: event.clientX - anchor.left, y: event.clientY - anchor.top },
        pointerId: event.pointerId,
        sourceEl: target,
        startedAt: Date.now(),
        moved: false,
      };

      // No pointer capture yet: see `begin`. Until the threshold this press is
      // still a click, and a captured pointer is what breaks the double-click
      // that opens a folder.
      attach();
      rafId = requestAnimationFrame(frame);
    },

    cancel,

    dispose(): void {
      if (session.current) {
        finishing = true;
        end();
      } else {
        detach();
      }
    },
  };
}

/**
 * One controller per mount, handed to the canvas so a tile's pointerdown can
 * start a drag. Stable for the life of the component: the canvas passes it
 * straight into memoized tiles, so a new identity per render would re-render
 * every tile in the folder.
 */
export function useDragController(): DragController {
  const ref = useRef<OwnedController | null>(null);
  if (ref.current === null) ref.current = createController();
  const controller = ref.current;

  useEffect(() => () => controller.dispose(), [controller]);

  return controller;
}
