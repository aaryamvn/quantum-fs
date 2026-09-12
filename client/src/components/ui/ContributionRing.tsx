import { motion, useReducedMotion, useSpring, useTransform } from "motion/react";
import { useEffect } from "react";

import { formatBytes } from "@/lib/format";

/** Degrees of empty ring left between two neighbouring contributions. */
const GAP_DEGREES = 3;

/** Both rings share one track so the unclaimed remainder reads as a single shape. */
const TRACK = "rgba(255,255,255,0.06)";

/** `--color-fg` at 55%: the used arc is the same ink as the label above it, quieted. */
const USED_STROKE = "rgba(245,245,247,0.55)";

/** The inner "used" arc is a hairline against the contribution ring, not a second bar. */
const INNER_STROKE = 4;

/** Clear air between the two rings, so neither reads as the other's shadow. */
const RING_GAP = 4;

export interface ContributionSegment {
  id: string;
  label: string;
  bytes: number;
  color: string;
}

export interface ContributionRingProps {
  segments: ContributionSegment[];
  /** What the vault can hold: the server's allocation plus every contribution. */
  totalBytes: number;
  /** What the vault's files actually occupy today. */
  usedBytes: number;
  size?: number;
  stroke?: number;
  className?: string;
}

/** 480 ms, settling rather than bouncing — a capacity that overshoots reads as a glitch. */
function springConfig(reduced: boolean) {
  return reduced ? { duration: 0, bounce: 0 } : { duration: 0.48, bounce: 0 };
}

interface ArcProps {
  cx: number;
  cy: number;
  r: number;
  stroke: number;
  color: string;
  circumference: number;
  /** Arc length in user units, already shortened by the gap. */
  length: number;
  /** Distance from twelve o'clock to this arc's start, in user units. */
  offset: number;
  reduced: boolean;
}

/**
 * One arc of the ring, animating its own length.
 *
 * A component per arc rather than a loop of hooks inside the ring: the segment
 * list changes whenever a member joins or re-pledges, and hooks cannot be
 * conditional on its length.
 */
function Arc({ cx, cy, r, stroke, color, circumference, length, offset, reduced }: ArcProps) {
  const config = springConfig(reduced);
  // Grows from nothing on mount, which is the whole animation the spec asks for;
  // the offset is sprung too so a segment that changes rank slides rather than jumps.
  const len = useSpring(0, config);
  const start = useSpring(offset, config);

  useEffect(() => {
    len.set(length);
  }, [len, length]);

  useEffect(() => {
    start.set(offset);
  }, [start, offset]);

  const dashArray = useTransform(len, (v) => `${Math.max(0, v)} ${circumference}`);
  // Negative: a positive dash offset walks the pattern backwards around the circle.
  const dashOffset = useTransform(start, (v) => -v);

  return (
    <motion.circle
      cx={cx}
      cy={cy}
      r={r}
      fill="none"
      stroke={color}
      strokeWidth={stroke}
      // Butt, not round: rounded ends eat the 3° gap and two neighbours fuse.
      strokeLinecap="butt"
      style={{
        strokeDasharray: dashArray,
        strokeDashoffset: dashOffset,
        transform: "rotate(-90deg)",
        transformOrigin: "center",
      }}
    />
  );
}

/**
 * Who the vault's capacity belongs to, as one ring.
 *
 * A vault is not a disk with an owner: the host allocates a slice and every
 * member who joins raises the ceiling by what they pledge from their own
 * machine. A single bar hides that — it can only say "full or not". Segmenting
 * the outer ring by contributor says where the room came from, and the thin
 * inner arc says how much of it is spoken for, which are two genuinely different
 * questions asked of the same number.
 *
 * Rings, never a pie: a pie claims the centre, and the centre is where the
 * capacity itself belongs.
 */
export function ContributionRing({
  segments,
  totalBytes,
  usedBytes,
  size = 168,
  stroke = 10,
  className,
}: ContributionRingProps) {
  const reduced = useReducedMotion() ?? false;

  const outerR = (size - stroke) / 2;
  const outerC = 2 * Math.PI * outerR;

  const innerR = outerR - stroke / 2 - RING_GAP - INNER_STROKE / 2;
  const innerC = 2 * Math.PI * innerR;

  const total = totalBytes > 0 ? totalBytes : 0;
  const drawn = segments.filter((s) => s.bytes > 0);
  const gap = drawn.length > 1 ? (GAP_DEGREES / 360) * outerC : 0;

  // Cumulative walk clockwise from twelve o'clock. Each arc keeps its true start
  // and gives up the gap at its end, so the boundaries land where the numbers say.
  let cursor = 0;
  const arcs = drawn.map((segment) => {
    const raw = total > 0 ? (segment.bytes / total) * outerC : 0;
    const offset = cursor;
    cursor += raw;
    return { segment, offset, length: Math.max(0, raw - gap) };
  });

  const usedFraction = total > 0 ? Math.max(0, Math.min(1, usedBytes / total)) : 0;

  const usedConfig = springConfig(reduced);
  const used = useSpring(0, usedConfig);
  useEffect(() => {
    used.set(usedFraction * innerC);
  }, [used, usedFraction, innerC]);
  const usedDash = useTransform(used, (v) => `${Math.max(0, v)} ${innerC}`);

  return (
    <div className={`relative shrink-0 ${className ?? ""}`} style={{ width: size, height: size }}>
      <svg
        width={size}
        height={size}
        viewBox={`0 0 ${size} ${size}`}
        role="img"
        aria-label={`${formatBytes(usedBytes)} used of ${formatBytes(totalBytes)} capacity`}
      >
        <circle
          cx={size / 2}
          cy={size / 2}
          r={outerR}
          fill="none"
          stroke={TRACK}
          strokeWidth={stroke}
        />
        {arcs.map(({ segment, offset, length }) => (
          <Arc
            key={segment.id}
            cx={size / 2}
            cy={size / 2}
            r={outerR}
            stroke={stroke}
            color={segment.color}
            circumference={outerC}
            length={length}
            offset={offset}
            reduced={reduced}
          />
        ))}

        <circle
          cx={size / 2}
          cy={size / 2}
          r={innerR}
          fill="none"
          stroke={TRACK}
          strokeWidth={INNER_STROKE}
        />
        <motion.circle
          cx={size / 2}
          cy={size / 2}
          r={innerR}
          fill="none"
          stroke={USED_STROKE}
          strokeWidth={INNER_STROKE}
          strokeLinecap="butt"
          style={{
            strokeDasharray: usedDash,
            transform: "rotate(-90deg)",
            transformOrigin: "center",
          }}
        />
      </svg>

      <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center gap-[4px]">
        <span className="text-[22px] font-medium leading-none text-fg tabular-nums">
          {formatBytes(totalBytes)}
        </span>
        <span className="text-[12.5px] leading-none text-fg-2 tabular-nums">
          {formatBytes(usedBytes)} used
        </span>
      </div>
    </div>
  );
}
