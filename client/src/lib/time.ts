/**
 * Time formatting for the vault workspace.
 *
 * Everything here is a pure function of `(at, now)` so a screenshot taken with a
 * frozen clock always renders the same words. Formatting goes through `Intl`
 * pinned to `en-GB` rather than the host locale: the workspace shows day-month
 * order and a 24h clock everywhere (list columns, inspector, history), and a
 * column that silently flips to `9/12/2026` on another machine is a bug, not a
 * localisation. No date library — the whole surface is the five functions below.
 */

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** Below this, "now" and "a moment ago" are the same thing to a reader. */
const JUST_NOW = 45_000;

const monthShort = new Intl.DateTimeFormat("en-GB", { month: "short" });
const weekdayName = new Intl.DateTimeFormat("en-GB", { weekday: "long" });
const clock = new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });

/**
 * Three-letter month.
 *
 * Current ICU gives en-GB "Sept" for September — the one month out of twelve
 * that is four letters — which makes a date column jitter by a character. The
 * slice pins every month to three, and only September is ever touched.
 */
function month(at: number): string {
  return monthShort.format(at).slice(0, 3);
}

/** "12 Sep". Day and year are plain numbers, so only the month needs Intl. */
function dayMonth(at: number): string {
  return `${new Date(at).getDate()} ${month(at)}`;
}

/** "12 Sep 2026". */
function dayMonthYear(at: number): string {
  return `${dayMonth(at)} ${new Date(at).getFullYear()}`;
}

/** Local midnight of the day `ts` falls in — calendar comparisons, not elapsed time. */
function startOfDay(ts: number): number {
  const d = new Date(ts);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

/**
 * Whole calendar days between two instants (negative = `at` is in the past).
 * Rounds because a DST boundary makes a "day" 23 or 25 hours long.
 */
function dayDelta(at: number, now: number): number {
  return Math.round((startOfDay(at) - startOfDay(now)) / DAY);
}

function sameYear(at: number, now: number): boolean {
  return new Date(at).getFullYear() === new Date(now).getFullYear();
}

/**
 * Short, glanceable age for list rows and activity lines.
 *
 * The ladder tightens as things get closer, because that is where precision is
 * worth pixels: seconds collapse to "just now", then minutes, then hours for the
 * first day, then names ("Yesterday", "Tuesday") for the last week, and finally
 * a plain date. The year only appears once it differs from `now`, so the common
 * case stays two words wide. Future instants mirror the same ladder ("in 5 min")
 * — clocks between peers drift, and a negative age must never render as a bug.
 */
export function formatRelative(at: number, now: number = Date.now()): string {
  const diff = now - at;
  const abs = Math.abs(diff);
  if (abs < JUST_NOW) return "just now";

  const future = diff < 0;
  if (abs < HOUR) {
    const mins = Math.max(1, Math.round(abs / MINUTE));
    return future ? `in ${mins} min` : `${mins} min ago`;
  }
  if (abs < DAY) {
    const hrs = Math.max(1, Math.round(abs / HOUR));
    return future ? `in ${hrs} hr` : `${hrs} hr ago`;
  }

  const days = dayDelta(at, now);
  if (days === -1) return "Yesterday";
  if (days === 1) return "Tomorrow";
  if (days > -7 && days < 7) return weekdayName.format(at);
  return sameYear(at, now) ? dayMonth(at) : dayMonthYear(at);
}

/** Absolute stamp for inspectors and tooltips: "12 Sep 2026, 14:02". No seconds — nobody reads them. */
export function formatDateTime(at: number): string {
  return `${dayMonthYear(at)}, ${clock.format(at)}`;
}

/** Date alone: "12 Sep 2026". */
export function formatDate(at: number): string {
  return dayMonthYear(at);
}

/**
 * Sticky section header for grouped history.
 *
 * The last two days get their familiar names, the rest of the week gets a
 * weekday plus the date (so "Tuesday" is never ambiguous once you scroll past
 * seven days), and anything older is a full date with its year.
 */
export function formatDayHeading(at: number, now: number = Date.now()): string {
  const days = dayDelta(at, now);
  if (days === 0) return "Today";
  if (days === -1) return "Yesterday";
  if (days === 1) return "Tomorrow";
  if (days > -7 && days < 7) return `${weekdayName.format(at)} ${dayMonth(at)}`;
  return dayMonthYear(at);
}
