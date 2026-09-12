import { useCallback, useRef, useState } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";
import { ChevronLeft, ChevronRight } from "lucide-react";

import { IconButton } from "@/components/ui/IconButton";
import { MenuItem, MenuLabel, MenuList } from "@/components/ui/Menu";
import { Popover } from "@/components/ui/Popover";
import { formatShortcut } from "@/lib/keys";

import { useWorkspace } from "../store";

/** Depth of history the right-click menu is willing to show at once. */
const HISTORY_LIMIT = 12;

/**
 * Back and forward, and — on a right-click — the stack behind them.
 *
 * The carets are deliberately the only navigation control with no label: they
 * are the browser gesture people already own, and a folder tree is shallow
 * enough that most trips are one press. The history menu exists for the trip
 * that was not: rather than teaching the store a "jump to index" action nobody
 * else needs, a jump replays `back()`/`forward()` synchronously, so every hop
 * runs the same arrival path (presence, selection, recents) a single press does.
 */
export function NavCarets() {
  const canBack = useWorkspace((s) => s.canBack());
  const canForward = useWorkspace((s) => s.canForward());
  const back = useWorkspace((s) => s.back);
  const forward = useWorkspace((s) => s.forward);

  const backRef = useRef<HTMLSpanElement>(null);
  const [historyOpen, setHistoryOpen] = useState(false);
  const closeHistory = useCallback(() => setHistoryOpen(false), []);

  function onBackContextMenu(e: ReactMouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (useWorkspace.getState().nav.entries.length > 1) setHistoryOpen(true);
  }

  return (
    <div data-testid="nav-carets" className="flex shrink-0 items-center gap-[2px]">
      <span ref={backRef} className="inline-flex" onContextMenu={onBackContextMenu}>
        <IconButton
          icon={<ChevronLeft size={16} strokeWidth={1.75} />}
          label="Back"
          shortcut={formatShortcut("mod+[")}
          size={28}
          disabled={!canBack}
          onClick={() => back()}
        />
      </span>

      <IconButton
        icon={<ChevronRight size={16} strokeWidth={1.75} />}
        label="Forward"
        shortcut={formatShortcut("mod+]")}
        size={28}
        disabled={!canForward}
        onClick={() => forward()}
      />

      <Popover
        open={historyOpen}
        onClose={closeHistory}
        anchor={backRef.current}
        placement="bottom-start"
        kind="menu"
      >
        <HistoryMenu onClose={closeHistory} />
      </Popover>
    </div>
  );
}

/**
 * Newest first, the way a browser draws it: the top of the list is where you
 * just were, and the current folder is the one wearing the check.
 *
 * Rendered only while the menu is open, and read straight from the store rather
 * than subscribed to — the stack cannot change underneath an open menu, and a
 * subscription here would re-render both carets on every tree delta.
 */
function HistoryMenu({ onClose }: { onClose(): void }) {
  const { nav, nodes } = useWorkspace.getState();

  const start = Math.max(0, nav.entries.length - HISTORY_LIMIT);
  const rows: { index: number; name: string }[] = [];
  for (let i = nav.entries.length - 1; i >= start; i--) {
    rows.push({ index: i, name: nodes[nav.entries[i].folderId]?.name ?? "Folder" });
  }

  return (
    <MenuList onClose={onClose}>
      <MenuLabel>History</MenuLabel>
      {rows.map((row) => (
        <MenuItem
          key={`${row.index}-${nav.entries[row.index].folderId}`}
          checked={row.index === nav.index}
          onSelect={() => jumpTo(row.index)}
        >
          {row.name}
        </MenuItem>
      ))}
    </MenuList>
  );
}

/**
 * Walk the stack to `index` one step at a time. Each call re-reads the store so
 * a step that refuses to move (a folder deleted out from under the entry) ends
 * the walk instead of spinning.
 */
function jumpTo(index: number): void {
  for (let guard = 0; guard < 64; guard++) {
    const current = useWorkspace.getState().nav.index;
    if (current === index) return;
    if (current > index) useWorkspace.getState().back();
    else useWorkspace.getState().forward();
    if (useWorkspace.getState().nav.index === current) return;
  }
}

export default NavCarets;
