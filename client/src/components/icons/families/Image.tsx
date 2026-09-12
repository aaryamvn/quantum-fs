import { ContactShadow, Monogram, NO_DETAIL_BELOW, shade, Slab } from "../base";
import type { IconFamilyProps } from "../types";

/**
 * Raster images: a landscape print standing on the stage.
 *
 * The card is wider than it is tall — the one proportion nobody confuses with a
 * document — and the picture on it is the oldest shorthand there is: a sun over
 * two ridges. Those are drawn from `spec.hue2` rather than from fixed greens, so
 * a PNG (blue), a HEIC (violet) and a RAW (amber) each get a scene in their own
 * color instead of three identical stock photos.
 *
 * The ridges and the sun survive below `NO_DETAIL_BELOW` because they are large
 * flat fills that still resolve at 18px and are the entire reason the shape
 * reads as a photo; the white print rim, the label pill and the monogram are
 * hairlines and type, and those are what turn to mush, so they go.
 */
export function Image({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;
  const clip = `${uid}-imgc`;

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={51.5} rx={25} ry={4} />
      <Slab uid={uid} x={8} y={9} w={48} h={40} r={5} />
      <clipPath id={clip}>
        <rect x={8} y={9} width={48} height={40} rx={5} />
      </clipPath>
      <g clipPath={`url(#${clip})`}>
        <circle cx={45} cy={20} r={4.5} fill="rgba(255,255,255,0.6)" />
        <path d="M12 49L26 26L40 49Z" fill={shade(spec.hue2, -0.15)} />
        <path d="M28 49L41 33L56 49Z" fill={shade(spec.hue2, -0.35)} />
      </g>
      {detail && (
        <g>
          <rect
            x={9}
            y={10}
            width={46}
            height={38}
            rx={4}
            fill="none"
            stroke="rgba(255,255,255,0.85)"
            strokeWidth="2"
          />
          <rect x={12} y={36} width={19} height={9} rx={4.5} fill="rgba(0,0,0,0.3)" />
          <Monogram label={spec.label} size={6} x={21.5} y={40.5} color="rgba(255,255,255,0.92)" />
        </g>
      )}
    </g>
  );
}
