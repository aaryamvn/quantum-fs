import { ContactShadow, Monogram, NO_DETAIL_BELOW, Slab } from "../base";
import type { IconFamilyProps } from "../types";

/**
 * Vector artwork: the same landscape card as the image family, caught mid-edit.
 *
 * SVG, AI, EPS and vector PDFs are pictures too, so they keep the picture-shaped
 * body — what separates them is *how* the picture is stored, and the only
 * honest way to draw that is the editor's own language: one bezier with its
 * anchors selected and both control handles pulled out. No print rim here; a
 * vector file is a live canvas, not a photograph, and dropping the white border
 * is what tells the two siblings apart in a grid at a glance.
 *
 * Everything drawn on the card is a 1–1.5 unit stroke, so below
 * `NO_DETAIL_BELOW` the whole path goes and the colored card carries the type.
 */
export function Vector({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={51.5} rx={25} ry={4} />
      <Slab uid={uid} x={8} y={9} w={48} h={40} r={5} />
      {detail && (
        <g>
          <path
            d="M14 32L25 14"
            stroke="rgba(255,255,255,0.5)"
            strokeWidth="1"
            strokeLinecap="round"
          />
          <path
            d="M52 17L39 40"
            stroke="rgba(255,255,255,0.5)"
            strokeWidth="1"
            strokeLinecap="round"
          />
          <circle cx={25} cy={14} r={1.4} fill="rgba(255,255,255,0.5)" />
          <circle cx={39} cy={40} r={1.4} fill="rgba(255,255,255,0.5)" />
          <path
            d="M14 32C25 14 39 40 52 17"
            fill="none"
            stroke="rgba(255,255,255,0.85)"
            strokeWidth="1.5"
            strokeLinecap="round"
          />
          <rect x={12} y={30} width={4} height={4} rx={0.5} fill="rgba(255,255,255,0.9)" />
          <rect x={50} y={15} width={4} height={4} rx={0.5} fill="rgba(255,255,255,0.9)" />
          <rect x={12} y={36} width={19} height={9} rx={4.5} fill="rgba(0,0,0,0.3)" />
          <Monogram label={spec.label} size={6} x={21.5} y={40.5} color="rgba(255,255,255,0.92)" />
        </g>
      )}
    </g>
  );
}
