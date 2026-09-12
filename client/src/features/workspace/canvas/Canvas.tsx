/**
 * The icon grid — the surface the whole workspace exists to show.
 *
 * Three decisions shape this file.
 *
 * *Delegation over per-tile listeners*: a folder can hold hundreds of tiles, and
 * hanging pointer/context handlers on each one costs a closure per tile per
 * render. The canvas listens once and resolves the target with
 * `closest("[data-node-id]")`, which also means the empty background is handled
 * by the same code path that handles a tile — the difference is one `if`, not two
 * components. The Tile's own callbacks still win when it wires them; a `WeakSet`
 * of already-handled events keeps the fallback from running the same click twice.
 *
 * *Selection is the store's, geometry is the module's*: the marquee reads tile
 * rectangles from the geometry registry rather than from React, so a sweep over
 * 200 tiles measures the DOM directly and never re-renders to find out where
 * something is. Only the resulting id list goes into the store, and only when it
 * actually changes — a sweep that crosses no new tile is free.
 *
 * *One scene per folder*: the grid is keyed by folder **and** by `demoResetTick`,
 * so navigating and the scripted demo starting over are the same transition with
 * different timing. Tiles inside the scene animate their own reflow through
 * `layout`, which is disabled mid-drag: a tile that is being carried must not
 * also be springing to a new slot.
 */

import { AnimatePresence, LayoutGroup, motion, useReducedMotion } from "motion/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent } from "react";
import { useShallow } from "zustand/react/shallow";

import type { FsNode, NodeId } from "@/lib/backend";

import {
  CANVAS_PAD,
  CHATBAR_CLEARANCE,
  EASE,
  TILE_GAP_X,
  TILE_GAP_Y,
  TILE_W,
} from "../layout";
import type { DragController, TileDropState } from "../store";
import {
  getCanvasEl,
  getTileEl,
  getTileRect,
  setCanvasEl,
  useChildren,
  useWorkspace,
} from "../store";
import { CanvasEmpty } from "./CanvasEmpty";
import { Marquee } from "./Marquee";
import { marqueeRect, rectsIntersect } from "./canvasGeometry";
import type { CanvasRect, Point } from "./canvasGeometry";
import { Tile } from "./Tile";
import { useGridKeyboard } from "./useGridKeyboard";
import { useTypeahead } from "./useTypeahead";

export interface CanvasProps {
  /**
   * The drag module's handle. Optional so the canvas renders standalone (tests,
   * screenshots) — without it a pointerdown selects and nothing lifts.
   */
  drag?: DragController;
}

/** Pointer slop before a sweep is a sweep; below it the gesture is still a click. */
const MARQUEE_THRESHOLD = 4;
/** How close to an edge starts auto-scrolling, and how fast it goes, per frame. */
const AUTOSCROLL_EDGE = 24;
const AUTOSCROLL_STEP = 8;
/**
 * Two presses on the same tile inside this window are one double-click.
 *
 * Measured here rather than trusted to `detail`: WebKit reports 0 on a
 * pointerdown, and a pointer capture (which every press that could become a drag
 * takes) can retarget the click that would have carried the count.
 */
const DOUBLE_MS = 350;
/** A node opened twice inside this window is one gesture seen by two paths. */
const OPEN_DEDUPE_MS = 500;
/** A reflow after a move or a create: weighted, but settled inside a beat. */
const SETTLE = { type: "spring", stiffness: 500, damping: 40 } as const;
/** The first grid's tile-by-tile entrance, in seconds. */
const STAGGER_BASE = 0.05;
const STAGGER_STEP = 0.02;
const STAGGER_MAX = 0.5;

/**
 * Events a Tile's own handler already dealt with.
 *
 * Module-level and weak: the canvas cannot know whether the Tile it renders wires
 * `onPointerDown`, so it keeps a fallback — and this is what stops the fallback
 * from re-running a click the Tile already handled on the way up.
 */
const handledEvents = new WeakSet<Event>();

/** The name under a tile's icon; a double-click here is a rename, not an open. */
const LABEL_SELECTOR = '[data-testid="tile-label"]';

