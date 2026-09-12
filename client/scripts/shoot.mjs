#!/usr/bin/env node
// Headless screenshot CLI for the quantam-fs client dev server.
//
// Standalone so many agents can capture the running app in parallel without
// sharing a browser. Uses the system Google Chrome via Playwright's
// `channel: "chrome"` (falls back to the bundled chromium if that fails).
//
// Usage:
//   node scripts/shoot.mjs --url <URL> --out <PNG path>
//        [--w 1280] [--h 800] [--dpr 1] [--wait 700]
//        [--hover <css>] [--focus <css>] [--click <css>] [--reduced]
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

function parseArgs(argv) {
  const flags = { reduced: false };
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
  ]);

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
    if (key === "reduced") {
      flags.reduced = value === null ? true : value !== "false";
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

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const args = parseArgs(process.argv.slice(2));

if (!args.url || !args.out) {
  process.stderr.write(
    "shoot: --url and --out are required\n" +
      "usage: node scripts/shoot.mjs --url <URL> --out <PNG path> " +
      "[--w 1280] [--h 800] [--dpr 1] [--wait 700] " +
      "[--hover <css>] [--focus <css>] [--click <css>] [--reduced]\n",
  );
  process.exit(1);
}

const url = args.url;
const out = resolve(args.out);
const w = num(args.w, 1280);
const h = num(args.h, 800);
const dpr = num(args.dpr, 1);
const wait = num(args.wait, 700);
const reduced = args.reduced === true;

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

  await mkdir(dirname(out), { recursive: true });
  await page.screenshot({ path: out, type: "png", fullPage: false });

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
    ...interactionErrors,
  };
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
