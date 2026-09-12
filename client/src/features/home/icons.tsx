import { Lock, Plus, Server } from "lucide-react";
import type { ComponentType } from "react";

/** Every glyph in the list is drawn on the same 18px box at 1.5 stroke. */
export const ICON_BOX = 18;
export const ICON_STROKE = 1.5;

/**
 * `--cut` is an unregistered custom property, so it snaps the instant the row
 * is hovered. Transitioning the badge's own background-color and box-shadow —
 * real animatable properties whose computed values re-resolve when the var
 * changes — puts the cut-out back in step with the row's 160ms colour fade.
 */
const CUT_MS = 160;
const CUT_EASE = "cubic-bezier(0.2, 0.8, 0.2, 1)";

type LucideLike = ComponentType<{
  size?: number;
  strokeWidth?: number;
  "aria-hidden"?: boolean;
}>;

export function ServerGlyph() {
  return <Server size={ICON_BOX} strokeWidth={ICON_STROKE} aria-hidden />;
}

/**
 * Vault glyph. lucide's own `Vault` collapses into a boxed X at 18px and reads
 * as "cancel", so the mark for a vault is `Lock` — legible at this size and
 * unambiguous next to the plus badge on "Join a Vault".
 */
export function VaultGlyph() {
  return <Lock size={ICON_BOX} strokeWidth={ICON_STROKE} aria-hidden />;
}

/**
 * Base glyph with a small plus badge at the bottom-right. The badge sits on a
 * disc painted with the row's own background (`--cut`, set by the row) and
 * carries a ring of the same colour, so it punches a clean hole out of the base
 * glyph instead of colliding with its strokes.
 *
 * The badge overhangs the 18px box by 4px on both axes: any less and the cut
 * circle reaches the middle of the glyph and bisects it (the server's lower bar
 * disappears) instead of taking a bite out of its corner.
 */
function WithPlusBadge({ Base }: { Base: LucideLike }) {
  return (
    <span className="relative block" style={{ width: ICON_BOX, height: ICON_BOX }}>
      <Base size={ICON_BOX} strokeWidth={ICON_STROKE} aria-hidden />
      <span
        className="absolute grid place-items-center rounded-full"
        style={{
          width: 10,
          height: 10,
          right: -4,
          bottom: -4,
          backgroundColor: "var(--cut, var(--color-surface))",
          boxShadow: "0 0 0 1.5px var(--cut, var(--color-surface))",
          transition: `background-color ${CUT_MS}ms ${CUT_EASE}, box-shadow ${CUT_MS}ms ${CUT_EASE}`,
        }}
      >
        <Plus size={10} strokeWidth={2} aria-hidden />
      </span>
    </span>
  );
}

export function ServerPlusGlyph() {
  return <WithPlusBadge Base={Server} />;
}

export function VaultPlusGlyph() {
  return <WithPlusBadge Base={Lock} />;
}

export { Plus };
