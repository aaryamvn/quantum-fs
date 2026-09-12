import { motion, useReducedMotion } from "motion/react";

/** Track 30 wide, knob 14 with 2px of inset on each side: 30 − 14 − 2 − 2 = 12. */
const TRAVEL = 12;

export interface ToggleProps {
  checked: boolean;
  onChange(next: boolean): void;
  disabled?: boolean;
  /** Names the switch for screen readers — it carries no visible text of its own. */
  label?: string;
}

/**
 * The app's only binary control. It is deliberately tiny: settings rows carry
 * their own label, so the switch is a state indicator first and a target second.
 *
 * The knob springs rather than tweens — a snap that overshoots by a hair is the
 * one cue that the value actually committed, since the track color change alone
 * is easy to miss at this size. Off is a neutral white wash, not a hue, so only
 * "on" spends the violet.
 */
export function Toggle({ checked, onChange, disabled = false, label }: ToggleProps) {
  const reduced = useReducedMotion() ?? false;

  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-[18px] w-[30px] shrink-0 items-center rounded-full p-[2px]
        transition-colors duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)]
        ${checked ? "bg-violet" : "bg-white/[0.12]"}
        ${disabled ? "opacity-40" : ""}`}
    >
      <motion.span
        aria-hidden
        className="block h-[14px] w-[14px] rounded-full bg-white"
        animate={{ x: checked ? TRAVEL : 0 }}
        transition={
          reduced
            ? { duration: 0.12, ease: [0.16, 1, 0.3, 1] }
            : { type: "spring", stiffness: 500, damping: 32 }
        }
      />
    </button>
  );
}
