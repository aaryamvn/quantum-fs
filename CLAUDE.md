@AGENTS.md

# Claude-specific policy (binding; the human mandate is in docs/agents/MANDATE.md)
Not Claude? Stop here — `AGENTS.md` is your entrypoint; nothing below applies to you.

## Model roles — Fable plans, Opus executes
- **Fable** = planner and orchestrator ONLY. It reads the request, plans, writes the task file and checkpoints, dispatches subagents, verifies their reports, and talks to the human. Fable never edits code files itself.
- **Opus 5** = executor ONLY. Every subagent runs on Opus 5, no exceptions (implementer, investigator, reviewer, Explore, Plan, general-purpose, claude-code-guide, Workflow agents).
- Enforced three ways; never remove any: `.claude/settings.json` env `CLAUDE_CODE_SUBAGENT_MODEL=opus` + `CLAUDE_CODE_SUBAGENT_MODEL_FORCE=1`; `model: opus` in every `.claude/agents/*.md`; pass `model: "opus"` on every `Agent` call anyway.
- Never use `subagent_type: "fork"` (a fork inherits Fable). Never call `Workflow` unless the human opted in; if so every `agent()` sets `model: 'opus'`.
- If the human launched the main session on Opus, it plans and executes itself; subagents are still Opus.

## Project agents (`.claude/agents/`)
- `implementer` — makes the changes a spec describes, verifies, reports facts. Use for every code change.
- `investigator` — read-only research; compact findings with `path:line`. Use so Fable never reads large files itself.
- `reviewer` — checks implementer output against its spec; defects only. Use after any non-trivial implementer run.

## Spec format Fable MUST use when dispatching `implementer`
Fable's prompts to Opus are precise, detailed, and granular. Every dispatch contains, in this order:
```
TASK:       docs/tasks/active/<file>   (or "none" for one-shot fixes)
GOAL:       one sentence
READ FIRST: exact paths (line ranges if large); applicable docs/decisions/ files
CHANGES:    numbered; per file: exact path · what to add/modify · signatures · behavior · edge cases · error handling
DO NOT:     files/areas not to touch; patterns to avoid
VERIFY:     exact commands to run · expected output
REPORT:     CHANGED · VERIFIED (verbatim output) · DEVIATIONS (+why) · UNRESOLVED
```
Granularity: one implementer = one coherent unit, typically 1–5 files. Independent units run as parallel implementers. Never two implementers on the same file at once. Fable updates the task Checkpoint after each unit returns, before dispatching the next.

## Hooks (automatic, in `.claude/settings.json`)
- `SessionStart` prints `docs/tasks/active/` so resumable work is seen without being asked.
- `PreToolUse(Bash)` blocks the git commands PROTOCOL §2 forbids. If blocked, do not work around it — stage explicit paths instead.

## Memory
Claude's private memory is per-machine and per-user. The repo trail (`docs/tasks/`, `docs/decisions/`) is the only source of truth for project state; never rely on memory for it.
