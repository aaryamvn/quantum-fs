import { ContactShadow, DEPTH, Monogram, NO_DETAIL_BELOW, shade } from "../base";
import type { IconFamilyProps } from "../types";

/** Center-line y of each disc's top rim; 14 apart = a 12-unit body plus the 2-unit gap. */
const TIERS: readonly number[] = [12, 26, 40];

/** One disc: 40 wide, 12 tall, capped top and bottom by the same 20×4 ellipse. */
function tierPath(t: number): string {
  return `M12 ${t}V${t + 12}A20 4 0 0 0 52 ${t + 12}V${t}A20 4 0 0 0 12 ${t}Z`;
}

/**
 * Three stacked platters — the shape that has meant "stored records" since
 * spinning disks actually looked like this.
 *
 * The stack is drawn top tier first so each lower disc's rim laps over the one
 * above it, the way real cylinders would nest; a darker crescent is laid down
 * just before each lower body so the seam reads as a gap in the stack rather
 * than as one tall tube with lines ruled across it. Discs are the whole
 * silhouette here, so only the monogram and the rim highlight are dropped below
 * `NO_DETAIL_BELOW` — the stack itself is already legible at 20px.
 */
export function Database({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;
  const rim = shade(spec.hue, 0.25);
  const gap = shade(spec.hue2, -0.45);

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={58.5} rx={22} ry={4} />
      <g transform={`translate(0 ${DEPTH})`}>
        <path d="M12 12V52A20 4 0 0 0 52 52V12A20 4 0 0 0 12 12Z" fill={`url(#${uid}-edge)`} />
      </g>
      {TIERS.map((t, i) => (
        <g key={t}>
          {i > 0 && <ellipse cx={32} cy={t - 2.5} rx={20} ry={4} fill={gap} />}
          <path d={tierPath(t)} fill={`url(#${uid}-body)`} />
          <ellipse cx={32} cy={t} rx={20} ry={4} fill={rim} />
        </g>
      ))}
      {detail && (
        <g>
          <path
            d="M12.7 10.96A20 4 0 0 1 37.2 8.14"
            fill="none"
            stroke="rgba(255,255,255,0.45)"
            strokeWidth="1.2"
            strokeLinecap="round"
          />
          <Monogram label={spec.label} size={8} x={32} y={46.5} color="rgba(255,255,255,0.92)" />
        </g>
      )}
    </g>
  );
}
