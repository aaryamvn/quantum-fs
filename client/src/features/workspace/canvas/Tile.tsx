import { motion, useReducedMotion } from "motion/react";
import { memo, useEffect, useRef, useState } from "react";
import type { MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent } from "react";

import { FileIcon, FolderIcon } from "@/components/icons";
import type { FsNode } from "@/lib/backend";
import { withAlpha } from "@/lib/color";

import { TILE_H, TILE_ICON, TILE_W } from "../layout";
import { registerDropTarget, registerTile, useJustCreated, useWorkspace } from "../store";
import type { TileDropState } from "../store";
import { AvailabilityBadge } from "./AvailabilityBadge";
import { TileLabel } from "./TileLabel";
import {
  DROP_FLASH_MS,
  DROP_FLASH_TRANSITION,
  FLASH_TRANSITION,
  POP_MS,
  VIOLET,
  tileTransition,
  tileVariants,
} from "./tileMotion";
import type { TileVariant } from "./tileMotion";

/**
 * The icon's own square: bigger than the 64px drawing so the selection fill and
 * the availability badge both have somewhere to live that is not the label's
 * business. 8 + 76 + TILE_LABEL_GAP + 34 = TILE_H exactly.
 */
const ICON_BOX = 76;

export interface TileProps {
  node: FsNode;
  selected: boolean;
  dropState: TileDropState;
  onPointerDown(e: ReactPointerEvent<HTMLDivElement>, node: FsNode): void;
  onContextMenu(e: ReactMouseEvent, node: FsNode): void;
  onLabelDoubleClick(e: ReactMouseEvent, node: FsNode): void;
}

/**
 * One file or folder in the grid, and every state it can be caught in.
 *
 * One tile, one visual state. Selection, a drag hovering over it, a drag
 * carrying it and the pop of a newborn are told apart by *different* signals —
 * a plate under the icon, a swell, a fade, a spring — rather than by shades of
 * one, because more than one of them is true at once more often than not. There
 * is deliberately no second ring for the keyboard: the canvas owns focus, the
 * selection *is* where the keyboard is, and a tile wearing both an outline and a
 * pill was the one place in the grid where two marks meant one thing.
 *
 * Selection follows Finder: the icon gets a light plate and the *label* gets the
 * colored pill. Coloring the whole tile would win the fight against the icon's
 * own color, which is the thing you are actually navigating by.
 *
 * It never decides anything. Selection, drag, open and menu are the canvas's,
 * handed down as callbacks; what a drag is over is handed down as `dropState`.
 * The tile owns only what it can know alone: its own geometry registration and
 * how it animates between the states it is told it is in.
 *
 * Memoized because a folder of 200 tiles must not re-render for a store write
 * that concerns one of them.
 */
