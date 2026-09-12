import { memo, useId } from "react";

import type { FolderColor } from "@/lib/backend/types";

import { ContactShadow, Defs, ICON_VIEW } from "./base";
import type { IconSpec } from "./types";

/**
 * The palette a user can tint a folder with.
 *
 * Two stops per color, not one: the whole system's depth comes from a lighter
 * face over a darker plate, so a folder color is a *pair* or it reads flat.
 * Violet and coral are the brand hues (#4E0EFF, #FF7B7B) softened at the top so
 * a wall of tinted folders never out-shouts the app chrome around it.
 */
export const FOLDER_COLOR_HEX: Record<FolderColor, { hue: string; hue2: string }> = {
  graphite: { hue: "#6E7A93", hue2: "#414B61" },
  coral: { hue: "#FF8E8E", hue2: "#D9535F" },
  violet: { hue: "#8A63FF", hue2: "#4E0EFF" },
  blue: { hue: "#63A9FF", hue2: "#2F6BDD" },
  teal: { hue: "#5FE3D0", hue2: "#1FA393" },
  green: { hue: "#7EDB8C", hue2: "#37A857" },
  amber: { hue: "#FFC77E", hue2: "#E38C2E" },
  pink: { hue: "#FF8ED6", hue2: "#D64FA4" },
  red: { hue: "#FF6E6E", hue2: "#C43A3A" },
};

export interface FolderIconProps {
  color?: FolderColor;
  size?: number;
  open?: boolean;
  className?: string;
}

/** The tab, drawn as a path because only its top corners are rounded. */
const TAB_PATH = "M9 10H23A3 3 0 0 1 26 13V20H6V13A3 3 0 0 1 9 10Z";

/**
 * A folder on the same 64-unit stage as every file icon, so a grid mixing the
 * two shares one horizon and one light direction.
 *
 * `open` leans the front flap rather than drawing a second silhouette: a real
 * open-folder shape changes the object's footprint, which makes drop targets
 * jump under the cursor mid-drag. A 6° skew pivoted on the bottom edge reads as
 * "open" while the hit area stays exactly where it was.
 */
function FolderIconImpl({ color = "graphite", size = 64, open = false, className }: FolderIconProps) {
  const uid = `f${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
  const { hue, hue2 } = FOLDER_COLOR_HEX[color];
  const spec: IconSpec = { family: "generic", label: "", hue, hue2, category: "folder" };
  // Pivot on the flap's bottom edge so the footprint never moves.
  const flap = open ? "translate(0 56) skewX(-6) translate(0 -56)" : undefined;

  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${ICON_VIEW} ${ICON_VIEW}`}
      className={className}
      aria-hidden="true"
      focusable="false"
    >
      <Defs uid={uid} spec={spec} />
      <ContactShadow uid={uid} cx={32} cy={59} rx={24} ry={4} />

      <g transform={flap}>
        <rect x={6} y={27} width={52} height={32} rx={5} fill={`url(#${uid}-edge)`} />
      </g>

      <path d={TAB_PATH} fill={`url(#${uid}-edge)`} />
      <rect x={6} y={16} width={52} height={38} rx={5} fill={`url(#${uid}-edge)`} />

      <g transform={flap}>
        <rect x={6} y={24} width={52} height={32} rx={5} fill={`url(#${uid}-body)`} />
        <path
          d="M11 24.9H53"
          stroke="rgba(255,255,255,0.35)"
          strokeWidth="1"
          strokeLinecap="round"
        />
        <clipPath id={`${uid}-flap`}>
          <rect x={6} y={24} width={52} height={32} rx={5} />
        </clipPath>
        <path
          d="M6 24H58L6 56Z"
          fill={`url(#${uid}-sheen)`}
          clipPath={`url(#${uid}-flap)`}
        />
      </g>
    </svg>
  );
}

export const FolderIcon = memo(FolderIconImpl);
