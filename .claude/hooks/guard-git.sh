#!/usr/bin/env bash
# PreToolUse(Bash) hook: block git commands that destroy another live session's
# work in a shared working tree. See docs/agents/PROTOCOL.md §2.
input=$(cat)
if command -v jq >/dev/null 2>&1; then
  cmd=$(printf '%s' "$input" | jq -r '.tool_input.command // empty' 2>/dev/null)
elif command -v python3 >/dev/null 2>&1; then
  cmd=$(printf '%s' "$input" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("tool_input",{}).get("command",""))' 2>/dev/null)
fi
[ -z "$cmd" ] && cmd="$input"

# "git" must sit in command position (line start, after ; & | ( ` or whitespace) so quoted mentions pass.
G='(^|[;&|(`]|[[:space:]])git([[:space:]]+-C[[:space:]]+[^[:space:]]+)?[[:space:]]+'
T='([[:space:]]|$|;|&|\|)'                                     # token terminator
block() { printf 'BLOCKED by .claude/hooks/guard-git.sh: %s. See docs/agents/PROTOCOL.md §2.\n' "$1" >&2; exit 2; }
has()   { printf '%s' "$cmd" | grep -Eq "$1"; }

has "${G}(add|stage)([[:space:]]+[^[:space:];&|]+)*[[:space:]]+(-A|--all|-u|--update|\.|\*)$T" \
  && block "git add -A/./-u stages other sessions' files — stage explicit paths"
has "${G}commit([[:space:]]+[^[:space:];&|]+)*[[:space:]]+(-a[a-zA-Z]*|--all)$T" \
  && block "git commit -a stages other sessions' files — git add explicit paths first"
has "${G}commit([[:space:]]+[^[:space:];&|]+)*[[:space:]]+--amend" \
  && block "git commit --amend rewrites history that may already be pushed"
has "${G}stash" && ! has "${G}stash[[:space:]]+(list|show)" \
  && block "git stash hides other sessions' uncommitted work"
has "${G}reset([[:space:]]+[^[:space:];&|]+)*[[:space:]]+(--hard|--merge)" \
  && block "git reset --hard discards other sessions' work"
has "${G}(checkout|restore)([[:space:]]+[^[:space:];&|]+)*[[:space:]]+(\.|--[[:space:]]+\.)$T" \
  && block "git checkout ./restore . discards every session's work"
has "${G}checkout[[:space:]]+(-b[[:space:]]|-B[[:space:]]|[^-.[:space:]][^[:space:]]*[[:space:]]*($|;|&|\|))" \
  && block "branch switching in a shared working tree breaks the other session — stay on main"
has "${G}switch" \
  && block "branch switching in a shared working tree breaks the other session — stay on main"
has "${G}clean" \
  && block "git clean deletes other sessions' untracked files"
has "${G}rebase([[:space:]]+[^[:space:];&|]+)*[[:space:]]+(-i|--interactive)" \
  && block "interactive rebase is not supported here"
has "${G}push([[:space:]]+[^[:space:];&|]+)*[[:space:]]+(-f|--force|--force-with-lease)" \
  && block "force push is forbidden"
exit 0
