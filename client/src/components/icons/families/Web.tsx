import { ContactShadow, Monogram, NO_DETAIL_BELOW, Slab } from "../base";
import type { IconFamilyProps } from "../types";

/** Globe center and radius on the 64-unit stage. */
const GLOBE = { cx: 32, cy: 24, r: 11 } as const;

/**
 * The web: a block carrying a wireframe globe.
 *
 * HTML, CSS and their dialects are the files that *become* a page on the open
 * internet, and the graticule globe is the one mark that says "internet"
 * everywhere — but only when it is drawn as a wireframe. A filled or shaded
 * sphere immediately reads as a planet instead, so this is strokes only:
 * outline, one meridian, one equator ellipse, and the equator seen edge-on as
 * a straight diameter. Those crossing lines are what tip it from "globe" to
 * "browser". The block body keeps it out of the document families, and at
 * sidebar sizes the hairlines go and the hue carries it.
 */
export function Web({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={59} rx={22} ry={4} />
      <Slab uid={uid} x={12} y={6} w={40} h={50} r={6} />
      {detail && (
        <g>
          <g fill="none" stroke="rgba(255,255,255,0.40)" strokeWidth="1.5" strokeLinecap="round">
            <circle cx={GLOBE.cx} cy={GLOBE.cy} r={GLOBE.r} />
            <ellipse cx={GLOBE.cx} cy={GLOBE.cy} rx={4.6} ry={GLOBE.r} />
            <ellipse cx={GLOBE.cx} cy={GLOBE.cy} rx={GLOBE.r} ry={4.4} />
            <path d={`M${GLOBE.cx - GLOBE.r} ${GLOBE.cy}H${GLOBE.cx + GLOBE.r}`} />
          </g>
          <Monogram label={spec.label} size={9} x={32} y={48} color="rgba(255,255,255,0.92)" />
        </g>
      )}
    </g>
  );
}
