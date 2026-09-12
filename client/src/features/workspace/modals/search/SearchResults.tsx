import { Clock, SearchX, X } from "lucide-react";
import { motion, useReducedMotion } from "motion/react";
import { useEffect, useRef } from "react";
import type { MouseEvent } from "react";

import { FileIcon, FolderIcon } from "@/components/icons";
import { Chip } from "@/components/ui/Chip";
import { EmptyState } from "@/components/ui/EmptyState";
import { EASE } from "@/features/workspace/layout";
import type { SearchHit } from "@/lib/backend";
import { formatBytes } from "@/lib/format";
import { formatPath } from "@/lib/path";
import { highlightRanges } from "@/lib/search";
import { formatRelative } from "@/lib/time";

/** Icon size inside a 46px row: big enough to read the file type, not a thumbnail. */
const ROW_ICON = 28;

interface ResultRowProps {
  hit: SearchHit;
  /** Position in the flat hit list — what the keyboard moves through. */
  index: number;
  active: boolean;
  /** Prefix the trail with the vault name (searching every vault). */
  showVault: boolean;
  onActivate(hit: SearchHit, action: "open" | "reveal"): void;
  onHover(index: number): void;
}

/**
 * One result. Two lines in 46px: what it is called, and where it lives — the
 * second is not decoration here, it is the answer to "which of the four files
 * called poster.hdr is this one".
 */
function ResultRow({ hit, index, active, showVault, onActivate, onHover }: ResultRowProps) {
  const { node } = hit;
  const segments = highlightRanges(node.name, hit.matches);
  // With no query every name is one unmatched run, so dimming it would dim the
  // whole list; the step back only exists to make a real match stand out.
  const dim = hit.matches.length > 0;
  const trail = showVault || hit.path.length === 0 ? [hit.vaultName, ...hit.path] : hit.path;

  return (
    <button
      type="button"
      data-node-id={node.id}
      data-index={index}
      aria-current={active || undefined}
      // Move, not enter: a pointer resting where the list scrolls under it must
      // not steal the selection from the arrow keys.
      onMouseMove={() => onHover(index)}
      onClick={(e: MouseEvent) => onActivate(hit, e.metaKey || e.ctrlKey ? "reveal" : "open")}
      className={`flex h-[46px] w-full items-center gap-[12px] rounded-[9px] px-[12px] text-left
        transition-colors duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)]
        ${active ? "bg-white/[0.08]" : "hover:bg-white/[0.04]"}`}
    >
      <span className="grid shrink-0 place-items-center" style={{ width: ROW_ICON, height: ROW_ICON }}>
        {node.kind === "folder" ? (
          <FolderIcon color={node.color ?? "graphite"} size={ROW_ICON} />
        ) : (
          <FileIcon name={node.name} size={ROW_ICON} />
        )}
      </span>

      <span className="flex min-w-0 flex-1 flex-col justify-center gap-[1px]">
        <span className={`truncate text-[15px] leading-[18px] ${dim ? "text-fg-2" : "text-fg"}`}>
          {segments.map((segment, i) =>
            segment.hit ? (
              <span key={i} className="text-fg underline decoration-violet/80 underline-offset-2">
                {segment.text}
              </span>
            ) : (
              <span key={i}>{segment.text}</span>
            ),
          )}
        </span>
        <span className="truncate text-[11.5px] leading-[15px] text-fg-3">{formatPath(trail)}</span>
      </span>

      <span className="flex shrink-0 items-center gap-[8px] text-[11px] whitespace-nowrap text-fg-3">
        {node.availability === "remote" ? <Chip size="xs">Not on this Mac</Chip> : null}
        <span className="tabular-nums">{formatBytes(node.sizeBytes)}</span>
        <span className="tabular-nums">{formatRelative(node.modifiedAt)}</span>
      </span>
    </button>
  );
}

/**
 * A section label; the only thing in the list that is not a control. Sentence
 * case at the app's caption size — "Recent searches", not a row of small caps.
 */
function SectionLabel({ children }: { children: string }) {
  return (
    <p className="px-[12px] pt-[16px] pb-[6px] text-[12.5px] leading-[16px] text-fg-3">
      {children}
    </p>
  );
}

export interface SearchResultsProps {
  hits: SearchHit[];
  /** Index into `hits`; -1 when there is nothing to activate. */
  activeIndex: number;
  onActivate(hit: SearchHit, action: "open" | "reveal"): void;
  onHover(index: number): void;
  /** The raw query. Empty switches the pane to its resting state. */
  query: string;
  /** A request is out: hold the "no results" line back rather than flashing it. */
  loading?: boolean;
  /** Searching every vault: rows carry their vault name and the list breaks by vault. */
  groupByVault?: boolean;
  recents?: string[];
  onRecent?(query: string): void;
  onRemoveRecent?(query: string): void;
}

