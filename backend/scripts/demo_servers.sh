#!/usr/bin/env bash
# quantam-fs demo: the three server processes on the host machine.
#   start   central directory + orchestration servers A and B, each in its own
#           Terminal window under demo_supervise.sh (caffeinated, auto-restarting),
#           then print the app connect strings and write $DIR/connect.txt.
#   stop    kill them, keep their data.
#   status  which demo ports are listening.
#   wipe    stop, then delete every server, seed and host-client directory.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BACKEND_DIR="$REPO_ROOT/backend"

BIN="$BACKEND_DIR/target/release/qfsd"
SUPERVISE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/demo_supervise.sh"
DIR="/tmp/qfs-demo"
IP=""
FRESH=0
ACTION=""

DIR_PORT=7440
A_PORT=7447
A_ADMIN=8447
B_PORT=7448
B_ADMIN=8448
PROFILE_PORT=8440         # the directory's profile store (see backend/README.md)
STOP_FLAG="$DIR/stop"     # its existence tells every supervisor to stay down
CONNECT_FILE="$DIR/connect.txt"

usage() {
  cat <<'EOF'
usage: demo_servers.sh start|stop|status|wipe [--bin PATH] [--dir DIR] [--ip LAN_IP] [--fresh]

  start   launch central directory + servers A and B in three Terminal windows,
          each supervised (auto-restart) and caffeinated until you stop them
  stop    terminate them (data under --dir is kept)
  status  report whether ports 7440/8440/7447/8447/7448/8448 are listening
  wipe    stop, then delete the server data dirs, logs, connect.txt, the seed
          tree, the seeder data dir and every client-* dir (app/ is kept)

  --bin PATH   qfsd binary (default: backend/target/release/qfsd, built if missing)
  --dir DIR    demo data root (default: /tmp/qfs-demo)
  --ip IP      advertise this LAN IP instead of the auto-detected one
  --fresh      delete the three server data dirs, logs and connect.txt (start only)

Each node runs under demo_supervise.sh: if qfsd exits it is restarted two
seconds later, and the whole thing sits under `caffeinate -dimsu`. Close the
window (Ctrl-C, or Cmd-W) or run `demo_servers.sh stop` to make it stay down.
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    start|stop|status|wipe) ACTION="$1"; shift ;;
    --bin) BIN="$2"; shift 2 ;;
    --dir) DIR="$2"; shift 2 ;;
    --ip)  IP="$2"; shift 2 ;;
    --fresh) FRESH=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "demo_servers.sh: unknown argument '$1'" >&2; usage >&2; exit 2 ;;
  esac
done

if [ -z "$ACTION" ]; then usage >&2; exit 2; fi

# --dir may have moved the demo root after the defaults above were set.
STOP_FLAG="$DIR/stop"
CONNECT_FILE="$DIR/connect.txt"

detect_ip() {
  local ip iface
  ip="$(ipconfig getifaddr en0 2>/dev/null || true)"
  if [ -z "$ip" ]; then
    iface="$(route -n get default 2>/dev/null | awk '/interface:/ {print $2; exit}')"
    if [ -n "$iface" ]; then
      ip="$(ipconfig getifaddr "$iface" 2>/dev/null || true)"
    fi
  fi
  printf '%s' "$ip"
}

# Read the kernel's socket table; never open a connection. A `nc -z` probe
# completes a TCP handshake and hangs up, which every peer and admin port logs
# as a red ATTENTION about a connection that closed -- so running `status` during
# a demo used to scribble fake faults across all three server windows.
port_listening() {
  local port="$1"
  if lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then return 0; fi
  netstat -an 2>/dev/null | grep -qE "[.:]${port}[[:space:]]+.*LISTEN"
}

# Open a Terminal window running one command in the foreground.
# $1 = window title, $2 = shell command
terminal_run() {
  local title="$1" cmd="$2" script
  # Escape for AppleScript's double-quoted string literal.
  script="printf '\\033]0;${title}\\a'; ${cmd}"
  script="${script//\\/\\\\}"
  script="${script//\"/\\\"}"
  osascript >/dev/null <<EOF
tell application "Terminal"
  activate
  do script "${script}"
end tell
EOF
}

# One node in one Terminal window, wrapped in demo_supervise.sh so it restarts
# itself (and holds a caffeinate assertion) until the stop flag appears or the
# human closes the window.
launch_node() {
  local title="$1" logname="$2"; shift 2
  local cmd
  cmd="cd $(printf '%q' "$BACKEND_DIR") && $(printf '%q' "$SUPERVISE")"
  cmd="$cmd $(printf '%q' "$logname") $(printf '%q' "$DIR/$logname.log") $(printf '%q' "$STOP_FLAG") --"
  cmd="$cmd $(printf '%q' "$BIN")"
  local arg
  for arg in "$@"; do cmd="$cmd $(printf '%q' "$arg")"; done
  terminal_run "$title" "$cmd"
}

