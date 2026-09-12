# Client shell: splash color-wave animation + server/vault list
area: client      status: done      opened: 2026-09-12      by: Aaryaman
prompt: >
  I now want to build the entire interface for the file system. I am not working on connecting it to the backend yet, and we will figure that integration out later, so for now we will just focus on design, animation, and user experience, and building out the scaffolding for all features. I am unsure what the ‘backend’ looks like yet, but try and predict what it will end up being (itll probably just be a rust daemon running constantly in the background). The design and animations have to be incredibly beautiful and precise.

  The name of the app is “QuantamFS”

  Here is the svg for our iconmark, this only will be rendered in the actual app and not the splash screen:

  [iconmark SVG — stored verbatim in client/src/components/brand/Iconmark.tsx]

  Right now, I do not need you to build any other features except for a splash screen and whatever other basic scaffolding is required for this exe. This will be a SUPER well-designed, insanely awesome animation where a barrage of colors flies in from the right like ‘waves’ and then ultimately settles on a gradient from #FF7B7B to #4E0EFF to #000000, all of which is fixed on the LEFT side of the screen and occupies perhaps 40% of the width of the screen post-animation. Ensure a sufficient guassian blur is applied to this whole thing as well as a bit of grainy noise texture overlayed for effect. After which, on the right side of the screen we display the following:

  QuantumFS

  [server icon] Server 1        [plus icon]
  [vault icon] Vault 1
  [vault icon] Vault 2
  [vault icon] Vault 3
  [server icon] Server 2       [plus icon]
  [vault icon] Vault 1
  [vault icon] Vault 2

  [server/plus icon] Add New Server

  —--- [muted line] or ------

  [vault/plus icon] Join a Vault

  The font we use is Urbanist, and we will have the “QuantumFS” wordmark at the top medium weight and slightly larger than the other text. Have the whole server list and vault options presented like full-width buttons with good padding at the top and bottom and a secondary black background color. Should be clean and elegant and sophisticated and match the design language. By the way, all of this content that isn’t the colors animation must be right-aligned, and, within the bounding ‘box’ for this actual content, everything must be left-aligned. Do a super sophisticated, very well designed job please. We want to win this hackathon in terms of the design especially - like ridiculous level super efficient animations and design quality.

  Also - we are going to be using Tauri to build the entire frontend, along with Tailwind Css and framer motion. Ensure that everything is implemented perfectly despite the fact that we are using tauri.

## Plan
- [x] 1 scaffold: Tauri 2 + Vite + React + TS + Tailwind 4 + Motion; Urbanist self-hosted; window config (overlay title bar, black bg); app icons; README
- [x] 2 architecture: splash timeline + dev query params (`?at=ms` freeze, `?splash=skip`, `?field=a|b|c`); backend bridge (types, mock, tauri stub, Rust command); WebGL color-field host + grain overlay; App composition; Home placeholder
- [x] 3 parallel: six color-field shader variants (a–f) + three home-screen variants (a–c); screenshot each at fixed times; independent judges; promote winners, delete the rest; refine
- [x] 4 review: reduced to one hygiene pass on the human's request to finish fast; desktop app launched
- [x] 5 outcome + commit

## Checkpoint   (overwrite in place · ≤10 lines · write BEFORE starting the next step)
done:       all steps; see Outcome
in-flight:  none
next:       none — task complete
open:       wordmark spelled "QuantamFS" (matches "name of the app" line and repo); the mock said "QuantumFS" — flip APP_NAME in client/src/lib/brand.ts if the other spelling was intended

## Outcome   (fill at completion · ≤10 lines · facts only)
changed:    client/ (new app: Tauri 2 + React 19 + TS + Vite 8 + Tailwind 4 + Motion 13; splash = WebGL color field shaders/field.ts + GrainOverlay + timeline with dev params ?at=<ms> ?splash=skip; home = server/vault list features/home; backend bridge lib/backend (TS interface, mock, Tauri client) + src-tauri/src/bridge.rs seeded commands; scripts/dev.mjs reuse-aware dev server, scripts/shoot.mjs headless screenshot CLI; Urbanist self-hosted; app icons), docs/decisions/client-stack.md
verified:   npm run typecheck → 0 · npm run build → ✓ built · cd src-tauri && cargo build → Finished, 0 warnings · npm run app:build → QuantamFS.app + .dmg · headless frames: t0 black; colored width from the right 20/46/88/100/100 % at 250/500/800/1100/1400 ms; settled: coral #f97b7b at x=0, violet #4e0efd at 49–51 % of the panel, black by 508 px of 1280 (39.7 %), idle drift ≤ 1 px; desktop app launched via tauri dev and seen as a foreground process
not-done:   multi-lens review round (human asked to finish fast; one hygiene pass done instead); real daemon integration (bridge is a seeded in-memory mock on both sides); the "no-server" branch of scripts/dev.mjs is untested at runtime; reduced-motion and 900×600 layouts not separately reviewed
gotchas:    ?at= freezes the splash clock but not the home reveal transition (stills at 2150–2700 ms show the list fully revealed) · scripts/dev.mjs must hold Node open with a ref'd setInterval (a bare pending promise exits Node 26 with code 13) · TS 7 removed baseUrl; Vite 8 minifier is "oxc" · lucide's Vault glyph collapses at 18 px, Lock is used for vaults · six shader variants and three home variants were built and judged from screenshots; only the winners remain
