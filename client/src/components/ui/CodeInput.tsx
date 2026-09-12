import { useRef, useState } from "react";

const sanitize = (raw: string) => raw.toUpperCase().replace(/[^A-Z0-9]/g, "");

export interface CodeInputProps {
  length?: number;
  value: string;
  onChange(value: string): void;
  onComplete?(value: string): void;
  onEnter?(): void;
}

/**
 * Join-code entry: one box per character, because the code is read aloud or
 * copied out of a chat, and the boxes make its length obvious before typing.
 *
 * `value` is the single source of truth — box `i` renders `value[i]` — so a
 * keystroke, a paste and a backspace all take the same path, and the code can
 * never hold a gap in the middle.
 */
export function CodeInput({
  length = 6,
  value,
  onChange,
  onComplete,
  onEnter,
}: CodeInputProps) {
  const boxes = useRef<Array<HTMLInputElement | null>>([]);
  const [focusIndex, setFocusIndex] = useState<number | null>(null);

  /**
   * The code as of the last keystroke, not the last render. A keystroke commits
   * and then moves focus in the same tick, so the `onFocus` guard below would
   * otherwise read the pre-commit `value`, decide the next box is out of range,
   * and bounce focus back — swallowing every second character.
   */
  const latest = useRef(value);
  latest.current = value;

  const focus = (i: number) => {
    const el = boxes.current[Math.max(0, Math.min(length - 1, i))];
    el?.focus();
    el?.select();
  };

  const commit = (next: string) => {
    const clipped = next.slice(0, length);
    latest.current = clipped;
    onChange(clipped);
    if (clipped.length === length) onComplete?.(clipped);
  };

  return (
    // Boxes flex to fill the dialog so the row lines up with the button beneath it.
    <div className="flex w-full gap-[8px]">
      {Array.from({ length }, (_, i) => {
        const char = value[i] ?? "";
        const isFocused = focusIndex === i;
        const border = isFocused
          ? "rgba(255,255,255,0.5)"
          : char
            ? "rgba(255,255,255,0.28)"
            : "var(--color-line-strong)";

        return (
          <input
            key={i}
            ref={(el) => {
              boxes.current[i] = el;
            }}
            value={char}
            aria-label={`Join code digit ${i + 1}`}
            inputMode="text"
            autoComplete="one-time-code"
            autoCorrect="off"
            autoCapitalize="characters"
            spellCheck={false}
            maxLength={1}
            onFocus={() => {
              // Never land past the first empty box: a character typed there
              // would render three boxes to the left of the caret.
              if (i > latest.current.length) {
                focus(latest.current.length);
                return;
              }
              setFocusIndex(i);
            }}
            onBlur={() => setFocusIndex((cur) => (cur === i ? null : cur))}
            onChange={(e) => {
              const typed = sanitize(e.target.value);
              if (!typed) return;
              // A password manager can drop the whole code into one box.
              const next =
                typed.length > 1
                  ? value.slice(0, i) + typed
                  : value.slice(0, i) + typed + value.slice(i + 1);
              commit(next);
              focus(typed.length > 1 ? Math.min(i + typed.length, length - 1) : i + 1);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                onEnter?.();
                return;
              }
              if (e.key === "ArrowLeft") {
                e.preventDefault();
                focus(i - 1);
                return;
              }
              if (e.key === "ArrowRight") {
                e.preventDefault();
                focus(i + 1);
                return;
              }
              if (e.key !== "Backspace") return;
              e.preventDefault();
              if (char) {
                commit(value.slice(0, i) + value.slice(i + 1));
                return;
              }
              if (i > 0) {
                commit(value.slice(0, i - 1) + value.slice(i));
                focus(i - 1);
              }
            }}
            onPaste={(e) => {
              e.preventDefault();
              const pasted = sanitize(e.clipboardData.getData("text"));
              if (!pasted) return;
              const merged = (value.slice(0, i) + pasted).slice(0, length);
              commit(merged);
              focus(merged.length);
            }}
            className="h-[52px] min-w-0 flex-1 basis-0 rounded-[10px] border bg-field text-center text-[20px] font-medium text-fg uppercase outline-none
              transition-[border-color] duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)]
              selection:bg-white/20 selection:text-fg"
            style={{ borderColor: border }}
          />
        );
      })}
    </div>
  );
}
