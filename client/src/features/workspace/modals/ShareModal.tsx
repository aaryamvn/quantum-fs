import { Copy, Link, RefreshCw } from "lucide-react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useEffect, useState } from "react";

import { Caption } from "@/components/ui/Caption";
import { Divider } from "@/components/ui/Divider";
import { GhostButton } from "@/components/ui/GhostButton";
import { Modal } from "@/components/ui/Modal";
import { Tooltip } from "@/components/ui/Tooltip";

import type { NodeId } from "@/lib/backend";

import { useNode, useWorkspace } from "../store";

/** The join code is always six characters; the boxes are drawn, not typed into. */
const CODE_LENGTH = 6;

/** House ease — matches --ease-out-expo. */
const EASE: [number, number, number, number] = [0.16, 1, 0.3, 1];

/**
 * Two ways in, side by side: a link to this one item, and the code that gets a
 * person into the vault at all.
 *
 * They are deliberately unequal. The link is inert text — anyone who is already
 * a member can open it, so copying it is safe and needs no confirmation. The
 * code is a credential: it is shown as boxes rather than as a string because it
 * is read aloud and typed by hand, and rotating it is an admin-only act whose
 * consequence ("the old code stops working") is stated before it is taken, not
 * after.
 */
export function ShareModal() {
  const modal = useWorkspace((s) => s.modal);
  const closeModal = useWorkspace((s) => s.closeModal);
  const client = useWorkspace((s) => s.client);
  const vaultId = useWorkspace((s) => s.vaultId);
  const vaultMeta = useWorkspace((s) => s.vaultMeta);
  const me = useWorkspace((s) => s.me);
  const toast = useWorkspace((s) => s.toast);
  const reduced = useReducedMotion() ?? false;

  const open = modal?.kind === "share";

  /** Shadow copy: the store clears `modal` while the panel is still animating out. */
  const [nodeId, setNodeId] = useState<NodeId | null>(null);
  useEffect(() => {
    if (modal?.kind === "share" && modal.nodeId !== nodeId) setNodeId(modal.nodeId);
  }, [modal, nodeId]);

  const node = useNode(nodeId);
  const [code, setCode] = useState<string>(vaultMeta?.joinCode ?? "");
  const [rotating, setRotating] = useState(false);

  // The store's meta is the source when it has one; otherwise this dialog is the
  // first thing in the session that needs the code, so it fetches it itself.
  useEffect(() => {
    if (!open) return;
    if (vaultMeta?.joinCode) {
      setCode(vaultMeta.joinCode);
      return;
    }
    if (!client || !vaultId) return;
    let live = true;
    void client
      .getVaultMeta(vaultId)
      .then((meta) => {
        if (live) setCode(meta.joinCode);
      })
      .catch(() => {
        if (live) setCode("");
      });
    return () => {
      live = false;
    };
  }, [open, client, vaultId, vaultMeta]);

  const link = `qfs://${vaultId ?? ""}/${nodeId ?? ""}`;
  const isAdmin = me?.role === "admin";

  const copy = (text: string, done: string) => {
    void navigator.clipboard
      .writeText(text)
      .then(() => toast(done, "success"))
      .catch(() => toast("Couldn't copy to the clipboard", "error"));
  };

  const rotate = () => {
    if (!client || !vaultId || rotating || !isAdmin) return;
    setRotating(true);
    void client
      .rotateJoinCode(vaultId)
      .then((next) => {
        setCode(next);
        setRotating(false);
        toast("Join code rotated", "success");
      })
      .catch((error: unknown) => {
        setRotating(false);
        toast(error instanceof Error ? error.message : "Couldn't rotate the code", "error");
      });
  };

  const rotateButton = (
    <GhostButton
      icon={
        <motion.span
          className="inline-flex"
          animate={rotating && !reduced ? { rotate: 360 } : { rotate: 0 }}
          transition={
            rotating && !reduced
              ? { duration: 0.9, ease: "linear", repeat: Infinity }
              : { duration: 0 }
          }
          aria-hidden
        >
          <RefreshCw size={14} strokeWidth={1.75} />
        </motion.span>
      }
      variant="secondary"
      disabled={!isAdmin || rotating}
      onClick={rotate}
    >
      Rotate
    </GhostButton>
  );

  return (
    <Modal
      open={open}
      onClose={closeModal}
      title="Share"
      description={node?.name}
      size="sm"
    >
      <div data-testid="share-modal" data-node-id={nodeId ?? undefined}>
        <Caption>Item link</Caption>
        <div className="mt-[8px] flex items-center gap-[8px]">
          <span
            data-selectable
            className="flex h-[34px] min-w-0 flex-1 items-center gap-[8px] truncate rounded-[8px]
              border border-line bg-field px-[10px] text-[12.5px] leading-none text-fg-2"
            style={{ fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace" }}
          >
            <Link size={14} strokeWidth={1.75} className="shrink-0 text-fg-3" aria-hidden />
            <span className="truncate">{link}</span>
          </span>
          <GhostButton
            icon={<Copy size={14} strokeWidth={1.75} />}
            variant="secondary"
            onClick={() => copy(link, "Link copied")}
          >
            Copy link
          </GhostButton>
        </div>
        <p className="mt-[8px] text-[11px] leading-[16px] text-fg-3">
          Anyone in the vault can open this link.
        </p>

        <Divider className="my-[20px]" />

        <Caption>Vault join code</Caption>
        <div className="mt-[8px] flex w-full gap-[8px]">
          {Array.from({ length: CODE_LENGTH }, (_, i) => (
            <span
              key={i}
              className="grid h-[36px] flex-1 place-items-center rounded-[8px] border border-line-strong
                bg-field text-[18px] leading-none font-medium text-fg uppercase tabular-nums"
            >
              {/* Keyed on the code so a rotation crossfades rather than swapping characters. */}
              <AnimatePresence mode="wait" initial={false}>
                <motion.span
                  key={code}
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  transition={{ duration: reduced ? 0 : 0.18, ease: EASE }}
                >
                  {code[i] ?? ""}
                </motion.span>
              </AnimatePresence>
            </span>
          ))}
        </div>
        <div className="mt-[10px] flex items-center gap-[8px]">
          <GhostButton
            icon={<Copy size={14} strokeWidth={1.75} />}
            variant="secondary"
            disabled={code.length === 0}
            onClick={() => copy(code, "Join code copied")}
          >
            Copy code
          </GhostButton>
          {isAdmin ? (
            rotateButton
          ) : (
            <Tooltip label="Only admins can rotate the code">
              <span className="inline-flex">{rotateButton}</span>
            </Tooltip>
          )}
        </div>
        <p className="mt-[8px] text-[11px] leading-[16px] text-fg-3">
          Rotating invalidates the old code for new joins.
        </p>
      </div>
    </Modal>
  );
}
