import { motion, useReducedMotion } from "motion/react";
import { useEffect, useRef, useState } from "react";

import type { DiveOrigin } from "@/app/appShell";
import type { VaultId } from "@/lib/backend";

import { EASE } from "../layout";

/**
 * Every beat of the dive, in ms. Exported because the reviewer and the
 * screenshot scripts have to be able to land on a frame *inside* the
 * transition, and guessing at a duration is how a capture ends up blank.
 */
export const DIVE = {
  /** Aperture: the clicked row's rect → the whole window. */
  apertureMs: 640,
  /** The workspace may mount now; it fades in behind the still-opaque fill. */
  revealAt: 380,
  /** The fill starts dissolving so the workspace shows through. */
  fillFadeStart: 520,
  /** Everything has landed; the dive can be unmounted. */
  doneAt: 900,
} as const;

/** Length of the fill dissolve — 520 → 860, finished a beat before `doneAt`. */
const FILL_FADE_MS = 340;
/** Reduced motion gets one short crossfade instead of the whole sequence. */
const REDUCED_MS = 200;
/** The origin highlight is a flash, not a light source. */
const HIGHLIGHT_MS = 400;
/** The title arrives once the aperture has some room to hold it. */
const TITLE_IN_MS = 420;
const TITLE_DELAY_MS = 120;

/**
 * Just under the Tauri drag strip (z-50) and above everything the workspace
 * paints while the dive runs — no modal or menu can be open during it.
 */
const DIVE_Z = 49;

/**
 * The brand washing through the aperture: coral shoulder, violet body, and the
 * page's own black taking over before the workspace is uncovered.
 */
const SWEEP = "linear-gradient(115deg, #FF7B7B 0%, #4E0EFF 38%, #010513 70%)";

const s = (ms: number): number => ms / 1000;

export interface VaultDiveProps {
  vaultId: VaultId;
  vaultName: string;
  /** The clicked row's viewport rect; the aperture starts exactly on it. */
  origin: DiveOrigin;
  /** Mount the workspace underneath — it is still hidden by the fill. */
  onReveal(): void;
  /** The transition is over; unmount this. */
  onDone(): void;
}

/**
 * Home → vault, as one continuous expansion rather than a page swap.
 *
 * The clicked vault row *becomes* the window: an aperture starting on its exact
 * rect grows to fill the screen, the brand gradient washes through it and
 * drains away, and what is left behind the dissolving fill is the workspace,
 * which has been mounting underneath since 380 ms. The vault's name carries a
 * shared `layoutId` with the root breadcrumb, so the title the dive puts in the
 * middle of the screen glides into the top bar when this unmounts — the one
 * element that survives the transition, which is what makes it read as going
 * *into* the vault instead of cutting to another screen.
 *
 * It swallows pointer events for its 900 ms on purpose: a second click on the
 * row that is still fading out underneath would start a second dive.
 */
