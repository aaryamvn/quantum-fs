#!/usr/bin/env bash
# quantam-fs demo: the client apps.
#   build        bundle QuantumFS.app and stage it where the VMs can see it
#   up           boot the Tart macOS guests (skipping any already running),
#                deploy the staged app into each and start it
#   redeploy     re-deploy + relaunch in the already-running guests and on the
#                host (use after a rebuild; never touches tart run/stop)
#   down         stop the guests
#   host-client  run a third client on this machine (separate data dir)
#
# Apple's Virtualization framework runs at most two macOS guests at once, so the
# third demo client runs on the host.
set -euo pipefail

CLIENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEMO_DIR="/tmp/qfs-demo"
APP_STAGE="$DEMO_DIR/app"
APP_NAME="QuantumFS.app"
APP_BIN="Contents/MacOS/quantamfs"   # the bundle's executable is lowercase
APP_TGZ="$DEMO_DIR/QuantumFS.app.tgz"
BUNDLE="$CLIENT_DIR/src-tauri/target/release/bundle/macos/$APP_NAME"
VM_COUNT=2
ACTION=""
DIR_PORT=7440                 # central directory port (see demo_servers.sh)
LAN_IP="${QFS_LAN_IP:-}"
DIRECTORY_ADDR=""

usage() {
  cat <<'EOF'
usage: vm-demo.sh build|up|redeploy|down|host-client [--vm-count 2] [--ip LAN_IP]

  build        npm run app:build, then stage the bundle in /tmp/qfs-demo/app
  up           start qfs-client-1..N (skipping running ones), deploy the staged
               app over scp and launch it in-guest
  redeploy     deploy + relaunch in the running guests, then relaunch the host
               client (after a rebuild; does not start or stop any VM)
  down         tart stop each qfs-client-N
  host-client  launch a third client on this host (QFS_DATA_DIR=/tmp/qfs-demo/client-host)

  --vm-count N  how many guests to bring up/down (default 2; the framework caps at 2)
  --ip IP       LAN IP of the central directory; every client is launched with
                QFS_DIRECTORY_ADDR=<ip>:7440 so 6-char join codes resolve
                (default: auto-detected like demo_servers.sh; env QFS_LAN_IP)
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    build|up|redeploy|down|host-client) ACTION="$1"; shift ;;
    --vm-count) VM_COUNT="$2"; shift 2 ;;
    --ip) LAN_IP="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "vm-demo.sh: unknown argument '$1'" >&2; usage >&2; exit 2 ;;
  esac
done

if [ -z "$ACTION" ]; then usage >&2; exit 2; fi

# Same detection as backend/scripts/demo_servers.sh: en0, else the default route's
# interface.
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

# The app resolves 6-char join codes through the central directory, so every
# client -- in-guest and on the host -- is launched with QFS_DIRECTORY_ADDR set.
require_directory_addr() {
  if [ -n "$DIRECTORY_ADDR" ]; then return 0; fi
  if [ -z "$LAN_IP" ]; then LAN_IP="$(detect_ip)"; fi
  if [ -z "$LAN_IP" ]; then
    echo "vm-demo.sh: could not detect a LAN IP; pass --ip <address>" >&2
    exit 1
  fi
  DIRECTORY_ADDR="${LAN_IP}:${DIR_PORT}"
}

# Open a Terminal window on the host running one command in the foreground.
terminal_run() {
  local title="$1" cmd="$2" script
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

require_stage() {
  if [ ! -d "$APP_STAGE/$APP_NAME" ]; then
    echo "vm-demo.sh: $APP_STAGE/$APP_NAME missing -- run 'vm-demo.sh build' first" >&2
    exit 1
  fi
  if [ ! -x "$APP_STAGE/$APP_NAME/$APP_BIN" ]; then
    echo "vm-demo.sh: $APP_STAGE/$APP_NAME/$APP_BIN missing -- re-run 'vm-demo.sh build'" >&2
    exit 1
  fi
}

# Unpack the tarball we scp'd in, de-quarantine and ad-hoc sign it. Any previous
# client in the guest is killed first so the relaunch owns the data dir.
guest_deploy_command() {
  cat <<'EOF'
set -e
pkill -x quantamfs >/dev/null 2>&1 || true
rm -rf ~/Desktop/QuantumFS.app
cd ~/Desktop && tar -xzf ~/QuantumFS.app.tgz
xattr -dr com.apple.quarantine ~/Desktop/QuantumFS.app || true
codesign --force --deep --sign - ~/Desktop/QuantumFS.app
EOF
}

# Start the app in a guest Terminal window with the log visible. The osascript
# is wrapped in a timeout because the first `do script` of a session can be slow
# enough to hit AppleScript's default limit.
guest_launch_command() {
  local addr="$1"
  cat <<EOF
cat >~/qfs-launch.applescript <<'SCPT'
with timeout of 90 seconds
  tell application "Terminal" to activate
  tell application "Terminal"
    do script "clear; QFS_DIRECTORY_ADDR=${addr} FORCE_COLOR=1 ~/Desktop/QuantumFS.app/Contents/MacOS/quantamfs 2>&1 | tee ~/qfs-client.log"
  end tell
end timeout
SCPT
osascript ~/qfs-launch.applescript >/dev/null
sleep 3
if pgrep -x quantamfs >/dev/null 2>&1; then
  echo "  quantamfs running"
else
  echo "  quantamfs NOT running -- check ~/qfs-client.log in the guest" >&2
fi
EOF
}

guest_ssh() {
  # $1 = guest IP, $2 = remote command
  local ip="$1" cmd="$2"
  if [ -n "${QFS_GUEST_PASSWORD:-}" ] && command -v expect >/dev/null 2>&1; then
    QFS_SSH_TARGET="admin@$ip" QFS_SSH_CMD="$cmd" expect <<'EOF'
set timeout 300
spawn -noecho ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
  $env(QFS_SSH_TARGET) $env(QFS_SSH_CMD)
expect {
  -re {(?i)password:} { send "$env(QFS_GUEST_PASSWORD)\r"; exp_continue }
  eof
}
EOF
  else
    ssh -o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
      "admin@$ip" "$cmd"
  fi
}

guest_scp() {
  # $1 = guest IP, $2 = local file, $3 = remote path (relative to admin's home)
  local ip="$1" src="$2" dest="$3"
  if [ -n "${QFS_GUEST_PASSWORD:-}" ] && command -v expect >/dev/null 2>&1; then
    QFS_SCP_SRC="$src" QFS_SCP_DEST="admin@$ip:$dest" expect <<'EOF'
set timeout 900
spawn -noecho scp -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
  $env(QFS_SCP_SRC) $env(QFS_SCP_DEST)
expect {
  -re {(?i)password:} { send "$env(QFS_GUEST_PASSWORD)\r"; exp_continue }
  eof
}
EOF
  else
    scp -o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
      "$src" "admin@$ip:$dest"
  fi
}

vm_is_running() {
  local vm="$1" row
  row="$(tart list 2>/dev/null | awk -v vm="$vm" '$2 == vm { print; exit }')"
  case "$row" in
    *running*) return 0 ;;
    *) return 1 ;;
  esac
}

