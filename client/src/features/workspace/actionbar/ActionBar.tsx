import { FilePlus, FolderPlus, History, LayoutGrid, Lock, Share2, Upload } from "lucide-react";
import { motion, useReducedMotion } from "motion/react";
import { useCallback, useRef } from "react";
import type { ChangeEvent, ReactElement } from "react";

import { Divider } from "@/components/ui/Divider";
import { GhostButton } from "@/components/ui/GhostButton";
import { IconButton } from "@/components/ui/IconButton";
import { Tooltip } from "@/components/ui/Tooltip";
import { formatShortcut } from "@/lib/keys";

import { ACTIONBAR_H, EASE, Z } from "../layout";
import { useIsCreator, useWorkspace } from "../store";
import { SortMenu } from "./SortMenu";

/**
 * The verbs of the folder you are standing in, spelled out.
 *
 * Every action here is also on the right-click menu and most are on a shortcut,
 * so the bar is not the only way in — it is the way in you can find without
 * knowing anything, which is why each control keeps its word instead of
 * shrinking to an icon. The row is deliberately short: anything that acts on a
 * *selection* belongs to the selection bar, and mixing the two would leave half
 * the strip grayed out whenever nothing was picked.
 *
 * Access is the human's per-folder permission control for the current folder and
 * is creator-only, disabled rather than hidden: a control that vanishes teaches
 * nobody that the permission exists, while a dimmed one with a reason does.
 */

/** Tooltips need an element that takes a ref; GhostButton is a plain function. */
function Hint({
  label,
  shortcut,
  children,
}: {
  label: string;
  shortcut?: string;
  children: ReactElement;
}) {
  return (
    <Tooltip label={label} shortcut={shortcut}>
      <span className="inline-flex">{children}</span>
    </Tooltip>
  );
}

export function ActionBar() {
  const reduced = useReducedMotion() ?? false;

  const folderId = useWorkspace((s) => s.folderId);
  const createNode = useWorkspace((s) => s.createNode);
  const openModal = useWorkspace((s) => s.openModal);
  const toast = useWorkspace((s) => s.toast);
  const isCreator = useIsCreator(folderId);

  const fileInput = useRef<HTMLInputElement>(null);

  // Upload has no daemon behind it yet. Rather than a dead button, the picker
  // opens for real and the count comes back as a toast, so the gap is honest and
  // the wiring (picker → count → message) is already proven when the daemon lands.
  const onPicked = useCallback(
    (e: ChangeEvent<HTMLInputElement>) => {
      const count = e.target.files?.length ?? 0;
      if (count > 0) toast(`Upload lands with the daemon — ${count} files skipped`, "info");
      e.target.value = "";
    },
    [toast],
  );

  const openFor = useCallback(
    (kind: "share" | "access" | "history") => {
      if (!folderId) return;
      openModal({ kind, nodeId: folderId });
    },
    [folderId, openModal],
  );

  return (
    <motion.div
      data-testid="actionbar"
      initial={reduced ? { opacity: 0 } : { opacity: 0, y: -6 }}
      animate={reduced ? { opacity: 1 } : { opacity: 1, y: 0 }}
      transition={{ duration: reduced ? 0 : 0.16, ease: EASE }}
      style={{ height: ACTIONBAR_H, zIndex: Z.chrome }}
      className="sticky top-0 flex shrink-0 items-center gap-[2px] border-b border-line bg-bg px-[14px]"
    >
      <Hint label="New Folder" shortcut={formatShortcut("mod+shift+n")}>
        <GhostButton
          icon={<FolderPlus size={14} strokeWidth={1.75} />}
          disabled={!folderId}
          onClick={() => void createNode("folder")}
        >
          New Folder
        </GhostButton>
      </Hint>

      <Hint label="New File" shortcut={formatShortcut("mod+n")}>
        <GhostButton
          icon={<FilePlus size={14} strokeWidth={1.75} />}
          disabled={!folderId}
          onClick={() => void createNode("file")}
        >
          New File
        </GhostButton>
      </Hint>

      <GhostButton
        icon={<Upload size={14} strokeWidth={1.75} />}
        disabled={!folderId}
        onClick={() => fileInput.current?.click()}
      >
        Upload
      </GhostButton>

      <input
        ref={fileInput}
        type="file"
        multiple
        hidden
        tabIndex={-1}
        aria-hidden
        onChange={onPicked}
      />

      {/* The hairline is 16px of the 44px row, so it is sized by a wrapper: `h-full`
          inside Divider then resolves to exactly that, with no utility-order gamble
          between `h-full` and an arbitrary height on the same element. */}
      <span className="mx-[6px] flex h-[16px] items-stretch">
        <Divider vertical />
      </span>

      <GhostButton
        icon={<Share2 size={14} strokeWidth={1.75} />}
        disabled={!folderId}
        onClick={() => openFor("share")}
      >
        Share
      </GhostButton>

      {isCreator ? (
        <GhostButton
          icon={<Lock size={14} strokeWidth={1.75} />}
          onClick={() => openFor("access")}
        >
          Access
        </GhostButton>
      ) : (
        <Hint label="Only the creator can change access">
          <GhostButton icon={<Lock size={14} strokeWidth={1.75} />} disabled>
            Access
          </GhostButton>
        </Hint>
      )}

      <GhostButton
        icon={<History size={14} strokeWidth={1.75} />}
        disabled={!folderId}
        onClick={() => openFor("history")}
      >
        History
      </GhostButton>

      <div className="ml-auto flex items-center gap-[6px]">
        <SortMenu />
        <IconButton
          icon={<LayoutGrid size={16} strokeWidth={1.75} />}
          label="Icon view"
          active
        />
      </div>
    </motion.div>
  );
}
