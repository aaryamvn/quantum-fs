import type { IconSpec } from "./types";

/**
 * The shared drawing kit every family is built from.
 *
 * The "3D" in these icons is hand-drawn, not filtered: a front face on a
 * 64-unit stage, a flat extrusion pushed straight down behind it, one contact
 * shadow ellipse and two gradients. A grid can show a hundred of these at once,
 * and `feGaussianBlur` / `feDropShadow` are per-instance raster passes — the
 * first thing that would make scrolling the vault stutter. So the whole system
 * is geometry plus gradients, and every family obeys the same light: key from
 * the top-left, body lighter at the top, darker plate underneath.
 */

/** Every family draws inside 0..64; `FileIcon` scales that to the rendered size. */
export const ICON_VIEW = 64;

/**
 * Below this *rendered pixel* size families drop monograms, rules and ornaments.
 * An 18–20px sidebar icon that still tries to draw "TSX" and three text lines
 * turns into gray mush; the silhouette and the hue carry the meaning instead.
 */
export const NO_DETAIL_BELOW = 28;

/** How far the extrusion is pushed down behind the front face. */
export const DEPTH = 3.5;

function clamp01(v: number): number {
  return v < 0 ? 0 : v > 1 ? 1 : v;
}

/** Trim float noise out of generated path data. */
function r2(v: number): number {
  return Math.round(v * 100) / 100;
}

