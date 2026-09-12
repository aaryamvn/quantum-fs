/**
 * The rubber-band the canvas draws while you sweep a selection.
 *
 * Deliberately dumb: it takes a rect and paints it. The canvas owns the gesture
 * (capture, threshold, auto-scroll, hit test) because the same numbers drive the
 * selection, and a component that both drew *and* measured would have to be
 * mounted to select anything.
 *
 * Positioned in content coordinates — the canvas is the scroll container and the
 * positioning context, so an absolutely placed box scrolls with the tiles it is
 * selecting instead of sliding off them. It never takes the pointer: the capture
 * lives on the canvas and a band under the cursor would swallow the move events
 * that are resizing it.
 */

import type { CanvasRect } from "./canvasGeometry";

export interface MarqueeProps {
  /** Content-coordinate box, or null when no sweep is in progress. */
  rect: CanvasRect | null;
}

export function Marquee({ rect }: MarqueeProps) {
  if (rect === null) return null;

  return (
    <div
      data-testid="marquee"
      aria-hidden
      className="pointer-events-none absolute rounded-[3px] border border-violet/60 bg-violet/15"
      style={{ left: rect.x, top: rect.y, width: rect.w, height: rect.h }}
    />
  );
}
