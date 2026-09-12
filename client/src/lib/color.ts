/**
 * Peer identity is drawn, never uploaded: no photos, no avatar service, no
 * extra bytes on the wire. Every color here is derived from the peer id, so
 * two machines that never speak still paint the same person the same way.
 */

/** The three brand stops. `ink` is the app background, used as a gradient floor. */
export const BRAND = {
  coral: "#FF7B7B",
  violet: "#4E0EFF",
  ink: "#010513",
} as const;

/**
 * Ink at full strength is the page itself — a gradient into it reads as a hole.
 * Gradients use this lifted ink instead so the darker half stays visible.
 */
const INK_LIFT = "#1B0F4D";

export interface Gradient2 {
  from: string;
  to: string;
  angle: number;
}

/** Ordered pairs; never the same stop twice, so no avatar is ever flat. */
const PAIRS: ReadonlyArray<readonly [string, string]> = [
  [BRAND.coral, BRAND.violet],
  [BRAND.violet, INK_LIFT],
  [BRAND.coral, INK_LIFT],
  [BRAND.violet, BRAND.coral],
  [INK_LIFT, BRAND.violet],
  [INK_LIFT, BRAND.coral],
];

/** FNV-1a, 32-bit. Cheap, stable across runtimes, and spreads short ids well. */
export function hashString(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

/**
 * Deterministic two-stop gradient for a peer. The angle is taken from higher
 * hash bits than the pair index so two peers sharing a pair still tilt apart.
 */
export function peerGradient(peerId: string): Gradient2 {
  const h = hashString(peerId);
  const pair = PAIRS[h % PAIRS.length]!;
  return { from: pair[0], to: pair[1], angle: 120 + ((h >>> 3) % 120) };
}

function channels(hex: string): [number, number, number] {
  let v = hex.trim().replace("#", "");
  if (v.length === 3) v = v[0]! + v[0]! + v[1]! + v[1]! + v[2]! + v[2]!;
  const n = Number.parseInt(v, 16);
  if (!Number.isFinite(n)) return [0, 0, 0];
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function toHex(r: number, g: number, b: number): string {
  const p = (c: number) =>
    Math.max(0, Math.min(255, Math.round(c)))
      .toString(16)
      .padStart(2, "0");
  return `#${p(r)}${p(g)}${p(b)}`.toUpperCase();
}

/** "#RRGGBB" → "rgba(r,g,b,a)". Used for tints that must sit over the grain. */
export function withAlpha(hex: string, alpha: number): string {
  const [r, g, b] = channels(hex);
  const a = Math.max(0, Math.min(1, alpha));
  return `rgba(${r},${g},${b},${a})`;
}

/** Linear blend in sRGB: `t` 0 → `a`, 1 → `b`. */
export function mixHex(a: string, b: string, t: number): string {
  const k = Math.max(0, Math.min(1, t));
  const [ar, ag, ab] = channels(a);
  const [br, bg, bb] = channels(b);
  return toHex(ar + (br - ar) * k, ag + (bg - ag) * k, ab + (bb - ab) * k);
}

/** "Aaryaman Maheshwari" → "AM", "Justin" → "J". Falls back to "?" for a blank name. */
export function initialsOf(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return "?";
  return words
    .slice(0, 2)
    .map((w) => w[0]!)
    .join("")
    .toUpperCase();
}

/** Foreground that stays legible on `hex`, by relative luminance. */
export function readableOn(hex: string): "#010513" | "#F5F5F7" {
  const [r, g, b] = channels(hex);
  const lum = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
  return lum > 0.6 ? "#010513" : "#F5F5F7";
}