function parseHex(hex: string): [number, number, number] {
  let h = hex.trim().replace(/^#/, "");
  if (h.length === 3) {
    h = `${h[0]}${h[0]}${h[1]}${h[1]}${h[2]}${h[2]}`;
  }
  const n = Number.parseInt(h, 16);
  if (!Number.isFinite(n) || h.length !== 6) return [107, 115, 134];
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/**
 * Lighten (`amount` > 0) or darken (`amount` < 0) a hex color in plain sRGB.
 *
 * sRGB rather than a perceptual space on purpose: the registry authors pick
 * hues by eye against these derived stops, so the maths has to be the one they
 * can predict, and the error at ±0.35 is invisible at icon scale.
 */
export function shade(hex: string, amount: number): string {
  const [r, g, b] = parseHex(hex);
  const a = Math.max(-1, Math.min(1, amount));
  const mix = (c: number): number =>
    Math.round(a >= 0 ? c + (255 - c) * a : c * (1 + a));
  const hx = (c: number): string => mix(c).toString(16).padStart(2, "0");
  return `#${hx(r)}${hx(g)}${hx(b)}`;
}

/** A hex color as `rgba(...)` — SVG attributes cannot take `#RRGGBBAA` reliably. */
export function withAlpha(hex: string, a: number): string {
  const [r, g, b] = parseHex(hex);
  return `rgba(${r}, ${g}, ${b}, ${clamp01(a)})`;
}

/**
 * The four gradients every family shares, namespaced by `uid` so a grid of
 * icons never cross-references another instance's defs.
 */
export function Defs({ uid, spec }: { uid: string; spec: IconSpec }) {
  return (
    <defs>
      <linearGradient id={`${uid}-body`} x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" stopColor={shade(spec.hue, 0.18)} />
        <stop offset="55%" stopColor={spec.hue} />
        <stop offset="100%" stopColor={spec.hue2} />
      </linearGradient>
      <linearGradient id={`${uid}-sheen`} x1="0" y1="0" x2="1" y2="1">
        <stop offset="0%" stopColor="#FFFFFF" stopOpacity="0.16" />
        <stop offset="55%" stopColor="#FFFFFF" stopOpacity="0" />
      </linearGradient>
      <linearGradient id={`${uid}-edge`} x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" stopColor={spec.hue2} />
        <stop offset="100%" stopColor={shade(spec.hue2, -0.35)} />
      </linearGradient>
      {/*
        A pool of the object's own color rather than a black shadow. The app
        sits on #010513, where black-on-near-black is simply invisible — the
        icon reads as cut out and floating. Bouncing the hue back up off the
        surface is what a real object does on a dark stage, and it is the one
        cue that grounds the whole grid.
      */}
      <radialGradient id={`${uid}-shadow`} cx="0.5" cy="0.5" r="0.5">
        <stop offset="0%" stopColor={withAlpha(spec.hue, 0.3)} />
        <stop offset="60%" stopColor={withAlpha(spec.hue2, 0.1)} />
        <stop offset="100%" stopColor={withAlpha(spec.hue2, 0)} />
      </radialGradient>
    </defs>
  );
}

export interface ContactShadowProps {
  uid: string;
  cx: number;
  cy: number;
  rx: number;
  ry: number;
}

/**
 * The soft pool under the object. Drawn first, before any body, so everything
 * else sits on top of it — a radial-gradient ellipse stands in for the blur
 * that filters would otherwise cost us per instance.
 */
export function ContactShadow({ uid, cx, cy, rx, ry }: ContactShadowProps) {
  return <ellipse cx={cx} cy={cy} rx={rx} ry={ry} fill={`url(#${uid}-shadow)`} />;
}

export interface SlabProps {
  uid: string;
  x: number;
  y: number;
  w: number;
  h: number;
  r: number;
  depth?: number;
}

/**
 * The default 3D block: a rounded front face with a darker plate pushed out
 * from behind its bottom edge. Ordering is the whole illusion — extrusion,
 * face, top highlight, sheen — so callers never have to think about z-order.
 */
export function Slab({ uid, x, y, w, h, r, depth = DEPTH }: SlabProps) {
  const cid = `${uid}-sl${Math.round(x)}_${Math.round(y)}`;
  return (
    <g>
      <rect x={x} y={r2(y + depth)} width={w} height={h} rx={r} fill={`url(#${uid}-edge)`} />
      <rect x={x} y={y} width={w} height={h} rx={r} fill={`url(#${uid}-body)`} />
      <path
        d={`M${r2(x + r)} ${r2(y + 0.9)}H${r2(x + w - r)}`}
        stroke="rgba(255,255,255,0.35)"
        strokeWidth="1"
        strokeLinecap="round"
      />
      <clipPath id={cid}>
        <rect x={x} y={y} width={w} height={h} rx={r} />
      </clipPath>
      <path
        d={`M${x} ${y}H${r2(x + w)}L${x} ${r2(y + h)}Z`}
        fill={`url(#${uid}-sheen)`}
        clipPath={`url(#${cid})`}
      />
    </g>
  );
}

export interface PageProps {
  uid: string;
  /** primary hue — the folded corner is a lighter tint of it, so it cannot come from a gradient */
  hue: string;
  x: number;
  y: number;
  w: number;
  h: number;
  fold: number;
  depth?: number;
}

/** The page silhouette: rounded rect with the top-right corner cut by `fold`. */
function pagePath(x: number, y: number, w: number, h: number, fold: number, r = 3): string {
  return [
    `M${r2(x + r)} ${r2(y)}`,
    `H${r2(x + w - fold)}`,
    `L${r2(x + w)} ${r2(y + fold)}`,
    `V${r2(y + h - r)}`,
    `A${r} ${r} 0 0 1 ${r2(x + w - r)} ${r2(y + h)}`,
    `H${r2(x + r)}`,
    `A${r} ${r} 0 0 1 ${r2(x)} ${r2(y + h - r)}`,
    `V${r2(y + r)}`,
    `A${r} ${r} 0 0 1 ${r2(x + r)} ${r2(y)}`,
    "Z",
  ].join("");
}

/**
 * A sheet of paper instead of a block: same light and extrusion as `Slab`, but
 * the top-right corner is turned down. That dog-ear is the one shape people
 * read as "a document" at 20px without any label, which is exactly the size
 * where the monogram has already been dropped.
 */
export function Page({ uid, hue, x, y, w, h, fold, depth = DEPTH }: PageProps) {
  const cid = `${uid}-pg${Math.round(x)}_${Math.round(y)}`;
  const d = pagePath(x, y, w, h, fold);
  return (
    <g>
      <g transform={`translate(0 ${depth})`}>
        <path d={d} fill={`url(#${uid}-edge)`} />
      </g>
      <path d={d} fill={`url(#${uid}-body)`} />
      <path
        d={`M${r2(x + 3)} ${r2(y + 0.9)}H${r2(x + w - fold - 1)}`}
        stroke="rgba(255,255,255,0.35)"
        strokeWidth="1"
        strokeLinecap="round"
      />
      <path
        d={`M${r2(x + w - fold)} ${r2(y)}L${r2(x + w)} ${r2(y + fold)}H${r2(x + w - fold)}Z`}
        fill={shade(hue, 0.32)}
      />
      <path
        d={`M${r2(x + w - fold)} ${r2(y)}V${r2(y + fold)}H${r2(x + w)}`}
        stroke="rgba(0,0,0,0.16)"
        strokeWidth="0.75"
        fill="none"
        strokeLinejoin="round"
      />
      <clipPath id={cid}>
        <path d={d} />
      </clipPath>
      <path
        d={`M${x} ${y}H${r2(x + w)}L${x} ${r2(y + h)}Z`}
        fill={`url(#${uid}-sheen)`}
        clipPath={`url(#${cid})`}
      />
    </g>
  );
}

export interface MonogramProps {
  label: string;
  /** cap height in viewBox units; the font size is derived so 4 chars still fit */
  size: number;
  x: number;
  y: number;
  color: string;
}

/** Cap height is ~0.72em in GT Walsheim; longer labels tighten to stay on the body. */
function fontSizeFor(label: string, cap: number): number {
  const base = cap / 0.72;
  const fit = label.length >= 4 ? 0.64 : label.length === 3 ? 0.82 : 1;
  return r2(base * fit);
}

/**
 * The 2–4 letter stamp on the body. Weight is capped at 500 like everything
 * else in the app (docs/decisions/client-typography.md), so the lift comes from
 * a dark copy sitting 0.6 units under the white one rather than from bolding.
 */
export function Monogram({ label, size, x, y, color }: MonogramProps) {
  if (label === "") return null;
  const fs = fontSizeFor(label, size);
  const common = {
    x,
    y,
    textAnchor: "middle" as const,
    dominantBaseline: "central" as const,
    fontFamily: "var(--font-sans)",
    fontWeight: 500,
    fontSize: fs,
    letterSpacing: "0.04em",
  };
  return (
    <g>
      <text {...common} y={r2(y + 0.6)} fill="rgba(0,0,0,0.30)">
        {label}
      </text>
      <text {...common} fill={color}>
        {label}
      </text>
    </g>
  );
}
