import { ContactShadow, Monogram, NO_DETAIL_BELOW, shade, Slab } from "../base";
import type { IconFamilyProps } from "../types";

/** The 3×3 layout guides, as [x1, y1, x2, y2] inside the artboard. */
const GUIDES: readonly [number, number, number, number][] = [
  [14, 24.5, 50, 24.5],
  [14, 36.5, 50, 36.5],
  [26, 12.5, 26, 48.5],
  [38, 12.5, 38, 48.5],
];

/** Selection handles, one per corner of the marquee. */
const HANDLES: readonly [number, number][] = [
  [20, 14],
  [44, 14],
  [20, 32],
  [44, 32],
];

/**
 * Source design documents — Figma, Sketch, XD, PSD, InDesign, Affinity.
 *
 * These files are not a picture, they are the *room the picture is made in*, so
 * the body is a square artboard rather than a landscape card: guides thirding
 * it, one object still marqueed with its four handles, and a pen nib parked in
 * the corner. Read together they say "layout in progress", which is the one
 * thing a .fig and a .psd genuinely have in common, and it stays true whatever
 * hue the registry hands each app.
 *
 * The guides are 0.18-alpha hairlines and the marquee is a 2/2 dash: both are
 * gone below `NO_DETAIL_BELOW`, where the square silhouette alone already
 * separates this family from the landscape image/vector cards.
 */
export function Design({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={56} rx={25} ry={4} />
      <Slab uid={uid} x={9} y={7.5} w={46} h={46} r={8} />
      {detail && (
        <g>
          {GUIDES.map(([x1, y1, x2, y2]) => (
            <path
              key={`${x1}-${y1}`}
              d={`M${x1} ${y1}L${x2} ${y2}`}
              stroke="rgba(255,255,255,0.18)"
              strokeWidth="1"
            />
          ))}
          <rect
            x={20}
            y={14}
            width={24}
            height={18}
            fill="none"
            stroke="rgba(255,255,255,0.7)"
            strokeWidth="1"
            strokeDasharray="2 2"
          />
          {HANDLES.map(([hx, hy]) => (
            <rect
              key={`${hx}-${hy}`}
              x={hx - 1.5}
              y={hy - 1.5}
              width={3}
              height={3}
              rx={0.5}
              fill="rgba(255,255,255,0.9)"
            />
          ))}
          <path d="M39.5 36L50 40.5L43 48Z" fill="rgba(255,255,255,0.82)" />
          <circle cx={44} cy={40.8} r={1.3} fill={shade(spec.hue2, -0.3)} />
          <Monogram label={spec.label} size={8} x={24} y={43.5} color="rgba(255,255,255,0.92)" />
        </g>
      )}
    </g>
  );
}
