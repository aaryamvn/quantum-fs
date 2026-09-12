import { ContactShadow, Monogram, NO_DETAIL_BELOW, Page } from "../base";
import type { IconFamilyProps } from "../types";

/** The faint copy above the stamp: x, y, width. */
const LINES: readonly [number, number, number][] = [
  [21, 18, 20],
  [21, 24, 16],
  [21, 30, 12],
];

/**
 * The one family whose monogram *is* the ornament.
 *
 * Every other family earns its read from geometry and keeps the label as a
 * footnote, but "PDF" has been three letters on a red page for thirty years —
 * drawing anything clever on top of that would only make it slower to
 * recognise. So the stamp is set at cap height 12 (a third larger than the
 * document's 9) and centerd on the body rather than tucked into the lower
 * third, and the three ruled lines are pushed to 0.22 so they read as the page
 * *behind* the label instead of competing with it. They also start below the
 * dog-ear, which the fold tint would otherwise cut through.
 */
export function Pdf({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={58.5} rx={21} ry={4} />
      <Page uid={uid} hue={spec.hue} x={14} y={6} w={36} h={50} fold={9} />
      {detail && (
        <g>
          {LINES.map(([lx, ly, lw]) => (
            <rect
              key={`${lx}-${ly}`}
              x={lx}
              y={ly}
              width={lw}
              height={2.5}
              rx={1.25}
              fill="rgba(255,255,255,0.22)"
            />
          ))}
          <Monogram label={spec.label} size={12} x={32} y={42} color="rgba(255,255,255,0.95)" />
        </g>
      )}
    </g>
  );
}
