# client
GUI application (desktop/mobile/tablet). Owner: Aaryaman. Rules: `docs/agents/PROTOCOL.md`.

Stack: Tauri 2 + React 19 + TypeScript + Vite + Tailwind CSS 4 + Motion.
       See `docs/decisions/client-stack.md`.

Run:
- `npm install`
- `npm run dev` — browser at http://localhost:1420 (mock backend). Dev params: `?at=<ms>` freezes the splash timeline, `?splash=skip` starts settled.
- `npm run app` — opens the desktop app with live reload (quit with ⌘Q in the app, or Ctrl-C in the terminal). Works even if `npm run dev` is already running.
- `npm run app:build` — builds a double-clickable app at `src-tauri/target/release/bundle/macos/QuantumFS.app` (open it from Finder or Spotlight; ⌘Q quits).

Test:
- `npm run typecheck`
- `npm run build`
- `cd src-tauri && cargo build`
- `npm run shoot -- --url http://localhost:1420/?splash=skip --out /tmp/qfs.png` captures a headless screenshot of the running dev server (design QA)

Layout:
- `src/`            `main.tsx`, `App.tsx`, `styles/` (Tailwind + tokens)
- `src/components/` `brand/`, `chrome/`
- `src/features/`   `splash/` (color field + timeline), `home/` (server/vault list)
- `src/lib/`        `backend/` (mock + Tauri clients), `devQuery.ts`, `brand.ts`
- `src-tauri/`      Rust shell + `bridge.rs` mock commands
- `scripts/`        `dev.mjs`, `shoot.mjs`
- fonts             GT Walsheim Trial, loaded from the OS via `local()` (trial licence — no font files in the repo)
