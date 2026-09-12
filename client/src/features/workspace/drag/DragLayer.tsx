/**
 * What a drag looks like: the tiles you picked up, flying above everything.
 *
 * The real tiles never move — they dim in place and this layer carries copies —
 * because a grid that reflows under the pointer loses the very thing a drag
 * needs, a stable map of where things are. The ghosts spring towards the pointer
 * instead of sitting on it, which is what reads as weight, and each one springs
 * towards the ghost in front of it, so a multi-select trails in a line rather
 * than moving as one rigid block.
 *
 * Not one value here is React state. Position, lean, scale and opacity are all
 * Motion values driven by a single frame callback, so a drag across the window
 * renders this component exactly twice: once when it starts, once when it ends.
 */

import {
  animate,
  motion,
  useAnimationFrame,
  useMotionValue,
  useReducedMotion,
  useSpring,
  useTransform,
} from "motion/react";
import type { MotionValue } from "motion/react";
import { useCallback, useEffect, useRef, useSyncExternalStore } from "react";

import { FileIcon, FolderIcon } from "@/components/icons";
import type { FsNode } from "@/lib/backend";

import { EASE, TILE_H, TILE_ICON, TILE_W, Z } from "../layout";
import { getTileRect, useWorkspace } from "../store";
import { dragMV, getActiveSession, onSession, session, setOutroPlayer } from "./dragState";
import type { DragOutro } from "./dragState";

/** Beyond five the stack is unreadable; the rest are counted on a pill instead. */
const MAX_GHOSTS = 5;
/** Each ghost sits this far behind the one in front, so the stack fans. */
const STACK_FAN = 4;
/** The flight spring — matched to SPRING_FLIGHT, so a carried tile has the same weight everywhere. */
const SPRING_CFG = { stiffness: 380, damping: 32, mass: 0.9 } as const;

const PICKUP_MS = 80;
const SUCK_MS = 220;
const SQUASH_MS = 180;
const FADE_MS = 160;
/** The lift, the moment it leaves the grid. */
const PICKUP_SCALE = 1.06;
/** How small a tile gets as the folder swallows it. */
const SUCK_SCALE = 0.2;
/**
 * The tile's own icon frame, repeated here rather than imported.
 *
 * A ghost that is a pixel off its tile flashes at pickup, so these have to match
 * `canvas/Tile`; they are copied because the drag layer must not depend on the
 * canvas (it draws over the sidebar and the breadcrumbs too).
 */
const ICON_BOX = 76;
const TILE_PAD_TOP = 8;

/** What the layer lends the ghost: its live position, and a way to end it. */
interface GhostHandle {
  x: MotionValue<number>;
  y: MotionValue<number>;
  play(outro: DragOutro): Promise<void>;
}

/**
 * The carried stack.
 *
 * Mounted once for the workspace; it renders nothing at all until a pointer
 * passes the drag threshold.
 */
