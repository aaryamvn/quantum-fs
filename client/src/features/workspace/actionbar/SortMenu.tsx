import { ArrowUpDown } from "lucide-react";
import { useCallback, useRef, useState } from "react";

import { GhostButton } from "@/components/ui/GhostButton";
import { MenuItem, MenuList, MenuSeparator } from "@/components/ui/Menu";
import { Popover } from "@/components/ui/Popover";

import { useWorkspace } from "../store";
import type { SortBy, SortDir } from "../store";

/**
 * The one control that changes what order the grid is in, and it says its own
 * answer out loud: the trigger reads "Sort: Modified" rather than an anonymous
 * glyph, so the current order is legible without opening anything. A folder that
 * looks wrong is almost always a folder sorted differently than you remember,
 * and a menu you have to open to answer that question is a menu you open twice.
 *
 * Direction lives in the same menu below a separator instead of in a second
 * button: it is a modifier of the column above it, never an independent control,
 * and splitting them would imply otherwise.
 */

const ORDERS: { value: SortBy; label: string }[] = [
  { value: "name", label: "Name" },
  { value: "kind", label: "Kind" },
  { value: "modified", label: "Modified" },
  { value: "size", label: "Size" },
];

const DIRECTIONS: { value: SortDir; label: string }[] = [
  { value: "asc", label: "Ascending" },
  { value: "desc", label: "Descending" },
];

export function SortMenu() {
  const wrap = useRef<HTMLSpanElement>(null);
  const [open, setOpen] = useState(false);

  const sortBy = useWorkspace((s) => s.sortBy);
  const sortDir = useWorkspace((s) => s.sortDir);
  const setSort = useWorkspace((s) => s.setSort);

  const close = useCallback(() => setOpen(false), []);

  const current = ORDERS.find((order) => order.value === sortBy);

  return (
    <span ref={wrap} className="inline-flex" data-testid="sort-menu">
      <GhostButton
        icon={<ArrowUpDown size={14} strokeWidth={1.75} />}
        active={open}
        onClick={() => (open ? close() : setOpen(true))}
      >
        {`Sort: ${current?.label ?? "Name"}`}
      </GhostButton>

      <Popover
        open={open}
        onClose={close}
        anchor={wrap.current}
        placement="bottom-end"
        kind="menu"
      >
        <MenuList onClose={close} className="min-w-[180px]">
          {/* The column always goes with an explicit direction: `setSort(by)` alone
              flips the direction when the column is unchanged — right for a clicked
              column header, wrong for a menu that lists direction separately. */}
          {ORDERS.map((order) => (
            <MenuItem
              key={order.value}
              checked={order.value === sortBy}
              onSelect={() => setSort(order.value, sortDir)}
            >
              {order.label}
            </MenuItem>
          ))}

          <MenuSeparator />

          {DIRECTIONS.map((direction) => (
            <MenuItem
              key={direction.value}
              checked={direction.value === sortDir}
              onSelect={() => setSort(sortBy, direction.value)}
            >
              {direction.label}
            </MenuItem>
          ))}
        </MenuList>
      </Popover>
    </span>
  );
}
