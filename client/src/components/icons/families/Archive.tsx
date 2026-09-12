import { ContactShadow, Monogram, NO_DETAIL_BELOW, shade, Slab } from "../base";
import type { IconFamilyProps } from "../types";

/** Zipper teeth: y of the tooth, and whether it bites from the left half. */
const TEETH: readonly [number, boolean][] = [
  [18.4, true],
  [21.8, false],
  [25.2, true],
  [28.6, false],
  [32, true],
  [35.4, false],
  [38.8, true],
];

/**
 * A zipped box — the one picture everyone already reads as "this is packed".
 *
 * An archive has no shape of its own (a `.zip` is whatever you put in it), so
 * the family leans entirely on the fastener: a darker channel straight down the
 * middle with teeth interlocking across it and a pull parked at the top. That
 * vertical seam survives being shrunk far better than a stack of tiny boxes
 * would, which is why below `NO_DETAIL_BELOW` the channel stays and everything
 * finer — teeth, pull, band, monogram — is dropped rather than smeared.
 */
export function Archive({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={58.5} rx={23} ry={4} />
      <Slab uid={uid} x={10} y={6} w={44} h={50} r={6} />
      {detail && <rect x={10} y={13} width={44} height={6} fill="rgba(255,255,255,0.14)" />}
      <rect x={29} y={9} width={6} height={32} rx={1.5} fill={shade(spec.hue2, -0.2)} />
      {detail && (
        <g>
          {TEETH.map(([ty, left]) => (
            <rect
              key={ty}
              x={left ? 29.4 : 32.2}
              y={ty}
              width={2.4}
              height={2.2}
              rx={0.7}
              fill="rgba(255,255,255,0.7)"
            />
          ))}
          <rect x={31.2} y={13.6} width={1.6} height={3.2} rx={0.8} fill="rgba(255,255,255,0.72)" />
          <rect x={29.4} y={9.6} width={5.2} height={4.4} rx={1.6} fill="rgba(255,255,255,0.9)" />
          <Monogram label={spec.label} size={9} x={32} y={48} color="rgba(255,255,255,0.92)" />
        </g>
      )}
    </g>
  );
}
