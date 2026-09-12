import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { X } from "lucide-react";
import { useEffect, useId, useRef } from "react";
import type { ReactNode } from "react";

/** House ease — matches --ease-out-expo. */
const EASE: [number, number, number, number] = [0.16, 1, 0.3, 1];

/** The description's fade matches the body crossfade in every dialog. */
const DESC = { duration: 0.2, ease: EASE };

const FOCUSABLE =
  'input:not([disabled]), button:not([disabled]), textarea:not([disabled]), select:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])';

export interface ModalProps {
  open: boolean;
  onClose(): void;
  title: string;
  description?: string;
  children: ReactNode;
}

/**
 * The app's one dialog. Fixed rather than portalled — nothing in the tree
 * clips or stacks above it, and z-[60] clears the Tauri drag strip at z-50.
 *
 * Focus moves to the first control on open and returns to the opener on close,
 * and Tab is cycled inside the panel so the dialog cannot be escaped by keyboard
 * while it is up.
 */
export function Modal({ open, onClose, title, description, children }: ModalProps) {
  const reduced = useReducedMotion() ?? false;
  const panel = useRef<HTMLDivElement | null>(null);
  const opener = useRef<HTMLElement | null>(null);
  const titleId = useId();
  const descId = useId();

  /**
   * The element to hand focus back to. It cannot be read at open time: React
   * has already committed the panel and honoured `autoFocus` by then, so
   * `document.activeElement` would be a control inside the dialog. Tracking the
   * last focus outside the panel instead keeps hold of the real opener.
   */
  // Set during render, so it is already true by the time React commits the
  // panel and honours `autoFocus` — the listener below must not record that.
  const isOpen = useRef(open);
  isOpen.current = open;

  useEffect(() => {
    const onFocusIn = (e: FocusEvent) => {
      if (isOpen.current) return;
      const target = e.target as HTMLElement | null;
      if (!target || target === document.body) return;
      opener.current = target;
    };
    document.addEventListener("focusin", onFocusIn, true);
    return () => document.removeEventListener("focusin", onFocusIn, true);
  }, []);

  // Focus the first control on open, hand focus back to the opener on close.
  useEffect(() => {
    if (!open) return;
    const entered = opener.current;
    const id = requestAnimationFrame(() => {
      const first = panel.current?.querySelector<HTMLElement>(FOCUSABLE);
      first?.focus();
    });
    return () => {
      cancelAnimationFrame(id);
      if (entered?.isConnected) entered.focus();
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
        return;
      }
      if (e.key !== "Tab") return;
      const nodes = panel.current?.querySelectorAll<HTMLElement>(FOCUSABLE);
      if (!nodes || nodes.length === 0) return;
      const first = nodes[0]!;
      const last = nodes[nodes.length - 1]!;
      const active = document.activeElement;
      if (e.shiftKey && (active === first || !panel.current?.contains(active))) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && active === last) {
        e.preventDefault();
        first.focus();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open, onClose]);

  const panelMotion = reduced
    ? {
        initial: { opacity: 0 },
        animate: { opacity: 1, transition: { duration: 0.15 } },
        exit: { opacity: 0, transition: { duration: 0.12 } },
      }
    : {
        initial: { opacity: 0, y: 10, scale: 0.97 },
        animate: {
          opacity: 1,
          y: 0,
          scale: 1,
          transition: { duration: 0.32, ease: EASE },
        },
        exit: {
          opacity: 0,
          y: 10,
          scale: 0.97,
          transition: { duration: 0.18, ease: EASE },
        },
      };

  return (
    <AnimatePresence>
      {open ? (
        <motion.div
          key="backdrop"
          className="fixed inset-0 z-[60] flex items-center justify-center backdrop-blur-[6px]"
          style={{ background: "rgba(1,5,19,0.72)" }}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1, transition: { duration: 0.2 } }}
          exit={{ opacity: 0, transition: { duration: 0.2 } }}
          onMouseDown={(e) => {
            if (e.target === e.currentTarget) onClose();
          }}
        >
          <motion.div
            ref={panel}
            role="dialog"
            aria-modal="true"
            aria-labelledby={titleId}
            aria-describedby={description ? descId : undefined}
            {...panelMotion}
            className="relative w-[min(420px,calc(100%-48px))] rounded-[16px] border border-line-strong pt-[28px] pr-[28px] pb-[24px] pl-[28px] text-left"
            style={{
              background: "var(--color-surface-2, #0A0F22)",
              boxShadow: "0 24px 80px rgba(0,0,0,0.6)",
            }}
          >
            <h2
              id={titleId}
              className="pr-[28px] text-[20px] leading-[26px] font-medium tracking-[-0.015em] text-fg"
            >
              {title}
            </h2>
            {/*
              The description is an instruction for the form, so it leaves with
              the form: dropping `description` on success collapses it in the
              same 200ms the body crossfades in, and the confirmation never sits
              under a line telling you to do what you just did.
            */}
            <AnimatePresence initial={false}>
              {description ? (
                <motion.div
                  key="description"
                  initial={{ opacity: 0, height: 0 }}
                  animate={{ opacity: 1, height: "auto", transition: DESC }}
                  exit={{ opacity: 0, height: 0, transition: DESC }}
                  style={{ overflow: "hidden" }}
                >
                  <p id={descId} className="pt-[6px] text-[14px] leading-[19px] text-fg-3">
                    {description}
                  </p>
                </motion.div>
              ) : null}
            </AnimatePresence>

            <div className="mt-[24px]">{children}</div>

            {/*
              Last in the DOM, first in the corner: absolutely positioned, so
              placing it here only means the dialog opens on its own field
              rather than on the dismiss control.
            */}
            <button
              type="button"
              aria-label="Close"
              onClick={onClose}
              className="absolute top-[16px] right-[16px] grid h-[26px] w-[26px] place-items-center rounded-full text-fg-3 transition-colors duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)] hover:bg-white/[0.06] hover:text-fg"
            >
              <X size={16} strokeWidth={1.75} aria-hidden />
            </button>
          </motion.div>
        </motion.div>
      ) : null}
    </AnimatePresence>
  );
}
