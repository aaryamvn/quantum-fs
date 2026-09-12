import { useEffect, useState } from "react";
import { Search } from "lucide-react";

import { IconButton } from "@/components/ui/IconButton";
import { Shortcut } from "@/components/ui/Kbd";
import { formatShortcut } from "@/lib/keys";

import { useWorkspace } from "../store";

/** Below this window width the field gives up its 200px and becomes a glyph. */
const COLLAPSE_AT = 1100;

/**
 * Search, drawn as a field it is not.
 *
 * A button shaped like an input is the honest thing here: search opens a modal
 * with its own field and its own result list, so typing in the bar would mean
 * two fields and a handoff mid-word. The false field buys the affordance (people
 * aim at the box, not at a magnifier) without the lie of a caret, and the keycap
 * teaches the gesture that actually gets used after the first day.
 *
 * When the window is too narrow to give it 200px it collapses rather than
 * squeezing: a 90px search field reads as broken, a glyph does not.
 */
export function SearchTrigger() {
  const openModal = useWorkspace((s) => s.openModal);
  const narrow = useNarrow(COLLAPSE_AT);

  const open = () => openModal({ kind: "search" });

  if (narrow) {
    return (
      <span data-testid="search-trigger" className="inline-flex shrink-0">
        <IconButton
          icon={<Search size={16} strokeWidth={1.75} />}
          label="Search"
          shortcut={formatShortcut("mod+k")}
          size={28}
          onClick={open}
        />
      </span>
    );
  }

  return (
    <button
      type="button"
      data-testid="search-trigger"
      aria-label="Search"
      onClick={open}
      className="flex h-[28px] w-[200px] shrink-0 items-center gap-[6px] rounded-[8px] border
        border-line bg-white/[0.03] px-[8px] text-fg-3 transition-colors duration-[160ms]
        ease-[cubic-bezier(0.2,0.8,0.2,1)] hover:border-line-strong hover:bg-white/[0.06]"
    >
      <Search size={14} strokeWidth={1.75} className="shrink-0" aria-hidden />
      <span className="flex-1 text-left text-[12.5px] leading-none">Search</span>
      <Shortcut combo="mod+k" />
    </button>
  );
}

/**
 * True while the window is narrower than `px`.
 *
 * Keyed off the window rather than the header's own width: the header is elastic
 * (the sidebar and inspector open and close under it), so measuring it would
 * collapse the field when a pane opens, which is not what "narrow window" means.
 */
function useNarrow(px: number): boolean {
  const query = `(max-width: ${px - 1}px)`;
  const [narrow, setNarrow] = useState(
    () => typeof window !== "undefined" && window.matchMedia(query).matches,
  );

  useEffect(() => {
    if (typeof window === "undefined") return;
    const mq = window.matchMedia(query);
    const onChange = (e: MediaQueryListEvent) => setNarrow(e.matches);
    setNarrow(mq.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [query]);

  return narrow;
}

export default SearchTrigger;
