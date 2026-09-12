import { ContactShadow, Monogram, NO_DETAIL_BELOW, shade } from "../base";
import type { IconFamilyProps } from "../types";

/**
 * The cube's six silhouette vertices on a 2:1 isometric grid, 40 units wide.
 * Apex (32,9) → left/right shoulders at y 19 → the seam corner at y 29 → the
 * skirt corners at y 41 → the bottom of the seam at y 51.
 */
const TOP_FACE = "M32 9L52 19L32 29L12 19Z";
const LEFT_FACE = "M12 19L32 29V51L12 41Z";
const RIGHT_FACE = "M52 19L32 29V51L52 41Z";
const OUTLINE = "M32 9L52 19V41L32 51L12 41V19Z";

/**
 * Geometry: an isometric cube, which is the only family that is its own light.
 *
 * Every other family fakes depth with a flat extrusion pushed down behind a
 * front face. A 3D asset should be the exception — the object itself is the
 * proof — so here the three faces carry the whole illusion: the top lifted to
 * `shade(hue, +0.28)`, the left at the registry's `hue`, the right at the
 * darker `hue2` that everywhere else is spent on the extrusion plate. That
 * keeps the key light top-left, consistent with the rest of the system, without
 * a single extra node.
 *
 * The seam edge is drawn only down to y 38 so it stops short of the monogram
 * instead of running a hairline through the letters at 128px; read as the edge
 * highlight falling off away from the key, which is what it is.
 */
export function Model3d({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={54} rx={21} ry={4.5} />
      <path d={TOP_FACE} fill={shade(spec.hue, 0.28)} />
      <path d={LEFT_FACE} fill={spec.hue} />
      <path d={RIGHT_FACE} fill={spec.hue2} />
      <clipPath id={`${uid}-m3d`}>
        <path d={OUTLINE} />
      </clipPath>
      <path d="M12 9H52L12 51Z" fill={`url(#${uid}-sheen)`} clipPath={`url(#${uid}-m3d)`} />
      {detail && (
        <g>
          <path
            d={TOP_FACE}
            fill="none"
            stroke="rgba(255,255,255,0.35)"
            strokeWidth="1"
            strokeLinejoin="round"
          />
          <path
            d="M13 19L32 9.5L51 19"
            fill="none"
            stroke="rgba(255,255,255,0.5)"
            strokeWidth="1"
            strokeLinejoin="round"
            strokeLinecap="round"
          />
          <path
            d="M32 29V38"
            stroke="rgba(255,255,255,0.35)"
            strokeWidth="1"
            strokeLinecap="round"
          />
          <Monogram label={spec.label} size={8} x={32} y={43} color="rgba(255,255,255,0.92)" />
        </g>
      )}
    </g>
  );
}
