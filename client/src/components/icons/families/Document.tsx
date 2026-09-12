import { ContactShadow, Monogram, NO_DETAIL_BELOW, Page } from "../base";
import type { IconFamilyProps } from "../types";

/** Ruled lines standing in for body copy: x, y, width. */
const LINES: readonly [number, number, number][] = [
  [21, 24, 20],
  [21, 31, 16],
  [21, 38, 12],
];

/**
 * The reference family every other one is measured against.
 *
 * A single sheet with a turned-down corner, three ruled lines where the text
 * would be, and the extension stamped in the lower third — the Big Sur document
 * read, which is the one file icon everybody already knows. The lines and the
 * stamp are the first things to go below `NO_DETAIL_BELOW`: in the 18px sidebar
 * they collapse into noise, while the dog-eared silhouette still says
 * "document" on its own.
 */
export function Document({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={58.5} rx={21} ry={4} />
      <Page uid={uid} hue={spec.hue} x={14} y={6} w={36} h={50} fold={10} />
      {detail && (
        <g>
          {LINES.map(([lx, ly, lw]) => (
            <rect
              key={`${lx}-${ly}`}
              x={lx}
              y={ly}
              width={lw}
              height={3}
              rx={1.5}
              fill="rgba(255,255,255,0.28)"
            />
          ))}
          <Monogram label={spec.label} size={9} x={32} y={50} color="rgba(255,255,255,0.92)" />
        </g>
      )}
    </g>
  );
}
