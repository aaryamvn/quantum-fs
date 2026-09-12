/**
 * The tiny grammar the search field understands.
 *
 * Power users type filters inline rather than hunting for a menu, so the field
 * accepts `ext:`, `type:`, `is:`, `by:`, `in:` and `modified:` mixed freely with
 * free text. Two rules keep it from ever feeling brittle: anything unrecognized
 * falls back to free text (a colon in a file name must not eat the query), and
 * keys are case-insensitive. Parsing is pure and takes `now` so the relative
 * date filters are testable and screenshot-stable.
 */

export interface ParsedQuery {
  /** Everything that was not a filter, joined back together — what the fuzzy scorer sees. */
  text: string;
  exts: string[] | null;
  kinds: ("folder" | "file")[] | null;
  by: string | null;
  availability: "local" | "remote" | null;
  modifiedAfter: number | null;
  /**
   * The `modified:` token that produced {@link modifiedAfter} ("today", "7d", …).
   *
   * The summary reads this back instead of re-deriving a bucket from the instant:
   * `modified:today` is a calendar boundary, so measuring elapsed milliseconds
   * against it flips to "since yesterday" the moment the local clock passes noon.
   */
  modifiedBucket: string | null;
  inFolder: string | null;
  category: string | null;
}

const DAY = 86_400_000;

/** Icon categories the registry can report; anything else typed after `type:` is free text. */
const CATEGORIES = ["image", "video", "audio", "document", "code", "archive", "design", "data"];

const CATEGORY_LABELS: Record<string, string> = {
  image: "images",
  video: "videos",
  audio: "audio",
  document: "documents",
  code: "code",
  archive: "archives",
  design: "design files",
  data: "data files",
};

const RELATIVE_DAYS = /^(\d+)d$/;
const RELATIVE_YEARS = /^(\d+)y$/;

interface Token {
  /** Lowercased key before the first unquoted colon, or `null` for free text. */
  key: string | null;
  value: string;
}

