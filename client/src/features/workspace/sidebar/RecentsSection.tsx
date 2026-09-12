import { AnimatePresence, motion, useReducedMotion } from "motion/react";

import { FileIcon, FolderIcon } from "@/components/icons";
import { Caption } from "@/components/ui/Caption";
import type { NodeId, Recent } from "@/lib/backend";

import { EASE } from "../layout";
import { useWorkspace } from "../store";

/** Five is what fits above the server tree without the tree losing its own scroll. */
const MAX_SHOWN = 5;

/**
 * The last things you touched, across every vault you are in.
 *
 * Recents are the only cross-vault surface in the rail, which is why a row can
 * do two different things: inside the open vault it is a navigation (go to the
 * folder, select the file), and outside it, it is a vault switch. The switch is
 * a `qfs:open-vault` window event rather than a store call, because opening
 * another vault means tearing down this one's tree, presence and subscription —
 * work the app shell owns. The rail only says which vault and where in it.
 *
 * Rows carry the same icons the canvas draws, at 18px. A separate "small" icon
 * set would make the recent copy of a file look like a different object from the
 * tile it points at, and recognition is the whole value of this list.
 */
export function RecentsSection() {
  const reduced = useReducedMotion() ?? false;
  const recents = useWorkspace((s) => s.recents);
  const vaultId = useWorkspace((s) => s.vaultId);

  const shown = recents.slice(0, MAX_SHOWN);

  function open(recent: Recent) {
    const node = recent.node;
    const isFolder = node.kind === "folder";
    // A file is shown by its parent folder with the file selected; a folder is
    // shown by entering it. `parentId` is null only for a root, which cannot be
    // a file — the fallback keeps the row harmless if the backend disagrees.
    const folderId: NodeId = isFolder ? node.id : (node.parentId ?? node.id);
    const select = isFolder ? undefined : [node.id];

    if (node.vaultId === vaultId) {
      const store = useWorkspace.getState();
      store.navigateTo(folderId);
      if (select) store.select(select, { anchor: node.id, focus: node.id });
      return;
    }

    window.dispatchEvent(
      new CustomEvent("qfs:open-vault", {
        detail: { vaultId: node.vaultId, folderId, select },
      }),
    );
  }

  return (
    <div data-testid="recents-section" className="shrink-0 px-[8px]">
      <Caption className="px-[4px] pt-[10px] pb-[6px]">Recents</Caption>

      {shown.length === 0 ? (
        <div className="px-[4px] pb-[4px] text-[12.5px] text-fg-3">Nothing yet</div>
      ) : (
        <AnimatePresence initial={false}>
          {shown.map((recent) => {
            const node = recent.node;
            return (
              <motion.button
                key={node.id}
                type="button"
                layout={reduced ? false : "position"}
                initial={reduced ? false : { opacity: 0, y: -4 }}
                animate={{ opacity: 1, y: 0 }}
                exit={reduced ? { opacity: 0 } : { opacity: 0, y: 4 }}
                transition={{ duration: reduced ? 0 : 0.22, ease: EASE }}
                onClick={() => open(recent)}
                data-node-id={node.id}
                title={node.name}
                className="mx-[4px] flex h-[28px] w-[calc(100%-8px)] items-center gap-[8px] rounded-[7px] px-[8px] transition-colors duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)] hover:bg-surface-hover"
              >
                <span aria-hidden className="grid shrink-0 place-items-center" style={{ lineHeight: 0 }}>
                  {node.kind === "folder" ? (
                    <FolderIcon color={node.color ?? "graphite"} size={18} />
                  ) : (
                    <FileIcon name={node.name} size={18} />
                  )}
                </span>

                <span className="min-w-0 flex-1 truncate text-left text-[13px] text-fg">
                  {node.name}
                </span>

                <span className="max-w-[84px] shrink-0 truncate text-[11.5px] text-fg-3">
                  {recent.vaultName}
                </span>
              </motion.button>
            );
          })}
        </AnimatePresence>
      )}
    </div>
  );
}

export default RecentsSection;
