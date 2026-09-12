# Client workspace: one BackendClient seam for the file system, deletable demo layer, zustand store
status: accepted
date: 2026-09-12      scope: client
decision: >
  Everything the vault workspace shows (tree, availability, members, per-node access, history, recents,
  search, presence, join codes) is read and written only through `BackendClient` methods and
  `BackendEvent`s in `client/src/lib/backend/`. The whole tree of a vault is loaded with `listTree`
  (folders are always available locally); ops return the affected nodes and every implementation also
  emits `fs-changed` deltas so remote and local changes flow through one path. The mock lives in
  `src/lib/backend/mock/` and Rust mirrors it in `src-tauri/src/fs_*.rs`, both seeded from the single
  file `src/lib/backend/seed/fs.json` (Rust `include_str!`s it). Scripted multiplayer is a wrapper,
  `withDemo(client)` in `src/lib/backend/demo/`, that only calls public client methods (with the
  mock-only `actor` field) and injects `presence` / `remote-op` events; deleting that folder and the
  one call in `createBackend()` removes the demo without touching UI. Search runs client-side over
  the loaded tree (`src/lib/search/`) in both runtimes. Workspace UI state is one zustand store in
  `src/features/workspace/store/`; per-frame values (drag, cursors) are Motion values, never React
  state. Icons are generated SVG in `src/components/icons/` (registry of extensions → family + hues),
  no filters inside tiles. Dev switches for screenshots: `?vault=<id>&folder=<id>&ui=<surface>&demo=off|<ms>`.
why:
- The human will replace the mock with the real daemon later; a single seam plus a deletable demo folder makes that a swap, not a rewrite.
- One seed file keeps browser and Tauri builds pixel-identical without hand-syncing two seeds.
- Tree-in-memory makes Finder-grade snappiness and nested search trivial; the tree is small by definition (it is replicated to every member).
- Motion values for drag/cursors keep 60fps with no React re-render per frame.
rejected:
- Per-directory lazy listing — slower navigation, complicates search and drag targets; folders are always local anyway.
- React context/reducer for workspace state — cursor and drag traffic would re-render the grid.
- Bitmap/PNG icon packs — cannot recolour folders, no crisp scaling from 18px to 128px, and licensing.
- Rust-side demo loop — duplicates the script; the wrapper runs over either client.
