import { createContext, useCallback, useContext, useEffect, useRef } from "react";
import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";
import { Check } from "lucide-react";

import { formatShortcut } from "@/lib/keys";

/**
 * Menu primitives with no opinion about where they sit — the right-click menu,
 * the sort menu and the selects all want the same list behavior but land in
 * three different places, so positioning stays with the Popover that wraps them.
 *
 * Focus is roving rather than per-item tabIndex: a menu is one stop, arrows move
 * inside it, and Tab is left alone to leave. Everything that activates an item
 * goes through the same context so a menu never stays open behind its own action.
 */

const ITEM_SELECTOR = '[role="menuitem"]:not([disabled]):not([aria-disabled="true"])';

const MenuContext = createContext<{ close(): void }>({ close() {} });

export function MenuList({
  children,
  className,
  autoFocus = true,
  onClose,
}: {
  children: ReactNode;
  className?: string;
  autoFocus?: boolean;
  onClose?(): void;
}) {
  const ref = useRef<HTMLDivElement>(null);

  const items = useCallback(
    () => Array.from(ref.current?.querySelectorAll<HTMLElement>(ITEM_SELECTOR) ?? []),
    [],
  );

  useEffect(() => {
    if (!autoFocus) return;
    items()[0]?.focus();
  }, [autoFocus, items]);

  function onKeyDown(e: ReactKeyboardEvent<HTMLDivElement>) {
    const list = items();
    if (list.length === 0) return;

    const current = list.indexOf(document.activeElement as HTMLElement);
    const go = (index: number) => {
      e.preventDefault();
      list[(index + list.length) % list.length]?.focus();
    };

    switch (e.key) {
      case "ArrowDown":
        return go(current + 1);
      case "ArrowUp":
        return go(current < 0 ? -1 : current - 1);
      case "Home":
        return go(0);
      case "End":
        return go(list.length - 1);
      default:
        break;
    }

    // Enter and Space are left to the native button, which already clicks.
    // Typeahead jumps to the next item starting with the typed letter, so a
    // long menu is one keystroke deep instead of six arrow presses.
    if (e.key.length === 1 && /\S/.test(e.key) && !e.metaKey && !e.ctrlKey && !e.altKey) {
      const char = e.key.toLowerCase();
      const from = current < 0 ? 0 : current;
      for (let step = 1; step <= list.length; step++) {
        const candidate = list[(from + step) % list.length];
        if (candidate?.textContent?.trim().toLowerCase().startsWith(char)) {
          e.preventDefault();
          candidate.focus();
          break;
        }
      }
    }
  }

  return (
    <MenuContext.Provider value={{ close: () => onClose?.() }}>
      <div
        ref={ref}
        role="menu"
        tabIndex={-1}
        onKeyDown={onKeyDown}
        className={["min-w-[220px] p-[6px]", className ?? ""].filter(Boolean).join(" ")}
      >
        {children}
      </div>
    </MenuContext.Provider>
  );
}

export interface MenuItemProps {
  icon?: ReactNode;
  children: ReactNode;
  shortcut?: string;
  onSelect?(): void;
  disabled?: boolean;
  danger?: boolean;
  /** Shows a check mark on the left when true, and reserves the space when false. */
  checked?: boolean;
  trailing?: ReactNode;
}

export function MenuItem({
  icon,
  children,
  shortcut,
  onSelect,
  disabled = false,
  danger = false,
  checked,
  trailing,
}: MenuItemProps) {
  const { close } = useContext(MenuContext);

  const color = disabled ? "text-fg-3 opacity-60" : danger ? "text-coral" : "text-fg";

  return (
    <button
      type="button"
      role="menuitem"
      tabIndex={-1}
      disabled={disabled}
      aria-disabled={disabled || undefined}
      onClick={() => {
        if (disabled) return;
        onSelect?.();
        close();
      }}
      className={[
        "flex h-[30px] w-full items-center gap-[10px] rounded-[7px] px-[10px]",
        "text-[13px] leading-none transition-colors duration-[160ms] ease-standard",
        color,
        disabled ? "" : "hover:bg-white/[0.07] focus:bg-white/[0.07]",
      ]
        .filter(Boolean)
        .join(" ")}
    >
      {checked === undefined ? null : (
        <span
          className="grid shrink-0 place-items-center text-fg-2"
          style={{ width: 16, height: 16, lineHeight: 0 }}
          aria-hidden
        >
          {checked ? <Check size={16} strokeWidth={1.75} /> : null}
        </span>
      )}

      {icon ? (
        <span
          className="grid shrink-0 place-items-center text-fg-2"
          style={{ width: 16, height: 16, lineHeight: 0 }}
          aria-hidden
        >
          {icon}
        </span>
      ) : null}

      <span className="min-w-0 flex-1 truncate text-left">{children}</span>

      {shortcut ? (
        <span className="shrink-0 text-[12px] leading-none text-fg-3 tabular-nums">
          {formatShortcut(shortcut)}
        </span>
      ) : null}

      {trailing ? <span className="shrink-0 text-fg-3">{trailing}</span> : null}
    </button>
  );
}

/** A pause between groups of items — the same hairline the panels use. */
export function MenuSeparator() {
  return <div role="separator" className="my-[6px] h-px bg-line" />;
}

/**
 * Names a group when the items alone are ambiguous ("Sort by", "View").
 * Sentence case, matching every other section caption in the app — a menu is
 * already a short list, and shouting its group headings only adds weight.
 */
export function MenuLabel({ children }: { children: ReactNode }) {
  return (
    <div className="px-[10px] py-[4px] text-[12.5px] leading-[16px] text-fg-3">{children}</div>
  );
}

/** Plain grouping wrapper — e.g. the icon row across the top of a context menu. */
export function MenuSection({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div role="group" className={className}>
      {children}
    </div>
  );
}
