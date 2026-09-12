#!/usr/bin/env node
// Tauri `beforeDevCommand` shim for the quantam-fs client.
//
// `npm run app` must work whether or not a Vite dev server is already running
// on :1420 (the human and several agents share one). Starting a second Vite
// would pick a different port and Tauri would load nothing.
//
//   - server already up  -> reuse it, and stay alive (Tauri kills this process
//                           when the app quits; exiting early would abort the
//                           dev run).
//   - no server          -> spawn `npx vite`, forward signals, mirror its code.

import { spawn } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const DEV_URL = "http://localhost:1420/";
const PROBE_TIMEOUT_MS = 1000;

const clientDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");

async function isServerUp() {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), PROBE_TIMEOUT_MS);
  try {
    // Any HTTP status means something is listening and speaking HTTP.
    await fetch(DEV_URL, { signal: controller.signal });
    return true;
  } catch {
    return false;
  } finally {
    clearTimeout(timer);
  }
}

if (await isServerUp()) {
  console.log(
    "[dev] reusing the dev server already running on http://localhost:1420",
  );
  const quit = () => process.exit(0);
  process.on("SIGINT", quit);
  process.on("SIGTERM", quit);
  // Block forever: Tauri expects beforeDevCommand to run for the app's life.
  // A pending promise alone is NOT enough — with nothing referenced in the
  // event loop node reports "unsettled top-level await" and exits 13. The
  // ref'd interval is what actually holds the process open.
  const keepAlive = setInterval(() => {}, 1 << 30);
  await new Promise(() => {});
  clearInterval(keepAlive);
} else {
  console.log("[dev] starting vite on http://localhost:1420");
  const child = spawn(
    process.platform === "win32" ? "npx.cmd" : "npx",
    ["vite"],
    { stdio: "inherit", cwd: clientDir },
  );

  const forward = (signal) => () => {
    if (!child.killed) child.kill(signal);
  };
  process.on("SIGINT", forward("SIGINT"));
  process.on("SIGTERM", forward("SIGTERM"));

  child.on("exit", (code, signal) => {
    process.exit(signal ? 1 : (code ?? 0));
  });
  child.on("error", (err) => {
    process.stderr.write(`[dev] failed to start vite: ${err?.message ?? err}\n`);
    process.exit(1);
  });
}
