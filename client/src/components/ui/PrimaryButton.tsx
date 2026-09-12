import { motion, useReducedMotion } from "motion/react";
import type { ReactNode } from "react";

export interface PrimaryButtonProps {
  children: ReactNode;
  onClick?(): void;
  type?: "button" | "submit";
  disabled?: boolean;
}

/**
 * The single loud control in the app: white on near-black, full width at the
 * foot of a dialog. Nothing else in the UI is filled, so it never competes.
 *
 * Unavailable, it is not a dimmed white slab — a faded fill still reads as the
 * button you are meant to press. It steps back to a ghost instead, and only
 * fills once the form is actually complete.
 */
export function PrimaryButton({
  children,
  onClick,
  type = "button",
  disabled = false,
}: PrimaryButtonProps) {
  const reduced = useReducedMotion() ?? false;

  return (
    <motion.button
      type={type}
      onClick={onClick}
      disabled={disabled}
      whileTap={reduced || disabled ? undefined : { scale: 0.99 }}
      className={`h-[44px] w-full rounded-[10px] border text-[15px] leading-none font-medium tracking-[-0.005em]
        transition-[background-color,border-color,color] duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)]
        ${disabled ? "cursor-default" : "hover:bg-[#E9EBF3]"}`}
      style={
        disabled
          ? {
              background: "rgba(255,255,255,0.08)",
              borderColor: "var(--color-line)",
              color: "var(--color-fg-3)",
            }
          : {
              background: "#FFFFFF",
              borderColor: "transparent",
              color: "var(--color-on-white, #010513)",
            }
      }
    >
      {children}
    </motion.button>
  );
}
