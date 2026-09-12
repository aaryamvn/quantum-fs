# Client typography: GT Walsheim, Medium is the heaviest weight
status: accepted
date: 2026-09-12      scope: client
decision: >
  The UI font is "GT Walsheim Trial", loaded from the operating system with CSS local() (no font
  files in the repo; trial licence). Weight 500 (Medium) is the maximum anywhere in the app:
  wordmark, modal headings, server headings, buttons. Body and metadata are 400. Never use 700 or
  heavier, and never use a synthesized 600. Hierarchy comes from size, colour (fg / fg-2 / fg-3)
  and spacing, not from heavier weights.
why:
- Human direction on 2026-09-12: the bold wordmark and modal heading read too heavy; "the max heading weight will always be one notch down from that".
- One weight cap keeps the geometric face calm at every size and avoids faux-bold on machines without the family.
rejected:
- Bundling the font files — trial licence.
- Bold (700) for the wordmark only — explicitly rejected by the human.
