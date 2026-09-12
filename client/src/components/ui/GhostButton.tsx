import { motion, useReducedMotion } from "motion/react";
import type { MouseEvent, ReactNode } from "react";

export interface GhostButtonProps {
  icon?: ReactNode;
  children: ReactNode;
  onClick?(e: MouseEvent): void;
  disabled?: boolean;
  variant?: "ghost" | "secondary" | "outline" | "danger";
  size?: "sm" | "md";
  active?: boolean;
  className?: string;
  title?: string;
  "aria-label"?: string;
}

const EASE = "ease-[cubic-bezier(0.2,0.8,0.2,1)]";

/**
 * The action-bar button, lifted verbatim from "Setup New Server" on the home
 * screen so the workspace speaks the same language: a label in fg-3 that warms
 * to fg on hover, with the surface only appearing under the pointer. Nothing
 * here is filled — PrimaryButton is the single loud control in the app, and a
 * second filled button anywhere would immediately compete with it.
 *
 * `secondary` is the dialog's other answer — Cancel, Download, Copy link. It
 * needs an edge and a floor so it reads as a control the moment the dialog
 * opens, without ever competing with the filled primary beside it, so it takes
 * the faintest surface that still separates from the panel.
 *
 * `outline` exists for the rare action that must be findable before it is
 * hovered, and `danger` tints rather than fills for the same reason.
 */
export function GhostButton({
  icon,
  children,
  onClick,
  disabled = false,
  variant = "ghost",
  size = "sm",
  active = false,
  className,
  title,
  "aria-label": ariaLabel,
}: GhostButtonProps) {
  const reduced = useReducedMotion() ?? false;

  const padding = size === "md" ? "px-[10px] py-[7px]" : "px-[8px] py-[6px]";
  const label = size === "md" ? "text-[13.5px]" : "text-[13px]";

  const tone =
    variant === "danger"
      ? "text-coral"
      : variant === "secondary"
        ? "text-fg-2"
        : active
          ? "bg-white/[0.07] text-fg"
          : "text-fg-3";

  // The hover wash is variant-exclusive: the arbitrary-value utility
  // `hover:bg-white/[0.05]` is emitted after the themed `hover:bg-surface-hover`
  // in the generated stylesheet, so letting both land on an outline button would
  // silently replace its opaque surface with a 5% white film over the page.
  // `secondary` has the same problem in reverse: its hover must be brighter
  // than its own floor, not the ghost's.
  const hover = disabled
    ? ""
    : variant === "danger"
      ? "hover:bg-coral/10 hover:text-coral focus-visible:text-coral"
      : variant === "outline"
        ? "hover:bg-surface-hover hover:text-fg focus-visible:text-fg"
        : variant === "secondary"
          ? "hover:bg-white/[0.09] hover:text-fg focus-visible:text-fg"
          : "hover:bg-white/[0.05] hover:text-fg focus-visible:text-fg";

  const surface =
    variant === "outline"
      ? "border border-line bg-surface"
      : variant === "secondary"
        ? "border border-line-strong bg-white/[0.06]"
        : "";

  return (
    <motion.button
      type="button"
      title={title}
      aria-label={ariaLabel}
      aria-pressed={active || undefined}
      disabled={disabled}
      onClick={onClick}
      whileTap={reduced || disabled ? undefined : { scale: 0.98 }}
      className={[
        "flex items-center gap-[6px] rounded-[8px] transition-colors duration-[160ms]",
        EASE,
        padding,
        tone,
        surface,
        hover,
        disabled ? "opacity-40" : "",
        className ?? "",
      ]
        .filter(Boolean)
        .join(" ")}
    >
      {icon ? (
        <span
          className="grid shrink-0 place-items-center"
          style={{ width: 14, height: 14, lineHeight: 0 }}
          aria-hidden
        >
          {icon}
        </span>
      ) : null}
      <span className={`${label} leading-none font-medium`}>{children}</span>
    </motion.button>
  );
}
