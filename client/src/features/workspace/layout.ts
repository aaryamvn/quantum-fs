/**
 * Every fixed measurement of the workspace, in one file.
 *
 * The canvas, the chrome above it and the drag layer all have to agree on where
 * a tile is to within a pixel — a carried tile that lands 3px off its slot reads
 * as a bug. Sharing the numbers (rather than each surface owning its own padding)
 * is what keeps those independent layers registered to the same grid, and it
 * makes a density change one edit instead of a hunt.
 *
 * Pure data: no imports, safe in any runtime.
 */

/** Vault/folder rail on the left. */
export const SIDEBAR_W = 240;
/** Details pane on the right, when open. */
export const INSPECTOR_W = 288;
/** Breadcrumb + controls strip above the canvas. */
export const TOPBAR_H = 44;
/**
 * Strip reserved for the Tauri overlay titlebar (window buttons + drag region).
 *
 * Reserved in the browser too, deliberately: design QA screenshots have to match
 * the shipped window, and a layout that shifts between runtimes cannot be checked.
 */
export const TITLEBAR_INSET = 28;
/** Selection action bar under the top bar. */
export const ACTIONBAR_H = 44;

export const TILE_W = 112;
/** 8 top pad + 76 icon box + {@link TILE_LABEL_GAP} + 34 two-line label. */
export const TILE_H = 126;
export const TILE_GAP_X = 14;
export const TILE_GAP_Y = 18;
/** Icon box inside a tile; the label sits under it. */
export const TILE_ICON = 64;
/**
 * Air between the icon's box and the name under it.
 *
 * Nonzero on purpose: with the label hard against the box, a selected tile reads
 * as one tall slab instead of an icon with a name, and the two highlights (the
 * icon plate and the label pill) touch. Exported because anything that rebuilds
 * a tile's proportions outside the grid has to use the same number or the copy
 * lands a few pixels off the original.
 */
export const TILE_LABEL_GAP = 8;
/** Inset from the canvas edge to the first tile. */
export const CANVAS_PAD = 20;
/** Space kept clear at the bottom so the agent bar never covers the last row. */
export const CHATBAR_CLEARANCE = 96;

/**
 * The stacking order of the workspace's layers.
 *
 * A carried tile rides above the canvas and its chrome so it is never clipped by
 * the surface it is crossing, and menus sit above modals because a modal opens
 * menus. The gaps between the numbers are deliberate: a new layer slots in
 * without renumbering the ones already registered to this ladder.
 */
export const Z = {
  canvas: 10,
  chrome: 20,
  dragLayer: 40,
  modal: 60,
  menu: 70,
  tooltip: 80,
} as const;

/**
 * The flight a moved tile takes.
 *
 * A spring, not a duration: a move that lands with a touch of weight reads as a
 * physical handoff rather than a jump cut. Shared so the drag layer and the
 * tiles settling behind it carry the same weight.
 */
export const SPRING_FLIGHT = { type: "spring", stiffness: 380, damping: 32, mass: 0.9 } as const;

/** The house ease. Every non-spring transition in the workspace uses it. */
export const EASE: [number, number, number, number] = [0.16, 1, 0.3, 1];
