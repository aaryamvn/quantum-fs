import { AlertTriangle } from "lucide-react";
import { useEffect, useState } from "react";

import { FileIcon, FolderIcon } from "@/components/icons";
import { GhostButton } from "@/components/ui/GhostButton";
import { Modal } from "@/components/ui/Modal";
import { PrimaryButton } from "@/components/ui/PrimaryButton";

import type { FsNode, NodeId } from "@/lib/backend";

import { useWorkspace } from "../store";

/** More than this and the list stops being a list and starts being a wall. */
const MAX_LISTED = 5;

/**
 * The only irreversible action in the app, so it is the only one that asks.
 *
 * What it asks with is the truth about a replicated file system: this is not a
 * local delete that a trash can undoes, it is an op that reaches every member's
 * copy. The dialog names the items rather than a count alone — "Delete 7 items?"
 * is not something anyone can actually confirm — and counts the children a
 * folder takes down with it, which is the part people are surprised by.
 *
 * Cancel reads above Delete, the way the safer choice should, while Delete is
 * first in the DOM so that the control holding focus is the one Enter fires —
 * a focus ring on Cancel and a Return key that deletes is the worst possible
 * pairing on the one dialog that cannot be undone. Cancel carries the secondary
 * surface so the way out is visible before it is hovered; nothing here has a
 * tooltip, because a hover label on a delete confirmation explains nothing the
 * sentence above it has not already said.
 */
export function ConfirmDeleteModal() {
  const modal = useWorkspace((s) => s.modal);
  const closeModal = useWorkspace((s) => s.closeModal);
  const deleteNodes = useWorkspace((s) => s.deleteNodes);
  const nodes = useWorkspace((s) => s.nodes);

  const open = modal?.kind === "confirm-delete";

  /** Shadow copy: the store clears `modal` while the panel is still animating out. */
  const [ids, setIds] = useState<NodeId[]>([]);
  useEffect(() => {
    if (modal?.kind === "confirm-delete" && modal.nodeIds !== ids) setIds(modal.nodeIds);
  }, [modal, ids]);

  const [pending, setPending] = useState(false);

  const items: FsNode[] = [];
  for (const id of ids) {
    const node = nodes[id];
    if (node) items.push(node);
  }

  const inside = items.reduce(
    (total, node) => total + (node.kind === "folder" ? node.childCount : 0),
    0,
  );

  const confirm = () => {
    if (!open || pending || ids.length === 0) return;
    setPending(true);
    void deleteNodes(ids).finally(() => setPending(false));
  };

  // Enter confirms — but only when the focus ring is on Delete itself, or on
  // nothing interactive at all. Anything else focusable in the panel (Cancel,
  // the dialog's own close X, any control a later revision adds) owns its own
  // Enter, and an irreversible delete must never be what a "close" key does.
  // Escape is the modal layer's: it closes the dialog already, so a second
  // listener here would only risk closing something underneath it too.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Enter") return;
      const active = document.activeElement as HTMLElement | null;
      const onConfirm = active?.closest("[data-confirm]") != null;
      const onControl = active?.closest("button, a, input, textarea, select") != null;
      if (!onConfirm && onControl) return;
      e.preventDefault();
      confirm();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  const first = items[0];
  const title =
    items.length === 1 && first ? `Delete “${first.name}”?` : `Delete ${items.length} items?`;

  return (
    <Modal
      open={open}
      onClose={closeModal}
      title={title}
      description="This removes it from every member's copy of the vault. There is no trash."
      size="sm"
    >
      <div data-testid="confirm-delete-modal">
        <div className="rounded-[10px] border border-line bg-bg px-[12px] py-[8px]">
          {items.slice(0, MAX_LISTED).map((node) => (
            <div
              key={node.id}
              data-node-id={node.id}
              className="flex h-[28px] items-center gap-[8px]"
            >
              <span className="grid h-[18px] w-[18px] shrink-0 place-items-center">
                {node.kind === "folder" ? (
                  <FolderIcon color={node.color ?? "graphite"} size={18} />
                ) : (
                  <FileIcon name={node.name} size={18} />
                )}
              </span>
              <span className="min-w-0 flex-1 truncate text-[13px] leading-[18px] text-fg-2">
                {node.name}
              </span>
            </div>
          ))}
          {items.length > MAX_LISTED ? (
            <div className="flex h-[28px] items-center pl-[26px] text-[12.5px] leading-none text-fg-3 tabular-nums">
              +{items.length - MAX_LISTED} more
            </div>
          ) : null}
        </div>

        {inside > 0 ? (
          <div className="mt-[12px] flex items-center gap-[6px] text-[12.5px] leading-[18px] text-coral">
            <AlertTriangle size={14} strokeWidth={1.75} className="shrink-0" aria-hidden />
            <span className="tabular-nums">
              Includes {inside} {inside === 1 ? "item" : "items"} inside
            </span>
          </div>
        ) : null}

        {/*
          Cancel reads above Delete but comes second in the DOM: the dialog hands
          its initial focus to the first control, and Enter is the confirm key,
          so the focused control and the key that fires have to be the same one.
        */}
        <div className="mt-[20px] flex flex-col-reverse gap-[8px]">
          <span data-confirm className="contents">
            <PrimaryButton onClick={confirm} disabled={pending || items.length === 0}>
              Delete
            </PrimaryButton>
          </span>
          <span data-cancel className="contents">
            <GhostButton
              variant="secondary"
              onClick={closeModal}
              className="h-[36px] w-full justify-center"
            >
              Cancel
            </GhostButton>
          </span>
        </div>
      </div>
    </Modal>
  );
}