export function DragLayer() {
  const reduced = useReducedMotion() ?? false;
  const active = useSyncExternalStore(onSession, getActiveSession, getActiveSession);
  const nodes = useWorkspace((s) => s.nodes);

  const handles = useRef<Map<number, GhostHandle>>(new Map()).current;
  /** False while an outro plays: the ending owns the ghosts, the pointer no longer does. */
  const feeding = useRef(true);

  const register = useCallback(
    (index: number, handle: GhostHandle) => {
      handles.set(index, handle);
      return () => {
        if (handles.get(index) === handle) handles.delete(index);
      };
    },
    [handles],
  );

  // The whole drag, per frame: the lead ghost chases the pointer, every other
  // ghost chases the one in front of it. Reading `get()` mid-loop is what makes
  // the trail — each follower aims at where its leader is *now*, not where the
  // pointer is.
  useAnimationFrame(() => {
    if (!feeding.current) return;
    const current = session.current;
    if (!current || !current.moved) return;
    let tx = dragMV.x.get() - current.grabOffset.x;
    let ty = dragMV.y.get() - current.grabOffset.y;
    for (let i = 0; i < MAX_GHOSTS; i += 1) {
      const handle = handles.get(i);
      if (!handle) continue;
      if (reduced) {
        handle.x.jump(tx);
        handle.y.jump(ty);
      } else {
        handle.x.set(tx);
        handle.y.set(ty);
      }
      tx = handle.x.get() + STACK_FAN;
      ty = handle.y.get() + STACK_FAN;
    }
  });

  useEffect(() => {
    return setOutroPlayer(async (outro) => {
      feeding.current = false;
      await Promise.all([...handles.values()].map((handle) => handle.play(outro)));
    });
  }, [handles]);

  useEffect(() => {
    if (active) feeding.current = true;
  }, [active]);

  if (!active) return null;

  const carried = active.nodeIds.filter((id) => nodes[id] !== undefined);
  const shown = carried.slice(0, MAX_GHOSTS);
  const hidden = carried.length - shown.length;

  return (
    <div
      data-testid="drag-layer"
      className="pointer-events-none fixed inset-0"
      style={{ zIndex: Z.dragLayer }}
    >
      {shown.map((id, index) => (
        <Ghost
          key={`${active.startedAt}-${id}`}
          index={index}
          node={nodes[id]}
          origin={active.originRects[id] ?? null}
          grab={active.grabOffset}
          extra={index === 0 ? hidden : 0}
          reduced={reduced}
          register={register}
        />
      ))}
    </div>
  );
}

interface GhostProps {
  /** 0 is the grabbed tile and leads the stack; the rest trail it. */
  index: number;
  node: FsNode;
  /** Where the tile sat at pickup. The flight's start; a cancel re-measures. */
  origin: DOMRect | null;
  grab: { x: number; y: number };
  /** Items beyond the five drawn, shown as a pill on the lead ghost. */
  extra: number;
  reduced: boolean;
  register(index: number, handle: GhostHandle): () => void;
}

/**
 * One carried tile.
 *
 * Two nested elements rather than one: the outer is the spring that follows the
 * pointer, the inner is everything an ending does to it. Keeping them apart is
 * what lets the suck-in and the spring-back be exact — the outer freezes where
 * it is and the inner animates a known delta, so the tile lands on the folder's
 * center (or back on its own square) to the pixel instead of racing a spring
 * that is still catching up.
 */
