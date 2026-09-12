import { ContactShadow, Monogram, NO_DETAIL_BELOW, Page } from "../base";
import type { IconFamilyProps } from "../types";

/**
 * The two braces, mirrored about the stage center at x=32. Hand-tuned quadratics
 * rather than a font glyph: a real `{` from GT Walsheim is far too fine to hold
 * up as a 22-unit ornament, and a text node would re-hint at every icon size.
 */
const BRACE_LEFT =
  "M27 17Q23.5 17 23.5 21.5V24.3Q23.5 27 21.5 27Q23.5 27 23.5 29.7V32.5Q23.5 37 27 37";
const BRACE_RIGHT =
  "M37 17Q40.5 17 40.5 21.5V24.3Q40.5 27 42.5 27Q40.5 27 40.5 29.7V32.5Q40.5 37 37 37";

/**
 * Structured data: a sheet carrying one big pair of curly braces.
 *
 * JSON, YAML, TOML and XML share no visual convention except this one — the
 * brace is the universal "a machine reads this" mark, and it survives being
 * scaled down far better than a tree of nested rows would. Drawn as strokes
 * with generous round joins so the shape stays fat and legible instead of
 * turning into two hairline squiggles, and left empty in the middle so the hue
 * still does most of the identifying work.
 */
export function Data({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={58.5} rx={21} ry={4} />
      <Page uid={uid} hue={spec.hue} x={14} y={6} w={36} h={50} fold={10} />
      {detail && (
        <g>
          <g
            fill="none"
            stroke="rgba(255,255,255,0.45)"
            strokeWidth="2.2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d={BRACE_LEFT} />
            <path d={BRACE_RIGHT} />
          </g>
          <Monogram label={spec.label} size={9} x={32} y={48.5} color="rgba(255,255,255,0.92)" />
        </g>
      )}
    </g>
  );
}
