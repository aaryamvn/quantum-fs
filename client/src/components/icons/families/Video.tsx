import { ContactShadow, Monogram, NO_DETAIL_BELOW, shade, Slab } from "../base";
import type { IconFamilyProps } from "../types";

/** Left edge of each sprocket strip; the frame is symmetrical about x = 32. */
const STRIPS: readonly number[] = [6, 52];

/** Top edge of each perforation, evenly pitched down the strip. */
const HOLES: readonly number[] = [16.5, 25, 33.5, 42];

/**
 * Film: a landscape frame with sprocket strips and a play mark.
 *
 * Every other family on the stage is portrait — a page or a tall block — so the
 * 16:9 proportion alone already separates video from documents before any
 * ornament lands. The perforations are cut in a darker tint of the registry's
 * own `hue2` rather than a fixed black, which is what lets a cyan `.mov` and an
 * amber `.avi` both read as film without the strips ever fighting the body.
 *
 * The play triangle survives below `NO_DETAIL_BELOW` while the strips and the
 * monogram do not: at 18px the 3-unit perforations turn into gray noise, but
 * the triangle is a single bold shape that still says "this plays" — it is the
 * silhouette here, not an ornament.
 */
export function Video({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;
  const sprocket = shade(spec.hue2, -0.4);

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={55.5} rx={25} ry={4} />
      <Slab uid={uid} x={6} y={12} w={52} h={38} r={5} />
      {detail && (
        <g>
          <clipPath id={`${uid}-vfr`}>
            <rect x={6} y={12} width={52} height={38} rx={5} />
          </clipPath>
          <g clipPath={`url(#${uid}-vfr)`}>
            {STRIPS.map((sx) => (
              <rect key={sx} x={sx} y={12} width={6} height={38} fill={sprocket} />
            ))}
          </g>
          {STRIPS.map((sx) =>
            HOLES.map((hy) => (
              <rect
                key={`${sx}-${hy}`}
                x={sx + 1.5}
                y={hy}
                width={3}
                height={3.5}
                rx={1.25}
                fill="rgba(255,255,255,0.55)"
              />
            )),
          )}
        </g>
      )}
      <path
        d="M27.5 22.5L41.5 29L27.5 35.5Z"
        fill="rgba(255,255,255,0.9)"
        stroke="rgba(255,255,255,0.9)"
        strokeWidth="1.6"
        strokeLinejoin="round"
      />
      {detail && (
        <Monogram label={spec.label} size={8} x={32} y={44} color="rgba(255,255,255,0.92)" />
      )}
    </g>
  );
}
