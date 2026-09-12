import { ContactShadow, Monogram, NO_DETAIL_BELOW, Page, shade } from "../base";
import type { IconFamilyProps } from "../types";

/**
 * A sealed document: keys, certificates and licences — paper that proves
 * something rather than paper that says something.
 *
 * It keeps the `Document` sheet on purpose, because these really are documents;
 * what separates them is the pair of trust marks laid on it. The padlock says
 * "this is secret material" and the stamped seal says "this was signed", and
 * between them they cover both halves of the family without needing the
 * monogram. The lock body — one solid white block — is the only mark that
 * survives below `NO_DETAIL_BELOW`; the shackle stroke and the tick inside the
 * seal would both be sub-pixel at 18px and would only muddy the sheet.
 */
export function Certificate({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={58.5} rx={21} ry={4} />
      <Page uid={uid} hue={spec.hue} x={14} y={6} w={36} h={50} fold={9} />
      {detail && (
        <path
          d="M28 21V18A4 4 0 0 1 36 18V21"
          fill="none"
          stroke="rgba(255,255,255,0.85)"
          strokeWidth="2.2"
          strokeLinecap="round"
        />
      )}
      <rect x={25.5} y={21} width={13} height={10} rx={2.5} fill="rgba(255,255,255,0.85)" />
      {detail && (
        <g>
          <circle cx={32} cy={25.2} r={1.4} fill={spec.hue2} />
          <rect x={31.4} y={25.2} width={1.2} height={3.2} rx={0.6} fill={spec.hue2} />
          <circle
            cx={41}
            cy={38}
            r={6}
            fill={shade(spec.hue, 0.2)}
            stroke="rgba(255,255,255,0.35)"
            strokeWidth="0.75"
          />
          <path
            d="M38.2 38L40.4 40.2L44 36"
            fill="none"
            stroke="rgba(255,255,255,0.9)"
            strokeWidth="1.8"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
          <Monogram label={spec.label} size={9} x={32} y={50} color="rgba(255,255,255,0.92)" />
        </g>
      )}
    </g>
  );
}
