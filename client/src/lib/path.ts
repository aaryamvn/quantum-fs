/**
 * Path, naming and rename helpers for the vault tree.
 *
 * These are the rules Finder teaches people: an extension is not part of the
 * name you edit, a duplicate is called "copy", and a new folder is "untitled
 * folder". Getting them right is most of what makes a file surface feel native,
 * so they live here as pure functions with no backend types attached — the node
 * shape below is structural, so the mock, the Tauri client and the tests all
 * satisfy it without importing each other.
 */

/** The minimum a node must expose to be walked as a tree. Any richer node satisfies it. */
export interface PathNode {
  id: string;
  parentId: string | null;
  name: string;
  kind: "folder" | "file";
}

/** Extensions whose last two segments belong together — splitting them loses meaning. */
const COMPOUND_EXTENSIONS = ["tar.gz", "tar.bz2", "tar.xz", "d.ts", "min.js", "min.css", "min.map"];

const MAX_NAME_LENGTH = 255;

/** Characters that cannot survive a round trip through a real file system. */
const ILLEGAL_NAME_CHARS = ["/", ":", "\\"];

/**
 * The chain from the root down to `id`, inclusive of both ends.
 *
 * Walks upward and reverses, which is the only direction the tree is linked.
 * A dangling `parentId` ends the walk rather than throwing: a half-applied
 * remote delta should render a short breadcrumb, not a blank screen. A cycle
 * (which a buggy move could create) is broken by the visited set.
 */
export function pathOf<T extends PathNode>(nodes: Record<string, T>, id: string): T[] {
  const chain: T[] = [];
  const seen = new Set<string>();
  let current: T | undefined = nodes[id];
  while (current && !seen.has(current.id)) {
    seen.add(current.id);
    chain.push(current);
    current = current.parentId ? nodes[current.parentId] : undefined;
  }
  chain.reverse();
  return chain;
}

/**
 * True when `ancestorId` is a *proper* ancestor of `id`.
 *
 * The guard behind every move and drop: a folder may never be dropped into its
 * own subtree, and a node is not its own ancestor.
 */
export function isDescendant(nodes: Record<string, PathNode>, id: string, ancestorId: string): boolean {
  if (id === ancestorId) return false;
  const seen = new Set<string>([id]);
  let parentId = nodes[id]?.parentId ?? null;
  while (parentId && !seen.has(parentId)) {
    if (parentId === ancestorId) return true;
    seen.add(parentId);
    parentId = nodes[parentId]?.parentId ?? null;
  }
  return false;
}

/**
 * Split a file name into the part a rename should edit and its extension (no dot).
 *
 * Three cases the naive "last dot" rule gets wrong: `archive.tar.gz` is one
 * archive, not a `.gz`; `types.d.ts` is a declaration file; and `.env` is a
 * dotfile whose leading dot is its name, not an extension.
 */
export function splitName(name: string): { base: string; ext: string } {
  const lower = name.toLowerCase();
  for (const compound of COMPOUND_EXTENSIONS) {
    const cut = name.length - compound.length - 1;
    if (cut > 0 && lower.endsWith(`.${compound}`)) {
      return { base: name.slice(0, cut), ext: name.slice(cut + 1) };
    }
  }
  const dot = name.lastIndexOf(".");
  if (dot <= 0 || dot === name.length - 1) return { base: name, ext: "" };
  return { base: name.slice(0, dot), ext: name.slice(dot + 1) };
}

/** Inverse of {@link splitName}; an empty extension joins to nothing, never a trailing dot. */
export function joinName(base: string, ext: string): string {
  return ext ? `${base}.${ext}` : base;
}

/** Matches a base that already carries a copy suffix, so duplicating a duplicate counts up. */
const COPY_SUFFIX = /^(.*?)\s+copy(?:\s+(\d+))?$/i;

/**
 * A name that does not collide, using Finder's scheme.
 *
 * Comparison is case-insensitive because the file systems we target are: a
 * vault holding `Poster.png` must not accept `poster.png`. The suffix goes on
 * the base, never after the extension ("poster copy.hdr"), and duplicating
 * something already called "… copy" increments instead of stacking suffixes.
 */
export function uniqueName(existing: Iterable<string>, desired: string): string {
  const taken = new Set<string>();
  for (const name of existing) taken.add(name.toLowerCase());
  if (!taken.has(desired.toLowerCase())) return desired;

  const { base, ext } = splitName(desired);
  const suffix = COPY_SUFFIX.exec(base);
  const root = suffix ? suffix[1] : base;
  let n = suffix ? (suffix[2] ? Number(suffix[2]) : 1) : 0;

  for (;;) {
    n += 1;
    const candidate = joinName(n === 1 ? `${root} copy` : `${root} copy ${n}`, ext);
    if (!taken.has(candidate.toLowerCase())) return candidate;
  }
}

/**
 * The name a freshly created node gets: "untitled folder", then "untitled folder 2".
 *
 * New things start unnamed and selected for rename, so the placeholder only has
 * to be unmistakable and unique — it is usually replaced within a second.
 */
export function nextUntitled(existing: Iterable<string>, kind: "folder" | "file"): string {
  const taken = new Set<string>();
  for (const name of existing) taken.add(name.toLowerCase());

  const base = kind === "folder" ? "untitled folder" : "untitled";
  const ext = kind === "folder" ? "" : "txt";
  for (let n = 1; ; n++) {
    const candidate = joinName(n === 1 ? base : `${base} ${n}`, ext);
    if (!taken.has(candidate.toLowerCase())) return candidate;
  }
}

/**
 * The message to show under a rename field, or `null` when the name is fine.
 *
 * Returns prose rather than a code because it is rendered verbatim; the caller
 * only has to decide whether a string came back. Leading dots stay legal —
 * `.env` and `.gitignore` are real files people keep in vaults.
 */
export function validateName(name: string): string | null {
  if (name.trim().length === 0) return "Name can't be empty";
  for (const char of ILLEGAL_NAME_CHARS) {
    if (name.includes(char)) return "Names can't contain / : or \\";
  }
  if (name.length > MAX_NAME_LENGTH) return "Name is too long";
  return null;
}

/**
 * What a rename field should pre-select, as `[start, end)`.
 *
 * Finder selects the whole name of a folder but stops before a file's
 * extension, so typing immediately replaces the part you meant and `.png`
 * survives. Dotfiles and extension-less names select whole, since their base
 * *is* the whole name.
 */
export function selectionRangeForRename(name: string, kind: "folder" | "file"): [number, number] {
  if (kind === "folder") return [0, name.length];
  return [0, splitName(name).base.length];
}

/** Breadcrumb text for one line: "Designs › Q4 › Posters". */
export function formatPath(names: string[]): string {
  return names.join(" › ");
}
