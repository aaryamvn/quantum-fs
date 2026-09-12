/**
 * The arithmetic the icon grid needs, kept out of React.
 *
 * The canvas has to answer three questions that have nothing to do with
 * rendering — how many columns fit, where a keyboard move lands, and which tiles
 * a rubber-band touches — and answering them inside a component would mean
 * re-deriving them on every pointer frame of a marquee. They are pure functions
 * over numbers here so the marquee can call them 60 times a second, and so the
 * column count the keyboard uses is provably the same one CSS `auto-fill`
 * produces from the same width.
 *
 * Two coordinate spaces appear below and must never be mixed:
 * *container* coords are pixels from the canvas's top-left as it sits on screen,
 * *content* coords are the same but scrolled — `y + scrollTop` — which is the
 * space tiles live in and therefore the space a marquee is anchored in.
 */

import { CANVAS_PAD, TILE_GAP_X, TILE_W } from "../layout";

export interface Point {
  x: number;
  y: number;
}

/** A box in content coordinates: the marquee, and every tile it is tested against. */
export interface CanvasRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * How many tiles `repeat(auto-fill, TILE_W)` will put on a row at this width.
 *
 * Mirrors the CSS exactly: the track list is laid inside the padding box, and n
 * columns cost `n * TILE_W + (n - 1) * TILE_GAP_X`, so adding one gap to the
 * usable width turns that into a clean division. At least one column always,
 * even at a width that cannot hold a tile — a zero would make ArrowDown a no-op.
 */
export function columnsFor(width: number): number {
  const usable = width - 2 * CANVAS_PAD + TILE_GAP_X;
  return Math.max(1, Math.floor(usable / (TILE_W + TILE_GAP_X)));
}

/** Where an index sits in the grid; the keyboard moves by whole rows. */
export function indexToRowCol(index: number, columns: number): { row: number; col: number } {
  const cols = Math.max(1, columns);
  return { row: Math.floor(index / cols), col: index % cols };
}

/** Overlap test, edges excluded: a zero-area marquee touches nothing. */
export function rectsIntersect(a: CanvasRect, b: CanvasRect): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}

/**
 * The rubber-band, normalised, in content coordinates.
 *
 * `start` was captured in content coords at pointerdown, `current` is where the
 * pointer is *now* in container coords: adding the live `scrollTop` to it is what
 * keeps the band anchored to the content when the canvas auto-scrolls under a
 * held pointer. Dragging up or left is the same gesture as down or right, hence
 * the abs/min pair rather than a signed box.
 */
export function marqueeRect(start: Point, current: Point, scrollTop: number): CanvasRect {
  const x = current.x;
  const y = current.y + scrollTop;
  return {
    x: Math.min(start.x, x),
    y: Math.min(start.y, y),
    w: Math.abs(x - start.x),
    h: Math.abs(y - start.y),
  };
}
