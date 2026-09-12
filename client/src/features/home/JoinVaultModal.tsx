import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState } from "react";

import { CodeInput } from "@/components/ui/CodeInput";
import { ConfirmState } from "@/components/ui/ConfirmState";
import { Modal } from "@/components/ui/Modal";
import { PrimaryButton } from "@/components/ui/PrimaryButton";
import { useBackend } from "@/lib/backend";

const CODE_LENGTH = 6;
const CONFIRM_MS = 1400;

/**
 * Keep only the Base32 alphabet the directory mints codes from.
 *
 * `CodeInput` accepts any alphanumeric, so the filter lives here: `0`, `1`, `8`
 * and `9` are not in RFC 4648 Base32, and a code read off another screen is
 * typed, so it arrives in whatever case the reader used.
 */
const sanitize = (raw: string) => raw.toUpperCase().replace(/[^A-Z2-7]/g, "");

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
    } catch (e) {
      // Whatever the daemon said: an expired code, a rotated one and an
      // unreachable directory are three different problems for the person
      // holding the invite, and only the daemon knows which one this was.
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Join a Vault"
      description={joined ? undefined : "Enter the 6-character code from the vault owner"}
    >
      <AnimatePresence mode="wait" initial={false}>
        {joined === null ? (
          <motion.div key="form" {...FADE}>
            <CodeInput
              length={CODE_LENGTH}
              value={code}
              onChange={(next) => {
                setCode(sanitize(next));
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
