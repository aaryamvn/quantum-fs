import type { MouseEvent, ReactNode } from "react";

import { Tooltip } from "@/components/ui/Tooltip";

export interface IconButtonProps {
  icon: ReactNode;
  /** Doubles as the aria-label and the tooltip text — an icon alone names nothing. */
  label: string;
  onClick?(e: MouseEvent): void;
  active?: boolean;
  disabled?: boolean;
  size?: 24 | 28 | 32;
  tone?: "default" | "danger";
  className?: string;
  tooltip?: boolean;
  shortcut?: string;
}

/**
 * The toolbar's workhorse: a square of hit area around a 16px glyph. It carries
 * no background at rest so a row of them reads as a row of icons rather than a
 * row of chips, and only lights up under the pointer or while its state is on.
 *
 * The label is required rather than optional because this is the one control in
 * the app with no visible text — without it the button is mute to a screen
 * reader and to anyone who does not recognize the glyph.
 */
export function IconButton({
  icon,
  label,
  onClick,
  active = false,
  disabled = false,
  size = 28,
  tone = "default",
  className,
  tooltip = true,
  shortcut,
}: IconButtonProps) {
  const toneClasses =
    tone === "danger"
      ? "text-coral hover:text-coral hover:bg-coral/10 active:bg-coral/15"
      : active
        ? "bg-white/[0.08] text-fg hover:bg-white/[0.09]"
        : "text-fg-3 hover:bg-white/[0.06] hover:text-fg active:bg-white/[0.09]";

  const button = (
    <button
      type="button"
      aria-label={label}
      aria-pressed={active || undefined}
      disabled={disabled}
      onClick={onClick}
      style={{ width: size, height: size }}
      className={[
        "grid shrink-0 place-items-center rounded-[7px] transition-colors duration-[160ms] ease-standard",
        toneClasses,
        disabled ? "pointer-events-none opacity-40" : "",
        className ?? "",
      ]
        .filter(Boolean)
        .join(" ")}
    >
      {icon}
    </button>
  );

  if (!tooltip) return button;

  return (
    <Tooltip label={label} shortcut={shortcut}>
      {button}
    </Tooltip>
  );
}
