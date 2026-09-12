#!/usr/bin/env bash
# quantam-fs demo: keep one qfsd node alive for the whole demo.
#
#   demo_supervise.sh <node-name> <log-path> <stop-flag-path> -- <command and args…>
#
# The command is run in the foreground with its output teed into <log-path>. When
# it exits for any reason the supervisor restarts it two seconds later, forever,
# unless <stop-flag-path> exists (demo_servers.sh stop creates it) or the human
# interrupts this window (Ctrl-C / ⌘W). The whole loop runs under
# `caffeinate -dimsu`, so the Mac never sleeps, never idles the display and never
# suspends the servers while the demo is on screen.
set -uo pipefail

usage() {
  cat <<'EOF'
usage: demo_supervise.sh <node-name> <log-path> <stop-flag-path> -- <command…>

  node-name        label printed in the supervisor lines (e.g. server-a)
  log-path         every line of the command's stdout+stderr is appended here
  stop-flag-path   when this file exists after an exit, the supervisor stops
  --               everything after it is the command to run and keep alive

The loop re-execs itself under `caffeinate -dimsu` (display, idle, system and
user-active assertions) so the demo machine cannot sleep while a node is up.
EOF
}

case "${1:-}" in
  -h|--help) usage; exit 0 ;;
esac

if [ "$#" -lt 4 ]; then usage >&2; exit 2; fi

NODE="$1"; LOG="$2"; STOP="$3"; shift 3
if [ "$1" != "--" ]; then
  echo "demo_supervise.sh: expected '--' before the command, got '$1'" >&2
  usage >&2
  exit 2
fi
shift
if [ "$#" -eq 0 ]; then
  echo "demo_supervise.sh: no command given after '--'" >&2
  exit 2
fi

# Re-exec under caffeinate exactly once. `exec` means caffeinate takes over this
# very process, so the pid printed below is caffeinate's own pid.
if [ -z "${QFS_SUPERVISE_CAFFEINATED:-}" ]; then
  export QFS_SUPERVISE_CAFFEINATED=1
  if command -v caffeinate >/dev/null 2>&1; then
    echo "[supervisor] $NODE caffeinate pid $$ (display+idle+system+user assertions)"
    exec caffeinate -dimsu /usr/bin/env bash "$0" "$NODE" "$LOG" "$STOP" -- "$@"
  fi
  echo "[supervisor] $NODE caffeinate not found — running without sleep assertions" >&2
fi

mkdir -p "$(dirname "$LOG")" 2>/dev/null || true

CHILD=""
on_signal() {
  if [ -n "$CHILD" ]; then kill "$CHILD" 2>/dev/null || true; fi
  echo
  echo "[supervisor] $NODE stopped by user"
  exit 0
}
trap on_signal INT TERM HUP

while :; do
  # A fifo instead of a plain pipeline: `cmd | tee` would only hand us tee's pid,
  # and the signal handler has to kill the node itself.
  FIFO="$(mktemp -u "${TMPDIR:-/tmp}/qfs-sup-XXXXXX")"
  mkfifo "$FIFO" 2>/dev/null || FIFO=""

  if [ -n "$FIFO" ]; then
    tee -a "$LOG" <"$FIFO" &
    TEE_PID=$!
    FORCE_COLOR=1 "$@" >"$FIFO" 2>&1 &
    CHILD=$!
    wait "$CHILD"; code=$?
    wait "$TEE_PID" 2>/dev/null || true
    rm -f "$FIFO"
  else
    FORCE_COLOR=1 "$@" 2>&1 | tee -a "$LOG"
    code="${PIPESTATUS[0]}"
  fi
  CHILD=""

  if [ -f "$STOP" ]; then
    echo "[supervisor] $NODE stopped (stop flag $STOP)"
    exit 0
  fi

  echo
  echo "[supervisor] $NODE exited (code $code) — restarting in 2 s"
  echo
  sleep 2
done
