/**
 * Keyboard shortcuts, in one place.
 *
 * The workspace is keyboard-first, so the same combo string ("mod+shift+n") has
 * to do three jobs: read back to a human as a keycap, match a real KeyboardEvent,
 * and mean ⌘ on a Mac and Ctrl everywhere else. Parsing it once here keeps the
 * menu label and the handler that fires from ever drifting apart.
 */

/** SSR-safe (and Tauri-safe) default: the app is built and used on macOS first. */
export const IS_MAC: boolean = detectMac();

/** The platform's primary modifier, for prose ("hold ⌘"). */
export const MOD: string = IS_MAC ? "⌘" : "Ctrl";

function detectMac(): boolean {
  if (typeof navigator === "undefined") return true;
  const data = (navigator as Navigator & { userAgentData?: { platform?: string } })
    .userAgentData;
  const platform = data?.platform ?? navigator.platform ?? "";
  const ua = navigator.userAgent ?? "";
  return /mac|iphone|ipad|ipod/i.test(platform) || /mac os x/i.test(ua);
}

type ModifierName = "ctrl" | "alt" | "shift" | "meta";

const MODIFIERS: Record<string, ModifierName | "mod"> = {
  mod: "mod",
  cmd: "meta",
  command: "meta",
  meta: "meta",
  super: "meta",
  win: "meta",
  ctrl: "ctrl",
  control: "ctrl",
  alt: "alt",
  opt: "alt",
  option: "alt",
  shift: "shift",
};

/** Spellings that mean the same key, folded so combo and e.key meet in the middle. */
const KEY_ALIASES: Record<string, string> = {
  " ": "space",
  spacebar: "space",
  esc: "escape",
  return: "enter",
  del: "delete",
  arrowup: "up",
  arrowdown: "down",
  arrowleft: "left",
  arrowright: "right",
};

/** Mac keycaps are glyphs; the arrows are glyphs on every platform. */
const KEY_GLYPHS: Record<string, string> = {
  backspace: "⌫",
  delete: "⌦",
  enter: "↩",
  escape: "⎋",
  tab: "⇥",
  space: "␣",
  up: "↑",
  down: "↓",
  left: "←",
  right: "→",
  comma: ",",
  period: ".",
  slash: "/",
};

/** Off the Mac the same keys are words, except the arrows. */
const KEY_WORDS: Record<string, string> = {
  backspace: "Backspace",
  delete: "Del",
  enter: "Enter",
  escape: "Esc",
  tab: "Tab",
  space: "Space",
  up: "↑",
  down: "↓",
  left: "←",
  right: "→",
  comma: ",",
  period: ".",
  slash: "/",
};

interface ParsedCombo {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
  key: string;
}

function normalizeKey(raw: string): string {
  const lower = raw.toLowerCase();
  return KEY_ALIASES[lower] ?? lower;
}

function parse(combo: string): ParsedCombo {
  const out: ParsedCombo = { ctrl: false, alt: false, shift: false, meta: false, key: "" };
  for (const raw of combo.split("+")) {
    const token = raw.trim().toLowerCase();
    if (!token) continue;
    const modifier = MODIFIERS[token];
    if (modifier === "mod") {
      if (IS_MAC) out.meta = true;
      else out.ctrl = true;
    } else if (modifier) {
      out[modifier] = true;
    } else {
      out.key = normalizeKey(token);
    }
  }
  return out;
}

function renderKey(key: string): string {
  const table = IS_MAC ? KEY_GLYPHS : KEY_WORDS;
  const glyph = table[key];
  if (glyph) return glyph;
  if (key.length === 1) return key.toUpperCase();
  return key.charAt(0).toUpperCase() + key.slice(1);
}

/**
 * "mod+shift+n" → "⇧⌘N" on a Mac (⌃⌥⇧⌘ order, key last), "Ctrl+Shift+N" elsewhere.
 */
export function formatShortcut(combo: string): string {
  const { ctrl, alt, shift, meta, key } = parse(combo);
  const rendered = key ? renderKey(key) : "";

  if (IS_MAC) {
    let out = "";
    if (ctrl) out += "⌃";
    if (alt) out += "⌥";
    if (shift) out += "⇧";
    if (meta) out += "⌘";
    return out + rendered;
  }

  const parts: string[] = [];
  if (ctrl) parts.push("Ctrl");
  if (alt) parts.push("Alt");
  if (shift) parts.push("Shift");
  if (meta) parts.push("Meta");
  if (rendered) parts.push(rendered);
  return parts.join("+");
}

/**
 * Exact-modifier match: "mod+n" must not fire on ⇧⌘N, or every shortcut becomes
 * a prefix of every other one.
 */
export function matchesShortcut(e: KeyboardEvent, combo: string): boolean {
  const want = parse(combo);
  if (!want.key) return false;
  if (
    e.ctrlKey !== want.ctrl ||
    e.altKey !== want.alt ||
    e.shiftKey !== want.shift ||
    e.metaKey !== want.meta
  ) {
    return false;
  }

  if (normalizeKey(e.key) === want.key) return true;

  // ⌥ rewrites e.key to a dead symbol on macOS (⌥N → "˜"), and ⇧ rewrites
  // letters to uppercase and digits to punctuation. The physical key survives.
  if (/^[a-z]$/.test(want.key) && e.code === `Key${want.key.toUpperCase()}`) return true;
  if (/^[0-9]$/.test(want.key) && e.code === `Digit${want.key}`) return true;

  return false;
}

/**
 * Global shortcuts must never eat a keystroke aimed at a field — renaming a file
 * types plain letters that are also single-key shortcuts.
 */
export function isEditableTarget(e: Event): boolean {
  const target = e.target;
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  return target.isContentEditable;
}