# One tarball, reused for every guest. The virtiofs share (--dir) goes stale
# whenever the host directory is recreated, so the copy goes over scp instead.
stage_tarball() {
  echo "packing $APP_NAME..."
  rm -f "$APP_TGZ"
  tar -C "$APP_STAGE" -czf "$APP_TGZ" "$APP_NAME"
}

deploy_to_guests() {
  local n vm ip deploy launch
  require_directory_addr
  deploy="$(guest_deploy_command)"
  launch="$(guest_launch_command "$DIRECTORY_ADDR")"
  stage_tarball
  for n in $(seq 1 "$VM_COUNT"); do
    vm="qfs-client-$n"
    echo "waiting for $vm to get an IP..."
    ip="$(tart ip "$vm" --wait 120)"
    echo "$vm  $ip"
    echo "  copying $APP_NAME..."
    guest_scp "$ip" "$APP_TGZ" "QuantumFS.app.tgz"
    echo "  unpacking + signing..."
    guest_ssh "$ip" "$deploy"
    echo "  launching..."
    guest_ssh "$ip" "$launch"
    echo "$vm  app launched in a guest Terminal (log: ~/qfs-client.log)"
  done
}

do_build() {
  echo "building the app bundle..."
  ( cd "$CLIENT_DIR" && npm run app:build )
  if [ ! -d "$BUNDLE" ]; then
    echo "vm-demo.sh: expected bundle at $BUNDLE" >&2
    exit 1
  fi
  rm -rf "$APP_STAGE"
  mkdir -p "$APP_STAGE"
  cp -R "$BUNDLE" "$APP_STAGE/"
  echo "staged $APP_STAGE/$APP_NAME"
}

do_up() {
  require_stage
  require_directory_addr
  mkdir -p "$DEMO_DIR"
  echo "central directory: $DIRECTORY_ADDR"

  local n vm
  for n in $(seq 1 "$VM_COUNT"); do
    vm="qfs-client-$n"
    if vm_is_running "$vm"; then
      echo "$vm already running -- leaving it alone"
      continue
    fi
    echo "starting $vm..."
    # The --dir share is kept for future use; deployment does not rely on it.
    nohup tart run "$vm" --dir="qfs:$APP_STAGE" \
      >"$DEMO_DIR/vm-$n.log" 2>&1 &
  done

  deploy_to_guests

  echo
  echo "guests up. Paste a connect string from demo_servers.sh into each app."
}

do_redeploy() {
  require_stage
  require_directory_addr
  mkdir -p "$DEMO_DIR"
  echo "central directory: $DIRECTORY_ADDR"
  deploy_to_guests

  echo
  echo "relaunching the host client..."
  pkill -f "$APP_STAGE/$APP_NAME/$APP_BIN" >/dev/null 2>&1 || true
  do_host_client
}

do_down() {
  local n vm
  for n in $(seq 1 "$VM_COUNT"); do
    vm="qfs-client-$n"
    echo "stopping $vm..."
    tart stop "$vm" 2>/dev/null || true
  done
}

do_host_client() {
  require_stage
  require_directory_addr
  local data="$DEMO_DIR/client-host"
  mkdir -p "$data"
  echo "central directory: $DIRECTORY_ADDR"
  echo "launching the host client (data dir $data)"
  terminal_run "QFS CLIENT 3 (host)" \
    "clear; QFS_DATA_DIR=$(printf '%q' "$data") QFS_DIRECTORY_ADDR=$(printf '%q' "$DIRECTORY_ADDR") FORCE_COLOR=1 $(printf '%q' "$APP_STAGE/$APP_NAME/$APP_BIN") 2>&1 | tee $(printf '%q' "$DEMO_DIR/client-host.log")"
}

case "$ACTION" in
  build)       do_build ;;
  up)          do_up ;;
  redeploy)    do_redeploy ;;
  down)        do_down ;;
  host-client) do_host_client ;;
esac
