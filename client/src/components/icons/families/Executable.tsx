import { ContactShadow, Monogram, NO_DETAIL_BELOW, shade, Slab } from "../base";
import type { IconFamilyProps } from "../types";

/** The three window buttons, left to right, in the traffic-light order everyone expects. */
const LIGHTS: readonly [number, string][] = [
  [17, "#FF7B7B"],
  [22.5, "#FFC27B"],
  [28, "#7BFFD6"],
];

/**
 * A terminal window: something that *runs* rather than something that opens.
 *
 * Every other family is a passive container, so an executable has to look
 * different in kind, not just in color — hence the inset dark screen, which
 * inverts the family's own light and is the only place in the set where the
 * body is punched through. The prompt does the rest of the talking.
 *
 * Below `NO_DETAIL_BELOW` that screen would eat ~half the face and take the
 * registry hue with it: every slate-colored `bat`/`dll`/`so` would collapse
 * into the same near-black square, and in the 18px sidebar it would read as a
 * hole. So the full screen is ornament like everything else, and the small
 * size keeps a token slot instead — barely a tenth of the face, only a fifth
 * darker — so the shape still says "window" while the hue stays in charge.
 */
export function Executable({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={58.5} rx={23} ry={4} />
      <Slab uid={uid} x={10} y={6} w={44} h={50} r={6} />
      {!detail && (
        <rect x={22} y={22} width={20} height={14} rx={2.5} fill={shade(spec.hue2, -0.2)} />
      )}
      {detail && (
        <g>
          <rect x={15} y={16} width={34} height={34} rx={3} fill={shade(spec.hue2, -0.5)} />
          {LIGHTS.map(([cx, fill]) => (
            <circle key={cx} cx={cx} cy={11.5} r={1.9} fill={fill} fillOpacity={0.9} />
          ))}
          <path
            d="M21 23L25.5 27L21 31"
            fill="none"
            stroke="rgba(255,255,255,0.9)"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
          <rect x={28} y={29} width={7.5} height={2.2} rx={1.1} fill="rgba(255,255,255,0.9)" />
          <Monogram label={spec.label} size={8} x={32} y={43} color="rgba(255,255,255,0.92)" />
        </g>
      )}
    </g>
  );
}
