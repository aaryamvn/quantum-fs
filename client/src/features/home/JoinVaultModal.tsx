import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState } from "react";

import { CodeInput } from "@/components/ui/CodeInput";
import { ConfirmState } from "@/components/ui/ConfirmState";
import { Modal } from "@/components/ui/Modal";
import { PrimaryButton } from "@/components/ui/PrimaryButton";
import { useBackend } from "@/lib/backend";

const CODE_LENGTH = 6;
const CONFIRM_MS = 1400;

const FADE = {
  initial: { opacity: 0 },
  animate: { opacity: 1, transition: { duration: 0.2 } },
  exit: { opacity: 0, transition: { duration: 0.2 } },
};

export function JoinVaultModal({ open, onClose }: { open: boolean; onClose(): void }) {
  const { joinVault } = useBackend();
  const [code, setCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [joined, setJoined] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );

  // Reset during render, not in an effect: an effect runs after the browser has
  // painted, so the previous confirmation would flash through the entrance of
  // the next open.
  const [wasOpen, setWasOpen] = useState(open);
  if (open !== wasOpen) {
    setWasOpen(open);
    if (open) {
      setCode("");
      setError(null);
      setBusy(false);
      setJoined(null);
    }
  }

  const complete = code.length === CODE_LENGTH;

  const submit = async () => {
    if (!complete || busy || joined) return;
    setBusy(true);
    setError(null);
    try {
      const vault = await joinVault(code);
      setJoined(vault.name);
      timer.current = setTimeout(onClose, CONFIRM_MS);
    } catch {
      // The directory either resolves the code or it doesn't; the reason it
      // gives back is for logs, not for the person holding the invite.
      setError("That code didn't work");
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Join a Vault"
      description={joined ? undefined : "Enter the join code"}
    >
      <AnimatePresence mode="wait" initial={false}>
        {joined === null ? (
          <motion.div key="form" {...FADE}>
            <CodeInput
              length={CODE_LENGTH}
              value={code}
              onChange={(next) => {
                setCode(next);
                if (error) setError(null);
              }}
              onEnter={() => void submit()}
            />
            {error ? (
              <p className="mt-[10px] text-[13px] leading-[18px] text-coral">{error}</p>
            ) : null}
            <div className="mt-[16px]">
              <PrimaryButton onClick={() => void submit()} disabled={!complete || busy}>
                Join
              </PrimaryButton>
            </div>
          </motion.div>
        ) : (
          <motion.div key="done" {...FADE}>
            <ConfirmState title="Joined vault" detail={joined} />
          </motion.div>
        )}
      </AnimatePresence>
    </Modal>
  );
}
