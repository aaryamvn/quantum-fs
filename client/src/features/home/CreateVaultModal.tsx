import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState } from "react";

import {
  AllocationSlider,
  clampQuota,
  MIN_QUOTA_BYTES,
  QUOTA_STEP_BYTES,
} from "@/components/ui/AllocationSlider";
import { ConfirmState } from "@/components/ui/ConfirmState";
import { Modal } from "@/components/ui/Modal";
import { PrimaryButton } from "@/components/ui/PrimaryButton";
import { TextField } from "@/components/ui/TextField";
import { useBackend } from "@/lib/backend";
import type { OrchestrationServer } from "@/lib/backend";
import { formatBytes } from "@/lib/format";

const CONFIRM_MS = 1400;
const MAX_NAME = 40;
/** A tenth of the server is a sane first offer: big enough to use, small enough to grow from. */
const DEFAULT_SHARE = 0.1;

const FADE = {
  initial: { opacity: 0 },
  animate: { opacity: 1, transition: { duration: 0.2 } },
  exit: { opacity: 0, transition: { duration: 0.2 } },
};

const consumedBy = (server: OrchestrationServer) =>
  server.vaults.reduce((sum, v) => sum + v.quotaBytes, 0);

export interface CreateVaultModalProps {
  /** The server the plus was pressed on; `null` closes the dialog. */
  server: OrchestrationServer | null;
  onClose(): void;
}

/**
 * New vault on a server: a name and a slice of that server's storage.
 *
 * The slice is the point of the dialog, so it is drawn rather than typed — the
 * bar shows what the other vaults already hold before it shows what this one
 * would take.
 */
export function CreateVaultModal({ server, onClose }: CreateVaultModalProps) {
  const { createVault } = useBackend();

  const [name, setName] = useState("");
  const [quota, setQuota] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [created, setCreated] = useState<{ name: string; quota: number } | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Also the server the panel keeps drawing while it animates out: `server` goes
  // null the instant the dialog is dismissed, and the panel is on screen for
  // another 180ms after that.
  const [opened, setOpened] = useState<OrchestrationServer | null>(null);

  // Reset during render, not in an effect: an effect runs after the browser has
  // painted, so the previous confirmation would flash through the entrance of
  // the next open.
  if (server !== null && server !== opened) {
    const room = Math.max(0, server.capacityBytes - consumedBy(server));
    setOpened(server);
    setName("");
    setQuota(
      clampQuota(server.capacityBytes * DEFAULT_SHARE, room, MIN_QUOTA_BYTES, QUOTA_STEP_BYTES),
    );
    setError(null);
    setBusy(false);
    setCreated(null);
  }

  const shown = server ?? opened;
  const capacity = shown?.capacityBytes ?? 0;
  const consumed = shown ? consumedBy(shown) : 0;
  const free = Math.max(0, capacity - consumed);

  // Never leave a dismiss timer behind: a fast close would close the next
  // dialog out from under the user.
  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );

  const fits = quota >= MIN_QUOTA_BYTES && quota <= free;
  const ready = name.trim().length > 0 && fits;

  const submit = async () => {
    if (!ready || busy || created || !shown) return;
    setBusy(true);
    setError(null);
    const value = name.trim();
    try {
      await createVault({ serverId: shown.id, name: value, quotaBytes: quota });
      setCreated({ name: value, quota });
      timer.current = setTimeout(onClose, CONFIRM_MS);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Could not create that vault");
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={server !== null}
      onClose={onClose}
      title="New Vault"
      description={created ? undefined : `Create a vault on ${shown?.name ?? ""}`}
    >
      <AnimatePresence mode="wait" initial={false}>
        {created === null ? (
          <motion.div key="form" {...FADE}>
            <TextField
              value={name}
              onChange={(next) => {
                setName(next);
                if (error) setError(null);
              }}
              onEnter={() => void submit()}
              placeholder="Vault name"
              maxLength={MAX_NAME}
              autoFocus
              aria-label="Vault name"
              aria-invalid={error !== null}
            />

            <div className="mt-[16px]">
              <AllocationSlider
                capacityBytes={capacity}
                consumedBytes={consumed}
                vaultCount={shown?.vaults.length ?? 0}
                value={quota}
                onChange={(next) => {
                  setQuota(next);
                  if (error) setError(null);
                }}
              />
            </div>

            <div className="mt-[20px]">
              <PrimaryButton onClick={() => void submit()} disabled={!ready || busy}>
                Create Vault
              </PrimaryButton>
            </div>
            {error ? (
              <p className="mt-[8px] text-[13px] leading-[18px] text-coral">{error}</p>
            ) : null}
          </motion.div>
        ) : (
          <motion.div key="done" {...FADE}>
            <ConfirmState
              title="Vault created"
              detail={`${created.name} · ${formatBytes(created.quota)}`}
            />
          </motion.div>
        )}
      </AnimatePresence>
    </Modal>
  );
}
