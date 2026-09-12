import { Copy, KeyRound, RefreshCw } from "lucide-react";
import { motion, useReducedMotion } from "motion/react";
import { useState } from "react";

import { Caption } from "@/components/ui/Caption";
import { Divider } from "@/components/ui/Divider";
import { GhostButton } from "@/components/ui/GhostButton";
import { Tooltip } from "@/components/ui/Tooltip";
import { useWorkspace } from "@/features/workspace/store";
import { formatRelative } from "@/lib/time";

import { useSettingsVault } from "./SettingsVaultContext";

/** The code is fixed-length; the boxes are drawn from this, never from its length. */
const CODE_LENGTH = 6;

/**
 * How someone else gets in, and how the vault's keys are kept fresh.
 *
 * The code is shown in boxes rather than as a string for the same reason the
 * join screen types it that way: it is read aloud or pasted into a chat, and the
 * boxes make its length and its character grouping obvious before anyone starts.
 *
 * Rotation is two steps because it is irreversible for everyone else — anyone
 * mid-join with the old code is locked out the moment it lands, so the sentence
 * says exactly that before the second click.
 */
export function SharingTab() {
  const reduced = useReducedMotion() ?? false;
  const client = useWorkspace((s) => s.client);
  const toast = useWorkspace((s) => s.toast);
  const { vaultId, meta, isAdmin, refresh } = useSettingsVault();

  const [arming, setArming] = useState(false);
  const [rotating, setRotating] = useState(false);

  if (!meta) return <p className="text-[13px] text-fg-3">Loading vault settings…</p>;

  const code = meta.joinCode;

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      toast("Join code copied", "success");
    } catch {
      toast("Couldn't copy the join code", "error");
    }
  };

  const rotate = async () => {
    if (!client || !vaultId || rotating) return;
    setRotating(true);
    try {
      await client.rotateJoinCode(vaultId);
      await refresh();
      toast("Join code rotated", "success");
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), "error");
    } finally {
      setRotating(false);
      setArming(false);
    }
  };

  return (
    <div data-testid="vault-settings-sharing" className="flex flex-col gap-[20px]">
      <div>
        <Caption className="mb-[10px]">Join code</Caption>

        <div className="flex w-full gap-[8px]">
          {Array.from({ length: CODE_LENGTH }, (_, i) => (
            <span
              key={i}
              data-selectable
              className="grid h-[52px] min-w-0 flex-1 basis-0 place-items-center rounded-[10px]
                border border-line-strong bg-bg text-[20px] leading-none font-medium text-fg uppercase"
            >
              {code[i] ?? ""}
            </span>
          ))}
        </div>

        <div className="mt-[12px] flex items-center gap-[6px]">
          <GhostButton
            variant="secondary"
            icon={<Copy size={14} strokeWidth={1.75} aria-hidden />}
            onClick={() => void copy()}
          >
            Copy code
          </GhostButton>

          {arming ? (
            <span className="flex min-w-0 items-center gap-[6px]">
              <span className="truncate text-[12.5px] text-fg-2">
                Rotate? Old code stops working —
              </span>
              <GhostButton variant="danger" disabled={rotating} onClick={() => void rotate()}>
                Rotate
              </GhostButton>
              <GhostButton variant="secondary" onClick={() => setArming(false)}>
                Cancel
              </GhostButton>
            </span>
          ) : (
            <GhostButton
              variant="secondary"
              disabled={!isAdmin || rotating}
              icon={
                <motion.span
                  className="grid place-items-center"
                  style={{ width: 14, height: 14, lineHeight: 0 }}
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
              onClick={() => setArming(true)}
            >
              Rotate code
            </GhostButton>
          )}
        </div>

        <p className="mt-[10px] text-[11px] leading-[16px] text-fg-3">
          Share this code out of band. The directory maps it to this server and vault
          (docs/decisions/net-vault-join-directory.md).
        </p>
        {isAdmin ? null : (
          <p className="mt-[4px] text-[11px] leading-[16px] text-fg-3">
            Only admins can rotate the join code.
          </p>
        )}
      </div>

      <Divider />

      <div className="flex items-center gap-[10px]">
        <KeyRound size={16} strokeWidth={1.75} className="shrink-0 text-fg-3" aria-hidden />
        <span className="flex min-w-0 flex-1 flex-col gap-[2px]">
          <span className="text-[13px] leading-none text-fg">Encryption keys</span>
          <span className="truncate text-[11.5px] leading-none text-fg-3">
            Rotated weekly · last {formatRelative(meta.keyRotatedAt)}
          </span>
        </span>
        {/*
          The tooltip anchors the wrapper, not the button: a disabled button
          fires no pointer events of its own, so the hint would never appear.
        */}
        <Tooltip label="Manual rotation lands with the daemon">
          <span className="inline-flex shrink-0">
            <GhostButton variant="secondary" disabled>
              Rotate now
            </GhostButton>
          </span>
        </Tooltip>
      </div>
    </div>
  );
}
