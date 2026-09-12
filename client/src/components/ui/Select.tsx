import { ChevronDown } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";

import { GhostButton } from "./GhostButton";
import { MenuItem, MenuList } from "./Menu";
import { Popover } from "./Popover";

/**
 * The app's dropdown, assembled rather than invented: a GhostButton trigger, the
 * Popover that already knows how to flip and clamp against the viewport, and the
 * MenuList that already owns roving focus and typeahead. A native `<select>` was
 * not an option — its popup is drawn by the OS in the system palette, which on a
 * near-black surface reads as a hole punched in the window.
 *
 * The menu is never narrower than its trigger: a list that shrinks under the
 * control it belongs to looks detached from it, so the trigger's measured width
 * becomes the panel's floor while long labels are still free to widen it.
 */

export interface SelectOption<T extends string> {
  value: T;
  label: string;
  icon?: ReactNode;
  /** A quiet hint shown at the right of the row, e.g. "default" or a count. */
  description?: string;
}

export interface SelectProps<T extends string> {
  value: T;
  onChange(v: T): void;
  options: SelectOption<T>[];
  size?: "sm" | "md";
  className?: string;
  "aria-label"?: string;
  disabled?: boolean;
}

export function Select<T extends string>({
  value,
  onChange,
  options,
  size = "sm",
  className,
  "aria-label": ariaLabel,
  disabled = false,
}: SelectProps<T>) {
  const wrap = useRef<HTMLSpanElement>(null);
  const [open, setOpen] = useState(false);
  const [minWidth, setMinWidth] = useState(0);

  const close = useCallback(() => setOpen(false), []);

  // Width is read at open time, not at mount: the trigger's label changes with
  // the selection, so a width captured once would be stale the first time the
  // user picks a longer option.
  const openMenu = useCallback(() => {
    if (disabled) return;
    setMinWidth(wrap.current?.offsetWidth ?? 0);
    setOpen(true);
  }, [disabled]);

  // GhostButton takes no ARIA beyond a label, and forking it for one combobox
  // would duplicate its hover rules; the two attributes a listener needs are set
  // on the rendered element instead, so assistive tech still hears the state.
  useEffect(() => {
    const button = wrap.current?.querySelector("button");
    if (!button) return;
    button.setAttribute("aria-haspopup", "menu");
    button.setAttribute("aria-expanded", open ? "true" : "false");
  }, [open]);

  function onKeyDown(e: ReactKeyboardEvent<HTMLSpanElement>) {
    if (open || disabled) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      openMenu();
    }
  }

  const selected = options.find((option) => option.value === value);

  return (
    <>
      <span
        ref={wrap}
        onKeyDown={onKeyDown}
        className={["inline-flex", className ?? ""].filter(Boolean).join(" ")}
      >
        <GhostButton
          variant="outline"
          size={size}
          disabled={disabled}
          aria-label={ariaLabel}
          onClick={() => (open ? close() : openMenu())}
        >
          <span className="flex items-center gap-[8px]">
            <span className="truncate">{selected?.label ?? value}</span>
            <ChevronDown size={14} strokeWidth={1.75} className="shrink-0 text-fg-3" />
          </span>
        </GhostButton>
      </span>

      <Popover
        open={open}
        onClose={close}
        anchor={wrap.current}
        placement="bottom-start"
        kind="menu"
      >
        <div style={{ minWidth }}>
          <MenuList onClose={close}>
            {options.map((option) => (
              <MenuItem
                key={option.value}
                icon={option.icon}
                checked={option.value === value}
                trailing={option.description}
                onSelect={() => onChange(option.value)}
              >
                {option.label}
              </MenuItem>
            ))}
          </MenuList>
        </div>
      </Popover>
    </>
  );
}

export default Select;
