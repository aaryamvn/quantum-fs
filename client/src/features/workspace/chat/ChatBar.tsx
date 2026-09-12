import { ArrowUp } from "lucide-react";
import { motion, useReducedMotion } from "motion/react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";

import { isEditableTarget, matchesShortcut } from "@/lib/keys";

import { EASE, Z } from "../layout";
import { CHAT_RIGHT, CHAT_WIDTH, useChat } from "./chatStore";

/** One line of the input; four of them is the tallest the bar ever grows. */
const LINE_H = 20;
const MAX_LINES = 4;

/**
 * The agent bar: one question, always about the folder you are looking at.
 *
 * It floats over the canvas instead of docking to a pane because the thing it
 * talks about is the grid behind it — `CHATBAR_CLEARANCE` keeps the last row of
 * tiles clear of it, so the bar hovers without ever hiding content.
 *
 * It is deliberately two objects and nothing else: a typing line and a send
 * button. The scope of the answer is the folder you are standing in, which the
 * canvas behind the bar already shows at full size — restating it as a chip
 * inside the bar spent width on a fact that was never in doubt, and a model
 * picker offered a choice with one real answer. What is left is the gesture:
 * the send button wears the signature gradient, slowly drifting, with its own
 * blurred copy behind it for the glow.
 *
 * The reply is a stub today; nothing in here knows that, which is the point: the
 * seam is `client.askAgent`, and wiring a real local model changes the daemon.
 */
export function ChatBar() {
  const reduced = useReducedMotion() ?? false;
  const input = useRef<HTMLTextAreaElement | null>(null);

  const [value, setValue] = useState("");
  const [focused, setFocused] = useState(false);

  const pending = useChat((s) => s.pending);
  const send = useChat((s) => s.send);

  const canSend = value.trim() !== "" && !pending;

  // Grow with the text rather than scrolling a one-line box: a pasted paragraph
  // that is invisible above the caret is a question you cannot proof-read.
  useLayoutEffect(() => {
    const el = input.current;
    if (!el) return;
    // An empty field is always exactly one line: Chrome counts the wrapped
    // placeholder in scrollHeight, so on a narrow window "Ask about your files"
    // would otherwise wrap and inflate the resting bar to four lines tall.
    if (value === "") {
      el.style.height = `${LINE_H}px`;
      return;
    }
    el.style.height = "0px";
    el.style.height = `${Math.min(el.scrollHeight, LINE_H * MAX_LINES)}px`;
  }, [value]);

  // ⌘/ from anywhere in the workspace. Guarded by `isEditableTarget` so it never
  // steals the keystroke from a rename field that is already open.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (isEditableTarget(e)) return;
      if (!matchesShortcut(e, "mod+/")) return;
      e.preventDefault();
      input.current?.focus();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  function submit() {
    const prompt = value.trim();
    if (prompt === "" || pending) return;
    setValue("");
    void send(prompt);
  }

  function onKeyDown(e: ReactKeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
      return;
    }
    if (e.key === "Escape") {
      // Stops here: Escape in the bar means "leave the bar", not "clear the canvas".
      e.stopPropagation();
      e.currentTarget.blur();
    }
  }

  const fade = { duration: reduced ? 0 : 0.18, ease: EASE };

  return (
    <div
      data-testid="chatbar"
      className="@container absolute bottom-[18px]"
      style={{ width: CHAT_WIDTH, right: CHAT_RIGHT, zIndex: Z.chrome }}
    >
      {/* The glow is the only thing that says "focused" from across the room. */}
      <motion.div
        aria-hidden
        className="brand-gradient pointer-events-none absolute inset-x-[8%] bottom-[2px] h-[34px] rounded-full blur-[22px]"
        initial={false}
        animate={{ opacity: focused ? 0.25 : 0, scale: 0.98 }}
        transition={fade}
      />

      <motion.div
        initial={false}
        animate={{ y: focused && !reduced ? -1 : 0 }}
        transition={{ duration: reduced ? 0 : 0.16, ease: EASE }}
        className={[
          "glass relative flex min-h-[50px] items-center gap-[10px] rounded-[16px] border pr-[8px] pl-[14px]",
          "shadow-[0_18px_50px_rgba(0,0,0,0.45)]",
          "transition-colors duration-[160ms] ease-standard",
          focused ? "border-white/25" : "border-line-strong",
        ].join(" ")}
      >
        <textarea
          ref={input}
          rows={1}
          value={value}
          placeholder="Ask about your files"
          aria-label="Ask about your files"
          data-selectable
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={onKeyDown}
          onFocus={() => setFocused(true)}
          onBlur={() => setFocused(false)}
          style={{ maxHeight: LINE_H * MAX_LINES }}
          className="scroll-thin min-w-0 flex-1 resize-none self-center bg-transparent py-0
            text-[14px] leading-[20px] text-fg outline-none placeholder:text-fg-3"
        />

        <motion.button
          type="button"
          aria-label="Send"
          disabled={!canSend}
          onClick={submit}
          initial={false}
          animate={{ opacity: canSend ? 1 : 0.5 }}
          whileHover={reduced ? undefined : { scale: 1.04 }}
          whileTap={reduced ? undefined : { scale: 0.96 }}
          transition={fade}
          className="relative grid h-[34px] w-[34px] shrink-0 place-items-center rounded-full text-white"
        >
          {/* The glow is the button's own face blurred behind itself, so it is
              always exactly the color that is on screen at that instant — a
              second, static gradient would drift out of step with the first. */}
          <span
            aria-hidden
            className="brand-gradient-animated pointer-events-none absolute inset-0 scale-[1.15]
              rounded-full opacity-[0.55] blur-[10px]"
          />
          <span aria-hidden className="brand-gradient-animated absolute inset-0 rounded-full" />
          <ArrowUp size={16} strokeWidth={1.75} className="relative" />
        </motion.button>
      </motion.div>
    </div>
  );
}

export default ChatBar;
