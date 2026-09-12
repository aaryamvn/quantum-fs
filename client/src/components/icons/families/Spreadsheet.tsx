import { ContactShadow, Monogram, NO_DETAIL_BELOW, Page } from "../base";
import type { IconFamilyProps } from "../types";

/**
 * The grid lives on half-unit coordinates so a 0.5px-wide stroke never straddles
 * a device pixel at the 1:1 size (64 units → 64px). Whole-unit rules here turned
 * the 4×5 grid into a gray haze the moment the icon was drawn at its natural
 * size, which is exactly where the spreadsheet read has to survive.
 */
const GRID_X = [19.5, 26, 32.5, 39, 45.5] as const;
const GRID_Y = [17.5, 21.5, 25.5, 29.5, 33.5, 37.5] as const;

const GRID_D = [
  ...GRID_X.map((x) => `M${x} ${GRID_Y[0]}V${GRID_Y[GRID_Y.length - 1]}`),
  ...GRID_Y.map((y) => `M${GRID_X[0]} ${y}H${GRID_X[GRID_X.length - 1]}`),
].join("");

/** The two "selected" cells, as [column, row] into the grid above. */
const HIGHLIGHTS: readonly [number, number][] = [
  [1, 1],
  [3, 3],
];

/**
 * A sheet of numbers: the document silhouette with a ruled table on it.
 *
 * The grid sits in the upper two thirds and starts below the dog-ear — the fold
 * tint is a lighter shade of the hue, and a table running under it would read as
 * a printing error rather than a turned corner. A filled header row plus two lit
 * cells are what make it "a spreadsheet" instead of "ruled paper": they are the
 * bits of an Excel window the eye actually recognises at 64px, before the
 * monogram is legible at all. Below `NO_DETAIL_BELOW` the whole table goes and
 * the green page carries the meaning on its own.
 */
export function Spreadsheet({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={58.5} rx={21} ry={4} />
      <Page uid={uid} hue={spec.hue} x={14} y={6} w={36} h={50} fold={9} />
      {detail && (
        <g>
          <rect
            x={GRID_X[0]}
            y={GRID_Y[0]}
            width={GRID_X[4] - GRID_X[0]}
            height={GRID_Y[1] - GRID_Y[0]}
            fill="rgba(255,255,255,0.22)"
          />
          {HIGHLIGHTS.map(([col, row]) => (
            <rect
              key={`${col}-${row}`}
              x={GRID_X[col]}
              y={GRID_Y[row]}
              width={GRID_X[col + 1] - GRID_X[col]}
              height={GRID_Y[row + 1] - GRID_Y[row]}
              fill="rgba(255,255,255,0.5)"
            />
          ))}
          <path d={GRID_D} stroke="rgba(255,255,255,0.28)" strokeWidth="1" fill="none" />
          <Monogram label={spec.label} size={9} x={32} y={48} color="rgba(255,255,255,0.92)" />
        </g>
      )}
    </g>
  );
}
