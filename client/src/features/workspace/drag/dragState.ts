/**
 * The live state of a local drag, deliberately outside React.
 *
 * A drag produces a pointer event every frame. Routing those through React state
 * would re-render the grid sixty times a second for a gesture that changes
 * nothing about the tree, so the position lives in Motion values and the session
 * lives in a module singleton (docs/decisions/client-workspace.md). The store is
 * told only the coarse facts — a drag started, the target under the pointer
 * changed, it ended — which is exactly what other surfaces need to react to.
 *
 * The pub/sub below is the whole bridge back to React: the drag layer subscribes
 * once and re-renders only when a session starts or ends, never while it moves.
 */

import { motionValue } from "motion/react";
import type { MotionValue } from "motion/react";

import type { NodeId } from "@/lib/backend";

/**
 * Everything the drag layer reads per frame.
 *
 * `x`/`y` are the raw pointer in viewport pixels — the ghosts spring towards it
 * rather than sitting on it, so this is a target, not a position. `vx`/`vy` are
 * smoothed (a raw per-event delta is far too noisy to drive a visual) and `tilt`
 * is the degrees the carried stack leans into its own motion.
 */
export const dragMV: {
  x: MotionValue<number>;
  y: MotionValue<number>;
  vx: MotionValue<number>;
  vy: MotionValue<number>;
  tilt: MotionValue<number>;
} = {
  x: motionValue(0),
  y: motionValue(0),
  vx: motionValue(0),
  vy: motionValue(0),
  tilt: motionValue(0),
};

/** One pickup: what is being carried, from where, and by which pointer. */
export interface DragSession {
  /** The grabbed node leads; the rest of the selection trails it. */
  nodeIds: NodeId[];
  /** Where each tile sat at pickup — the ghosts start here and spring back here. */
  originRects: Record<NodeId, DOMRect>;
  /** Pointer minus the grabbed tile's top-left, so the ghost keeps the grip point. */
  grabOffset: { x: number; y: number };
  pointerId: number;
  /** The element holding pointer capture; every move/up arrives on it. */
  sourceEl: HTMLElement;
  startedAt: number;
  /** False until the pointer passes the threshold — before that, a click is still a click. */
  moved: boolean;
}

/** The one live session, or none. A box rather than a bare binding so importers see updates. */
export const session: { current: DragSession | null } = { current: null };

type SessionListener = () => void;

const listeners = new Set<SessionListener>();

/** Subscribe to session start/end. Returns its own unsubscribe, for an effect cleanup. */
export function onSession(listener: SessionListener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Announce a start or an end. Never called per move: that is what the Motion values are for. */
export function notifySession(): void {
  for (const listener of listeners) listener();
}

/**
 * The session a renderer should draw, which is not quite the session that exists:
 * a pointer that has gone down but not travelled 4px yet is still a click.
 *
 * Referentially stable between notifications, so it can be a
 * `useSyncExternalStore` snapshot without looping.
 */
export function getActiveSession(): DragSession | null {
  const current = session.current;
  return current && current.moved ? current : null;
}

/**
 * How a drag ends, visually: sucked into the folder it was dropped on, or sprung
 * back to where it came from.
 */
export type DragOutro = { kind: "suck"; x: number; y: number } | { kind: "return" };

let outroPlayer: ((outro: DragOutro) => Promise<void>) | null = null;

/**
 * The drag layer lends the controller its animation.
 *
 * The controller decides *what* happens on drop (move, or not) and the layer
 * decides what it *looks* like; neither imports the other. Returns the
 * unregister so the layer's effect cleanup is the whole story.
 */
export function setOutroPlayer(play: (outro: DragOutro) => Promise<void>): () => void {
  outroPlayer = play;
  return () => {
    if (outroPlayer === play) outroPlayer = null;
  };
}

/** Play the ending and resolve when it is over — immediately if nothing is drawing. */
export function playOutro(outro: DragOutro): Promise<void> {
  return outroPlayer ? outroPlayer(outro) : Promise.resolve();
}
