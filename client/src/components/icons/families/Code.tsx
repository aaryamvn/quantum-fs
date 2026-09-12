import { ContactShadow, Monogram, NO_DETAIL_BELOW, Page, shade } from "../base";
import type { IconFamilyProps } from "../types";

/** The inset editor panel, in page-relative units (the page is x14..50, y6..56). */
const PANEL = { x: 19, y: 16, w: 26, h: 28, r: 2.5 } as const;

/**
 * Four lines of "source": x (indent level), y, width, and which ink to use.
 * `0` = plain white, `1` = the hue-lightened accent, `2` = the dimmed comment.
 */
const BARS: readonly [number, number, number, 0 | 1 | 2][] = [
  [22.5, 28, 14, 0],
  [26, 32, 10.5, 1],
  [26, 36, 14.5, 2],
  [22.5, 40, 8, 1],
];

/** The `‹ ›` pair, as two open chevrons rather than a text glyph. */
const CHEVRON_LEFT = "M25 18.8L22.2 22L25 25.2";
const CHEVRON_RIGHT = "M28.2 18.8L31 22L28.2 25.2";

/**
 * Source code: a sheet with a dark editor window sunk into it.
 *
 * This one family carries most of the registry — Python, Rust, TypeScript, Go,
 * Ruby, every hue the palette has — so it cannot lean on color to mean
 * "code"; the drawing has to. The dark panel does that job, and it does it the
 * same way for every hue because its fill is derived from `hue2` rather than
 * fixed: `shade(hue2, -0.45)` lands far enough down that a yellow `.js` and a
 * blue `.ts` both end up with a near-black window, and white/accent bars read
 * on all of them. The bars sit at two indents so the panel reads as a nested
 * block rather than a paragraph, and the chevrons name the thing outright for
 * anyone who does not know the extension.
 */
export function Code({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;
  const inks = ["rgba(255,255,255,0.50)", shade(spec.hue, 0.5), "rgba(255,255,255,0.25)"] as const;

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={58.5} rx={21} ry={4} />
      <Page uid={uid} hue={spec.hue} x={14} y={6} w={36} h={50} fold={10} />
      {detail && (
        <g>
          <rect
            x={PANEL.x}
            y={PANEL.y}
            width={PANEL.w}
            height={PANEL.h}
            rx={PANEL.r}
            fill={shade(spec.hue2, -0.45)}
            stroke="rgba(0,0,0,0.18)"
            strokeWidth="0.75"
          />
          <g
            fill="none"
            stroke="rgba(255,255,255,0.55)"
            strokeWidth="1.4"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d={CHEVRON_LEFT} />
            <path d={CHEVRON_RIGHT} />
          </g>
          {BARS.map(([bx, by, bw, ink]) => (
            <rect
              key={by}
              x={bx}
              y={by}
              width={bw}
              height={2.2}
              rx={1.1}
              fill={inks[ink]}
            />
          ))}
          <Monogram label={spec.label} size={8.5} x={32} y={50} color="rgba(255,255,255,0.92)" />
        </g>
      )}
    </g>
  );
}
