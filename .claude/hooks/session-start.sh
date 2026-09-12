#!/usr/bin/env bash
# SessionStart hook: show resumable tasks so no agent has to be told to look.
cd "${CLAUDE_PROJECT_DIR:-.}" 2>/dev/null || exit 0
if ls docs/tasks/active/*.md >/dev/null 2>&1; then
  echo "ACTIVE TASKS — resume per docs/agents/PROTOCOL.md §3 if one matches your prompt:"
  for f in docs/tasks/active/*.md; do
    echo "  $f — $(sed -n '1s/^# //p' "$f")"
  done
else
  echo "No active tasks (docs/tasks/active/ is empty)."
fi
exit 0
