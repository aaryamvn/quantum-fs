import { TriangleAlert } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { KeyboardEvent, ReactNode } from "react";

import { EmptyState } from "@/components/ui/EmptyState";
import { Shortcut } from "@/components/ui/Kbd";
import { Modal } from "@/components/ui/Modal";
import { Segmented } from "@/components/ui/Segmented";
import type { SegmentedOption } from "@/components/ui/Segmented";
import { useWorkspace } from "@/features/workspace/store";
import type { SearchHit } from "@/lib/backend";
import { matchesShortcut } from "@/lib/keys";
import { formatPath } from "@/lib/path";
import { describeQuery } from "@/lib/search";

import { addRecentSearch, readRecentSearches, removeRecentSearch } from "./recentSearches";
import { SEARCH_FILTERS, SearchFilters, searchFilterIndex } from "./SearchFilters";
import { SearchInput } from "./SearchInput";
import { SearchResults } from "./SearchResults";
import { useSearch } from "./useSearch";

/** Which vaults the field is looking at. */
type Scope = "vault" | "all";

const SCOPES: SegmentedOption<Scope>[] = [
  { value: "vault", label: "This vault" },
  { value: "all", label: "All vaults" },
];

/** What a row can be asked to do; the first two are also the mouse's two clicks. */
type ResultAction = "open" | "reveal" | "info" | "copy-path";

/**
 * A key hint in the footer: the cap, then what it does. Quiet enough to ignore,
 * present enough that nobody has to be told the modal is keyboard-driven.
 */
function Hint({ children, label }: { children: ReactNode; label: string }) {
  return (
    <span className="inline-flex items-center gap-[5px]">
      {children}
      <span>{label}</span>
    </span>
  );
}

/**
 * The body of ⌘K. Separate from {@link SearchModal} so every hook in it — the
 * query, the debounce, the request — exists only while the dialog is up: the
 * `Modal` renders its children only when open, so a closed palette costs one
 * store subscription and nothing else, and each open starts from a blank field
 * instead of last time's leftovers.
 */
