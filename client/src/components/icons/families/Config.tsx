import { ContactShadow, Monogram, NO_DETAIL_BELOW, Slab } from "../base";
import type { IconFamilyProps } from "../types";

/**
 * An 8-tooth gear as a single path, built once at module load.
 *
 * Per tooth the outline walks outer-radius → outer-radius → root → root, which
 * is enough at 64 units to read as machined teeth without the arc maths; the
 * hole is a second subpath so `evenodd` punches the body gradient through the
 * middle instead of us stacking an opaque disc on top of it.
 */
const GEAR = (() => {
  const cx = 22.5;
  const cy = 18.5;
  const ro = 6.6;
  const ri = 4.8;
  const hole = 2.4;
  const at = (r: number, deg: number): string => {
    const a = (deg * Math.PI) / 180;
    return `${(cx + r * Math.cos(a)).toFixed(2)} ${(cy + r * Math.sin(a)).toFixed(2)}`;
  };
  const pts: string[] = [];
  for (let i = 0; i < 8; i += 1) {
    const a = i * 45;
    pts.push(at(ro, a - 11), at(ro, a + 11), at(ri, a + 16), at(ri, a + 29));
  }
  const ring = `M${pts[0]}${pts.slice(1).map((p) => `L${p}`).join("")}Z`;
  const bore =
    `M${cx + hole} ${cy}` +
    `A${hole} ${hole} 0 1 0 ${cx - hole} ${cy}` +
    `A${hole} ${hole} 0 1 0 ${cx + hole} ${cy}Z`;
  return ring + bore;
})();

/** Slider tracks: center y, and the knob's x along the 18..46 track. */
const SLIDERS: readonly [number, number][] = [
  [30, 39],
  [36, 24],
  [42, 32],
];

/**
 * Settings: a solid block with a gear and three sliders on it.
 *
 * `.env`, `.ini`, `Dockerfile`, `Makefile`, lockfiles — none of these are
 * documents, they are knobs someone turned, so the silhouette is a `Slab`
 * rather than a `Page`; the missing dog-ear alone separates this family from
 * `Data` at a glance. Gear plus sliders instead of a gear alone because a lone
 * gear is the universal "settings" chrome glyph and would read as an app
 * preference pane; three knobs at three different positions say "values that
 * were set", which is what a config file actually is.
 */
export function Config({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={59} rx={22} ry={4} />
      <Slab uid={uid} x={12} y={6} w={40} h={50} r={6} />
      {detail && (
        <g>
          <path d={GEAR} fillRule="evenodd" fill="rgba(255,255,255,0.40)" />
          {SLIDERS.map(([cy, kx]) => (
            <g key={cy}>
              <rect
                x={18}
                y={cy - 1.2}
                width={28}
                height={2.4}
                rx={1.2}
                fill="rgba(255,255,255,0.35)"
              />
              <circle cx={kx} cy={cy} r={2.3} fill="rgba(255,255,255,0.80)" />
            </g>
          ))}
          <Monogram label={spec.label} size={8} x={32} y={50} color="rgba(255,255,255,0.92)" />
        </g>
      )}
    </g>
  );
}
