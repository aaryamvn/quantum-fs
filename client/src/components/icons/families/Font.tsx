import { ContactShadow, Monogram, NO_DETAIL_BELOW, Slab } from "../base";
import type { IconFamilyProps } from "../types";

/**
 * Typefaces: the specimen card every font menu has shown since metal type.
 *
 * "Aa" sitting on a baseline rule is the only mark that says "this file is a
 * font and not a picture of one" — a capital and a lowercase together show
 * cap height, x-height and the shape of the bowl at once. The pair is set in
 * the app's own face at weight 500, the heaviest weight this codebase allows
 * (docs/decisions/client-typography.md); the lowercase drops to 0.6 alpha so
 * the capital still leads even though neither glyph is bolder than the other.
 *
 * The specimen is the silhouette here, so it stays below `NO_DETAIL_BELOW`
 * where an 18px card with no letters on it would be indistinguishable from any
 * other block; the baseline hairline and the TTF/OTF/WOFF stamp drop out.
 */
export function Font({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={52.5} rx={26} ry={4} />
      <Slab uid={uid} x={6} y={7} w={52} h={46} r={9} />
      {detail && (
        <path
          d="M9 40H55"
          stroke="rgba(255,255,255,0.3)"
          strokeWidth="1"
          strokeLinecap="round"
        />
      )}
      <text
        x={32}
        y={40}
        textAnchor="middle"
        fontFamily="var(--font-sans)"
        fontWeight={500}
        fontSize={32}
      >
        <tspan fill="rgba(255,255,255,0.95)">A</tspan>
        <tspan fill="rgba(255,255,255,0.6)">a</tspan>
      </text>
      {detail && (
        <Monogram label={spec.label} size={7} x={32} y={47.5} color="rgba(255,255,255,0.92)" />
      )}
    </g>
  );
}
