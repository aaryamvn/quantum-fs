import { ContactShadow, Monogram, NO_DETAIL_BELOW, Slab } from "../base";
import type { IconFamilyProps } from "../types";

/** Bar heights, left to right. Deliberately lopsided — a symmetrical waveform reads as a chart. */
const BARS: readonly number[] = [6, 10, 15, 21, 25, 28, 23, 18, 13, 9, 6];

/** Vertical center the waveform is mirrored about. */
const AXIS = 27;

/** First bar's left edge; 2-unit bars on a 3-unit pitch put the 11th at x = 48. */
const BAR_X = 16;

/** The three chunky stand-ins drawn instead of the waveform below `NO_DETAIL_BELOW`: x, height. */
const COARSE: readonly [number, number][] = [
  [15, 14],
  [29, 26],
  [43, 18],
];

/**
 * Sound: a block carrying a waveform rather than a note.
 *
 * A quaver is a music glyph, and most of what lands in a vault under this
 * family is not music — takes, stems, voice memos, `.wav` captures. The
 * amplitude trace is the honest mark for "a signal you can hear", and it is
 * also the one audio shape that survives being drawn in flat white on any hue
 * the registry hands us.
 *
 * The inner five bars sit at 0.9 white and the outer six at 0.45 so the eye
 * lands on the middle of the envelope instead of reading eleven equal ticks.
 * Below `NO_DETAIL_BELOW` those 2-unit bars would render sub-pixel and smear
 * into a gray band, so three 6-unit bars stand in: the same gesture at a weight
 * an 18px sidebar can actually resolve.
 */
export function Audio({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={59} rx={23} ry={4} />
      <Slab uid={uid} x={10} y={6} w={44} h={50} r={6} />
      {detail ? (
        <g>
          <circle cx={16.5} cy={13.5} r={2.4} fill="rgba(255,255,255,0.45)" />
          <circle cx={16.5} cy={13.5} r={1} fill="rgba(255,255,255,0.85)" />
          {BARS.map((h, i) => (
            <rect
              key={BAR_X + i * 3}
              x={BAR_X + i * 3}
              y={AXIS - h / 2}
              width={2}
              height={h}
              rx={1}
              fill={i >= 3 && i <= 7 ? "rgba(255,255,255,0.9)" : "rgba(255,255,255,0.45)"}
            />
          ))}
          <Monogram label={spec.label} size={8} x={32} y={47.5} color="rgba(255,255,255,0.92)" />
        </g>
      ) : (
        <g>
          {COARSE.map(([bx, h]) => (
            <rect
              key={bx}
              x={bx}
              y={AXIS - h / 2}
              width={6}
              height={h}
              rx={3}
              fill="rgba(255,255,255,0.8)"
            />
          ))}
        </g>
      )}
    </g>
  );
}
