#!/usr/bin/env node
// Headless screenshot CLI for the quantam-fs client dev server.
//
// Standalone so many agents can capture the running app in parallel without
// sharing a browser. Uses the system Google Chrome via Playwright's
// `channel: "chrome"` (falls back to the bundled chromium if that fails).
//
// Usage:
//   node scripts/shoot.mjs --url <URL> --out <PNG path>
//        [--viewport 1280x800] [--w 1280] [--h 800] [--dpr 1] [--wait 700]
//        [--hover <css>] [--focus <css>] [--click <css>] [--reduced]
//        [--steps '<json array>'] [--settle 250] [--strict]
//        [--clip "x,y,w,h"] [--full]
//
// --steps is an ordered interaction script, run AFTER --hover/--focus/--click,
// so a single invocation can reach deep UI states (context menu -> submenu,
// rename-in-place, drag a file onto a folder). Each entry is one of:
//   {"action":"click","selector":"css","button":"left|right",
//    "clickCount":1|2,"modifiers":["Shift","Meta"]}
//   {"action":"dblclick","selector":"css"}
//   {"action":"rightclick","selector":"css"}
//   {"action":"hover","selector":"css"}
//   {"action":"press","key":"Meta+K"}            (Playwright key syntax)
//   {"action":"type","text":"hello"}
//   {"action":"drag","from":"css","to":"css","steps":12}
//   {"action":"dragTo","from":"css","x":123,"y":456,"steps":12}
//   {"action":"wait","ms":300}
//   {"action":"scroll","selector":"css","dy":400}
//   {"action":"eval","js":"window.__qfs && window.__qfs.seek(12000)"}
// Every step is followed by a settle pause (--settle, default 250ms). A step
// that throws is recorded in "stepErrors" and the sequence continues, so one
// missing selector never costs the whole capture — unless --strict, which
// stops at the first failure (the screenshot is still taken).
//
// Prints ONE JSON line to stdout. Exit 0 on success (even with console
// errors — the caller inspects the JSON); exit 1 only if navigation or the
// screenshot itself fails.

import { mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { chromium } from "playwright";

const LAUNCH_ARGS = [
  "--enable-unsafe-swiftshader",
  "--use-gl=angle",
  "--use-angle=swiftshader",
  "--ignore-gpu-blocklist",
  "--hide-scrollbars",
];

const FIELD_POLL_TIMEOUT_MS = 3000;
const FIELD_POLL_INTERVAL_MS = 100;
// Interactions run after load/fonts/field/--wait, so the target should already
// exist; cap the wait so a missing selector fails fast instead of stalling on
// Playwright's 30s default.
const INTERACTION_TIMEOUT_MS = 5000;
const HOVER_SETTLE_MS = 400;
const FOCUS_SETTLE_MS = 200;
const CLICK_SETTLE_MS = 400;
const STEP_SETTLE_MS = 250;
const DRAG_STEPS = 12;
const TYPE_DELAY_MS = 12;

const USAGE =
  "usage: node scripts/shoot.mjs --url <URL> --out <PNG path> " +
  "[--viewport 1280x800] [--w 1280] [--h 800] [--dpr 1] [--wait 700] " +
  "[--hover <css>] [--focus <css>] [--click <css>] [--reduced] " +
  "[--steps '<json array>'] [--settle 250] [--strict] " +
  '[--clip "x,y,w,h"] [--full]\n' +
  "\n" +
  "--steps actions: click | dblclick | rightclick | hover | press | type | " +
  "drag | dragTo | wait | scroll | eval\n" +
  '  example: --steps \'[{"action":"rightclick","selector":"[data-node-id=\\"n1\\"]"},' +
  '{"action":"click","selector":"[data-menu-item=\\"rename\\"]"},' +
  '{"action":"type","text":"quarterly.pdf"}]\'\n';

function parseArgs(argv) {
  const flags = { reduced: false, full: false, strict: false, help: false };
  const takesValue = new Set([
    "url",
    "out",
    "w",
    "h",
    "dpr",
    "wait",
    "hover",
    "focus",
    "click",
    "viewport",
    "steps",
    "settle",
    "clip",
  ]);
  const booleans = new Set(["reduced", "full", "strict", "help"]);

  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (!token.startsWith("--")) continue;
    let key = token.slice(2);
    let value = null;
    const eq = key.indexOf("=");
    if (eq !== -1) {
      value = key.slice(eq + 1);
      key = key.slice(0, eq);
    }
    if (booleans.has(key)) {
      flags[key] = value === null ? true : value !== "false";
      continue;
    }
    if (!takesValue.has(key)) continue;
    if (value === null) {
      value = argv[i + 1];
      i += 1;
    }
    flags[key] = value;
  }
  return flags;
}