# Last occurrence of $2 (grep -E pattern) in log $1; empty when the file or the
# line is not there yet. -a so the terminal colour codes in the logs never make
# grep treat one as binary.
scrape_line() {
  local log="$1" pattern="$2"
  [ -f "$log" ] || return 0
  grep -aEo "$pattern" "$log" 2>/dev/null | tail -n 1 || true
}

CENTRAL_LINE=""
A_LINE=""
B_LINE=""

# Poll all three logs every 0.5 s for up to 25 s, and only stop once every line
# has shown up (a log file that does not exist yet just counts as not found).
# One shared budget used to be split across three sequential waits, so the first
# wait consumed it all and the other two returned without polling at all -- and
# the directory pattern was [0-9a-fA-F.:\[\]]+, whose bracket expression closes
# at the first ']' and so demanded a literal ']' before the port: it could never
# match an IPv4 address, which is why the first wait always burned the budget.
wait_for_lines() {
  local tries=50 announced=0
  while :; do
    [ -n "$CENTRAL_LINE" ] || CENTRAL_LINE="$(scrape_line "$DIR/central.log" 'directory address [^[:space:]]+')" || true
    [ -n "$A_LINE" ] || A_LINE="$(scrape_line "$DIR/server-a.log" 'app connect string [^[:space:]]+')" || true
    [ -n "$B_LINE" ] || B_LINE="$(scrape_line "$DIR/server-b.log" 'app connect string [^[:space:]]+')" || true
    if [ -n "$CENTRAL_LINE" ] && [ -n "$A_LINE" ] && [ -n "$B_LINE" ]; then return 0; fi
    tries=$(( tries - 1 ))
    if [ "$tries" -le 0 ]; then return 1; fi
    if [ "$announced" -eq 0 ]; then echo "waiting for servers…"; announced=1; fi
    sleep 0.5
  done
}

do_start() {
  if [ "$FRESH" -eq 1 ]; then
    # Only our own data dirs and logs. $DIR itself holds the VM demo's shared
    # app/ (virtiofs), its vm-*.log files and client-host: never remove those.
    echo "clearing server data in $DIR (app/, vm-*.log and client-host kept)"
    rm -rf "$DIR/central" "$DIR/server-a" "$DIR/server-b"
    rm -f "$DIR/central.log" "$DIR/server-a.log" "$DIR/server-b.log" "$DIR/pids"
    rm -f "$CONNECT_FILE" "$STOP_FLAG"
  fi

  if [ ! -x "$BIN" ]; then
    echo "building qfsd (release)…"
    ( cd "$BACKEND_DIR" && cargo build --release --bin qfsd )
  fi
  if [ ! -x "$BIN" ]; then
    echo "demo_servers.sh: qfsd binary not found at $BIN" >&2
    exit 1
  fi

  if [ -z "$IP" ]; then IP="$(detect_ip)"; fi
  if [ -z "$IP" ]; then
    echo "demo_servers.sh: could not detect a LAN IP; pass --ip <address>" >&2
    exit 1
  fi

  mkdir -p "$DIR/central" "$DIR/server-a" "$DIR/server-b"
  # A leftover flag from the last `stop` would make every supervisor quit after
  # its first run, so it goes before anything is launched.
  rm -f "$STOP_FLAG"
  : >"$DIR/central.log"
  : >"$DIR/server-a.log"
  : >"$DIR/server-b.log"

  echo "LAN IP: $IP"
  echo "data:   $DIR"

  launch_node "QFS CENTRAL DIRECTORY" central \
    --directory --data-dir "$DIR/central" --listen-addr "0.0.0.0:$DIR_PORT"
  sleep 2
  launch_node "QFS SERVER A" server-a \
    --data-dir "$DIR/server-a" --listen-addr "0.0.0.0:$A_PORT" \
    --admin-addr "0.0.0.0:$A_ADMIN" --directory-addr "$IP:$DIR_PORT"
  launch_node "QFS SERVER B" server-b \
    --data-dir "$DIR/server-b" --listen-addr "0.0.0.0:$B_PORT" \
    --admin-addr "0.0.0.0:$B_ADMIN" --directory-addr "$IP:$DIR_PORT"

  # Record pids so stop/status have something to report even if the log scrape
  # fails. Match the three data dirs, never the bare demo dir: the VM demo's
  # tart processes and the host client carry $DIR in their command lines too.
  : >"$DIR/pids"
  local p node
  for node in central server-a server-b; do
    for p in $(pgrep -f "$DIR/$node" 2>/dev/null || true); do
      echo "$p" >>"$DIR/pids"
    done
  done

  wait_for_lines || true

  local central a b
  central="${CENTRAL_LINE#directory address }"
  a="${A_LINE#app connect string }"
  b="${B_LINE#app connect string }"

  # The seeder and the human both read connect.txt rather than re-scraping logs.
  {
    echo "DIRECTORY ${central}"
    echo "SERVER_A ${a}"
    echo "SERVER_B ${b}"
  } >"$CONNECT_FILE"

  local profile
  profile="$(scrape_line "$DIR/central.log" 'profile address [^[:space:]]+')"
  profile="${profile#profile address }"

  echo
  echo "──────────── quantam-fs demo servers ────────────"
  echo "DIRECTORY  ${central:-"(not seen yet — check $DIR/central.log)"}"
  if [ -n "$profile" ]; then
    echo "PROFILES   $profile"
  fi
  echo "SERVER A   ${a:-"(not seen yet — check $DIR/server-a.log)"}"
  echo "SERVER B   ${b:-"(not seen yet — check $DIR/server-b.log)"}"
  echo
  echo "Paste a connect string into the app's \"Add server\" field."
  echo "connect.txt: $CONNECT_FILE"
  echo
  echo "Combined demo log (corner terminal):"
  echo "  python3 $REPO_ROOT/backend/demo_monitor.py \\"
  echo "    central=$DIR/central/demo-events.log \\"
  echo "    serverA=$DIR/server-a/demo-events.log \\"
  echo "    serverB=$DIR/server-b/demo-events.log"
  echo "─────────────────────────────────────────────────"
}

