import { useState } from "react";

import { formatBytes } from "@/lib/format";

/** The smallest slice of a server a vault can hold — and the grain the slider moves on. */
export const MIN_QUOTA_BYTES = 268435456;
export const QUOTA_STEP_BYTES = 268435456;

/** Segments and thumb move together, fast enough to feel attached to the drag. */
const MOVE = "120ms cubic-bezier(0.2, 0.8, 0.2, 1)";

const TRACK_H = 8;
const THUMB = 16;
/** The pointer target is taller than the 8px track; the track stays thin. */
const HIT_H = 28;

export interface AllocationSliderProps {
  /** Everything the server can provision. */
  capacityBytes: number;
  /** Already promised to the vaults that exist. */
  consumedBytes: number;
  vaultCount: number;
  value: number;
  onChange(value: number): void;
  min?: number;
  step?: number;
  disabled?: boolean;
}

/** Snaps to the step grid, then holds the result inside [min, free]. */
export function clampQuota(raw: number, free: number, min: number, step: number): number {
  if (free < min) return 0;
  const snapped = Math.round(raw / step) * step;
  return Math.min(free, Math.max(min, snapped));
}

const pct = (part: number, whole: number) =>
  whole > 0 ? Math.max(0, Math.min(100, (part / whole) * 100)) : 0;

/**
 * How much of a server this vault gets.
 *
 * One bar carries the whole story: what the existing vaults already hold (grey),
 * what this vault is about to take (white), and what is left (the empty track).
 * The range input covers only the free stretch, so the drag can never reach into
 * storage that is already spoken for.
 */
export function AllocationSlider({
  capacityBytes,
  consumedBytes,
  vaultCount,
  value,
  onChange,
  min = MIN_QUOTA_BYTES,
  step = QUOTA_STEP_BYTES,
  disabled = false,
}: AllocationSliderProps) {
  const [ringed, setRinged] = useState(false);

  const free = Math.max(0, capacityBytes - consumedBytes);
  const full = free < min;
  const live = !disabled && !full;
  const taken = live ? value : 0;

  const usedPct = pct(consumedBytes, capacityBytes);
  const edgePct = pct(consumedBytes + taken, capacityBytes);
  const freePct = Math.max(0, 100 - usedPct);

  const vaults = `${vaultCount} vault${vaultCount === 1 ? "" : "s"}`;

  return (
    <div>
      <div className="flex items-baseline justify-between">
        <span className="text-[13px] leading-[18px] text-fg-2">Storage</span>
        <span className="text-[14px] leading-[18px] font-medium text-fg tabular-nums">
          {formatBytes(taken)}
        </span>
      </div>

      {/*
        The wrapper is exactly the track: the thumb and the pointer target are
        absolutely positioned and centred on it, so they can be taller than the
        bar without pushing the captions away from it.
      */}
      <div className="relative mt-[10px]" style={{ height: TRACK_H }}>
        {live ? (
          <input
            type="range"
            min={0}
            max={free}
            step={step}
            value={value}
            aria-label="Storage to allocate"
            aria-valuetext={formatBytes(value)}
            onChange={(e) => onChange(clampQuota(Number(e.target.value), free, min, step))}
            onFocus={(e) => setRinged(e.currentTarget.matches(":focus-visible"))}
            onBlur={() => setRinged(false)}
            // Zero-width native thumb: the value then maps linearly edge to edge,
            // so the drawn thumb below sits exactly under the pointer.
            className="absolute top-1/2 m-0 w-full -translate-y-1/2 cursor-default appearance-none bg-transparent opacity-0
              [&::-webkit-slider-thumb]:h-0 [&::-webkit-slider-thumb]:w-0 [&::-webkit-slider-thumb]:appearance-none"
            style={{ left: `${usedPct}%`, width: `${freePct}%`, height: HIT_H }}
          />
        ) : null}

        <div
          className="pointer-events-none absolute inset-0 overflow-hidden rounded-full"
          style={{ background: "rgba(255,255,255,0.06)" }}
        >
          <span
            className="absolute inset-y-0 left-0 block"
            style={{
              width: `${usedPct}%`,
              background: "rgba(255,255,255,0.20)",
              transition: `width ${MOVE}`,
            }}
          />
          <span
            className="absolute inset-y-0 block bg-white"
            style={{
              left: `${usedPct}%`,
              width: `${Math.max(0, edgePct - usedPct)}%`,
              transition: `width ${MOVE}`,
            }}
          />
        </div>

        {live ? (
          <span
            className="pointer-events-none absolute top-1/2 block rounded-full bg-white"
            style={{
              width: THUMB,
              height: THUMB,
              // Clamped so the disc caps the end of the bar instead of hanging off it.
              left: `clamp(${THUMB / 2}px, ${edgePct}%, calc(100% - ${THUMB / 2}px))`,
              transform: "translate(-50%, -50%)",
              boxShadow: ringed
                ? "0 0 0 2px var(--color-surface-2), 0 0 0 4px rgba(255,255,255,0.32), 0 1px 4px rgba(0,0,0,0.5)"
                : "0 0 0 2px var(--color-surface-2), 0 1px 4px rgba(0,0,0,0.5)",
              transition: `left ${MOVE}, box-shadow 120ms linear`,
            }}
          />
        ) : null}
      </div>

      <div className="mt-[8px] flex items-baseline justify-between gap-[12px] text-[12px] leading-[16px] text-fg-3 tabular-nums">
        <span className="truncate">
          {formatBytes(consumedBytes)} used by {vaults}
        </span>
        <span className="shrink-0">
          {full
            ? "This server is full"
            : `${formatBytes(free - taken)} free of ${formatBytes(capacityBytes)}`}
        </span>
      </div>
    </div>
  );
}
