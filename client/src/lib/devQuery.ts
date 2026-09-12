/**
 * Dev-only URL switches, parsed once at module load.
 *
 *   ?at=<ms>        freeze the splash timeline at this elapsed time (screenshot / review)
 *   ?splash=skip    start already settled
 *   ?field=<id>     color-field shader id (only one shader today)
 */
export interface DevQuery {
  /** Frozen timeline position in ms, or null when the splash should play. */
  at: number | null;
  /** Jump straight to the settled state. */
  skip: boolean;
  /** Raw `field` param, resolved by the shader registry. */
  field: string | null;
  /** OS-level reduced-motion preference. */
  reduced: boolean;
}

function parse(): DevQuery {
  if (typeof window === "undefined") {
    return { at: null, skip: false, field: null, reduced: false };
  }

  const params = new URLSearchParams(window.location.search);

  const rawAt = params.get("at");
  let at: number | null = null;
  if (rawAt !== null && rawAt.trim() !== "") {
    const parsed = Number.parseInt(rawAt, 10);
    if (Number.isFinite(parsed) && parsed >= 0) at = parsed;
  }

  const reduced =
    typeof window.matchMedia === "function"
      ? window.matchMedia("(prefers-reduced-motion: reduce)").matches
      : false;

  return {
    at,
    skip: params.get("splash") === "skip",
    field: params.get("field"),
    reduced,
  };
}

export const devQuery: DevQuery = parse();
