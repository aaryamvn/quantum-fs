import { FOLDER_COLOR_HEX } from "@/components/icons";
import { Tooltip } from "@/components/ui/Tooltip";
import type { FolderColor, FsNode } from "@/lib/backend";
import { FOLDER_COLORS } from "@/lib/backend";

import { useWorkspace } from "../store";

export interface ColorRowProps {
  node: FsNode;
}

/** "graphite" → "Graphite". The palette is authored lowercase; only the label is titled. */
function titled(color: FolderColor): string {
  return color.charAt(0).toUpperCase() + color.slice(1);
}

/**
 * The nine folder tints, as the swatches themselves.
 *
 * A dropdown would hide the one thing that matters — what the colors look like
 * against this background — so the whole palette is always on screen and one
 * click away. Each swatch carries the icon's own two-stop gradient rather than a
 * flat fill, so what you pick is literally what the folder becomes.
 *
 * Graphite leads because it is the default, and picking it clears the tint
 * (`null`) rather than storing "graphite": a folder that was never colored and
 * one that was colored back must be the same node.
 */
export function ColorRow({ node }: ColorRowProps) {
  const setColor = useWorkspace((s) => s.setColor);
  const current: FolderColor = node.color ?? "graphite";

  if (node.kind !== "folder") return null;

  return (
    <div data-testid="inspector-colors" data-node-id={node.id} className="flex items-center gap-[10px]">
      {FOLDER_COLORS.map((color) => {
        const { hue, hue2 } = FOLDER_COLOR_HEX[color];
        const selected = color === current;

        return (
          <Tooltip key={color} label={titled(color)}>
            <button
              type="button"
              aria-label={titled(color)}
              aria-pressed={selected}
              onClick={() => void setColor(node.id, color === "graphite" ? null : color)}
              className={`h-[18px] w-[18px] shrink-0 rounded-full transition-[box-shadow] duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)] ${
                selected ? "ring-2 ring-white/60 ring-offset-2 ring-offset-surface" : ""
              }`}
              style={{ background: `linear-gradient(160deg, ${hue}, ${hue2})` }}
            />
          </Tooltip>
        );
      })}
    </div>
  );
}
