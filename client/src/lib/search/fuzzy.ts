/**
 * Subsequence scorer for the search modal and command palette.
 *
 * The bar is the one every good palette sets: typing "pst" must surface
 * `poster.hdr` without the reader thinking about why. So a match is a plain
 * in-order subsequence (cheap, forgiving) and the ranking does the real work —
 * it rewards the three shapes a person actually means when they abbreviate: the
 * start of the name, the start of a word (so "pst" also finds
 * `pine-sunset-tree.png`, which is correct — those are its initials), and runs
 * of adjacent characters, while penalising the skipping and the sheer length
 * that make a match accidental.
 *
 * Hot path: this runs over every node in a vault on every keystroke, so there
 * is no regex here, no allocation per character, and one lowercase per string.
 */

export interface FuzzyResult {
  /** Higher is better. Integer, unbounded, only ever compared to other scores from this function. */
  score: number;
  /** Matched character runs as `[start, end)` index pairs into the original target. */
  matches: [number, number][];
}

/** Word breaks inside file names. Checked by lookup, never by regex. */
const SEPARATORS = " ._-/";

const SCORE_EXACT = 100;
const SCORE_PREFIX = 60;
const SCORE_BOUNDARY = 40;
const SCORE_RUN = 12;
const SCORE_AFTER_SEPARATOR = 6;
const PENALTY_SKIP = 2;
const PENALTY_PER_UNMATCHED = 1;
/** Gaps stop mattering past this many characters — otherwise long paths could never win. */
const MAX_SKIPPED = 20;

function isSeparator(ch: string): boolean {
  return SEPARATORS.indexOf(ch) !== -1;
}

function isUpper(ch: string): boolean {
  return ch >= "A" && ch <= "Z";
}

function isLowerOrDigit(ch: string): boolean {
  return (ch >= "a" && ch <= "z") || (ch >= "0" && ch <= "9");
}

/**
 * Score `target` against `query`, or `null` when it simply does not match.
 *
 * Matching is greedy leftmost: the first place each query character can land is
 * where it lands. That is not always the prettiest alignment, but it is linear,
 * and the boundary and run bonuses recover almost all of the ranking a full
 * search would buy at many times the cost. An empty query matches everything
 * with score 0, which lets callers skip a special case.
 */
export function fuzzyMatch(query: string, target: string): FuzzyResult | null {
  const q = query.toLowerCase();
  const t = target.toLowerCase();
  if (q.length === 0) return { score: 0, matches: [] };
  if (q.length > t.length) return null;

  const indices: number[] = [];
  let cursor = 0;
  for (let qi = 0; qi < q.length; qi++) {
    const wanted = q.charCodeAt(qi);
    let found = -1;
    while (cursor < t.length) {
      const hit = t.charCodeAt(cursor) === wanted;
      cursor++;
      if (hit) {
        found = cursor - 1;
        break;
      }
    }
    if (found === -1) return null;
    indices.push(found);
  }

  let score = 0;
  if (q === t) score += SCORE_EXACT;
  else if (t.startsWith(q)) score += SCORE_PREFIX;

  // Characters the match had to step over: the run-up plus every interior gap.
  let skipped = indices[0];

  for (let k = 0; k < indices.length; k++) {
    const i = indices[k];
    const prev = i > 0 ? target[i - 1] : "";
    const afterSeparator = prev !== "" && isSeparator(prev);
    const camelBoundary = prev !== "" && isLowerOrDigit(prev) && isUpper(target[i]);
    if (i === 0 || afterSeparator || camelBoundary) score += SCORE_BOUNDARY;
    if (afterSeparator) score += SCORE_AFTER_SEPARATOR;
    if (k > 0) {
      const gap = i - indices[k - 1] - 1;
      if (gap === 0) score += SCORE_RUN;
      else skipped += gap;
    }
  }

  score -= PENALTY_SKIP * Math.min(skipped, MAX_SKIPPED);
  score -= PENALTY_PER_UNMATCHED * (t.length - q.length);

  const matches: [number, number][] = [];
  for (const i of indices) {
    const last = matches[matches.length - 1];
    if (last && last[1] === i) last[1] = i + 1;
    else matches.push([i, i + 1]);
  }

  return { score, matches };
}

/**
 * Slice `text` into alternating plain and matched segments for rendering.
 *
 * Returned as data rather than markup so the row component decides how a hit
 * looks (a weight change, never a highlight block). Ranges are clamped and
 * de-overlapped here so a caller can hand over anything without guarding.
 */
export function highlightRanges(text: string, ranges: [number, number][]): { text: string; hit: boolean }[] {
  const segments: { text: string; hit: boolean }[] = [];
  const sorted = [...ranges].sort((a, b) => a[0] - b[0]);
  let cursor = 0;
  for (const [rawStart, rawEnd] of sorted) {
    const start = Math.max(cursor, Math.min(rawStart, text.length));
    const end = Math.max(start, Math.min(rawEnd, text.length));
    if (end <= start) continue;
    if (start > cursor) segments.push({ text: text.slice(cursor, start), hit: false });
    segments.push({ text: text.slice(start, end), hit: true });
    cursor = end;
  }
  if (cursor < text.length) segments.push({ text: text.slice(cursor), hit: false });
  return segments;
}
