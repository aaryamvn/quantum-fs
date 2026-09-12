# Prime the repo for human–agent collaboration
area: shared      status: done      opened: 2026-09-11      by: Aaryaman
prompt: >
  Come up with a ridiculously strong, efficient, very lightweight system for all agents (Grok, Claude, OpenAI)
  and both humans to collaborate on this repo: shared context trail viewed only when necessary, merge-safe
  across two machines, mid-task hand-off to any other model, and provider entrypoints that route to it.
  Full text: docs/agents/MANDATE.md

## Plan
- [x] Rules in one file (docs/agents/PROTOCOL.md); verbatim mandate kept separately
- [x] Task files as the hand-off unit; decisions as the only binding trail; templates for both
- [x] Entrypoints: AGENTS.md (universal), CLAUDE.md (imports it + Claude policy), GROK.md (safety net)
- [x] Claude enforcement: Opus-pinned agents, subagent-model env vars, session-start + git-guard hooks
- [x] .gitignore, area READMEs, README pointer

## Checkpoint
done:       all steps
in-flight:  none
next:       none
open:       teammate's name is still "teammate" in PROTOCOL §1, AGENTS.md, backend/README.md

## Outcome
changed:    AGENTS.md CLAUDE.md GROK.md README.md .gitignore .claude/ docs/agents/ docs/tasks/ docs/decisions/ client/README.md backend/README.md
verified:   guard hook — 43 command cases (25 must-block, 18 must-allow) all correct; settings.json parses; session-start prints task list
not-done:   nothing
gotchas:    Codex and both Grok CLIs read AGENTS.md natively; Grok Build also reads CLAUDE.md, hence its "Not Claude? stop" line.
            The git guard matches the command text, so a shell heredoc that merely quotes a forbidden command can trip it; use an editor tool for that.