export const Tile = memo(function Tile({
  node,
  selected,
  dropState,
  onPointerDown,
  onContextMenu,
  onLabelDoubleClick,
}: TileProps) {
  const reduced = useReducedMotion() ?? false;
  const id = node.id;
  const isFolder = node.kind === "folder";

  const elRef = useRef<HTMLDivElement | null>(null);

  const renaming = useWorkspace((s) => s.renamingId === id);
  const dimmed = useWorkspace((s) => s.drag.active && s.drag.nodeIds.includes(id));

  // Birth is read once, at mount: a tile that re-renders mid-pop must not
  // restart it, and one that outlives the 2s window must not pop on a re-mount.
  const created = useJustCreated(id);
  const me = useWorkspace((s) => s.me?.peerId ?? null);
  const [popping, setPopping] = useState(() => created !== null);
  const [byPeer] = useState(() => created !== null && me !== null && created.by !== me);

  const flashTick = useWorkspace((s) => (s.dropFlashFolderId === id ? s.dropFlashTick : 0));
  const [flashing, setFlashing] = useState(0);
  /**
   * The tick this tile has already accounted for, seeded from the *first* value
   * it ever saw. The store never clears `dropFlashFolderId`/`dropFlashTick` — a
   * drop is a fact, not a mode — so a folder that swallowed something an hour ago
   * still mounts with its own tick showing. Comparing against a seeded ref makes
   * the flash fire on a *change* rather than on a value, which is what stops the
   * tile from re-playing an old drop every time you navigate back to its parent.
   */
  const seenTick = useRef(flashTick);

  useEffect(() => {
    const el = elRef.current;
    if (!el) return;
    const unregisterTile = registerTile(id, el);
    // Files are not drop targets; only a folder can swallow something.
    const unregisterTarget = isFolder ? registerDropTarget(id, el, "tile") : null;
    return () => {
      unregisterTile();
      unregisterTarget?.();
    };
  }, [id, isFolder]);

  useEffect(() => {
    if (!popping) return;
    const timer = window.setTimeout(() => setPopping(false), POP_MS);
    return () => window.clearTimeout(timer);
  }, [popping]);

  useEffect(() => {
    if (flashTick === seenTick.current) return;
    seenTick.current = flashTick;
    // The flash moved to another folder: drop ours rather than leaving a spent
    // overlay mounted for the rest of the session.
    if (flashTick === 0) {
      setFlashing(0);
      return;
    }
    setFlashing(flashTick);
    const timer = window.setTimeout(() => setFlashing(0), DROP_FLASH_MS);
    return () => window.clearTimeout(timer);
  }, [flashTick]);

  const variant: TileVariant = dimmed
    ? "dim"
    : dropState === "over"
      ? "over"
      : dropState === "near"
        ? "near"
        : "rest";

  /** The only ring a tile ever wears: a drag is about to land in this folder. */
  const ring = dropState === "over" ? `0 0 0 2px ${withAlpha(VIOLET, 0.6)}` : undefined;

  const iconPlate =
    dropState === "over" ? "bg-violet/10" : selected ? "bg-white/[0.08]" : "bg-transparent";

  return (
    <motion.div
      ref={elRef}
      data-node-id={id}
      data-kind={node.kind}
      data-testid="tile"
      role="option"
      aria-selected={selected}
      aria-label={node.name}
      className="relative flex select-none flex-col items-center rounded-[var(--radius-tile)] pt-[8px]
        outline-none focus:outline-none focus-visible:outline-none"
      style={{ width: TILE_W, height: TILE_H }}
      variants={tileVariants(reduced)}
      initial={popping ? "pop" : false}
      animate={variant}
      transition={popping ? tileTransition("pop", reduced) : tileTransition(variant, reduced)}
      onPointerDown={(e) => onPointerDown(e, node)}
      onContextMenu={(e) => onContextMenu(e, node)}
    >
      {/* Positioning frame for the badge: it never scales, so it never wobbles. */}
      <div className="relative" style={{ width: ICON_BOX, height: ICON_BOX }}>
        <div
          className={`absolute inset-0 flex items-center justify-center rounded-[10px] ${iconPlate}
            transition-colors duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)]`}
          style={{ boxShadow: ring }}
        >
          {isFolder ? (
            <FolderIcon
              color={node.color ?? "graphite"}
              size={TILE_ICON}
              open={dropState === "over"}
            />
          ) : (
            <FileIcon name={node.name} size={TILE_ICON} />
          )}
        </div>

        {!isFolder ? (
          <span className="absolute top-[2px] right-[6px]">
            <AvailabilityBadge node={node} />
          </span>
        ) : null}
      </div>

      <TileLabel
        node={node}
        selected={selected}
        renaming={renaming}
        onDoubleClick={(e) => onLabelDoubleClick(e, node)}
      />

      {/* Someone else made this. It says so once, in violet, and then never again. */}
      {popping && byPeer && !reduced ? (
        <motion.span
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 rounded-[var(--radius-tile)] bg-violet
            pulse-violet"
          initial={{ opacity: 0.35 }}
          animate={{ opacity: 0 }}
          transition={FLASH_TRANSITION}
        />
      ) : null}

      {/* A drop landed in here: the folder acknowledges it. */}
      {flashing !== 0 && !reduced ? (
        <motion.span
          key={flashing}
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 rounded-[var(--radius-tile)] bg-violet"
          initial={{ opacity: 0 }}
          animate={{ opacity: [0, 0.35, 0] }}
          transition={DROP_FLASH_TRANSITION}
        />
      ) : null}
    </motion.div>
  );
});
