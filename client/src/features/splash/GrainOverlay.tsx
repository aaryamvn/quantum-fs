import { devQuery } from "@/lib/devQuery";

const GRAIN_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="240" height="240">' +
  '<filter id="n"><feTurbulence type="fractalNoise" baseFrequency="0.85" numOctaves="2" stitchTiles="stitch"/>' +
  '<feColorMatrix type="saturate" values="0"/></filter>' +
  '<rect width="100%" height="100%" filter="url(#n)"/></svg>';

const GRAIN_URL = `url("data:image/svg+xml,${encodeURIComponent(GRAIN_SVG)}")`;

export interface GrainOverlayProps {
  opacity?: number;
  /** Freeze the drift (screenshot mode). */
  frozen?: boolean;
}

/**
 * Film grain. Blended with `overlay`, so it is invisible over pure black and
 * only reads across the coloured field — that is intended.
 */
export function GrainOverlay({ opacity = 0.09, frozen = false }: GrainOverlayProps) {
  const animated = !frozen && !devQuery.reduced;

  return (
    <div
      aria-hidden
      className={animated ? "grain-anim" : undefined}
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 1,
        pointerEvents: "none",
        mixBlendMode: "overlay",
        opacity,
        transform: "translateZ(0)",
        backgroundImage: GRAIN_URL,
        backgroundSize: "240px 240px",
      }}
    />
  );
}