/** Local midnight of the day `ts` falls in. */
function startOfDay(ts: number): number {
  const d = new Date(ts);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

/**
 * Split the raw string into tokens, honoring double quotes.
 *
 * A colon only opens a key when it is unquoted and nothing before it was
 * quoted, so `"my:file"` stays a phrase while `in:"Q4 Posters"` is a filter
 * with spaces in its value.
 */
function tokenize(raw: string): Token[] {
  const tokens: Token[] = [];
  let i = 0;
  while (i < raw.length) {
    while (i < raw.length && raw[i] === " ") i++;
    if (i >= raw.length) break;

    let key: string | null = null;
    let buffer = "";
    let quoted = false;
    let inQuote = false;
    while (i < raw.length && (inQuote || raw[i] !== " ")) {
      const ch = raw[i];
      if (ch === '"') {
        inQuote = !inQuote;
        quoted = true;
        i++;
        continue;
      }
      if (ch === ":" && !inQuote && !quoted && key === null && buffer.length > 0) {
        key = buffer.toLowerCase();
        buffer = "";
        i++;
        continue;
      }
      buffer += ch;
      i++;
    }
    if (key !== null || buffer.length > 0) tokens.push({ key, value: buffer });
  }
  return tokens;
}

/** Comma-separated filter values, trimmed and lowercased; empties dropped. */
function values(value: string): string[] {
  return value
    .split(",")
    .map((part) => part.trim().toLowerCase())
    .filter((part) => part.length > 0);
}

/**
 * `modified:` vocabulary → the instant a node must be newer than plus the bucket
 * that produced it, or `null` if unrecognized.
 *
 * The bucket travels with the instant because the two are not interchangeable:
 * "today" is a midnight boundary that is minutes old at 00:05 and 23 hours old at
 * 23:05, so only the token itself can be read back faithfully.
 */
function resolveModified(value: string, now: number): { after: number; bucket: string } | null {
  if (value === "today") return { after: startOfDay(now), bucket: "today" };
  if (value === "yesterday") return { after: startOfDay(now) - DAY, bucket: "yesterday" };
  const days = RELATIVE_DAYS.exec(value);
  if (days) return { after: now - Number(days[1]) * DAY, bucket: `${Number(days[1])}d` };
  const years = RELATIVE_YEARS.exec(value);
  if (years) return { after: now - Number(years[1]) * 365 * DAY, bucket: `${Number(years[1])}y` };
  return null;
}

/**
 * Parse a raw search string into filters plus the free text that remains.
 *
 * Every unknown key and every unknown value is put back into the text verbatim,
 * which is what makes the field safe to type into: a half-finished `mod:` or a
 * file literally called `type:draft` degrades to a normal name search instead of
 * silently filtering everything away.
 */
export function parseQuery(raw: string, now: number = Date.now()): ParsedQuery {
  const text: string[] = [];
  const exts: string[] = [];
  const kinds: ("folder" | "file")[] = [];
  let by: string | null = null;
  let availability: "local" | "remote" | null = null;
  let modifiedAfter: number | null = null;
  let modifiedBucket: string | null = null;
  let inFolder: string | null = null;
  let category: string | null = null;

  for (const token of tokenize(raw)) {
    const { key, value } = token;
    if (key === null) {
      if (value.length > 0) text.push(value);
      continue;
    }
    const literal = `${key}:${value}`;

    if (key === "ext") {
      const parsed = values(value).map((ext) => (ext.startsWith(".") ? ext.slice(1) : ext)).filter((ext) => ext.length > 0);
      if (parsed.length === 0) text.push(literal);
      else exts.push(...parsed);
      continue;
    }

    if (key === "type") {
      const match = values(value).find((candidate) => CATEGORIES.includes(candidate));
      if (match) category = match;
      else text.push(literal);
      continue;
    }

    if (key === "is" || key === "kind") {
      const parsed = values(value);
      let understood = parsed.length > 0;
      for (const item of parsed) {
        if (item === "folder" || item === "file") kinds.push(item);
        else if (key === "is" && (item === "local" || item === "remote")) availability = item;
        else understood = false;
      }
      if (!understood) text.push(literal);
      continue;
    }

    if (key === "by") {
      if (value.trim().length === 0) text.push(literal);
      else by = value.trim();
      continue;
    }

    if (key === "in") {
      if (value.trim().length === 0) text.push(literal);
      else inFolder = value.trim();
      continue;
    }

    if (key === "modified" || key === "mod") {
      const resolved = resolveModified(value.trim().toLowerCase(), now);
      if (resolved === null) {
        text.push(literal);
      } else {
        modifiedAfter = resolved.after;
        modifiedBucket = resolved.bucket;
      }
      continue;
    }

    text.push(literal);
  }

  return {
    text: text.join(" ").trim(),
    exts: exts.length > 0 ? Array.from(new Set(exts)) : null,
    kinds: kinds.length > 0 ? Array.from(new Set(kinds)) : null,
    by,
    availability,
    modifiedAfter,
    modifiedBucket,
    inFolder,
    category,
  };
}

/** The bucket the user typed, as prose: "modified today", "modified in the last 7 days". */
function describeBucket(bucket: string): string {
  if (bucket === "today") return "modified today";
  if (bucket === "yesterday") return "modified since yesterday";
  const days = RELATIVE_DAYS.exec(bucket);
  if (days) {
    const n = Number(days[1]);
    return n === 1 ? "modified in the last day" : `modified in the last ${n} days`;
  }
  const years = RELATIVE_YEARS.exec(bucket);
  if (years) {
    const n = Number(years[1]);
    return n === 1 ? "modified in the last year" : `modified in the last ${n} years`;
  }
  return `modified ${bucket}`;
}

/**
 * Fallback prose for a `modifiedAfter` that arrived without its bucket — a query
 * assembled in code rather than typed.
 *
 * Whole days are floored, not rounded, and the two calendar cases are answered by
 * comparing against local midnight, so an instant that *is* the start of today can
 * never be described as yesterday.
 */
function describeInstant(modifiedAfter: number, now: number): string {
  if (modifiedAfter >= startOfDay(now)) return "modified today";
  if (modifiedAfter >= startOfDay(now) - DAY) return "modified since yesterday";
  const days = Math.max(2, Math.floor((now - modifiedAfter) / DAY));
  if (days >= 360) {
    const years = Math.max(1, Math.round(days / 365));
    return years === 1 ? "modified in the last year" : `modified in the last ${years} years`;
  }
  return `modified in the last ${days} days`;
}

/**
 * A one-line, human summary of what is being filtered, for the results header.
 *
 * The point is that a filter typed as shorthand is always readable back: seeing
 * "images · modified in the last 7 days · by Maya" is how you notice you meant
 * `by:maria`. Free text is quoted first so the phrase you typed leads the line.
 */
export function describeQuery(q: ParsedQuery, now: number = Date.now()): string {
  const parts: string[] = [];
  if (q.text) parts.push(`“${q.text}”`);

  if (q.kinds) {
    const labels = q.kinds.map((kind) => (kind === "folder" ? "folders" : "files"));
    parts.push(labels.join(" and "));
  }
  if (q.category) parts.push(CATEGORY_LABELS[q.category] ?? q.category);
  if (q.exts) parts.push(`${q.exts.map((ext) => `.${ext}`).join(" or ")} files`);
  if (q.availability) parts.push(q.availability === "local" ? "on this device" : "remote only");
  if (q.modifiedBucket) parts.push(describeBucket(q.modifiedBucket));
  else if (q.modifiedAfter !== null) parts.push(describeInstant(q.modifiedAfter, now));
  if (q.inFolder) parts.push(`in ${q.inFolder}`);
  if (q.by) parts.push(`by ${q.by}`);

  return parts.join(" · ");
}
