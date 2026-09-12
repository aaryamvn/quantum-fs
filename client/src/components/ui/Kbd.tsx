import { formatShortcut, IS_MAC } from "@/lib/keys";

/**
 * A keycap, sized to sit on a 13px line without pushing it apart: 18px tall,
 * square until the glyph is wider. It is the quietest thing in any row — a hint
 * you read after the label, never a control — so it borrows line-strong and a
 * 4% wash rather than a real surface.
 */
export function Kbd({ children }: { children: string }) {
  return (
    <kbd
      className="inline-grid h-[18px] min-w-[18px] place-items-center rounded-[5px]
        border border-line-strong bg-white/[0.04] px-[5px] font-sans text-[11px]
        leading-[16px] text-fg-3 tabular-nums"
    >
      {children}
    </kbd>
  );
}

/**
 * Modifiers ride together on one cap (⇧⌘ is read as one gesture, not two) and
 * the key gets its own, which is how the OS draws them in its own menus.
 */
export function Shortcut({ combo }: { combo: string }) {
  const [modifiers, key] = splitShortcut(formatShortcut(combo));

  return (
    <span className="inline-flex items-center gap-[3px]">
      {modifiers ? <Kbd>{modifiers}</Kbd> : null}
      {key ? <Kbd>{key}</Kbd> : null}
    </span>
  );
}

const MAC_MODIFIER_GLYPHS = "⌃⌥⇧⌘";

/** Mac shortcuts are a run of glyphs then the key; elsewhere they are "+"-joined. */
function splitShortcut(formatted: string): [string, string] {
  if (!IS_MAC) {
    const parts = formatted.split("+");
    const key = parts.pop() ?? "";
    return [parts.join("+"), key];
  }

  let cut = 0;
  for (const char of formatted) {
    if (!MAC_MODIFIER_GLYPHS.includes(char)) break;
    cut += char.length;
  }
  return [formatted.slice(0, cut), formatted.slice(cut)];
}
