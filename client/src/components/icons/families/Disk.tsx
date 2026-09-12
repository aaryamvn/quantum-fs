import { ContactShadow, Monogram, NO_DETAIL_BELOW, shade } from "../base";
import type { IconFamilyProps } from "../types";

/** A 60° specular sweep on an r-20 circle about (32,30), centerd at the top-left key. */
const SPECULAR = "M26.8 10.7A20 20 0 0 0 12.7 24.8";

/**
 * Volumes: a platter, not a slab.
 *
 * `.dmg`, `.iso`, `.vmdk` are not files people read, they are things people
 * mount — so this is the one family with no page and no block. A disc is round,
 * and round is the fastest possible discriminator in a grid where everything
 * else is a rounded rectangle; at 20px it is already unmistakable with the
 * monogram long gone.
 *
 * Depth here is the same trick as `Slab` done in the round: the edge-gradient
 * circle sits 3 units lower so only a crescent of plate shows under the body.
 * The label rides in a recessed pill rather than straight on the face, because
 * the hub and the inner ring already own the middle and an unbacked "QCOW2"
 * across a specular arc is unreadable. The pill is 26 wide and bottoms out at
 * y 50 for one reason: a circle of r 24 has only 26.5 units of chord left at
 * that height, so anything larger would poke its corners out through the rim.
 */
export function Disk({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={58} rx={22} ry={4} />
      <circle cx={32} cy={33} r={24} fill={`url(#${uid}-edge)`} />
      <circle cx={32} cy={30} r={24} fill={`url(#${uid}-body)`} />
      <clipPath id={`${uid}-dpl`}>
        <circle cx={32} cy={30} r={24} />
      </clipPath>
      <path d="M8 6H56L8 54Z" fill={`url(#${uid}-sheen)`} clipPath={`url(#${uid}-dpl)`} />
      {detail && (
        <g>
          <circle
            cx={32}
            cy={30}
            r={23.2}
            fill="none"
            stroke="rgba(255,255,255,0.16)"
            strokeWidth="1"
          />
          <circle
            cx={32}
            cy={30}
            r={17}
            fill="none"
            stroke="rgba(255,255,255,0.14)"
            strokeWidth="2"
          />
          <path
            d={SPECULAR}
            fill="none"
            stroke="rgba(255,255,255,0.35)"
            strokeWidth="2"
            strokeLinecap="round"
          />
        </g>
      )}
      <circle cx={32} cy={30} r={6} fill={shade(spec.hue2, -0.4)} />
      {detail && (
        <g>
          <circle cx={32} cy={30} r={2} fill="rgba(255,255,255,0.8)" />
          <rect x={19} y={39} width={26} height={11} rx={5.5} fill={shade(spec.hue2, -0.45)} />
          <path
            d="M24.5 39.9H39.5"
            stroke="rgba(255,255,255,0.18)"
            strokeWidth="1"
            strokeLinecap="round"
          />
          <Monogram label={spec.label} size={7} x={32} y={44.5} color="rgba(255,255,255,0.92)" />
        </g>
      )}
    </g>
  );
}
