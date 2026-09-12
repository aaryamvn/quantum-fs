import type { CSSProperties, ReactNode } from "react";

export type ChipTone = "neutral" | "violet" | "coral" | "success" | "warning";

/** Green and amber exist nowhere else in the palette; they live here, named once. */
const SUCCESS = "#3DDC84";
const WARNING = "#FFC27B";

/**
 * Tones are written as inline style rather than classes because two of them mix
 * a hex that is not a token, and a half-token/half-style split would be worse to
 * read than one table.
 */
const TONES: Record<ChipTone, { className: string; style?: CSSProperties }> = {
  neutral: { className: "border-line-strong bg-white/[0.04] text-fg-2" },
  violet: { className: "border-violet/40 bg-violet/20 text-fg" },
  coral: { className: "border-coral/40 bg-coral/10 text-coral" },
  success: {
    className: "text-fg",
    style: { borderColor: `${SUCCESS}66`, background: `${SUCCESS}1F` },
  },
  warning: {
    className: "text-fg",
    style: { borderColor: `${WARNING}66`, background: `${WARNING}1F` },
  },
};

export interface ChipProps {
  children: ReactNode;
  tone?: ChipTone;
  size?: "xs" | "sm";
  icon?: ReactNode;
  className?: string;
}

/**
 * A status label, not a control: encryption state, peer count, "read only".
 * It is deliberately the smallest type in the app and never taller than a line
 * of body text, so a row can carry two of them without the row growing.
 *
 * Nothing here is clickable — a chip that does something is a button, and this
 * is the one shape in the UI that is only ever telling you something.
 */
export function Chip({
  children,
  tone = "neutral",
  size = "sm",
  icon,
  className = "",
}: ChipProps) {
  const { className: toneClass, style } = TONES[tone];
  const box = size === "xs" ? "h-[16px] px-[6px]" : "h-[18px] px-[7px]";

  return (
    <span
      className={`inline-flex shrink-0 items-center gap-[4px] rounded-full border ${box}
        text-[11px] leading-none whitespace-nowrap ${toneClass} ${className}`}
      style={style}
    >
      {icon ? <span className="inline-flex items-center">{icon}</span> : null}
      {children}
    </span>
  );
}
