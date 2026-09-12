import { Plus, Server, Shield } from "lucide-react";
import type { ComponentType } from "react";

/** List glyphs are drawn at 16px / 1.75 stroke; the bottom-right link at 14px. */
export const ICON_SIZE = 16;
export const ICON_STROKE = 1.75;
/** Fixed column the vault glyph sits in, so every label starts on one line. */
export const ICON_COL = 20;

/**
 * `--cut` is an unregistered custom property, so it snaps the instant a row is
 * hovered. Transitioning the badge's own background-color and box-shadow —
 * real animatable properties, whose computed values re-resolve when the var
 * changes — keeps the cut-out in step with the row's 160ms color fade.
 */
const CUT_MS = 160;
const CUT_EASE = "cubic-bezier(0.2, 0.8, 0.2, 1)";

type LucideLike = ComponentType<{
  size?: number;
  strokeWidth?: number;
  "aria-hidden"?: boolean;
}>;

export function ServerGlyph({ size = ICON_SIZE }: { size?: number }) {
  return <Server size={size} strokeWidth={ICON_STROKE} aria-hidden />;
}

/** A vault reads as a shield: it is a guarded space, not a container. */
export function VaultGlyph({ size = ICON_SIZE }: { size?: number }) {
  return <Shield size={size} strokeWidth={ICON_STROKE} aria-hidden />;
}

/**
 * Base glyph with a plus badge at its bottom-right. The badge sits on a disc
 * painted with whatever surface is behind it (`--cut`, set by the row or the
 * button) and carries a ring of the same color, so it punches a clean hole out
 * of the base glyph instead of colliding with its strokes.
 */
function WithPlusBadge({ Base, size }: { Base: LucideLike; size: number }) {
  // The knockout is a circle of radius disc/2 + ring centerd on the badge. Any
  // larger and it reaches the middle of the base glyph — on a shield that means
  // eating the point, which is the whole silhouette — so it is kept small and
  // pushed a quarter of the box outside, where it only bites the corner.
  const disc = Math.round(size * 0.58);
  const plus = disc - 1;
  const out = -Math.round(size * 0.3);

  return (
    <span
      className="relative block shrink-0"
      style={{ width: size, height: size, lineHeight: 0 }}
    >
      <Base size={size} strokeWidth={ICON_STROKE} aria-hidden />
      <span
        className="absolute grid place-items-center rounded-full"
        style={{
          width: disc,
          height: disc,
          right: out,
          bottom: out,
          backgroundColor: "var(--cut, var(--color-surface))",
          boxShadow: "0 0 0 1.5px var(--cut, var(--color-surface))",
          transition: `background-color ${CUT_MS}ms ${CUT_EASE}, box-shadow ${CUT_MS}ms ${CUT_EASE}`,
        }}
      >
        <Plus size={plus} strokeWidth={2.5} aria-hidden />
      </span>
    </span>
  );
}

export function ServerPlusGlyph({ size = 14 }: { size?: number }) {
  return <WithPlusBadge Base={Server} size={size} />;
}

export function VaultPlusGlyph({ size = ICON_SIZE }: { size?: number }) {
  return <WithPlusBadge Base={Shield} size={size} />;
}

export { Plus, Server, Shield };
