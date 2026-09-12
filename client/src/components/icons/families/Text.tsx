import { ContactShadow, Monogram, NO_DETAIL_BELOW, Page } from "../base";
import type { IconFamilyProps } from "../types";

/** Ruled copy lines: y, width. Widths taper so the block reads as a paragraph. */
const LINES: readonly [number, number][] = [
  [17, 21],
  [23, 19],
  [29, 16],
  [35, 13],
  [41, 9],
];

/**
 * Plain prose: a sheet dense with ruled lines and nothing else.
 *
 * `.txt` and `.md` have no ornament to draw — the *only* honest signal is "this
 * is words". So the family says it by volume: five lines instead of the
 * `Document` family's three, packed tighter and thinner, tapering the way a
 * real paragraph ends. Side by side in a folder that reads as "more text, less
 * structure" without either icon needing its label. Below `NO_DETAIL_BELOW` the
 * hairlines would alias into a gray smear, so only the dog-eared page survives.
 */
export function Text({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={58.5} rx={21} ry={4} />
      <Page uid={uid} hue={spec.hue} x={14} y={6} w={36} h={50} fold={9} />
      {detail && (
        <g>
          {LINES.map(([ly, lw]) => (
            <rect
              key={ly}
              x={21}
              y={ly}
              width={lw}
              height={2.5}
              rx={1.25}
              fill="rgba(255,255,255,0.30)"
            />
          ))}
          <Monogram label={spec.label} size={9} x={32} y={50} color="rgba(255,255,255,0.92)" />
        </g>
      )}
    </g>
  );
}
