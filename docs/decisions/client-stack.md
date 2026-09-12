# Client stack: Tauri 2 desktop shell, React UI, mock-first backend bridge
status: accepted
date: 2026-09-12      scope: client
decision: >
  The GUI is a Tauri 2 desktop app in `client/`: React 19 + TypeScript + Vite, Tailwind CSS 4 for
  styling, Motion (the Framer Motion library) for UI animation, WebGL for the full-screen color field,
  Urbanist self-hosted under `client/public/fonts/`. No network fonts, CDNs, or remote assets: the app
  must render fully offline. The webview never speaks to the daemon directly. All daemon access goes
  through `client/src/lib/backend/` (one `BackendClient` interface); its Tauri implementation calls Rust
  commands in `client/src-tauri/`, which will own the connection to the local `qfsd` daemon. Outside
  Tauri (plain browser) a mock `BackendClient` with seeded data is selected automatically, so every
  screen runs at `npm run dev` for design work and screenshot review.
why:
- Tauri: small binary, native window with overlay title bar, and a Rust side that can hold the daemon socket and secrets out of the webview.
- Mock-first bridge: UI work proceeds before the daemon IPC exists; swapping in the real client is one interface.
- Self-hosted font: deterministic rendering in the packaged app; no first-paint fallback flash during the splash.
rejected:
- Electron — heavier, no Rust-native seam to qfsd.
- Webview talking to qfsd over loopback directly — exposes IPC surface to web content; the Rust side is the trust boundary.
