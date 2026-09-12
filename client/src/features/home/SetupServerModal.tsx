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
/** A bracketed IPv6 literal, as a URL writes one: `[fe80::1]`. */
const IPV6 = /^\[[0-9a-f:.]{2,45}\]$/i;
const PORT = /^\d{1,5}$/;
/** The admin token the host prints: base32, no padding. */
const TOKEN = /^[A-Z2-7]{16,32}$/;

/**
 * The connect string, tidied into the one form the daemon accepts.
 *
 * The token the host prints is base32 and upper-case, but it arrives by way of a
 * chat message or a terminal a person retyped, so its case is not something to
 * hold a valid address hostage over. The address half is left exactly as typed:
 * a hostname is case-insensitive to DNS but not to the operator who wrote it,
 * and rewriting it would put a string on screen the user never entered.
 */
export function normalizeAddress(raw: string): string {
  const value = raw.trim();
  // The last slash, not the first: only the trailing segment is the token, and a
  // paste with a stray path in it stays invalid rather than being half-fixed.
  const slash = value.lastIndexOf("/");
  if (slash === -1) return value;
  return `${value.slice(0, slash)}/${value.slice(slash + 1).toUpperCase()}`;
}

/** The address half, `IP:PORT` — the part of a connect string before the slash. */
function isValidAuthority(authority: string): boolean {
  if (!authority) return false;
  // An IPv6 literal is bracketed precisely so its own colons cannot be read as
  // the port separator, so the split happens after the closing bracket.
  const close = authority.lastIndexOf("]");
  const colon = authority.indexOf(":", close === -1 ? 0 : close);
  const host = colon === -1 ? authority : authority.slice(0, colon);
  const port = colon === -1 ? null : authority.slice(colon + 1);
  if (port !== null) {
    if (!PORT.test(port)) return false;
    const n = Number(port);
    if (n < 1 || n > 65535) return false;
  }
  return IPV4.test(host) || IPV6.test(host) || HOSTNAME.test(host);
}

/**
 * The connect string a vault server prints, `IP:PORT/TOKEN`
 * (docs/decisions/client-backend-embed.md), in full.
 *
 * The token is what authenticates the app to the host's admin port, so it is
 * part of the address as far as this field is concerned: one paste, one control,
 * no second box for a secret that arrived on the same line.
 *
 * It is required, not optional. An address alone does add a server, and the
 * daemon even reports it online — but every vault it is asked to create fails,
 * because creating one is an admin-port call. A server that appears and then
 * refuses the only thing you can do with it is worse than a field that says
 * up front that half the string is missing.
 */
export function isValidAddress(raw: string): boolean {
  const value = raw.trim();
  if (!value) return false;

  const slash = value.indexOf("/");
  if (slash === -1) return false;
  if (!TOKEN.test(value.slice(slash + 1))) return false;
  return isValidAuthority(value.slice(0, slash));
}

/**
 * A well-formed address with the key left off — the one near-miss worth a
 * sentence, because it is what a person types when they read the connect string
 * as "the server's address" and stopped at the slash.
 */
export function isMissingToken(raw: string): boolean {
  const value = raw.trim();
  if (!value || value.includes("/")) return false;
  return isValidAuthority(value);
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

  // Validated and submitted in the same normalized form the button is gated on,
  // so a lower-case token can never be rejected by a field that looks enabled.
  const value = normalizeAddress(address);
  const valid = isValidAddress(value);
  const missingToken = !valid && isMissingToken(value);

  const submit = async () => {
    if (!valid || busy || added) return;
    setBusy(true);
    setError(null);
    try {
      await addServer({ name: `Server ${servers.length + 1}`, address: value });
      // The confirmation names the host, not the key: the token was a secret one
      // second ago and leaving it on screen is the one thing this panel can do wrong.
      const slash = value.indexOf("/");
      setAdded(slash === -1 ? value : value.slice(0, slash));
      timer.current = setTimeout(onClose, CONFIRM_MS);
    } catch (e) {
      // The daemon's own sentence, verbatim: "bad token", "connection refused"
      // and "not a vault server" are three different things to go and fix, and
      // one house phrase would hide which of them happened.
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Setup New Server"
      description={
        added ? undefined : "Paste the connect string printed by the vault server (ip:port/KEY)"
      }
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
              placeholder="172.26.28.115:8447/K7QF2MXJ4ZB6TDLA"
              inputMode="url"
              autoFocus
              aria-label="Server connect string"
              aria-invalid={error !== null}
            />
            {/* The missing key is a hint, not an error: nothing has failed yet,
                and colouring an unfinished paste red would blame the user for
                still typing. The daemon's own failures keep the coral. */}
            {error ? (
              <p className="mt-[8px] text-[13px] leading-[18px] text-coral">{error}</p>
            ) : missingToken ? (
              <p className="mt-[8px] text-[13px] leading-[18px] text-fg-3">
                Include the key after the slash from the server&rsquo;s connect string
              </p>
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
