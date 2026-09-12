import { motion, useReducedMotion } from "motion/react";
import { useState } from "react";

import { PrimaryButton } from "@/components/ui/PrimaryButton";
import { TextField } from "@/components/ui/TextField";
import { useBackend } from "@/lib/backend";
import { APP_NAME } from "@/lib/brand";

/** Gaps of the column, in px. The wordmark sits exactly where the home's does. */
const GAP_HEADING = 14;
const GAP_FIELD = 24;

/** The question is one line at every window width the app is used at. */
const HEADING = "Hey there, what is your name?";

/** The same ceiling the engine enforces, so the field can never post a rejection. */
const MAX_NAME = 40;

export interface OnboardingScreenProps {
  /** The name is stored; the shell may slide the home list in. */
  onDone(): void;
}

/**
 * The first thing a fresh install shows: the wordmark, one question, one field.
 *
 * It draws no background of its own — App keeps the color field and the grain
 * behind every screen — and it mirrors the home's column exactly (same side,
 * same width, same 34px lockup), so answering the question slides the list in
 * under a wordmark that never moves.
 *
 * The name goes straight through `BackendClient.setProfile`, which is also what
 * renames this member on every vault list. Nothing is written to `localStorage`:
 * the profile lives in the daemon (docs/decisions/client-workspace.md).
 */
export function OnboardingScreen({ onDone }: OnboardingScreenProps) {
  const { client } = useBackend();
  const reduced = useReducedMotion() ?? false;

  const [value, setValue] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const ready = value.trim().length > 0 && !saving;

  const submit = () => {
    if (!ready) return;
    setSaving(true);
    setError(null);
    void client
      .setProfile({ name: value.trim() })
      .then(() => {
        onDone();
      })
      .catch((e: unknown) => {
        setSaving(false);
        setError(e instanceof Error ? e.message : String(e));
      });
  };

  return (
    <section className="fixed inset-y-0 right-0 z-10 flex w-[60%] items-center justify-end pt-7 pr-[clamp(40px,9vw,128px)]">
      <motion.div
        className="w-[min(460px,calc(100%-48px))] text-left"
        initial={reduced ? false : { opacity: 0, y: 8 }}
        animate={{ opacity: 1, y: 0 }}
        transition={reduced ? { duration: 0 } : { duration: 0.42, ease: [0.2, 0.8, 0.2, 1] }}
      >
        <h1 className="font-heading text-[34px] leading-none font-medium tracking-[-0.02em] text-fg">
          {APP_NAME}
        </h1>

        <h2
          className="font-heading text-[22px] leading-[28px] font-medium tracking-[-0.01em] text-fg"
          style={{ marginTop: GAP_HEADING }}
        >
          {HEADING}
        </h2>

        <form
          style={{ marginTop: GAP_FIELD }}
          onSubmit={(e) => {
            e.preventDefault();
            submit();
          }}
        >
          <div className="flex items-start gap-[10px]">
            <div className="flex-1">
              <TextField
                value={value}
                onChange={(next) => {
                  setValue(next);
                  if (error !== null) setError(null);
                }}
                onEnter={submit}
                placeholder="Your name"
                autoFocus
                maxLength={MAX_NAME}
                aria-label="Your name"
                aria-invalid={error !== null}
              />
            </div>
            <div className="w-[104px] shrink-0">
              <PrimaryButton type="submit" disabled={!ready}>
                Next
              </PrimaryButton>
            </div>
          </div>

          {error !== null ? (
            <p className="mt-[8px] text-[13px] leading-[18px] text-coral">{error}</p>
          ) : null}
        </form>
      </motion.div>
    </section>
  );
}
