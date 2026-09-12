import { motion, useReducedMotion } from "motion/react";
import { useId, useRef } from "react";
import type { KeyboardEvent, ReactNode } from "react";

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  icon?: ReactNode;
}

export interface SegmentedProps<T extends string> {
  value: T;
  onChange(v: T): void;
  options: SegmentedOption<T>[];
  size?: "sm" | "md";
  className?: string;
}

/**
 * A two-to-four way switch for views that are peers, not a menu of commands:
 * list vs. grid, all vs. mine. Every choice stays visible, so the cost of
 * switching is one click and the set of alternatives never has to be discovered.
 *
 * The selection is one shared highlight that slides between options (layoutId)
 * instead of a per-option background that pops on and off — the movement is what
 * tells you the two states are the same control, not two controls.
 *
 * Arrow keys move the selection the way they do in a radio group, so the whole
 * control is a single tab stop rather than one stop per option.
 */
export function Segmented<T extends string>({
  value,
  onChange,
  options,
  size = "sm",
  className = "",
}: SegmentedProps<T>) {
  const reduced = useReducedMotion() ?? false;
  // Scopes the sliding highlight to this instance: two Segmenteds on one screen
  // must not hand their highlight to each other.
  const layout = useId();
  const root = useRef<HTMLDivElement | null>(null);

  const h = size === "md" ? "h-[28px]" : "h-[24px]";
  const text = size === "md" ? "text-[13px]" : "text-[12.5px]";

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    const i = options.findIndex((o) => o.value === value);
    if (i < 0) return;
    e.preventDefault();
    const step = e.key === "ArrowRight" ? 1 : -1;
    const next = options[(i + step + options.length) % options.length];
    if (!next) return;
    onChange(next.value);
    // Focus follows selection, so the roving tab stop stays on the chosen one.
    root.current?.querySelectorAll<HTMLButtonElement>("button")[
      options.indexOf(next)
    ]?.focus();
  };

  return (
    <div
      ref={root}
      role="radiogroup"
      onKeyDown={onKeyDown}
      className={`inline-flex rounded-[8px] border border-line bg-white/[0.03] p-[2px] ${className}`}
    >
      {options.map((option) => {
        const selected = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={selected}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(option.value)}
            className={`relative inline-flex ${h} items-center gap-[5px] rounded-[6px] px-[10px] ${text} leading-none
              transition-colors duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)]
              ${selected ? "text-fg" : "text-fg-3 hover:text-fg"}`}
          >
            {selected ? (
              <motion.span
                layoutId={`segmented-${layout}`}
                aria-hidden
                className="absolute inset-0 rounded-[6px] bg-white/[0.09]"
                transition={
                  reduced ? { duration: 0 } : { type: "spring", stiffness: 520, damping: 42 }
                }
              />
            ) : null}
            {option.icon ? (
              <span className="relative inline-flex items-center">{option.icon}</span>
            ) : null}
            <span className="relative">{option.label}</span>
          </button>
        );
      })}
    </div>
  );
}
