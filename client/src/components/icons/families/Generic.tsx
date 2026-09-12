import { ContactShadow, Monogram, NO_DETAIL_BELOW, Slab } from "../base";
import type { IconFamilyProps } from "../types";

/**
 * The fallback body: a plain rounded block with nothing on it but the
 * extension.
 *
 * It is deliberately featureless. An unknown type should look like an unknown
 * type — inventing ornaments for it would make `.qqq` read as a real, specific
 * format. It also doubles as the stand-in body for families that have not been
 * drawn yet, so the grid is never empty mid-build.
 */
export function Generic({ size, spec, uid }: IconFamilyProps) {
  const detail = size >= NO_DETAIL_BELOW;

  return (
    <g>
      <ContactShadow uid={uid} cx={32} cy={59} rx={22} ry={4} />
      <Slab uid={uid} x={12} y={8} w={40} h={48} r={6} />
      {detail && (
        <Monogram label={spec.label} size={10} x={32} y={42} color="rgba(255,255,255,0.92)" />
      )}
    </g>
  );
}