export function VaultDive({ vaultId, vaultName, origin, onReveal, onDone }: VaultDiveProps) {
  const reduced = useReducedMotion() ?? false;

  // The viewport as it was when the dive started. The aperture animates to
  // pixel targets (Motion cannot interpolate 420px → 100vw), and re-measuring
  // for a resize inside a 640 ms transition is not worth the frame cost.
  const [vw] = useState(() => (typeof window === "undefined" ? 0 : window.innerWidth));
  const [vh] = useState(() => (typeof window === "undefined" ? 0 : window.innerHeight));

  // Callbacks live in refs so the timers are armed exactly once: a parent
  // re-render handing down a new closure must never restart the sequence.
  const reveal = useRef(onReveal);
  reveal.current = onReveal;
  const done = useRef(onDone);
  done.current = onDone;

  useEffect(() => {
    const revealAt = reduced ? 0 : DIVE.revealAt;
    const doneAt = reduced ? REDUCED_MS : DIVE.doneAt;
    const timers = [
      setTimeout(() => reveal.current(), revealAt),
      setTimeout(() => done.current(), doneAt),
    ];
    return () => {
      for (const t of timers) clearTimeout(t);
    };
  }, [reduced]);

  if (reduced) {
    return (
      <motion.div
        data-testid="vault-dive"
        data-vault-id={vaultId}
        aria-hidden
        initial={{ opacity: 1 }}
        animate={{ opacity: 0 }}
        transition={{ duration: s(REDUCED_MS), ease: "linear" }}
        style={{
          position: "fixed",
          inset: 0,
          zIndex: DIVE_Z,
          background: "var(--color-surface)",
        }}
      />
    );
  }

  const aperture = { duration: s(DIVE.apertureMs), ease: EASE };
  const centerX = origin.x + origin.w / 2;
  const centerY = origin.y + origin.h / 2;

  return (
    <div
      data-testid="vault-dive"
      data-vault-id={vaultId}
      aria-hidden
      style={{ position: "fixed", inset: 0, zIndex: DIVE_Z }}
    >
      <motion.div
        initial={{
          left: origin.x,
          top: origin.y,
          width: origin.w,
          height: origin.h,
          borderRadius: 12,
          backgroundColor: "rgba(7, 11, 27, 1)",
          borderColor: "rgba(255, 255, 255, 0.13)",
        }}
        animate={{
          left: 0,
          top: 0,
          width: vw,
          height: vh,
          borderRadius: 0,
          backgroundColor: "rgba(7, 11, 27, 0)",
          borderColor: "rgba(255, 255, 255, 0)",
        }}
        transition={{
          ...aperture,
          // The geometry lands at 640; the fill hangs on, then dissolves to let
          // the workspace through without ever showing a seam between them.
          backgroundColor: { delay: s(DIVE.fillFadeStart), duration: s(FILL_FADE_MS), ease: "linear" },
          borderColor: { delay: s(DIVE.fillFadeStart), duration: s(FILL_FADE_MS), ease: "linear" },
        }}
        style={{
          position: "fixed",
          overflow: "hidden",
          borderWidth: 1,
          borderStyle: "solid",
        }}
      >
        {/*
          Counter-offset by exactly what the aperture moves, on the same curve:
          the color inside is anchored to the window, so the aperture reads as
          a hole opening onto it rather than as a box with a gradient in it.
        */}
        <motion.div
          initial={{ left: -origin.x, top: -origin.y }}
          animate={{ left: 0, top: 0 }}
          transition={aperture}
          style={{ position: "absolute", width: vw, height: vh }}
        >
          <motion.div
            initial={{ opacity: 0, scale: 1.1, backgroundPosition: "0% 50%" }}
            animate={{
              opacity: [0, 0, 0.9, 0.9, 0],
              scale: [1.1, 1.1, 1, 1, 1],
              backgroundPosition: "100% 50%",
            }}
            transition={{
              duration: s(DIVE.doneAt),
              ease: EASE,
              // 0 · 80 · 360 · 520 · 900 — in, hold, then drain with the fill.
              times: [0, 80 / DIVE.doneAt, 360 / DIVE.doneAt, DIVE.fillFadeStart / DIVE.doneAt, 1],
              backgroundPosition: { duration: s(DIVE.doneAt), ease: EASE },
            }}
            style={{
              position: "absolute",
              inset: 0,
              backgroundImage: SWEEP,
              backgroundSize: "180% 180%",
            }}
          />
          <motion.div
            initial={{ opacity: 1 }}
            animate={{ opacity: 0 }}
            transition={{ duration: s(HIGHLIGHT_MS), ease: EASE }}
            style={{
              position: "absolute",
              inset: 0,
              backgroundImage: `radial-gradient(240px circle at ${centerX}px ${centerY}px, rgba(255, 255, 255, 0.18), transparent)`,
            }}
          />
        </motion.div>

        <motion.div
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: s(TITLE_DELAY_MS), duration: s(TITLE_IN_MS), ease: EASE }}
          style={{ position: "absolute", inset: 0, display: "grid", placeItems: "center" }}
        >
          <motion.span
            layoutId="vault-title"
            className="font-heading max-w-[70vw] truncate px-[24px] text-[28px] leading-none
              font-medium tracking-[-0.02em] text-fg"
          >
            {vaultName}
          </motion.span>
        </motion.div>
      </motion.div>
    </div>
  );
}