do_stop() {
  echo "stopping demo servers (data in $DIR is kept)"
  # The flag goes down before anything is killed: each supervisor checks it when
  # its qfsd exits, and without it they would all cheerfully restart.
  mkdir -p "$DIR"
  : >"$STOP_FLAG"
  if [ -f "$DIR/pids" ]; then
    local p
    while read -r p; do
      [ -n "$p" ] || continue
      kill "$p" 2>/dev/null || true
    done <"$DIR/pids"
  fi
  # Per data dir, not $DIR: a bare $DIR match also hits the VM demo's tart
  # processes and the host client, which are not ours to kill.
  local node
  for node in central server-a server-b; do
    pkill -f "$DIR/$node" 2>/dev/null || true
  done
  sleep 1
  for node in central server-a server-b; do
    pkill -9 -f "$DIR/$node" 2>/dev/null || true
  done
  rm -f "$DIR/pids"
  echo "stopped."
}

do_status() {
  echo "demo dir: $DIR"
  local entry port label
  for entry in "$DIR_PORT:central directory" \
               "$PROFILE_PORT:directory profile store" \
               "$A_PORT:server A peers" "$A_ADMIN:server A admin" \
               "$B_PORT:server B peers" "$B_ADMIN:server B admin"; do
    port="${entry%%:*}"
    label="${entry#*:}"
    if port_listening "$port"; then
      printf '  %-5s LISTENING   %s\n' "$port" "$label"
    else
      printf '  %-5s down        %s\n' "$port" "$label"
    fi
  done
}

# Everything the demo generates, gone: servers, logs, the seed tree, the seeder's
# data dir and every client-* dir. The built app bundle and its tarball stay --
# rebuilding those takes minutes and they hold no state.
do_wipe() {
  do_stop
  echo
  echo "wiping demo state in $DIR (app/ and QuantumFS.app.tgz are kept)"
  local target
  for target in "$DIR/central" "$DIR/server-a" "$DIR/server-b" \
                "$DIR/pids" "$CONNECT_FILE" "$DIR/seed" "$DIR/seeder" \
                "$DIR"/*.log "$DIR"/client-*; do
    case "$target" in
      "$DIR/app"|"$DIR/QuantumFS.app.tgz") continue ;;
      *'*'*) continue ;;   # an unmatched glob, not a real path
    esac
    [ -e "$target" ] || continue
    rm -rf "$target"
    echo "  removed $target"
  done
  echo 'wiped. (the stop flag stays until the next start, so nothing revives)'
}

case "$ACTION" in
  start)  do_start ;;
  stop)   do_stop ;;
  status) do_status ;;
  wipe)   do_wipe ;;
esac
