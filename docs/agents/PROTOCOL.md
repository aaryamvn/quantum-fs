# Collaboration Protocol — quantam-fs

Read this file fully, once per session. Everything else below is loaded on demand.
Applies to every human and every agent (any model, any vendor). Binding.

## 0. What to read, and when

| Need                         | Read                                              | When                                                        |
|------------------------------|---------------------------------------------------|-------------------------------------------------------------|
| Rules                        | this file                                         | every session, once                                         |
| Resume / hand-off            | `ls docs/tasks/active/`                           | every session, before doing anything                        |
| Product brief                | `docs/VISION.md`                                  | only if the task involves product or architecture choices   |
| Binding decisions            | `ls docs/decisions/` → open files in your scope   | before making a design choice in that scope                 |
| Why is this code like this   | `ls docs/tasks/done/` → open by area/slug         | only when the code surprises you and you need the reason    |
| How to run / test an area    | `client/README.md`, `backend/README.md`           | when working in that area                                   |
| Original human mandate       | `docs/agents/MANDATE.md`                          | only if this file is ambiguous                              |

Do not read anything in that table earlier than its "When" column says. Tokens are budget.

## 1. Areas and owners

- `client/`  — GUI app (desktop/mobile/tablet). Owner: Aaryaman.
- `backend/` — protocol core: peers, sync, storage, crypto, queue. Owner: teammate.
- `docs/`, root config — shared; change only when a human asks.
- Exact client/backend split for a feature: decide, then record it in `docs/decisions/`.

Work inside your area. A cross-area change must be named in the task file and the commit message.

## 2. Concurrent sessions (same working tree, or separate clones)

Two humans and several agents may edit this repo at the same time, sometimes in the same checkout.
- Files outside your area changing under you = another live session. Do not revert, stage, or investigate them. Continue.
- `git add` explicit paths only. Never: `add -A`, `add .`, `add -u`, `commit -a`, `stash`, `reset --hard`, `checkout .`/`checkout <branch>`, `switch`, `clean`, `restore .`, `rebase -i`, `push --force`, `commit --amend` on pushed commits.
- Pull/push only when the human asks. Then: `git pull --rebase --autostash origin main && git push origin main`.
- Conflict in `docs/tasks/` or `docs/decisions/`: keep both sides (they are separate files; conflicts there mean a bad rename). Conflict in code: stop and report; never resolve by picking a side silently.

## 3. Tasks — the hand-off unit

A task file is the complete state of one piece of work, written so that a stranger with no memory, on any model, can continue it.

- Create one for any work with more than one step or that could be interrupted. One-shot fixes need none.
- Path: `docs/tasks/active/<area>-<yymmdd>-<slug>.md`, copied from `docs/agents/templates/task.md`. Area+date+slug never collide across people.
- `prompt:` holds the human's request verbatim. A resuming agent matches its prompt against this.
- **Checkpoint** is overwritten in place, ≤10 lines, and written *before* starting the next step — never only at the end. This is the one rule that makes hand-off work.
- Completion: fill **Outcome** (facts only), `git mv` to `docs/tasks/done/`, commit with the code. Abandoned work: same, with `status: abandoned` and the reason.

Resuming (any model): open the matching active task → read Checkpoint → `git status --short` and `git diff --stat` on the task's files. Uncommitted changes not described in Checkpoint are the interrupted in-flight step: inspect them, then continue from `next:`. Never redo done steps, never restart from scratch, never rewrite the plan without telling the human.

## 4. Decisions — the only binding trail

`docs/decisions/<scope>-<slug>.md` from `docs/agents/templates/decision.md`. Scopes: `crypto sync storage net client backend protocol`.
- Binding until superseded. To change one: write a new file, set the old one's `status: superseded-by: <new file>`. Never edit an accepted decision's body; never silently violate one. If you disagree, say so to the human and propose the superseding decision.
- Any resolution of a question in `docs/VISION.md` ("Problems to work around") is a decision. Record it.

## 5. Trail hygiene — no bias, no doctrine

- The trail is history, not precedent. Only decisions bind. A done task shows what happened, not what to imitate.
- Learn "how we do things" from the code, never from done tasks. Read done tasks only for "why".
- Write facts: what changed, what was verified (command → result), what failed, what is undone. No "we should always…" — that is a decision; put it there or nowhere.
- Provider-neutral: plain markdown. No model names, tool names, session IDs, or vendor features in any trail file. Another vendor's agent must resume from your checkpoint knowing nothing about your tooling.
- Question the trail. If a checkpoint's plan looks wrong, say so before continuing; do not silently follow it and do not silently deviate.

## 6. Token and writing discipline

- Only these documents exist: task files, decision files, area READMEs (run/test/layout only), code comments where non-obvious, this protocol. No changelogs, summaries, status reports, or design docs elsewhere.
- Task and decision files ≤ ~40 lines. Checkpoint ≤10. Outcome ≤10. If it does not fit, it is two tasks.
- Never rewrite a file to restate what it already says. Never document what the code or git history already shows.

## 7. Commits

- Message: `[area] imperative summary` — e.g. `[client] add peer list panel`. Cross-area: `[client+backend] …`.
- Trailer when a task applies: `Task: docs/tasks/done/<file>`.
- Stage only your paths plus your task/decision files. Commit at task completion and at stable checkpoints. Never commit secrets; `.env*` is ignored.

## 8. Adding another agent vendor

Copy `GROK.md` (3 lines) to whatever filename that vendor's tool reads. Nothing else changes; all rules live here.
