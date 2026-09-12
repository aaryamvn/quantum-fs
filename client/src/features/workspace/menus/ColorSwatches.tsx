import { motion, useReducedMotion } from "motion/react";
import { Check } from "lucide-react";

import { FOLDER_COLOR_HEX } from "@/components/icons";
import { Tooltip } from "@/components/ui/Tooltip";
import { FOLDER_COLORS } from "@/lib/backend";
import type { FolderColor, NodeId } from "@/lib/backend";

import { useNode, useWorkspace } from "../store";

/**
 * A folder's color, changed in one click without leaving the menu.
 *
 * Color is the only folder property people re-touch while comparing — you set
 * one, look at the grid, and immediately want a different one. A submenu would
 * make that a three-step round trip each time, so the swatches sit inline and,
 * uniquely in this menu, do *not* dismiss it: click, see the folder change
 * behind the panel, click again.
 *
 * Each disc is painted with the same two-stop gradient the folder icon uses, so
 * what you pick here is literally what the tile will look like. "Graphite" is the
 * absence of a color rather than a color, which is why it writes back `null`.
 */

/** The names the tooltips say — {@link FOLDER_COLORS} in the same order. */
const LABELS: Record<FolderColor, string> = {
  graphite: "Graphite",
  coral: "Coral",
  violet: "Violet",
  blue: "Blue",
  teal: "Teal",
  green: "Green",
  amber: "Amber",
  pink: "Pink",
  red: "Red",
};

export interface ColorSwatchesProps {
  nodeId: NodeId;
}

export function ColorSwatches({ nodeId }: ColorSwatchesProps) {
  const reduced = useReducedMotion() ?? false;
  const node = useNode(nodeId);
  const setColor = useWorkspace((s) => s.setColor);

  const current: FolderColor = node?.color ?? "graphite";

  return (
    <span data-testid="color-swatches" className="flex items-center gap-[5px]">
      {FOLDER_COLORS.map((color) => {
        const { hue, hue2 } = FOLDER_COLOR_HEX[color];
        const active = color === current;

        return (
          <Tooltip key={color} label={LABELS[color]}>
            <motion.button
              type="button"
              // Deliberately not a "menuitem": the roving focus of the list
              // treats a row as one stop, and nine extra stops in the middle of
              // it would bury everything below the color row.
              role="menuitemradio"
              aria-checked={active}
              aria-label={LABELS[color]}
              tabIndex={-1}
              whileTap={reduced ? undefined : { scale: 0.94 }}
              transition={reduced ? { duration: 0 } : { duration: 0.12 }}
              onClick={(e) => {
                // The row this sits in is inert, but the click still reaches the
                // menu; stopping it here is what keeps the panel open.
                e.stopPropagation();
                void setColor(nodeId, color === "graphite" ? null : color);
              }}
              className={`grid size-[14px] shrink-0 place-items-center rounded-full ${
                active ? "ring-1 ring-white/70" : ""
              }`}
              style={{ background: `linear-gradient(135deg, ${hue}, ${hue2})` }}
            >
              {active ? <Check size={9} strokeWidth={3} color="#FFFFFF" /> : null}
            </motion.button>
          </Tooltip>
        );
      })}
    </span>
  );
}

export default ColorSwatches;