function Ghost({ index, node, origin, grab, extra, reduced, register }: GhostProps) {
  const startX = origin ? origin.left : dragMV.x.get() - grab.x;
  const startY = origin ? origin.top : dragMV.y.get() - grab.y;
  const x = useSpring(startX, SPRING_CFG);
  const y = useSpring(startY, SPRING_CFG);

  const offsetX = useMotionValue(0);
  const offsetY = useMotionValue(0);
  const scale = useMotionValue(1);
  const squash = useMotionValue(1);
  const opacity = useMotionValue(1);
  const shadow = useMotionValue(0);
  // Trailing ghosts lean half as far: a stack that all tilts identically reads as
  // one sheet of cardboard rather than loose objects.
  const rotate = useTransform(dragMV.tilt, (tilt) =>
    reduced ? 0 : index === 0 ? tilt : tilt / 2,
  );

  useEffect(() => {
    if (reduced) return;
    const lift = animate(scale, PICKUP_SCALE, { duration: PICKUP_MS / 1000, ease: EASE });
    const drop = animate(shadow, 1, { duration: PICKUP_MS / 1000, ease: EASE });
    return () => {
      lift.stop();
      drop.stop();
    };
  }, [reduced, scale, shadow]);

  useEffect(() => {
    const handle: GhostHandle = {
      x,
      y,
      async play(outro) {
        // Park the follow spring on its own current value so it stops chasing;
        // from here the ending is the only thing moving this ghost.
        x.jump(x.get());
        y.jump(y.get());

        if (outro.kind === "suck") {
          const transition = { duration: reduced ? 0 : SUCK_MS / 1000, ease: "easeInOut" as const };
          await Promise.all([
            animate(offsetX, outro.x - (x.get() + TILE_W / 2), transition).finished,
            animate(offsetY, outro.y - (y.get() + TILE_H / 2), transition).finished,
            animate(scale, SUCK_SCALE, transition).finished,
            animate(opacity, 0, transition).finished,
            animate(shadow, 0, transition).finished,
          ]);
          return;
        }

        // Measured now, not at pickup: an auto-scroll during the drag moves the
        // real tile, and a spring-back to the rect it had when it was picked up
        // would land the ghost on empty canvas.
        const home = getTileRect(node.id) ?? origin;

        // Nothing to go back to (the tile scrolled out of the tree, or a peer
        // deleted it mid-drag): fade rather than fly somewhere arbitrary.
        if (!home) {
          await animate(opacity, 0, { duration: reduced ? 0 : FADE_MS / 1000, ease: EASE }).finished;
          return;
        }

        const spring = reduced
          ? { duration: 0 }
          : ({ type: "spring", ...SPRING_CFG } as const);
        const settle = { duration: reduced ? 0 : SQUASH_MS / 1000, ease: EASE };
        await Promise.all([
          animate(offsetX, home.left - x.get(), spring).finished,
          animate(offsetY, home.top - y.get(), spring).finished,
          animate(scale, 1, settle).finished,
          animate(shadow, 0, settle).finished,
          // Restores the ghost after a *refused* drop, where the suck-in has
          // already faded it out; on a plain cancel it is a no-op.
          animate(opacity, 1, settle).finished,
        ]);
        if (reduced) return;
        // The landing: a shallow squash as the weight arrives, played on the ghost
        // in the last frames before the real tile takes its place again.
        await animate(squash, [0.96, 1], { duration: SQUASH_MS / 1000, ease: EASE }).finished;
      },
    };
    return register(index, handle);
  }, [index, register, node.id, origin, reduced, x, y, offsetX, offsetY, scale, squash, opacity, shadow]);

  return (
    <motion.div
      data-node-id={node.id}
      className="absolute top-0 left-0"
      style={{ x, y, width: TILE_W, height: TILE_H, zIndex: MAX_GHOSTS - index }}
    >
      <motion.div
        className="relative flex h-full w-full flex-col items-center"
        style={{
          x: offsetX,
          y: offsetY,
          scale,
          scaleY: squash,
          rotate,
          opacity,
          paddingTop: TILE_PAD_TOP,
        }}
      >
        {!reduced && (
          <motion.div
            aria-hidden="true"
            className="absolute h-[16px] w-[64px] rounded-[50%]"
            style={{
              top: TILE_PAD_TOP + ICON_BOX - 12,
              left: "50%",
              marginLeft: -32,
              opacity: shadow,
              background: "radial-gradient(closest-side, rgba(0,0,0,0.55), rgba(0,0,0,0))",
              filter: "blur(8px)",
            }}
          />
        )}

        <div
          className="flex items-center justify-center"
          style={{ width: ICON_BOX, height: ICON_BOX }}
        >
          {node.kind === "folder" ? (
            <FolderIcon color={node.color ?? "graphite"} size={TILE_ICON} />
          ) : (
            <FileIcon name={node.name} size={TILE_ICON} />
          )}
        </div>

        <span
          className="max-w-[104px] overflow-hidden px-[6px] py-[2px] text-center text-[12.5px]
            leading-[15px] break-words text-fg"
          style={{ display: "-webkit-box", WebkitBoxOrient: "vertical", WebkitLineClamp: 2 }}
        >
          {node.name}
        </span>

        {extra > 0 && (
          <span
            className="bg-violet absolute top-[2px] right-[6px] rounded-full px-[6px] py-[1px]
              text-[11px] leading-[14px] text-white"
          >
            +{extra}
          </span>
        )}
      </motion.div>
    </motion.div>
  );
}
