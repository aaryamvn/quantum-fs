import { Archive, Code2, File, FileText, Folder, Image, Music, Palette, Video, X } from "lucide-react";
import type { LucideIcon } from "lucide-react";

import { Chip } from "@/components/ui/Chip";
import { parseQuery } from "@/lib/search";

/** One chip of the filter row. `kind` and `category` are the two axes it sets at once. */
export interface SearchFilterOption {
  id: string;
  label: string;
  kind: "all" | "folder" | "file";
  /** Icon-registry category ("image", "code", …); null means "no category filter". */
  category: string | null;
  Icon: LucideIcon | null;
}

/**
 * The chip row, in reading order: the two structural filters first (a folder is
 * not a kind of file), then the file categories people actually hunt by.
 *
 * Exported because the modal cycles this list with Tab — the order the keyboard
 * walks and the order the eye walks have to be the same list, not two copies.
 */
export const SEARCH_FILTERS: SearchFilterOption[] = [
  { id: "all", label: "All", kind: "all", category: null, Icon: null },
  { id: "folders", label: "Folders", kind: "folder", category: null, Icon: Folder },
  { id: "files", label: "Files", kind: "file", category: null, Icon: File },
  { id: "images", label: "Images", kind: "all", category: "image", Icon: Image },
  { id: "video", label: "Video", kind: "all", category: "video", Icon: Video },
  { id: "audio", label: "Audio", kind: "all", category: "audio", Icon: Music },
  { id: "docs", label: "Docs", kind: "all", category: "document", Icon: FileText },
  { id: "code", label: "Code", kind: "all", category: "code", Icon: Code2 },
  { id: "archives", label: "Archives", kind: "all", category: "archive", Icon: Archive },
  { id: "design", label: "Design", kind: "all", category: "design", Icon: Palette },
];

/** Which chip the current (kind, category) pair is; 0 ("All") when nothing matches. */
export function searchFilterIndex(kind: "all" | "folder" | "file", category: string | null): number {
  const found = SEARCH_FILTERS.findIndex(
    (option) => option.kind === kind && option.category === category,
  );
  return found === -1 ? 0 : found;
}

/**
 * Every filter key the grammar understands, with quoted values kept whole.
 *
 * Removing a chip edits the string the user typed rather than re-serialising the
 * parse, because the string is theirs: `in:"Q4 Posters"` has to come back out
 * exactly as it went in, minus the one token that was removed.
 */
const FILTER_TOKEN = /(^|\s)(ext|type|is|kind|by|in|modified|mod):("[^"]*"|\S*)/gi;

function dropToken(raw: string, keys: string[], values: string[] | null): string {
  return raw
    .replace(FILTER_TOKEN, (match: string, _lead: string, key: string, value: string) => {
      if (!keys.includes(key.toLowerCase())) return match;
      if (values) {
        const parts = value
          .replace(/"/g, "")
          .toLowerCase()
          .split(",")
          .map((part) => part.trim());
        // `is:` carries both kinds and availability, so a chip only claims the
        // tokens whose value it is actually showing.
        if (!parts.some((part) => values.includes(part))) return match;
      }
      return " ";
    })
    .replace(/\s{2,}/g, " ")
    .trim();
}

/** A parsed filter, as the row draws it: what it says, and what removing it deletes. */
interface TokenChip {
  id: string;
  label: string;
  keys: string[];
  values: string[] | null;
}

function tokenChips(raw: string): TokenChip[] {
  const q = parseQuery(raw);
  const chips: TokenChip[] = [];
  if (q.exts) {
    chips.push({ id: "ext", label: `ext: ${q.exts.join(", ")}`, keys: ["ext"], values: q.exts });
  }
  if (q.category) {
    chips.push({ id: "type", label: `type: ${q.category}`, keys: ["type"], values: [q.category] });
  }
  if (q.kinds) {
    chips.push({ id: "kind", label: `is: ${q.kinds.join(", ")}`, keys: ["is", "kind"], values: q.kinds });
  }
  if (q.availability) {
    chips.push({
      id: "availability",
      label: `is: ${q.availability}`,
      keys: ["is"],
      values: [q.availability],
    });
  }
  if (q.modifiedBucket) {
    chips.push({
      id: "modified",
      label: `modified: ${q.modifiedBucket}`,
      keys: ["modified", "mod"],
      values: null,
    });
  }
  if (q.inFolder) {
    chips.push({ id: "in", label: `in: ${q.inFolder}`, keys: ["in"], values: null });
  }
  if (q.by) {
    chips.push({ id: "by", label: `by: ${q.by}`, keys: ["by"], values: null });
  }
  return chips;
}

export interface SearchFiltersProps {
  /** The raw string in the field — a removed chip edits it. */
  query: string;
  onQuery(next: string): void;
  category: string | null;
  onCategory(next: string | null): void;
  kind: "all" | "folder" | "file";
  onKind(next: "all" | "folder" | "file"): void;
}

/**
 * The band under the field: what you can narrow by, and what you already have.
 *
 * The two halves are the same language read from opposite ends. On the left, ten
 * chips are the discoverable form of `type:` and `is:` — you click Images once
 * and never have to learn the token. On the right, anything typed as a token is
 * echoed back as a chip you can delete, so a filter can always be undone by the
 * surface that is showing it, not only by editing the string that produced it.
 *
 * Chips here are buttons, not {@link Chip}s: that primitive is display-only by
 * design. Only the echoed tokens — which say something rather than do something
 * until you press their × — are real Chips.
 */
export function SearchFilters({
  query,
  onQuery,
  category,
  onCategory,
  kind,
  onKind,
}: SearchFiltersProps) {
  const activeIndex = searchFilterIndex(kind, category);
  const chips = tokenChips(query);

  return (
    <div
      data-testid="search-filters"
      className="flex flex-wrap items-center gap-[6px] px-[16px] pb-[10px]"
    >
      {SEARCH_FILTERS.map((option, index) => {
        const active = index === activeIndex;
        const { Icon } = option;
        return (
          <button
            key={option.id}
            type="button"
            aria-pressed={active}
            onClick={() => {
              onKind(option.kind);
              onCategory(option.category);
            }}
            className={`inline-flex h-[26px] shrink-0 items-center gap-[5px] rounded-full border px-[10px]
              text-[12.5px] leading-none whitespace-nowrap
              transition-colors duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)]
              ${
                active
                  ? "border-violet/40 bg-violet/20 text-fg"
                  : "border-line bg-white/[0.03] text-fg-3 hover:border-line-strong hover:text-fg"
              }`}
          >
            {Icon ? <Icon size={13} strokeWidth={1.75} aria-hidden /> : null}
            {option.label}
          </button>
        );
      })}

      {chips.length > 0 ? (
        <span aria-hidden className="mx-[4px] h-[16px] w-px shrink-0 bg-line-strong" />
      ) : null}

      {chips.map((chip) => (
        <button
          key={chip.id}
          type="button"
          aria-label={`Remove filter ${chip.label}`}
          onClick={() => onQuery(dropToken(query, chip.keys, chip.values))}
          className="rounded-full transition-opacity duration-[160ms]
            ease-[cubic-bezier(0.2,0.8,0.2,1)] hover:opacity-70"
        >
          <Chip tone="violet">
            {chip.label}
            <X size={11} strokeWidth={2} aria-hidden />
          </Chip>
        </button>
      ))}
    </div>
  );
}
