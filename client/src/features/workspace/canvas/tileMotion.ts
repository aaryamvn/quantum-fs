/**
 * Every number the tile animates by, in one place.
 *
 * A tile is animated by three independent stories at once — it pops when it is
 * born, swells when a drag is over it, and shrinks back when the drag is over
 * its neighbor — and each of those has to read as the *same* gesture on a folder
 * and on a file. Sharing the constants is what keeps them identical; a spring
 * tuned in the tile and re-guessed in the drag layer is how a flight ends up
 * landing with a different weight than the pop that preceded it.
 *
 * Targets and transitions are deliberately split. Motion lets a variant carry
 * its own `transition`, which then outranks the component's — and the mount pop
 * has to override the resting transition on exactly the frame the tile appears.
 * So {@link tileVariants} carries geometry only and {@link tileTransition}
 * chooses the curve, which leaves the component one honest decision instead of
 * two that silently fight.
 *
 * Everything here animates transform or opacity, never a layout property: a grid
 * of 200 tiles re-laying-out mid-drag is the one thing that breaks the illusion
 * that a file is a physical object you are carrying.
 */

import type { Transition, Variants } from "motion/react";

import { EASE, SPRING_FLIGHT } from "../layout";

/** A newborn tile springs to size. Stiff and lightly damped: it should land, not settle. */
export const POP_SPRING: Transition = { type: "spring", stiffness: 420, damping: 26 };

/** A tile under a drag swells with the same weight a flight lands with. */
export const OVER_SPRING: Transition = SPRING_FLIGHT;

/** The violet wash over a tile a peer just created. */
export const FLASH_TRANSITION: Transition = { duration: 0.6, ease: EASE };

/** Long enough for the flash and the `.pulse-violet` ring (520ms) to both finish. */
export const POP_MS = 620;

/** A folder that just swallowed a drop: up to 0.35 and back out. */
export const DROP_FLASH_TRANSITION: Transition = {
  duration: 0.52,
  ease: EASE,
  times: [0, 0.35, 1],
};

export const DROP_FLASH_MS = 520;

/** The one violet the tile's overlays are painted in; matches `--color-violet`. */
export const VIOLET = "#4e0eff";

/**
 * The resting states of a tile, in precedence order when several are true:
 * being dragged beats being a drop target, which beats being nudged aside.
 */
export type TileVariant = "pop" | "rest" | "over" | "near" | "dim";

/**
 * Geometry only — see the file note for why the curves live next door.
 *
 * The reduced branch keeps every opacity difference (they carry the meaning:
 * dimmed = in flight, nudged = making room) and drops every scale and offset,
 * which is exactly what the setting asks for: tell me, don't move me.
 */
export function tileVariants(reduced: boolean): Variants {
  if (reduced) {
    return {
      pop: { scale: 1, y: 0, opacity: 0 },
      rest: { scale: 1, y: 0, opacity: 1 },
      over: { scale: 1, y: 0, opacity: 1 },
      near: { scale: 1, y: 0, opacity: 0.9 },
      dim: { scale: 1, y: 0, opacity: 0.3 },
    };
  }
  return {
    pop: { scale: 0.8, y: 0, opacity: 0 },
    rest: { scale: 1, y: 0, opacity: 1 },
    over: { scale: 1.06, y: 0, opacity: 1 },
    // Not a gap in the layout — a gap in the *reading* of it: the neighbors
    // shrink back so the target is the only thing at full size under the pointer.
    near: { scale: 0.98, y: 0, opacity: 0.9 },
    dim: { scale: 1, y: 0, opacity: 0.3 },
  };
}

/** Instant, for the reduced-motion branch: state changes, nothing travels. */
const NONE: Transition = { duration: 0 };

/** The house tween; short enough that a hover never feels like it lags the pointer. */
const QUICK: Transition = { duration: 0.18, ease: EASE };

/** Which curve carries a tile into {@link TileVariant}. */
export function tileTransition(variant: TileVariant, reduced: boolean): Transition {
  if (reduced) return NONE;
  switch (variant) {
    case "pop":
      return POP_SPRING;
    case "over":
      return OVER_SPRING;
    case "dim":
      return { duration: 0.12, ease: EASE };
    default:
      return QUICK;
  }
}
