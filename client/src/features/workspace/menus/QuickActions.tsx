import { motion, useReducedMotion } from "motion/react";
import { Files, Info, Pencil, Share2, Trash2 } from "lucide-react";
import type { ReactNode } from "react";

import { Tooltip } from "@/components/ui/Tooltip";
import type { NodeId } from "@/lib/backend";
import { formatShortcut } from "@/lib/keys";

import { useWorkspace } from "../store";

/**
 * The five things you actually do to a file, as one centered row of discs above
 * the menu list.
 *
 * A right-click menu is read top-down, so the items people reach for constantly
 * — info, rename, duplicate, share, delete — would otherwise be scattered down a
 * twelve-row list and cost a scan every time. Lifting them into a row of targets
 * at the top makes them muscle memory (fixed position, fixed order) while the
 * list below stays the complete, labeled inventory: nothing here is an action
 * you cannot also find written out underneath.
 *
 * Each disc still acts on the *selection* rather than only the node that was
 * right-clicked, so a quick action and its twin in the list can never disagree.
 * Tooltips carry the label and the shortcut, which is what a naked icon owes the
 * reader — except on Delete, which says what it does through a trash glyph and a
 * red hover and would only get a hover label that reads as a warning nobody
 * asked for. Its name still reaches screen readers through `aria-label`.
 */

/** House icon metrics; the discs are big enough that the standard 16 still reads. */
const ICON = { size: 16, strokeWidth: 1.75 } as const;

interface ActionProps {
  label: string;
  shortcut?: string;
  danger?: boolean;
  /** Off for the destructive disc, which carries no hover label at all. */
  hint?: boolean;
  onSelect(): void;
  children: ReactNode;
}

/**
 * One disc. Hoisted out of {@link QuickActions} so its identity is stable across
 * renders — a component redefined inside another remounts on every render, which
 * would reset each tooltip's hesitation timer mid-hover.
 */
function Action({
  label,
  shortcut,
  danger = false,
  hint = true,
  onSelect,
  children,
}: ActionProps) {
  const reduced = useReducedMotion() ?? false;
  const closeContextMenu = useWorkspace((s) => s.closeContextMenu);

  const disc = (
    <motion.button
      type="button"
      role="menuitem"
      tabIndex={-1}
      aria-label={label}
      whileTap={reduced ? undefined : { scale: 0.94 }}
      transition={reduced ? { duration: 0 } : { duration: 0.12 }}
      onClick={() => {
        onSelect();
        closeContextMenu();
      }}
      className={[
        "grid size-[34px] place-items-center rounded-full bg-white/[0.05]",
        "transition-colors duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)]",
        danger
          ? "text-fg-2 hover:bg-coral/15 hover:text-coral focus:bg-coral/15 focus:text-coral"
          : "text-fg-2 hover:bg-white/[0.1] hover:text-fg focus:bg-white/[0.1] focus:text-fg",
      ].join(" ")}
    >
      {children}
    </motion.button>
  );

  if (!hint) return disc;

  return (
    <Tooltip label={label} shortcut={shortcut ? formatShortcut(shortcut) : undefined}>
      {disc}
    </Tooltip>
  );
}

export interface QuickActionsProps {
  nodeId: NodeId;
}

export function QuickActions({ nodeId }: QuickActionsProps) {
  const selection = useWorkspace((s) => s.selection);
  const openModal = useWorkspace((s) => s.openModal);
  const startRename = useWorkspace((s) => s.startRename);
  const duplicateNodes = useWorkspace((s) => s.duplicateNodes);
  const requestDelete = useWorkspace((s) => s.requestDelete);

  // Right-clicking inside a multi-selection acts on all of it; right-clicking
  // outside one acts on the single node under the pointer.
  const targets = selection.length > 1 && selection.includes(nodeId) ? selection : [nodeId];

  return (
    <div
      data-testid="quick-actions"
      className="flex items-center justify-center gap-[6px] px-[4px] pt-[4px] pb-[8px]"
    >
      <Action
        label="Get Info"
        shortcut="mod+i"
        onSelect={() => openModal({ kind: "info", nodeId })}
      >
        <Info {...ICON} />
      </Action>

      <Action label="Rename" shortcut="enter" onSelect={() => startRename(nodeId)}>
        <Pencil {...ICON} />
      </Action>

      <Action label="Duplicate" shortcut="mod+d" onSelect={() => void duplicateNodes(targets)}>
        <Files {...ICON} />
      </Action>

      <Action label="Share" onSelect={() => openModal({ kind: "share", nodeId })}>
        <Share2 {...ICON} />
      </Action>

      {/* No shortcut hint either: without a tooltip there is nowhere to show it,
          and ⌘⌫ is still bound on the canvas and written out in the menu below. */}
      <Action label="Delete" danger hint={false} onSelect={() => requestDelete(targets)}>
        <Trash2 {...ICON} />
      </Action>
    </div>
  );
}

export default QuickActions;
