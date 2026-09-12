/**
 * Dev-only URL switches, parsed once at module load.
 *
 * These exist so a screenshot agent (or a human doing design QA) can land on any
 * state of the app in one navigation, without clicking through the real flow.
 *
 *   ?at=<ms>          freeze the splash timeline at this elapsed time
 *   ?splash=skip      start already settled (no splash)
 *   ?field=<id>       color-field shader id (only one shader today)
 *   ?vault=<vaultId>  open this vault's workspace directly (implies splash=skip)
 *   ?folder=<nodeId>  start inside this folder
 *   ?ui=<surface>     force a surface open: icons | search |
 *                     context:<nodeId> | settings:<tab> | info:<nodeId> |
 *                     history:<nodeId> | access:<nodeId>
 *   ?demo=off         no scripted peers at all
 *   ?demo=<ms>        scripted peers frozen at this elapsed time (default: "on")
 *   ?select=<ids>     preselect one or more nodes, comma separated
 *   ?onboard=1        mock profile starts unnamed, so the app opens on onboarding
 */
export interface DevQuery {
  /** Frozen timeline position in ms, or null when the splash should play. */
  at: number | null;
  /** Jump straight to the settled state. */
  skip: boolean;
  /** Raw `field` param, resolved by the shader registry. */
  field: string | null;
  /** Vault to open straight into, or null to stay on home. */
  vault: string | null;
  /** Folder node to open inside the vault, or null for the vault root. */
  folder: string | null;
  /** Surface to force open, e.g. `context:n_42`; null for the resting view. */
  ui: string | null;
  /** Scripted-peer demo: live ("on"), disabled ("off"), or frozen at N ms. */
  demo: "on" | "off" | number;
  /** Node ids to preselect, comma separated, or null for no selection. */
  select: string | null;
  /** Browser mock only: start with an unnamed profile, i.e. a first launch. */
  onboard: boolean;
  /** OS-level reduced-motion preference. */
  reduced: boolean;
}

function parse(): DevQuery {
  if (typeof window === "undefined") {
    return {
      at: null,
      skip: false,
      field: null,
      vault: null,
      folder: null,
      ui: null,
      demo: "on",
      select: null,
      onboard: false,
      reduced: false,
    };
  }

  const params = new URLSearchParams(window.location.search);

  const rawAt = params.get("at");
  let at: number | null = null;
  if (rawAt !== null && rawAt.trim() !== "") {
    const parsed = Number.parseInt(rawAt, 10);
    if (Number.isFinite(parsed) && parsed >= 0) at = parsed;
  }

  // A vault deep-link is never reached through the splash, so it skips it.
  const vault = nonEmpty(params.get("vault"));

  // `demo` is tri-state: absent/on = live, off = silent, a number = frozen.
  const rawDemo = params.get("demo");
  let demo: "on" | "off" | number = "on";
  if (rawDemo !== null && rawDemo.trim() !== "") {
    const token = rawDemo.trim();
    if (token === "off") {
      demo = "off";
    } else if (token !== "on") {
      const parsed = Number.parseInt(token, 10);
      if (Number.isFinite(parsed) && parsed >= 0) demo = parsed;
    }
  }

  const reduced =
    typeof window.matchMedia === "function"
      ? window.matchMedia("(prefers-reduced-motion: reduce)").matches
      : false;

  return {
    at,
    skip: params.get("splash") === "skip" || vault !== null,
    field: params.get("field"),
    vault,
    folder: nonEmpty(params.get("folder")),
    ui: nonEmpty(params.get("ui")),
    demo,
    select: nonEmpty(params.get("select")),
    // Any value but an explicit "0"/"false" turns it on: `?onboard` alone should work.
    onboard: (() => {
      const raw = params.get("onboard");
      if (raw === null) return false;
      const token = raw.trim().toLowerCase();
      return token !== "0" && token !== "false" && token !== "off";
    })(),
    reduced,
  };
}

/** `?vault=` is the same as no vault at all — treat blank params as absent. */
function nonEmpty(value: string | null): string | null {
  if (value === null) return null;
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}

export const devQuery: DevQuery = parse();