function SearchPanel() {
  const vaultId = useWorkspace((s) => s.vaultId);
  const client = useWorkspace((s) => s.client);
  const navigateTo = useWorkspace((s) => s.navigateTo);
  const select = useWorkspace((s) => s.select);
  const openModal = useWorkspace((s) => s.openModal);
  const closeModal = useWorkspace((s) => s.closeModal);
  const toast = useWorkspace((s) => s.toast);

  const [raw, setRaw] = useState("");
  const [scope, setScope] = useState<Scope>("vault");
  const [category, setCategory] = useState<string | null>(null);
  const [kind, setKind] = useState<"all" | "folder" | "file">("all");
  const [active, setActive] = useState(0);
  const [recents, setRecents] = useState<string[]>(() => readRecentSearches());
  const field = useRef<HTMLInputElement | null>(null);

  /**
   * Every control in the panel hands the caret straight back to the field:
   * clicking a chip is a way of refining a query, not a way of leaving it, and
   * the next thing anyone does after either is type.
   */
  const refocus = (): void => field.current?.focus();

  const { hits, loading, parsed, error } = useSearch(raw, {
    vaultId: scope === "all" ? null : vaultId,
    category,
    kind,
  });

  // A new query means a new best answer, so the selection goes back to the top.
  // Clamping rather than resetting on `hits` keeps the highlight still while a
  // refresh lands under an unchanged query.
  useEffect(() => {
    setActive(0);
  }, [raw, category, kind, scope]);

  const activeIndex = hits.length === 0 ? -1 : Math.min(active, hits.length - 1);
  const current = activeIndex >= 0 ? hits[activeIndex] : null;
  const resting = raw.trim().length === 0;

  /** Only queries that were actually used are worth offering again. */
  const remember = (): void => {
    const query = raw.trim();
    if (query.length > 0) setRecents(addRecentSearch(query));
  };

  const copyPath = (hit: SearchHit): void => {
    const text = formatPath([hit.vaultName, ...hit.path, hit.node.name]);
    try {
      void navigator.clipboard
        .writeText(text)
        .then(() => toast("Path copied", "success"))
        .catch(() => toast("Couldn't copy the path", "error"));
    } catch {
      toast("Couldn't copy the path", "error");
    }
  };

  /**
   * What a result does when you pick it.
   *
   * Finder's rules, deliberately: opening a folder enters it and opening a file
   * hands it to the OS application, while Reveal is the one that takes you to
   * where the file lives and selects it. Opening a file needs no vault of its
   * own — the daemon can open a file in any vault this client belongs to — so a
   * cross-vault open never switches vaults. Everything that *is* a place, in a
   * vault the store is not holding, leaves as a `qfs:open-vault` event for the
   * shell to honor.
   */
  const run = (hit: SearchHit, action: ResultAction): void => {
    const { node } = hit;
    remember();

    if (action === "copy-path") {
      copyPath(hit);
      closeModal();
      return;
    }

    if (action === "open" && node.kind === "file") {
      if (client) {
        void client.openFile(node.vaultId, node.id).catch((error: unknown) => {
          const message = error instanceof Error ? error.message : String(error);
          toast(`Couldn't open ${node.name}: ${message}`, "error");
        });
      } else {
        // Closing on a pick that did nothing would read as the app losing the
        // keystroke, so the one reason it could not happen is said out loud.
        toast("Not connected to the daemon", "error");
      }
      closeModal();
      return;
    }

    if (node.vaultId !== vaultId) {
      const entering = action === "open" && node.kind === "folder";
      window.dispatchEvent(
        new CustomEvent("qfs:open-vault", {
          detail: {
            vaultId: node.vaultId,
            folderId: entering ? node.id : node.parentId,
            select: entering ? [] : [node.id],
          },
        }),
      );
      closeModal();
      return;
    }

    if (action === "info") {
      // Replaces this dialog rather than stacking on it: one modal at a time.
      openModal({ kind: "info", nodeId: node.id });
      return;
    }

    if (action === "open" && node.kind === "folder") {
      navigateTo(node.id);
      closeModal();
      return;
    }

    // Reveal: stand in the parent with the node selected. Navigation clears the
    // selection, so selecting has to come after it.
    if (node.parentId) navigateTo(node.parentId);
    select([node.id], { anchor: node.id, focus: node.id });
    closeModal();
  };

  /** Tab walks the chip row without ever taking focus off the field. */
  const cycleFilter = (step: number): void => {
    const index = searchFilterIndex(kind, category);
    const next = SEARCH_FILTERS[(index + step + SEARCH_FILTERS.length) % SEARCH_FILTERS.length];
    setKind(next.kind);
    setCategory(next.category);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>): void => {
    const total = hits.length;
    const clamped = Math.min(active, Math.max(0, total - 1));

    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (total > 0) setActive((clamped + 1) % total);
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      if (total > 0) setActive((clamped - 1 + total) % total);
      return;
    }
    if (e.key === "Home") {
      e.preventDefault();
      setActive(0);
      return;
    }
    if (e.key === "End") {
      e.preventDefault();
      setActive(Math.max(0, total - 1));
      return;
    }
    if (e.key === "Tab") {
      e.preventDefault();
      // The dialog's own Tab trap may already have moved focus to the close
      // button; the field is where every key in here belongs, so take it back.
      const field = e.currentTarget;
      cycleFilter(e.shiftKey ? -1 : 1);
      field.focus();
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      if (current) run(current, e.metaKey || e.ctrlKey ? "reveal" : "open");
      return;
    }
    if (matchesShortcut(e.nativeEvent, "mod+i")) {
      e.preventDefault();
      if (current) run(current, "info");
      return;
    }
    if (matchesShortcut(e.nativeEvent, "mod+shift+c")) {
      e.preventDefault();
      if (current) run(current, "copy-path");
    }
  };

  const described = describeQuery(parsed);
  const summary =
    resting && described.length === 0
      ? "Try ext:svg · type:image · by:maya · modified:7d"
      : `${hits.length} result${hits.length === 1 ? "" : "s"}${described ? ` · ${described}` : ""}`;

  return (
    <div
      data-testid="search-modal"
      aria-busy={loading}
      className="flex h-full min-h-0 flex-col"
    >
      {/* pr clears the dialog's own close button, which floats at 16px. */}
      <div className="flex shrink-0 items-center gap-[14px] border-b border-line pr-[52px] pl-[18px]">
        <SearchInput value={raw} onChange={setRaw} onKeyDown={onKeyDown} fieldRef={field} />
        <Segmented<Scope> value={scope} onChange={(next) => setScope(next)} options={SCOPES} />
      </div>

      <div className="shrink-0 border-b border-line pt-[10px]">
        <SearchFilters
          query={raw}
          onQuery={(next) => {
            setRaw(next);
            refocus();
          }}
          category={category}
          onCategory={setCategory}
          kind={kind}
          onKind={(next) => {
            setKind(next);
            refocus();
          }}
        />
      </div>

      {error ? (
        <div className="flex min-h-0 flex-1 items-center justify-center">
          <EmptyState
            icon={<TriangleAlert size={22} strokeWidth={1.5} aria-hidden />}
            title="Search failed"
            detail={error}
          />
        </div>
      ) : (
        <SearchResults
          hits={hits}
          activeIndex={activeIndex}
          onActivate={run}
          onHover={setActive}
          query={raw}
          loading={loading}
          groupByVault={scope === "all"}
          recents={recents}
          onRecent={(entry) => {
            setRaw(entry);
            refocus();
          }}
          onRemoveRecent={(entry) => setRecents(removeRecentSearch(entry))}
        />
      )}

      <div
        className="flex h-[38px] shrink-0 items-center justify-between gap-[16px]
          border-t border-line px-[16px] text-[11px] text-fg-3"
      >
        <p className="min-w-0 truncate">{summary}</p>
        <div className="flex shrink-0 items-center gap-[12px]">
          <Hint label="navigate">
            <Shortcut combo="up" />
            <Shortcut combo="down" />
          </Hint>
          <Hint label="open">
            <Shortcut combo="enter" />
          </Hint>
          <Hint label="reveal">
            <Shortcut combo="mod+enter" />
          </Hint>
          <Hint label="info">
            <Shortcut combo="mod+i" />
          </Hint>
          <Hint label="copy path">
            <Shortcut combo="mod+shift+c" />
          </Hint>
          <Hint label="close">
            <Shortcut combo="escape" />
          </Hint>
        </div>
      </div>
    </div>
  );
}

/**
 * ⌘K: one field that reaches every file in the vault, at any depth.
 *
 * It is mounted on the workspace and driven entirely by `modal.kind === "search"`,
 * so the top bar's search affordance, the shortcut and the `?ui=search` dev switch
 * all open the same surface through the same state.
 *
 * It borrows the app's dialog for its backdrop, focus trap, Escape handling and
 * entrance — a palette that invented its own would be the second implementation
 * of all four. The panel is the dialog's large stage, used whole rather than as a
 * card floating inside it, because `Modal` paints the surface itself.
 */
export function SearchModal() {
  const open = useWorkspace((s) => s.modal?.kind === "search");
  const closeModal = useWorkspace((s) => s.closeModal);

  return (
    <Modal open={open} onClose={closeModal} size="lg" padded={false}>
      <SearchPanel />
    </Modal>
  );
}
