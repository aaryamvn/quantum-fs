import { useCallback, useMemo, useRef, useState } from "react";
import { useAnimationFrame, useMotionValue, type MotionValue } from "motion/react";
import { cubicBezier } from "motion";

import { devQuery } from "@/lib/devQuery";

/** Every duration of the splash, in ms. Single source of truth. */
export const SPLASH = {
  /** Waves fly in from the right over this window. */
  barrageMs: 1700,
  /** Elapsed time at which the field starts compressing into the panel. */
  settleStartMs: 1250,
  /** Length of the compression. */
  settleMs: 1250,
  /** Elapsed time at which the shell is fully revealed. */
  revealMs: 2150,
  /** Elapsed time after which nothing is animating in any more. */
  totalMs: 3400,
} as const;

export type SplashPhase = "barrage" | "settling" | "settled";

export interface SplashTimeline {
  /** 0..1 eased barrage progress. */
  progress: MotionValue<number>;
  /** 0..1 eased settle progress. */
  settle: MotionValue<number>;
  /** Seconds since mount; keeps advancing forever so the field can breathe. */
  time: MotionValue<number>;
  phase: SplashPhase;
  /** True under `?at=` — the timeline never advances. */
  frozen: boolean;
  /** Fast-forward to the end (no-op when frozen). */
  skip(): void;
}

// Barrage: anticipation beat → surge → decelerate. Measured against the
// colour-from-the-right targets at 1280×800 — t250 ≤25% · t500 35–60% ·
// t800 70–90% · t1100 ≥95% · t1400 full — this lands 18/40/89/100/100.
// (0.16,1,0.3,1) put the waves at ~65% of their travel by 250 ms.
const easeBarrage = cubicBezier(0.7, 0, 0.2, 1);
const easeInOut = cubicBezier(0.65, 0, 0.35, 1);

const clamp01 = (v: number): number => (v < 0 ? 0 : v > 1 ? 1 : v);

const SKIP_RATE = 4;

function progressAt(elapsed: number): number {
  return easeBarrage(clamp01(elapsed / SPLASH.barrageMs));
}

function settleAt(elapsed: number): number {
  return easeInOut(clamp01((elapsed - SPLASH.settleStartMs) / SPLASH.settleMs));
}

function phaseAt(elapsed: number): SplashPhase {
  if (elapsed < SPLASH.settleStartMs) return "barrage";
  if (elapsed < SPLASH.revealMs) return "settling";
  return "settled";
}

function initialElapsed(): number {
  if (devQuery.at !== null) return devQuery.at;
  if (devQuery.skip || devQuery.reduced) return SPLASH.totalMs;
  return 0;
}

export function useSplashTimeline(): SplashTimeline {
  const frozen = devQuery.at !== null;

  // Lazy + stable across a StrictMode double-mount: state initialisers and refs
  // are created once per component instance, so no loop is ever duplicated.
  const [start] = useState(initialElapsed);

  const progress = useMotionValue(progressAt(start));
  const settle = useMotionValue(settleAt(start));
  const time = useMotionValue(start / 1000);

  const elapsedRef = useRef(start);
  const rateRef = useRef(1);

  const [phase, setPhase] = useState<SplashPhase>(() => phaseAt(start));

  useAnimationFrame((_t, delta) => {
    if (frozen) return;

    elapsedRef.current += delta * rateRef.current;
    const elapsed = elapsedRef.current;

    if (rateRef.current !== 1 && elapsed >= SPLASH.totalMs) rateRef.current = 1;

    progress.set(progressAt(elapsed));
    settle.set(settleAt(elapsed));
    time.set(elapsed / 1000);

    const next = phaseAt(elapsed);
    setPhase((prev) => (prev === next ? prev : next));
  });

  const skip = useCallback(() => {
    if (frozen) return;
    if (elapsedRef.current >= SPLASH.totalMs) return;
    rateRef.current = SKIP_RATE;
  }, [frozen]);

  return useMemo(
    () => ({ progress, settle, time, phase, frozen, skip }),
    [progress, settle, time, phase, frozen, skip],
  );
}