function num(value, fallback) {
  const n = Number(value);
  return Number.isFinite(n) ? n : fallback;
}

/** "1280x800" -> [1280, 800]; anything unparseable keeps the defaults. */
function parseViewport(value, fallbackW, fallbackH) {
  if (typeof value !== "string") return [fallbackW, fallbackH];
  const m = /^\s*(\d+)\s*[x×,]\s*(\d+)\s*$/i.exec(value);
  if (!m) return [fallbackW, fallbackH];
  return [Number(m[1]), Number(m[2])];
}

/** "x,y,w,h" -> a Playwright clip rect, or null when absent/unparseable. */
function parseClip(value) {
  if (typeof value !== "string") return null;
  const parts = value.split(",").map((p) => Number(p.trim()));
  if (parts.length !== 4 || parts.some((p) => !Number.isFinite(p))) return null;
  return { x: parts[0], y: parts[1], width: parts[2], height: parts[3] };
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** No Playwright call should outlive one action budget, keyboard included. */
function withTimeout(promise, ms, label) {
  let timer = null;
  const guard = new Promise((_, reject) => {
    timer = setTimeout(
      () => reject(new Error(`${label} timed out after ${ms}ms`)),
      ms,
    );
  });
  return Promise.race([promise, guard]).finally(() => {
    if (timer) clearTimeout(timer);
  });
}

const args = parseArgs(process.argv.slice(2));

if (args.help) {
  process.stdout.write(USAGE);
  process.exit(0);
}

if (!args.url || !args.out) {
  process.stderr.write(`shoot: --url and --out are required\n${USAGE}`);
  process.exit(1);
}

const url = args.url;
const out = resolve(args.out);
const [vw, vh] = parseViewport(args.viewport, 1280, 800);
const w = num(args.w, vw);
const h = num(args.h, vh);
const dpr = num(args.dpr, 1);
const wait = num(args.wait, 700);
const settle = num(args.settle, STEP_SETTLE_MS);
const reduced = args.reduced === true;
const full = args.full === true;
const strict = args.strict === true;
const clip = parseClip(args.clip);

let steps = [];
let stepsParseError = null;
if (typeof args.steps === "string" && args.steps.trim() !== "") {
  try {
    const parsed = JSON.parse(args.steps);
    if (!Array.isArray(parsed)) throw new Error("--steps must be a JSON array");
    steps = parsed;
  } catch (err) {
    stepsParseError = err?.message ?? String(err);
  }
}

/** Centre of an element in page coordinates — the anchor every drag uses. */
async function centerOf(page, selector) {
  const locator = page.locator(selector).first();
  await locator.waitFor({ state: "visible", timeout: INTERACTION_TIMEOUT_MS });
  const box = await locator.boundingBox();
  if (!box) throw new Error(`no bounding box for ${selector}`);
  return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
}

/** Pointer down, N intermediate moves, pointer up — HTML5 DnD needs the moves. */
async function dragPointer(page, from, to, moveSteps) {
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  const n = Math.max(1, moveSteps);
  for (let i = 1; i <= n; i += 1) {
    await page.mouse.move(
      from.x + ((to.x - from.x) * i) / n,
      from.y + ((to.y - from.y) * i) / n,
    );
    await sleep(8);
  }
  await page.mouse.up();
}

async function runStep(page, step) {
  const action = step?.action;
  const opts = { timeout: INTERACTION_TIMEOUT_MS };
  switch (action) {
    case "click":
      return page.click(step.selector, {
        ...opts,
        button: step.button ?? "left",
        clickCount: step.clickCount ?? 1,
        ...(step.modifiers ? { modifiers: step.modifiers } : {}),
      });
    case "dblclick":
      return page.dblclick(step.selector, opts);
    case "rightclick":
      return page.click(step.selector, { ...opts, button: "right" });
    case "hover":
      return page.hover(step.selector, opts);
    case "press":
      return page.keyboard.press(step.key);
    case "type":
      return page.keyboard.type(String(step.text ?? ""), {
        delay: TYPE_DELAY_MS,
      });
    case "drag": {
      const from = await centerOf(page, step.from);
      const to = await centerOf(page, step.to);
      return dragPointer(page, from, to, num(step.steps, DRAG_STEPS));
    }
    case "dragTo": {
      const from = await centerOf(page, step.from);
      const to = { x: num(step.x, from.x), y: num(step.y, from.y) };
      return dragPointer(page, from, to, num(step.steps, DRAG_STEPS));
    }
    case "wait":
      return sleep(num(step.ms, 0));
    case "scroll": {
      const at = await centerOf(page, step.selector);
      await page.mouse.move(at.x, at.y);
      return page.mouse.wheel(num(step.dx, 0), num(step.dy, 0));
    }
    case "eval":
      return page.evaluate(String(step.js ?? ""));
    default:
      throw new Error(`unknown action ${JSON.stringify(action)}`);
  }
}

async function launch() {
  try {
    const browser = await chromium.launch({
      headless: true,
      channel: "chrome",
      args: LAUNCH_ARGS,
    });
    return { browser, channel: "chrome", launchNote: null };
  } catch (err) {
    const browser = await chromium.launch({ headless: true, args: LAUNCH_ARGS });
    return {
      browser,
      channel: "bundled",
      launchNote: `channel "chrome" failed: ${err?.message ?? String(err)}`,
    };
  }
}

let browser = null;
try {
  const launched = await launch();
  browser = launched.browser;

  const context = await browser.newContext({
    viewport: { width: w, height: h },
    deviceScaleFactor: dpr,
    colorScheme: "dark",
    reducedMotion: reduced ? "reduce" : "no-preference",
  });
  const page = await context.newPage();

  const consoleErrors = [];
  const consoleWarnings = [];
  const pageErrors = [];

  page.on("console", (msg) => {
    const type = msg.type();
    if (type === "error") consoleErrors.push(msg.text());
    else if (type === "warning") consoleWarnings.push(msg.text());
  });
  page.on("pageerror", (err) => {
    pageErrors.push(err?.message ?? String(err));
  });

  await page.goto(url, { waitUntil: "load", timeout: 20000 });
  await page.evaluate(() => document.fonts.ready);

  // Wait for the WebGL field to report ready, but never fail on its absence.
  const fieldDeadline = Date.now() + FIELD_POLL_TIMEOUT_MS;
  for (;;) {
    let ready = false;
    try {
      ready = await page.evaluate(
        () => !!(window.__qfsField && window.__qfsField.ok),
      );
    } catch {
      ready = false;
    }
    if (ready || Date.now() >= fieldDeadline) break;
    await sleep(FIELD_POLL_INTERVAL_MS);
  }

  await sleep(wait);

  const interactionErrors = {};
  if (args.hover) {
    try {
      await page.hover(args.hover, { timeout: INTERACTION_TIMEOUT_MS });
      await sleep(HOVER_SETTLE_MS);
    } catch (err) {
      interactionErrors.hoverError = err?.message ?? String(err);
    }
  }
  if (args.focus) {
    try {
      await page.focus(args.focus, { timeout: INTERACTION_TIMEOUT_MS });
      await sleep(FOCUS_SETTLE_MS);
    } catch (err) {
      interactionErrors.focusError = err?.message ?? String(err);
    }
  }
  if (args.click) {
    try {
      await page.click(args.click, { timeout: INTERACTION_TIMEOUT_MS });
      await sleep(CLICK_SETTLE_MS);
    } catch (err) {
      interactionErrors.clickError = err?.message ?? String(err);
    }
  }

  const stepErrors = [];
  if (stepsParseError) {
    stepErrors.push({ index: -1, action: "parse", error: stepsParseError });
  }
  for (let i = 0; i < steps.length; i += 1) {
    const step = steps[i];
    try {
      await withTimeout(
        Promise.resolve(runStep(page, step)),
        INTERACTION_TIMEOUT_MS + 1000,
        `step ${i} (${step?.action})`,
      );
    } catch (err) {
      stepErrors.push({
        index: i,
        action: step?.action ?? null,
        error: err?.message ?? String(err),
      });
      if (strict) break;
    }
    await sleep(settle);
  }

  await mkdir(dirname(out), { recursive: true });
  // Playwright rejects clip + fullPage together; an explicit region wins.
  await page.screenshot({
    path: out,
    type: "png",
    ...(clip ? { clip } : { fullPage: full }),
  });

  let webgl2 = false;
  try {
    webgl2 = await page.evaluate(
      () => !!document.createElement("canvas").getContext("webgl2"),
    );
  } catch {
    webgl2 = false;
  }

  let field = null;
  try {
    field = await page.evaluate(() => window.__qfsField ?? null);
  } catch {
    field = null;
  }

  const summary = {
    out,
    url,
    w,
    h,
    webgl2,
    field,
    channel: launched.channel,
    consoleErrors,
    consoleWarnings,
    pageErrors,
    stepErrors,
    ...interactionErrors,
  };
  if (clip) summary.clip = clip;
  if (full && !clip) summary.fullPage = true;
  if (launched.launchNote) summary.launchNote = launched.launchNote;

  process.stdout.write(`${JSON.stringify(summary)}\n`);
} catch (err) {
  process.stderr.write(`shoot: ${err?.stack ?? err?.message ?? String(err)}\n`);
  process.exitCode = 1;
} finally {
  if (browser) {
    try {
      await browser.close();
    } catch {
      /* browser already gone */
    }
  }
}