/** The two coordinates and the target every resolver below needs, and nothing else. */
interface HitLike {
  target: EventTarget | null;
  clientX: number;
  clientY: number;
}

/**
 * Which tile a gesture landed on.
 *
 * The event's own target is asked first — it is the truth whenever nothing
 * captured the pointer, and it still answers correctly for a tile the first
 * click has already navigated away from. `elementFromPoint` is the fallback for
 * the case that breaks the naive version: a press that could become a drag takes
 * pointer capture, and a captured pointer retargets the click (and the
 * double-click that follows) to the capturing element.
 */
function tileIdAt(e: HitLike): NodeId | null {
  const target = e.target as HTMLElement | null;
  const own = target?.closest<HTMLElement>("[data-node-id]")?.dataset.nodeId;
  if (own !== undefined && own !== "") return own;
  const under = document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null;
  const below = under?.closest<HTMLElement>("[data-node-id]")?.dataset.nodeId;
  return below !== undefined && below !== "" ? below : null;
}

/** Same two-step, for the one part of a tile that means something different. */
function isLabelAt(e: HitLike): boolean {
  const target = e.target as HTMLElement | null;
  if (target?.closest(LABEL_SELECTOR) != null) return true;
  const under = document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null;
  return under?.closest(LABEL_SELECTOR) != null;
}

/** One in-flight rubber-band. Lives in a ref: none of it is worth a render except the rect. */
interface Sweep {
  pointerId: number;
  /** Content coordinates, captured at pointerdown so auto-scroll cannot move it. */
  start: Point;
  /** Selection to add to, when the sweep began with ⇧ held. */
  base: NodeId[];
  additive: boolean;
  /** False until the pointer has traveled past the threshold. */
  active: boolean;
  clientX: number;
  clientY: number;
}

