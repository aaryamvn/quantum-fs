import { motion, useMotionValue, useReducedMotion, useSpring, useTransform } from "motion/react";
import { useEffect } from "react";

/** The arc the indeterminate ring shows: a quarter of the circle, spinning. */
const INDETERMINATE_SWEEP = 0.25;

export interface ProgressRingProps {
  /** 0..1. NaN or undefined means "working, length unknown" — the ring spins instead. */
  value: number | undefined;
  size?: number;
  stroke?: number;
  className?: string;
}

/**
 * The download/upload indicator that sits inside a file row, at icon scale.
 *
 * It is a ring rather than a bar because it has to live in the 16px slot a file
 * icon occupies without changing the row's rhythm — and because a transfer that
 * finishes is then a filled circle, one glance from an empty one.
 *
 * Progress is sprung, not set: bytes arrive in bursts, and a raw value jumps the
 * arc around. The spring turns that into one continuous sweep, which is the
 * honest reading of a transfer that is in fact continuous.
 */
export function ProgressRing({ value, size = 14, stroke = 1.75, className }: ProgressRingProps) {
  const reduced = useReducedMotion() ?? false;

  const determinate = typeof value === "number" && Number.isFinite(value);
  const target = determinate ? Math.max(0, Math.min(1, value)) : 0;

  const r = (size - stroke) / 2;
  const circumference = 2 * Math.PI * r;

  const raw = useMotionValue(target);
  const sprung = useSpring(
    raw,
    // A duration-based spring with no bounce is exactly the 160ms tween the
    // reduced branch calls for, so both paths stay one motion value.
    reduced ? { duration: 0.16, bounce: 0 } : { stiffness: 220, damping: 30, mass: 0.6 },
  );
  const offset = useTransform(sprung, (v) => circumference * (1 - v));

  useEffect(() => {
    raw.set(target);
  }, [raw, target]);

  const common = {
    cx: size / 2,
    cy: size / 2,
    r,
    fill: "none",
    strokeWidth: stroke,
  };

  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${size} ${size}`}
      className={className}
      role="img"
      aria-label={determinate ? `${Math.round(target * 100)}% complete` : "Working"}
    >
      <circle {...common} stroke="rgba(255,255,255,0.14)" />

      {determinate ? (
        // Rotated so 0 is at twelve o'clock and the arc fills clockwise.
        <motion.circle
          {...common}
          stroke="currentColor"
          strokeLinecap="round"
          strokeDasharray={circumference}
          style={{
            strokeDashoffset: offset,
            transform: "rotate(-90deg)",
            transformOrigin: "center",
          }}
        />
      ) : (
        <motion.circle
          {...common}
          stroke="currentColor"
          strokeLinecap="round"
          strokeDasharray={`${circumference * INDETERMINATE_SWEEP} ${circumference}`}
          style={{ transformOrigin: "center" }}
          // Reduced motion gets the same arc, parked: a ring that never stops
          // spinning is the exact thing that setting asks us not to do.
          animate={reduced ? { rotate: -90 } : { rotate: 360 }}
          transition={
            reduced
              ? { duration: 0 }
              : { duration: 1, ease: "linear", repeat: Infinity, repeatType: "loop" }
          }
        />
      )}
    </svg>
  );
}
