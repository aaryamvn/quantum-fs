import { memo, useId } from "react";

import { Defs, ICON_VIEW } from "./base";
import { FAMILY_COMPONENTS } from "./families";
import { iconSpecForName } from "./registry";

export interface FileIconProps {
  name: string;
  size?: number;
  className?: string;
}

/**
 * The one entry point for a file's icon: give it a file name, get the drawing.
 *
 * Gradient and clip ids are per instance (`useId`), because a vault grid mounts
 * hundreds of these at once and SVG ids are document-global — sharing them
 * would make every icon inherit the colors of whichever one rendered last.
 * Memoised for the same reason: scrolling a folder must not redraw a hundred
 * unchanged icons, and the props are two primitives, so the comparison is free.
 */
function FileIconImpl({ name, size = 64, className }: FileIconProps) {
  const uid = `i${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
  const spec = iconSpecForName(name);
  const Family = FAMILY_COMPONENTS[spec.family];

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
      <Family size={size} spec={spec} uid={uid} />
    </svg>
  );
}

export const FileIcon = memo(FileIconImpl);
