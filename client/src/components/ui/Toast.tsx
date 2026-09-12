import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { AlertCircle, Check, Info } from "lucide-react";

/** House ease — matches --ease-out-expo. */
const EASE: [number, number, number, number] = [0.16, 1, 0.3, 1];

/** The one green in the app, shared with Chip's success tone. */
const SUCCESS = "#3DDC84";

export type ToastKind = "info" | "success" | "error";

export interface ToastData {
  id: string;
  text: string;
  kind: ToastKind;
  action?: { label: string; onClick(): void };
}

export interface ToastStackProps {
  toasts: ToastData[];
  onDismiss(id: string): void;
  className?: string;
  /**
   * Lets a row grow past one line instead of running off its container. Off by
   * default: over the canvas a toast has the whole window to be one line in, and
   * one line is what makes a stack readable. Inside a fixed-width column — the
   * home list — a sentence the daemon wrote is not bounded by anything, so the
   * row has to wrap or it leaves the screen.
   */
  wrap?: boolean;
}

/**
 * Every background outcome the app has to mention — a file synced, a peer
 * dropped, a key rejected — without stealing focus. Toasts never block, never
 * ask a question, and carry at most one action ("Undo", "Retry").
 *
 * Newest at the bottom: the stack grows toward the corner it is anchored in, so
 * the newest line lands nearest the eye and the older ones drift away rather
 * than being shoved down under the cursor.
 *
 * Presentation only. The lifetime of a toast — when it appears, how long it
 * lives, when it is auto-dismissed — belongs to the workspace store, because
 * that is what knows whether the work it describes is still happening.
 */
export function ToastStack({ toasts, onDismiss, className = "", wrap = false }: ToastStackProps) {
  const reduced = useReducedMotion() ?? false;

  // A wrapping row cannot keep a fixed height, so it keeps the same minimum and
  // pads instead: a one-line toast in a wrapping stack is pixel-identical.
  const metrics = wrap ? "min-h-[36px] max-w-full py-[8px]" : "h-[36px]";

  const enter = reduced
    ? {
        initial: { opacity: 0 },
        animate: { opacity: 1, transition: { duration: 0.15 } },
        exit: { opacity: 0, transition: { duration: 0.12 } },
      }
    : {
        initial: { opacity: 0, y: 8, scale: 0.98 },
        animate: { opacity: 1, y: 0, scale: 1, transition: { duration: 0.2, ease: EASE } },
        exit: { opacity: 0, y: 4, transition: { duration: 0.14, ease: EASE } },
      };

  return (
    <div
      aria-live="polite"
      className={`pointer-events-none flex flex-col items-start gap-[8px] ${className}`}
    >
      <AnimatePresence initial={false}>
        {toasts.map((toast) => (
          <motion.div
            key={toast.id}
            // Layout so the survivors slide into the gap a dismissed toast
            // leaves instead of jumping.
            layout={reduced ? false : "position"}
            {...enter}
            onClick={() => onDismiss(toast.id)}
            className={`pointer-events-auto flex ${metrics} items-center gap-[10px] rounded-[10px] border border-line-strong bg-surface-2 px-[12px] text-[13px] text-fg shadow-[0_12px_32px_rgba(0,0,0,0.5)]`}
          >
            <ToastIcon kind={toast.kind} />
            <span className={wrap ? "min-w-0 leading-[18px]" : "whitespace-nowrap"}>
              {toast.text}
            </span>
            {toast.action ? (
              <button
                type="button"
                // The row itself dismisses; the action must not do both.
                onClick={(e) => {
                  e.stopPropagation();
                  toast.action?.onClick();
                }}
                className="text-fg-2 transition-colors duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)] hover:text-fg"
              >
                {toast.action.label}
              </button>
            ) : null}
          </motion.div>
        ))}
      </AnimatePresence>
    </div>
  );
}

/** One line of color is the whole difference between the three kinds. */
function ToastIcon({ kind }: { kind: ToastKind }) {
  if (kind === "success") {
    return (
      <Check
        size={14}
        strokeWidth={1.75}
        className="shrink-0"
        style={{ color: SUCCESS }}
        aria-hidden
      />
    );
  }
  if (kind === "error") {
    return <AlertCircle size={14} strokeWidth={1.75} className="shrink-0 text-coral" aria-hidden />;
  }
  return <Info size={14} strokeWidth={1.75} className="shrink-0 text-fg-3" aria-hidden />;
}
