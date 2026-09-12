import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { ReactNode } from "react";

import { useLayer } from "@/lib/layers";

/** House ease — matches --ease-out-expo. */
const EASE: [number, number, number, number] = [0.16, 1, 0.3, 1];

/** Never let a floating layer touch the window edge. */
const MARGIN = 8;

export type Placement =
  | "top"
  | "bottom"
  | "left"
  | "right"
  | "bottom-start"
  | "bottom-end"
  | "top-start"
  | "top-end";

export interface PopoverProps {
  open: boolean;
  onClose(): void;
  /** element to anchor to, or a fixed point */
  anchor: HTMLElement | { x: number; y: number } | null;
  placement?: Placement;
  offset?: number;
  /** "menu" registers a dismissable layer (Escape/outside click close); "tooltip" does not */
  kind?: "popover" | "menu" | "tooltip";
  children: ReactNode;
  className?: string;
  /** keep mounted for exit animation */
}

type Side = "top" | "bottom" | "left" | "right";
type Align = "start" | "center" | "end";

interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

interface Solved {
  left: number;
  top: number;
  side: Side;
  align: Align;
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(Math.max(lo, hi), v));
}

function boxOf(anchor: HTMLElement | { x: number; y: number }): Box {
  if (anchor instanceof HTMLElement) {
    const r = anchor.getBoundingClientRect();
    return { left: r.left, top: r.top, width: r.width, height: r.height };
  }
  return { left: anchor.x, top: anchor.y, width: 0, height: 0 };
}

/**
 * Flip to the opposite side only when the preferred side overflows AND the
 * opposite one fits — a layer that flips into an equally bad position just
 * moves the problem, so in that case it stays put and gets clamped instead.
 */
function solve(box: Box, w: number, h: number, placement: Placement, offset: number): Solved {
  const parts = placement.split("-");
  let side = parts[0] as Side;
  const align: Align = (parts[1] as Align | undefined) ?? "center";
  const vw = window.innerWidth;
  const vh = window.innerHeight;

  if (side === "top" || side === "bottom") {
    const below = box.top + box.height + offset;
    const above = box.top - offset - h;
    if (side === "bottom" && below + h > vh - MARGIN && above >= MARGIN) side = "top";
    else if (side === "top" && above < MARGIN && below + h <= vh - MARGIN) side = "bottom";

    const top = side === "bottom" ? below : above;
    const raw =
      align === "start"
        ? box.left
        : align === "end"
          ? box.left + box.width - w
          : box.left + box.width / 2 - w / 2;
    return {
      left: clamp(raw, MARGIN, vw - MARGIN - w),
      top: clamp(top, MARGIN, vh - MARGIN - h),
      side,
      align,
    };
  }

  const before = box.left - offset - w;
  const after = box.left + box.width + offset;
  if (side === "right" && after + w > vw - MARGIN && before >= MARGIN) side = "left";
  else if (side === "left" && before < MARGIN && after + w <= vw - MARGIN) side = "right";

  const left = side === "right" ? after : before;
  const raw =
    align === "start"
      ? box.top
      : align === "end"
        ? box.top + box.height - h
        : box.top + box.height / 2 - h / 2;
  return {
    left: clamp(left, MARGIN, vw - MARGIN - w),
    top: clamp(raw, MARGIN, vh - MARGIN - h),
    side,
    align,
  };
}

/** Grow out of the anchor, not out of thin air. */
function originOf(side: Side, align: Align): string {
  const cross = align === "start" ? "left" : align === "end" ? "right" : "center";
  if (side === "bottom") return `top ${cross}`;
  if (side === "top") return `bottom ${cross}`;
  return side === "right" ? "left center" : "right center";
}

/**
 * The app's one floating layer. Portalled to the body because anything anchored
 * inside a scrolling pane would otherwise be clipped by it, and positioned in
 * two passes — mount hidden, measure, place — since a flip decision cannot be
 * made before the panel's own size is known.
 *
 * z-index sits above the dialog's z-[60] so a menu opened inside a modal is
 * reachable; tooltips sit above menus because they annotate them.
 */
export function Popover({
  open,
  onClose,
  anchor,
  placement = "bottom",
  offset = 8,
  kind = "popover",
  children,
  className = "",
}: PopoverProps) {
  const reduced = useReducedMotion() ?? false;
  const panel = useRef<HTMLDivElement | null>(null);
  const [node, setNode] = useState<HTMLDivElement | null>(null);
  const [pos, setPos] = useState<Solved | null>(null);

  const setPanel = useCallback((el: HTMLDivElement | null) => {
    panel.current = el;
    setNode(el);
  }, []);

  // Tooltips pass through as an inert layer, so Escape still reaches the dialog.
  useLayer(open, kind, onClose);

  // Measured with offsetWidth/Height rather than the bounding rect: the enter
  // animation scales the panel, and a scaled rect would place it wrong.
  useLayoutEffect(() => {
    if (!open || !node || !anchor) return;
    const measure = () => {
      setPos(solve(boxOf(anchor), node.offsetWidth, node.offsetHeight, placement, offset));
    };
    measure();
    window.addEventListener("resize", measure);
    window.addEventListener("scroll", measure, true);
    return () => {
      window.removeEventListener("resize", measure);
      window.removeEventListener("scroll", measure, true);
    };
  }, [open, node, anchor, placement, offset]);

  // Outside press closes. Capture phase, so a click that also triggers something
  // underneath still dismisses first; the anchor is excluded so a toggle button
  // is not closed and reopened by the same press.
  useEffect(() => {
    if (!open || kind === "tooltip") return;
    const onDown = (e: PointerEvent) => {
      const target = e.target as Node | null;
      if (!target) return;
      if (panel.current?.contains(target)) return;
      if (anchor instanceof HTMLElement && anchor.contains(target)) return;
      onClose();
    };
    document.addEventListener("pointerdown", onDown, true);
    return () => document.removeEventListener("pointerdown", onDown, true);
  }, [open, kind, anchor, onClose]);

  if (typeof document === "undefined") return null;

  const side = pos?.side ?? (placement.split("-")[0] as Side);
  const enter = reduced
    ? {
        initial: { opacity: 0 },
        animate: { opacity: 1, transition: { duration: 0.16 } },
        exit: { opacity: 0, transition: { duration: 0.12 } },
      }
    : {
        initial: { opacity: 0, scale: 0.96, y: side === "top" ? 4 : -4 },
        animate: { opacity: 1, scale: 1, y: 0, transition: { duration: 0.16, ease: EASE } },
        exit: { opacity: 0, scale: 0.98, transition: { duration: 0.12 } },
      };

  return createPortal(
    <AnimatePresence>
      {open && anchor ? (
        <motion.div
          ref={setPanel}
          role={kind === "tooltip" ? "tooltip" : undefined}
          {...enter}
          className={`fixed rounded-[12px] border border-line-strong bg-surface-2 shadow-[0_16px_48px_rgba(0,0,0,0.55)] ${className}`}
          style={{
            left: pos?.left ?? 0,
            top: pos?.top ?? 0,
            zIndex: kind === "tooltip" ? 80 : 70,
            transformOrigin: originOf(side, pos?.align ?? "center"),
            visibility: pos ? "visible" : "hidden",
          }}
        >
          {children}
        </motion.div>
      ) : null}
    </AnimatePresence>,
    document.body,
  );
}

export default Popover;