export function Canvas({ drag }: CanvasProps) {
  const reduced = useReducedMotion() ?? false;
  const containerRef = useRef<HTMLDivElement | null>(null);

  const folderId = useWorkspace((s) => s.folderId);
  const treeLoaded = useWorkspace((s) => s.treeLoaded);
  const treeError = useWorkspace((s) => s.treeError);
  const demoResetTick = useWorkspace((s) => s.demoResetTick);
  const selection = useWorkspace((s) => s.selection);
  const focusedId = useWorkspace((s) => s.focusedId);
  const renamingId = useWorkspace((s) => s.renamingId);

  // A shallow slice, so the 60-per-second `overTargetId` churn of a drag re-runs
  // this selector but only re-renders when the answer actually changed.
  const dragState = useWorkspace(
    useShallow((s) => ({
      active: s.drag.active,
      overTargetId: s.drag.overTargetId,
      overKind: s.drag.overKind,
    })),
  );

  const children = useChildren(folderId);
  const orderedIds = useMemo(() => children.map((node) => node.id), [children]);
  const names = useMemo(() => children.map((node) => node.name), [children]);
  const selected = useMemo(() => new Set(selection), [selection]);

  // Read by the sweep's rAF loop, which outlives the render that started it.
  const orderedRef = useRef(orderedIds);
  orderedRef.current = orderedIds;

  const sweep = useRef<Sweep | null>(null);
  const sweepFrame = useRef(0);
  const sweepIds = useRef("");
  const [band, setBand] = useState<CanvasRect | null>(null);

  /** The last press, and the last open: together they make a double-click idempotent. */
  const lastPress = useRef<{ id: NodeId; at: number } | null>(null);
  const lastOpen = useRef<{ id: NodeId; at: number } | null>(null);

  useGridKeyboard(containerRef, orderedIds);
  useTypeahead(containerRef, orderedIds, names);

  /**
   * Which tile the drag is over, and which two sit next to it.
   *
   * Neighbors get a "near" state so a folder that is about to swallow something
   * reads as a target *in a row of tiles* rather than a lone glow. A target that
   * is not in this grid (a breadcrumb, a sidebar row) leaves the grid untouched.
   */
  const dropStates = useMemo(() => {
    const map = new Map<NodeId, TileDropState>();
    if (!dragState.active || dragState.overTargetId === null) return map;
    if (dragState.overKind !== null && dragState.overKind !== "tile") return map;
    const index = orderedIds.indexOf(dragState.overTargetId);
    if (index === -1 || children[index].kind !== "folder") return map;

    map.set(dragState.overTargetId, "over");
    const before = orderedIds[index - 1];
    const after = orderedIds[index + 1];
    if (before !== undefined) map.set(before, "near");
    if (after !== undefined) map.set(after, "near");
    return map;
  }, [dragState, orderedIds, children]);

  /** The registries every overlay layer measures against point at this element. */
  useEffect(() => {
    const el = containerRef.current;
    setCanvasEl(el);
    return () => {
      if (getCanvasEl() === el) setCanvasEl(null);
    };
  }, []);

  /** Peers follow each other by folder; arriving anywhere has to say so. */
  useEffect(() => {
    if (folderId === null) return;
    useWorkspace.getState().publishPresence({ folderId });
  }, [folderId]);

  /**
   * Keep the focused tile visible.
   *
   * `block: "nearest"` is doing the work: it is a no-op when the tile is already
   * on screen, so the same effect serves a keyboard walk, a typeahead jump and a
   * plain click without any of them needing to know about scrolling. Suspended
   * mid-sweep and mid-drag, where the pointer — not the selection — owns the view.
   */
  useEffect(() => {
    if (focusedId === null || sweep.current !== null || dragState.active) return;
    getTileEl(focusedId)?.scrollIntoView({ block: "nearest" });
  }, [focusedId, dragState.active]);

  /** A node created below the fold opens its rename field where nobody can see it. */
  useEffect(() => {
    if (renamingId === null) return;
    getTileEl(renamingId)?.scrollIntoView({ block: "nearest" });
  }, [renamingId]);

  /** Stop a sweep that outlived its pointer (unmount, folder switch mid-gesture). */
  useEffect(() => {
    return () => {
      if (sweepFrame.current !== 0) cancelAnimationFrame(sweepFrame.current);
      sweepFrame.current = 0;
      sweep.current = null;
    };
  }, []);

  /** Recompute the band and the ids under it from the pointer's last position. */
  const updateSweep = (): void => {
    const el = containerRef.current;
    const current = sweep.current;
    if (!el || !current) return;

    const box = el.getBoundingClientRect();
    const rect = marqueeRect(
      current.start,
      { x: current.clientX - box.left, y: current.clientY - box.top },
      el.scrollTop,
    );
    if (!current.active) {
      if (Math.max(rect.w, rect.h) < MARQUEE_THRESHOLD) return;
      current.active = true;
    }
    setBand(rect);

    const hits: NodeId[] = [];
    for (const id of orderedRef.current) {
      const tile = getTileRect(id);
      if (!tile) continue;
      const inContent: CanvasRect = {
        x: tile.left - box.left,
        y: tile.top - box.top + el.scrollTop,
        w: tile.width,
        h: tile.height,
      };
      if (rectsIntersect(rect, inContent)) hits.push(id);
    }

    const next = current.additive
      ? [...current.base, ...hits.filter((id) => !current.base.includes(id))]
      : hits;
    const key = next.join(",");
    if (key === sweepIds.current) return;
    sweepIds.current = key;
    useWorkspace.getState().select(next);
  };

  /**
   * Sweeping past the edge scrolls, the way every canvas does.
   *
   * A frame loop rather than a pointermove handler: the pointer can be perfectly
   * still at the bottom edge and the view still has to keep moving.
   */
  const autoScroll = (): void => {
    const el = containerRef.current;
    const current = sweep.current;
    if (!el || !current) {
      sweepFrame.current = 0;
      return;
    }
    // Nothing scrolls until the gesture is a sweep: a press-and-hold near the
    // bottom edge is still a click, and a click must not move the view.
    if (!current.active) {
      sweepFrame.current = requestAnimationFrame(autoScroll);
      return;
    }
    const box = el.getBoundingClientRect();
    let delta = 0;
    if (current.clientY < box.top + AUTOSCROLL_EDGE) delta = -AUTOSCROLL_STEP;
    else if (current.clientY > box.bottom - AUTOSCROLL_EDGE) delta = AUTOSCROLL_STEP;
    if (delta !== 0) {
      const before = el.scrollTop;
      el.scrollTop = before + delta;
      if (el.scrollTop !== before) updateSweep();
    }
    sweepFrame.current = requestAnimationFrame(autoScroll);
  };

  const endSweep = (pointerId: number): void => {
    const el = containerRef.current;
    if (sweep.current === null) return;
    if (el?.hasPointerCapture(pointerId)) el.releasePointerCapture(pointerId);
    sweep.current = null;
    sweepIds.current = "";
    if (sweepFrame.current !== 0) cancelAnimationFrame(sweepFrame.current);
    sweepFrame.current = 0;
    setBand(null);
  };

  /**
   * Open a node once, however many paths decide it was opened.
   *
   * A folder navigates and a file opens — the same split `openNode` makes, said
   * here so the canvas never has to care which one it just double-clicked. The
   * window is what makes the two detectors (the delegated `dblclick` and the
   * second pointerdown) safe to have at once: whichever fires first does the
   * work, the other one lands inside the window and returns.
   */
  const openOnce = useCallback((id: NodeId): void => {
    const at = performance.now();
    const previous = lastOpen.current;
    if (previous !== null && previous.id === id && at - previous.at < OPEN_DEDUPE_MS) return;
    lastOpen.current = { id, at };

    const store = useWorkspace.getState();
    const node = store.nodes[id];
    if (node === undefined) return;
    if (node.kind === "folder") store.navigateTo(id);
    else store.openNode(id);
  }, []);

  /** Was this press the second one on the same tile, inside the double-click window? */
  const pressIsSecond = useCallback((e: ReactPointerEvent, id: NodeId): boolean => {
    const at = e.timeStamp > 0 ? e.timeStamp : performance.now();
    const previous = lastPress.current;
    const second =
      e.detail >= 2 || (previous !== null && previous.id === id && at - previous.at < DOUBLE_MS);
    lastPress.current = { id, at };
    return second;
  }, []);

  /**
   * The one selection rule, shared by the Tile's handler and the fallback.
   *
   * A pointerdown inside an existing multi-selection deliberately changes nothing:
   * that is the gesture that drags five files at once, and collapsing to one on
   * press would make it impossible.
   *
   * The second press of a double-click hands over to `openOnce` and stops there:
   * the press is never given to the drag module, so the gesture that opens a
   * folder cannot also be the gesture that starts carrying it. The press itself
   * is never `preventDefault`ed — that is what would stop the browser ever
   * synthesizing the `dblclick` this leans on.
   */
  const pressTile = useCallback(
    (e: ReactPointerEvent, id: NodeId): void => {
      const store = useWorkspace.getState();
      const mod = e.metaKey || e.ctrlKey;

      if (e.shiftKey) store.rangeSelect(id, orderedRef.current);
      else if (mod) store.toggleSelect(id);
      else if (!store.selection.includes(id)) store.select([id]);

      if (pressIsSecond(e, id)) {
        // On the name, the second press belongs to the rename the dblclick opens.
        if (!isLabelAt(e)) openOnce(id);
        return;
      }

      if (drag && !mod) {
        const carrying = useWorkspace.getState().selection;
        drag.start(e, carrying.length > 0 ? carrying : [id]);
      }
    },
    [drag, openOnce, pressIsSecond],
  );

  /** Keyboard only works on a focused canvas — but never steal a rename field's focus. */
  const focusCanvas = useCallback((e: ReactPointerEvent): void => {
    const target = e.target as HTMLElement | null;
    if (target?.closest("input, textarea, [contenteditable='true']")) return;
    containerRef.current?.focus({ preventScroll: true });
  }, []);

  // Stable identities: `Tile` is memoized, and a fresh closure per render would
  // make that memo useless — every presence packet would re-render every tile.
  const onTilePointerDown = useCallback(
    (e: ReactPointerEvent, node: FsNode): void => {
      if (e.button !== 0) return;
      handledEvents.add(e.nativeEvent);
      focusCanvas(e);
      pressTile(e, node.id);
    },
    [focusCanvas, pressTile],
  );

  const onTileContextMenu = useCallback((e: ReactMouseEvent, node: FsNode): void => {
    e.preventDefault();
    handledEvents.add(e.nativeEvent);
    useWorkspace.getState().openContextMenu(e.clientX, e.clientY, node.id);
  }, []);

  const onLabelDoubleClick = useCallback((e: ReactMouseEvent, node: FsNode): void => {
    handledEvents.add(e.nativeEvent);
    useWorkspace.getState().startRename(node.id);
  }, []);

  /**
   * Opening, delegated — the only path that survives every runtime.
   *
   * A per-tile `onDoubleClick` looks tidier and is wrong: the press that might
   * become a drag captures the pointer, and WebKit then delivers the click to
   * the tile rather than to whatever was under the finger, so a handler on any
   * element *inside* the tile never hears it. One listener above the whole grid
   * hears it either way, and {@link tileIdAt} puts the id back.
   */
  const onDoubleClick = useCallback(
    (e: ReactMouseEvent<HTMLDivElement>): void => {
      if (handledEvents.has(e.nativeEvent)) return;
      const target = e.target as HTMLElement | null;
      if (target?.closest("input, textarea, [contenteditable='true']")) return;

      const id = tileIdAt(e);
      if (id === null) return;
      if (isLabelAt(e)) {
        useWorkspace.getState().startRename(id);
        return;
      }
      openOnce(id);
    },
    [openOnce],
  );

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>): void => {
    if (e.button !== 0) return;
    if (handledEvents.has(e.nativeEvent)) return;

    const target = e.target as HTMLElement | null;
    if (target?.closest("input, textarea, [contenteditable='true']")) return;

    const tile = target?.closest<HTMLElement>("[data-node-id]");
    const id = tile?.dataset.nodeId;
    focusCanvas(e);
    if (id !== undefined && id !== "") {
      pressTile(e, id);
      return;
    }

    // Background: clear unless the gesture is additive, then start the band.
    const store = useWorkspace.getState();
    const additive = e.shiftKey || e.metaKey || e.ctrlKey;
    if (!additive && store.selection.length > 0) store.clearSelection();

    const el = containerRef.current;
    if (!el) return;
    const box = el.getBoundingClientRect();
    sweep.current = {
      pointerId: e.pointerId,
      start: { x: e.clientX - box.left, y: e.clientY - box.top + el.scrollTop },
      base: additive ? [...useWorkspace.getState().selection] : [],
      additive,
      active: false,
      clientX: e.clientX,
      clientY: e.clientY,
    };
    sweepIds.current = "";
    el.setPointerCapture(e.pointerId);
    if (sweepFrame.current === 0) sweepFrame.current = requestAnimationFrame(autoScroll);
  };

  const onPointerMove = (e: ReactPointerEvent<HTMLDivElement>): void => {
    const current = sweep.current;
    if (current === null || current.pointerId !== e.pointerId) return;
    current.clientX = e.clientX;
    current.clientY = e.clientY;
    updateSweep();
  };

  const onPointerUp = (e: ReactPointerEvent<HTMLDivElement>): void => {
    endSweep(e.pointerId);
  };

  const onContextMenu = (e: ReactMouseEvent<HTMLDivElement>): void => {
    e.preventDefault();
    if (handledEvents.has(e.nativeEvent)) return;
    const target = e.target as HTMLElement | null;
    const tile = target?.closest<HTMLElement>("[data-node-id]");
    const id = tile?.dataset.nodeId;
    useWorkspace.getState().openContextMenu(e.clientX, e.clientY, id ?? null);
  };

  /**
   * Was this render caused by the demo restarting rather than by navigation?
   *
   * The comparison is against a ref written in an effect, never during render, so
   * StrictMode's double render answers the same thing both times.
   */
  const shown = useRef({ folderId, tick: demoResetTick });
  const resetting = shown.current.folderId === folderId && shown.current.tick !== demoResetTick;
  useEffect(() => {
    shown.current = { folderId, tick: demoResetTick };
  }, [folderId, demoResetTick]);

  // Seconds, Motion's unit: a demo reset dissolves slower than a folder switch.
  const enterSec = resetting ? 0.32 : 0.18;
  const layoutTransition = reduced || dragState.active ? false : "position";

  /**
   * The first grid of an opened vault arrives tile by tile.
   *
   * The dive hands over a window that is still filling in, and a grid that
   * blinks on whole reads as a page load rather than as arriving somewhere. Only
   * the *first* scene does it: a folder switch is already a crossfade, and
   * staggering that too would put a beat between clicking a folder and seeing it.
   *
   * The ref is written during render on purpose — once, guarded, and to the same
   * value both times StrictMode renders — because the flag has to be stable for
   * the whole life of the scene it describes, which an effect could not promise.
   */
  const sceneKey = `${folderId ?? "none"}:${demoResetTick}`;
  const firstSceneKey = useRef<string | null>(null);
  if (treeLoaded && firstSceneKey.current === null) firstSceneKey.current = sceneKey;
  const entering = !reduced && firstSceneKey.current === sceneKey;

  const scene =
    treeError !== null ? (
      <CanvasEmpty kind="error" message={treeError} />
    ) : !treeLoaded ? (
      <CanvasEmpty kind="loading" />
    ) : children.length === 0 ? (
      <CanvasEmpty kind="empty-folder" />
    ) : (
      <div
        data-testid="canvas-grid"
        className="grid"
        style={{
          gridTemplateColumns: `repeat(auto-fill, ${TILE_W}px)`,
          columnGap: TILE_GAP_X,
          rowGap: TILE_GAP_Y,
          justifyContent: "start",
        }}
      >
        {children.map((node, index) => (
          <motion.div
            key={node.id}
            layout={layoutTransition}
            style={{ width: TILE_W }}
            initial={entering ? { opacity: 0, scale: 0.92 } : false}
            animate={entering ? { opacity: 1, scale: 1 } : undefined}
            transition={
              entering
                ? {
                    layout: SETTLE,
                    duration: 0.3,
                    ease: EASE,
                    // Reading order, capped: a folder of two hundred must not
                    // take four seconds to finish appearing.
                    delay: Math.min(STAGGER_BASE + STAGGER_STEP * index, STAGGER_MAX),
                  }
                : { layout: SETTLE }
            }
          >
            <Tile
              node={node}
              selected={selected.has(node.id)}
              dropState={dropStates.get(node.id) ?? "none"}
              onPointerDown={onTilePointerDown}
              onContextMenu={onTileContextMenu}
              onLabelDoubleClick={onLabelDoubleClick}
            />
          </motion.div>
        ))}
      </div>
    );

  return (
    <div
      ref={containerRef}
      data-testid="canvas"
      role="listbox"
      aria-multiselectable
      tabIndex={0}
      className="scroll-thin relative min-h-0 flex-1 overflow-x-hidden overflow-y-auto outline-none"
      style={{
        paddingTop: CANVAS_PAD,
        paddingRight: CANVAS_PAD,
        paddingLeft: CANVAS_PAD,
        paddingBottom: CHATBAR_CLEARANCE + 24,
      }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onDoubleClick={onDoubleClick}
      onContextMenu={onContextMenu}
    >
      <LayoutGroup>
        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={`${folderId ?? "none"}:${demoResetTick}`}
            initial={reduced ? { opacity: 0 } : { opacity: 0, scale: resetting ? 1 : 0.985 }}
            animate={reduced ? { opacity: 1 } : { opacity: 1, scale: 1 }}
            exit={{ opacity: 0, transition: { duration: 0.12, ease: EASE } }}
            transition={{ duration: enterSec, ease: EASE }}
            style={{ transformOrigin: "top left" }}
          >
            {scene}
          </motion.div>
        </AnimatePresence>
      </LayoutGroup>
      <Marquee rect={band} />
    </div>
  );
}
