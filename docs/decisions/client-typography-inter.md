# Client typography: Inter for body text, GT Walsheim for headings only
status: accepted
date: 2026-09-12      scope: client
decision: >
  Body text (labels, buttons, menus, inputs, captions, tile names, metadata) is set in Inter, bundled from the
  `@fontsource-variable/inter` package (OFL licence) so the app still renders fully offline; `--font-sans` is
  Inter. Headings only — the wordmark, modal and settings-pane titles, the inspector item name, the vault name in
  the dive transition — use "GT Walsheim Trial" via `--font-heading` (still loaded with local(), weight 500 max).
  Inter weights: 400 body, 500 emphasis, 600 for the strongest UI labels (server names in the sidebar). Never 700+.
  Section captions ("Recents", "Servers", "Access", "Color") are sentence case, 12.5px, muted — never all caps.
why:
- Human direction on 2026-09-12: "Use only Inter for body text and keep the GT Walsheim for exclusively headings"; captions must not be all caps and should be slightly larger.
- Inter's larger x-height reads better at 12–13px than the geometric display face; Walsheim keeps its character where it is large.
rejected:
- Keeping Walsheim everywhere at ≤500 (docs/decisions/client-typography.md) — superseded by this file.
- Loading Inter from Google Fonts — the app must render with no network (docs/decisions/client-stack.md).
