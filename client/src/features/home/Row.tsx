import { motion, useReducedMotion } from "motion/react";
import type { Variants } from "motion/react";
import type { CSSProperties, ReactNode } from "react";

import { ICON_BOX, Plus } from "./icons";

export type RowKind = "server" | "vault" | "add-server" | "join-vault";

/** Row geometry, in px. Kept here so the vault thread can line up to the pixel. */
export const PAD_X = 16;
/**
 * Vault rows step right to clear the thread: 32 = THREAD_X (25) + 7px air.
 * 36 left a dead dark band between the group edge and the first vault glyph.
 */
export const VAULT_PAD_L = 32;
/** Thread x, on the centre of the server row's icon column (16 + 18/2). */
export const THREAD_X = 25;
export const SERVER_H = 50;
export const VAULT_H = 46;
export const STANDALONE_H = 50;
/** Inner radius of a 14px group with a 1px border. */
export const INNER_R = 13;

const TONE: Record<RowKind, string> = {
  server: "font-medium text-fg",
  vault: "font-normal text-fg-2 group-hover/line:text-fg",
  "add-server": "font-medium text-fg",
  "join-vault": "font-medium text-fg",
};

const EASE = "ease-[cubic-bezier(0.2,0.8,0.2,1)]";

export interface RowProps {
  kind: RowKind;
  label: string;
  glyph: ReactNode;
  height: number;
  /** Left padding of the label column; vault rows step right. */
  padLeft?: number;
  /** Reserve room for the trailing plus button. */
  padRight?: number;
  /** Corner rounding, applied to the painted surface only. */
  radius?: string;
  /** Row surface at rest; server rows sit a shade brighter than their vaults. */
  surface?: string;
  /** Row surface on hover; kept one perceptual step above `surface`. */
  surfaceHover?: string;
  /** Faint top-edge light, used to read the server row as the group's capstone. */
  topLight?: boolean;
  /** Hairline above this row, drawn on the wrapper so it spans edge to edge. */
  divider?: "none" | "line" | "strong";
  /** Trailing "add vault" affordance; a sibling of the row button, never a child. */
  plusLabel?: string;
  /** Entrance variant; the row is a variant child of the screen container. */
  variants?: Variants;
  className?: string;
  style?: CSSProperties;
}

const DIVIDER: Record<"none" | "line" | "strong", string> = {
  none: "",
  line: "border-t border-line",
  strong: "border-t border-line-strong",
};

/**
 * One line of the list: the row button plus, optionally, a sibling plus button.
 * The wrapper owns the hover group so that pointing at either half lights the
 * whole line.
 */
export function Row({
  kind,
  label,
  glyph,
  height,
  padLeft = PAD_X,
  padRight = PAD_X,
  radius,
  surface = "var(--color-surface)",
  surfaceHover = "var(--color-surface-hover)",
  topLight = false,
  divider = "none",
  plusLabel,
  variants,
  className = "",
  style,
}: RowProps) {
  const reduced = useReducedMotion() ?? false;

  return (
    <motion.div
      variants={variants}
      className={`group/line relative ${DIVIDER[divider]} ${className}`}
      style={style}
    >
      <motion.button
        type="button"
        data-row={kind}
        whileTap={reduced ? undefined : { scale: 0.99 }}
        style={{
          height,
          paddingLeft: padLeft,
          paddingRight: padRight,
          borderRadius: radius,
          boxShadow: topLight ? "inset 0 1px 0 rgba(255,255,255,0.055)" : undefined,
          ["--row-surface" as string]: surface,
          ["--row-surface-hover" as string]: surfaceHover,
          ["--cut" as string]: surface,
        }}
        className={`flex w-full items-center text-left text-[15px] leading-none tracking-[-0.005em]
          bg-[var(--row-surface)] group-hover/line:bg-[var(--row-surface-hover)]
          group-hover/line:[--cut:var(--row-surface-hover)]
          transition-colors duration-[160ms] ${EASE} ${TONE[kind]}`}
      >
        <span
          className={`flex shrink-0 items-center justify-center text-fg-2 group-hover/line:text-fg transition-colors duration-[160ms] ${EASE}`}
          style={{ width: ICON_BOX, height: ICON_BOX }}
        >
          {glyph}
        </span>
        <span className="ml-[12px] min-w-0 truncate">{label}</span>
      </motion.button>

      {/*
        The plus is in the mock as a persistent part of the row, so it rests
        visible rather than appearing only on hover. Three steps, all colour:
        0.58 x fg-2 at rest, full fg-2 when the line is hovered or focused,
        fg plus a disc when the plus itself is the target.
      */}
      {plusLabel ? (
        <button
          type="button"
          data-plus=""
          aria-label={plusLabel}
          className={`absolute top-1/2 right-[11px] grid h-[28px] w-[28px] -translate-y-1/2 place-items-center
            rounded-full text-fg-2 opacity-[0.58]
            transition-[opacity,color,background-color,scale] duration-[160ms] ${EASE}
            group-hover/line:opacity-100 hover:bg-white/[0.07] hover:text-fg
            focus-visible:opacity-100 ${reduced ? "" : "active:scale-[0.9]"}`}
        >
          <Plus size={16} strokeWidth={1.75} aria-hidden />
        </button>
      ) : null}
    </motion.div>
  );
}