/**
 * The list under the field: recent searches and the most recently touched files
 * when nothing is typed, ranked hits once something is.
 *
 * An empty search field is not an empty pane. What you did last and what changed
 * last are the two things that make ⌘K useful before you have typed anything, so
 * the resting state is a destination rather than a prompt.
 *
 * Grouping (all-vaults scope) breaks the list wherever the vault changes rather
 * than sorting by vault: the ranking is the order, and the keyboard walks the
 * list in exactly the order the eye does. A vault can therefore head two
 * sections, which is the honest picture of interleaved relevance.
 */
export function SearchResults({
  hits,
  activeIndex,
  onActivate,
  onHover,
  query,
  loading = false,
  groupByVault = false,
  recents = [],
  onRecent,
  onRemoveRecent,
}: SearchResultsProps) {
  const reduced = useReducedMotion() ?? false;
  const root = useRef<HTMLDivElement | null>(null);
  const resting = query.trim().length === 0;

  // Keep the keyboard's selection on screen. `nearest` scrolls by the minimum
  // needed, so arrowing down a long list creeps instead of jumping by a page.
  useEffect(() => {
    if (activeIndex < 0) return;
    const row = root.current?.querySelector<HTMLElement>(`[data-index="${activeIndex}"]`);
    row?.scrollIntoView({ block: "nearest" });
  }, [activeIndex]);

  const rows = hits.map((hit, index) => {
    const previous = index > 0 ? hits[index - 1] : null;
    const heading =
      groupByVault && (previous === null || previous.vaultName !== hit.vaultName)
        ? hit.vaultName
        : null;
    return (
      <div key={`${hit.node.vaultId}:${hit.node.id}`}>
        {heading ? <SectionLabel>{heading}</SectionLabel> : null}
        <ResultRow
          hit={hit}
          index={index}
          active={index === activeIndex}
          showVault={groupByVault}
          onActivate={onActivate}
          onHover={onHover}
        />
      </div>
    );
  });

  const fade = {
    initial: reduced ? false : { opacity: 0 },
    animate: { opacity: 1 },
    transition: { duration: reduced ? 0 : 0.14, ease: EASE },
  };

  // The list is sized between two bounds rather than to whatever is left over:
  // 120px so it never collapses to a sliver on a short window, 60vh so it can
  // never push the footer of key hints off the bottom of the dialog.
  return (
    <div
      ref={root}
      id="search-results"
      data-testid="search-results"
      aria-label="Search results"
      className="scroll-thin max-h-[60vh] min-h-[120px] flex-1 overflow-y-auto px-[8px] pb-[8px]"
    >
      {resting ? (
        <motion.div key="resting" {...fade}>
          {recents.length > 0 ? (
            <>
              <SectionLabel>Recent searches</SectionLabel>
              {recents.map((entry) => (
                <div
                  key={entry}
                  className="group flex h-[34px] items-center rounded-[9px]
                    transition-colors duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)]
                    hover:bg-white/[0.04]"
                >
                  <button
                    type="button"
                    onClick={() => onRecent?.(entry)}
                    className="flex h-full min-w-0 flex-1 items-center gap-[10px] px-[12px] text-left"
                  >
                    <Clock size={14} strokeWidth={1.75} className="shrink-0 text-fg-3" aria-hidden />
                    <span className="truncate text-[13px] leading-[16px] text-fg-2">{entry}</span>
                  </button>
                  <button
                    type="button"
                    aria-label={`Forget “${entry}”`}
                    onClick={() => onRemoveRecent?.(entry)}
                    className="mr-[8px] grid h-[22px] w-[22px] shrink-0 place-items-center rounded-[6px]
                      text-fg-3 opacity-0 transition-colors duration-[160ms]
                      ease-[cubic-bezier(0.2,0.8,0.2,1)] group-hover:opacity-100
                      hover:bg-white/[0.06] hover:text-fg"
                  >
                    <X size={13} strokeWidth={1.75} aria-hidden />
                  </button>
                </div>
              ))}
            </>
          ) : null}

          {hits.length > 0 ? (
            <>
              <SectionLabel>Recently modified</SectionLabel>
              {rows}
            </>
          ) : null}
        </motion.div>
      ) : (
        <motion.div key="results" {...fade}>
          {hits.length > 0 ? (
            rows
          ) : loading ? null : (
            <EmptyState
              className="pt-[64px]"
              icon={<SearchX size={22} strokeWidth={1.5} aria-hidden />}
              title="No matches"
              detail="Try fewer words, or drop a filter chip."
            />
          )}
        </motion.div>
      )}
    </div>
  );
}
