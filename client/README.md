# client
GUI application (desktop/mobile/tablet). Owner: Aaryaman. Rules: `docs/agents/PROTOCOL.md`.

Stack: Tauri 2 + React 19 + TypeScript + Vite + Tailwind CSS 4 + Motion.
       See `docs/decisions/client-stack.md`.

Run:
- `npm install`
- `npm run dev` — browser at http://localhost:1420 (mock backend).
- `npm run app` — opens the desktop app with live reload (quit with ⌘Q in the app, or Ctrl-C in the terminal). Works even if `npm run dev` is already running.
- `npm run app:build` — builds a double-clickable app at `src-tauri/target/release/bundle/macos/QuantumFS.app` (open it from Finder or Spotlight; ⌘Q quits).
- `npm run seed:fs` — regenerates `src/lib/backend/seed/fs.json`, the one seed the browser mock and the Rust core both read. Deterministic: re-running produces a byte-identical file.

Dev URL switches (`src/lib/devQuery.ts`), for landing on any state in one navigation:
- `?at=<ms>` freeze the splash timeline · `?splash=skip` start settled · `?field=<id>` colour-field shader
- `?vault=<vaultId>` open a vault's workspace directly (implies `splash=skip`) · `?folder=<nodeId>` start inside a folder · `?select=<ids>` preselect, comma separated
- `?ui=<surface>` force a surface open: `icons` · `search` · `context:<nodeId>` · `settings[:<tab>]` · `info:<nodeId>` · `history:<nodeId>` · `access:<nodeId>` · `share:<nodeId>` · `delete:<nodeId>`
- `?demo=off` no scripted peers · `?demo=<ms>` scripted peers frozen at that elapsed time (default: live)

Test:
- `npm run typecheck`
- `npm run build`
- `cd src-tauri && cargo build`
- `npm run shoot -- --url http://localhost:1420/?splash=skip --out /tmp/qfs.png` captures a headless screenshot of the running dev server (design QA)
- `npm run shoot -- --url "http://localhost:1420/?vault=vlt_1_1&demo=off" --out /tmp/qfs-menu.png --steps '[{"action":"rightclick","selector":"[data-node-id=\"n_1_1_brand\"]"},{"action":"click","selector":"[data-menu-item=\"rename\"]"},{"action":"type","text":"quarterly.pdf"}]'` drives a multi-step interaction before the shot (`node scripts/shoot.mjs --help` lists every action)
- workspace shots need `--wait 4000`: the grid's entrance stagger runs on rAF, which headless Chrome starves for the first few seconds

Layout:
- `src/`                    `main.tsx`, `App.tsx` (home ↔ dive ↔ workspace), `app/appShell.ts` (which screen), `styles/` (Tailwind + tokens)
- `src/components/`         `brand/`, `chrome/`, `icons/` (file/folder artwork), `ui/` (buttons, Modal, Popover, Menu, Toast, Tooltip, …)
- `src/features/splash/`    colour field + timeline
- `src/features/home/`      server/vault list
- `src/features/workspace/` the vault: `store/` (state, selectors, events, geometry) · `sidebar/` `topbar/` `actionbar/` `canvas/` `inspector/` `chat/` `menus/` `modals/` (+ `modals/settings/`, `modals/search/`) · `drag/` `keyboard/` `transition/` · `layout.ts` (every measurement + `Z`) · `WorkspaceScreen.tsx` (the composition)
- `src/features/dev/`       icon gallery (`?ui=icons`)
- `src/lib/`                `backend/` (`client.ts`, `mock/`, `demo/` (scripted peers, deletable), `seed/fs.json`, `tauri.ts`), `search/`, `devQuery.ts`, `keys.ts`, `layers.ts`, `path.ts`, `time.ts`, `color.ts`, `format.ts`, `brand.ts`
- `src-tauri/`              Rust shell + `bridge.rs`, `fs_types.rs`, `fs_state.rs`, `fs_commands.rs`
- `scripts/`                `dev.mjs`, `shoot.mjs`, `seed-fs.mjs`
- fonts                     GT Walsheim Trial, loaded from the OS via `local()` (trial licence — no font files in the repo)

Demo (three clients, two Tart macOS guests + this host):
- `backend/scripts/demo_servers.sh start` — central directory + orchestration servers A and B,
  each in its own Terminal window. It prints `DIRECTORY`, `SERVER A` and `SERVER B`; the two
  server lines are the `IP:PORT/TOKEN` connect strings you paste into the app's "Add server".
  `demo_servers.sh status` shows which ports are up, `stop` shuts them down (data kept).
- `client/scripts/vm-demo.sh build && client/scripts/vm-demo.sh up` — bundles the app into
  `/tmp/qfs-demo/app`, boots `qfs-client-1` and `qfs-client-2`, copies the app in over the
  `qfs` share and starts it in a guest Terminal (guest login `admin`/`admin`; it prints each
  guest IP). `vm-demo.sh down` stops the guests.
- `client/scripts/vm-demo.sh host-client` — the third client, on this machine, in its own
  Terminal with `QFS_DATA_DIR=/tmp/qfs-demo/client-host`.
Apple's Virtualization framework runs at most two macOS guests at once, which is why client 3
lives on the host rather than in a third VM.
