import { ContactShadow, Monogram, NO_DETAIL_BELOW, Slab } from "../base";
import type { IconFamilyProps } from "../types";

/** Where the columns stand: x, height above `BASELINE`, fill opacity. */
const BASELINE = 37.5;
const BARS: readonly [number, number, number][] = [
  [12, 6, 0.55],
  [19.5, 9.5, 0.55],
  [27, 13, 0.55],
  [34.5, 17, 0.85],
];

/**
 * A slide, not a sheet: the only landscape body in the set.
 *
 * Orientation is doing most of the work here — 4:3 is the one proportion that
 * says "deck" before any ornament is readable, so the block is wide and the
 * silhouette alone survives the sidebar. On top of it sits the most-photographed
 * slide in the world: a title rule top-left and a rising bar chart. The tallest
 * column is brighter than the other three so the chart reads as *going
 * somewhere* rather than as four random rules, and the dot top-right is the
 * clicker's laser — the small cue that this is a slide being presented.
 */
export function Presentation({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={56.5} rx={24} ry={3.5} />
      <Slab uid={uid} x={6} y={12} w={52} h={38} r={4} />
      {detail && (
        <g>
          <rect x={12} y={16} width={24} height={4} rx={2} fill="rgba(255,255,255,0.6)" />
          <circle cx={50} cy={18} r={2} fill="rgba(255,255,255,0.45)" />
          {BARS.map(([bx, bh, op]) => (
            <rect
              key={bx}
              x={bx}
              y={BASELINE - bh}
              width={5.5}
              height={bh}
              rx={1}
              fill={`rgba(255,255,255,${op})`}
            />
          ))}
          <path
            d={`M12 ${BASELINE}H46`}
            stroke="rgba(255,255,255,0.35)"
            strokeWidth="1"
            strokeLinecap="round"
          />
          <Monogram label={spec.label} size={8} x={32} y={45} color="rgba(255,255,255,0.92)" />
        </g>
      )}
    </g>
  );
}
