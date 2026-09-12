import { ContactShadow, Monogram, NO_DETAIL_BELOW, shade, Slab } from "../base";
import type { IconFamilyProps } from "../types";

/**
 * The spine: the cover's left 8 units, square where it meets the board and
 * rounded where it meets the world, so it shares the `Slab` corner radius (3)
 * without a second rounded rect fighting it.
 */
const SPINE_D = "M17 6H22V56H17A3 3 0 0 1 14 53V9A3 3 0 0 1 17 6Z";

/** The raised bands across the spine, and the fore-edge leaves on the right. */
const BAND_Y: readonly number[] = [15.5, 42.5];
const LEAF_X: readonly number[] = [45.5, 47, 48.5];

/**
 * A closed book seen face-on, which is the only view that survives 20px.
 *
 * A three-quarter book — the view everybody reaches for first — spends its
 * whole silhouette on perspective and turns to mush the moment it is small, so
 * this one stays flat and buys its depth from the same extrusion every other
 * family uses. The read comes from the two asymmetries: a dark spine strip down
 * the left, and the page block down the right. That pair is unmistakably a book
 * and nothing else in the set has it, so it is drawn at *every* size while the
 * bands, the title plaque and the stamp drop out below `NO_DETAIL_BELOW`. The
 * spine takes its color from `hue2` darkened rather than from a fixed brown,
 * so an EPUB and a CBZ can be different colors and still both be books.
 */
export function Book({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={58.5} rx={21} ry={4} />
      <Slab uid={uid} x={14} y={6} w={36} h={50} r={3} />
      <path d={SPINE_D} fill={shade(spec.hue2, -0.3)} />
      <path
        d="M17 6.9H21.5"
        stroke="rgba(255,255,255,0.22)"
        strokeWidth="1"
        strokeLinecap="round"
      />
      {detail && (
        <g>
          {BAND_Y.map((by) => (
            <rect
              key={by}
              x={15.5}
              y={by}
              width={5}
              height={1.5}
              rx={0.75}
              fill="rgba(255,255,255,0.35)"
            />
          ))}
          <rect x={27.5} y={17} width={17} height={9} rx={1.5} fill="rgba(255,255,255,0.18)" />
          {LEAF_X.map((lx) => (
            <rect
              key={lx}
              x={lx}
              y={11}
              width={0.75}
              height={38}
              rx={0.375}
              fill="rgba(255,255,255,0.45)"
            />
          ))}
          <Monogram label={spec.label} size={8} x={34} y={45} color="rgba(255,255,255,0.92)" />
        </g>
      )}
    </g>
  );
}
