/**
 * The last handful of things you searched for, kept across sessions.
 *
 * Recent searches are a convenience, never data: they live in `localStorage`
 * rather than in the vault because they are about this person on this machine,
 * and a query that named a file someone has since deleted must not travel to
 * the other members. Every access is wrapped — Safari in private mode throws on
 * `localStorage` access itself, and an empty list is always a correct answer.
 *
 * Each function returns the resulting list so a caller can drive React state
 * from the write without a second read.
 */

const KEY = "qfs.recentSearches";

/** Eight fits the empty modal without scrolling and is more than anyone re-runs. */
const MAX = 8;

function sanitize(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const out: string[] = [];
  for (const entry of value) {
    if (typeof entry !== "string") continue;
    const query = entry.trim();
    if (query.length > 0) out.push(query);
    if (out.length === MAX) break;
  }
  return out;
}

function write(list: string[]): string[] {
  try {
    localStorage.setItem(KEY, JSON.stringify(list));
  } catch {
    // Storage disabled or full: the list still stands for this session.
  }
  return list;
}

/** Newest first. Anything unparseable is treated as "nothing remembered yet". */
export function readRecentSearches(): string[] {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return [];
    return sanitize(JSON.parse(raw) as unknown);
  } catch {
    return [];
  }
}

/**
 * Record a query as the most recent one.
 *
 * Matching is case-insensitive, so re-running a search with different casing
 * moves the existing entry to the top instead of stacking a near-duplicate.
 */
export function addRecentSearch(query: string): string[] {
  const trimmed = query.trim();
  if (trimmed.length === 0) return readRecentSearches();
  const lower = trimmed.toLowerCase();
  const rest = readRecentSearches().filter((entry) => entry.toLowerCase() !== lower);
  return write([trimmed, ...rest].slice(0, MAX));
}

export function removeRecentSearch(query: string): string[] {
  const lower = query.trim().toLowerCase();
  return write(readRecentSearches().filter((entry) => entry.toLowerCase() !== lower));
}

export function clearRecentSearches(): string[] {
  return write([]);
}
