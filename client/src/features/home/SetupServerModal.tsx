import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState } from "react";

import { ConfirmState } from "@/components/ui/ConfirmState";
import { Modal } from "@/components/ui/Modal";
import { PrimaryButton } from "@/components/ui/PrimaryButton";
import { TextField } from "@/components/ui/TextField";
import { useBackend } from "@/lib/backend";

/** How long the confirmation holds before the dialog dismisses itself. */
const CONFIRM_MS = 1400;

const IPV4 = /^(?:(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\.){3}(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)$/;
const HOSTNAME = /^(?=.{1,253}$)[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)*$/i;
const PORT = /^\d{1,5}$/;

/** An IPv4 address or a hostname, either one optionally carrying `:port`. */
export function isValidAddress(raw: string): boolean {
  const value = raw.trim();
  if (!value) return false;
  const colon = value.lastIndexOf(":");
  const host = colon === -1 ? value : value.slice(0, colon);
  const port = colon === -1 ? null : value.slice(colon + 1);
  if (port !== null) {
    if (!PORT.test(port)) return false;
    const n = Number(port);
    if (n < 1 || n > 65535) return false;
  }
  return IPV4.test(host) || HOSTNAME.test(host);
}

/** Fades the form out and the confirmation in without the panel resizing abruptly. */
const FADE = {
  initial: { opacity: 0 },
  animate: { opacity: 1, transition: { duration: 0.2 } },
  exit: { opacity: 0, transition: { duration: 0.2 } },
};

export function SetupServerModal({ open, onClose }: { open: boolean; onClose(): void }) {
  const { servers, addServer } = useBackend();
  const [address, setAddress] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [added, setAdded] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Never leave a dismiss timer behind: a fast close would reopen-and-close the
  // next dialog out from under the user.
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
      setAddress("");
      setError(null);
      setBusy(false);
      setAdded(null);
    }
  }

  const valid = isValidAddress(address);

  const submit = async () => {
    if (!valid || busy || added) return;
    setBusy(true);
    setError(null);
    const value = address.trim();
    try {
      await addServer({ name: `Server ${servers.length + 1}`, address: value });
      setAdded(value);
      timer.current = setTimeout(onClose, CONFIRM_MS);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Could not reach that address");
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Setup New Server"
      description={added ? undefined : "Enter the server's IP address"}
    >
      <AnimatePresence mode="wait" initial={false}>
        {added === null ? (
          <motion.div key="form" {...FADE}>
            <TextField
              value={address}
              onChange={(next) => {
                setAddress(next);
                if (error) setError(null);
              }}
              onEnter={() => void submit()}
              placeholder="192.168.1.24:7447"
              inputMode="url"
              autoFocus
              aria-label="Server IP address"
              aria-invalid={error !== null}
            />
            {error ? (
              <p className="mt-[8px] text-[13px] leading-[18px] text-coral">{error}</p>
            ) : null}
            <div className="mt-[16px]">
              <PrimaryButton onClick={() => void submit()} disabled={!valid || busy}>
                Add Server
              </PrimaryButton>
            </div>
          </motion.div>
        ) : (
          <motion.div key="done" {...FADE}>
            <ConfirmState title="Server added" detail={added} />
          </motion.div>
        )}
      </AnimatePresence>
    </Modal>
  );
}
