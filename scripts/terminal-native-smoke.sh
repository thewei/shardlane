#!/usr/bin/env bash
# -----------------------------------------------------------------------------
# [INPUT]: macOS + herdr CLI + target/debug/shardlane (or the build artifact named by SHARDLANE_BIN)
# [OUTPUT]: A Shardlane native Terminal smoke run with isolated HOME/socket/log; after the session ends it
#           cleans up only the app/server/runtime this script created; prints Ghostty A/B commands and trace
#           analysis commands for the same runtime
# [POS]: Terminal performance forensics harness; owns no product runtime logic and never touches the user's default Herdr session
# -----------------------------------------------------------------------------

set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD=0
KEEP=0

usage() {
  cat <<'EOF'
Usage: scripts/terminal-native-smoke.sh [--build] [--keep]

Start one isolated Herdr server and one Shardlane native app for manual Terminal
checks. The script prints the exact Ghostty command and trace parser invocation.

Options:
  --build  build target/debug/shardlane before starting the smoke
  --keep   keep the temporary runtime/log directory after the app exits
EOF
}

while (($# > 0)); do
  case "$1" in
    --build)
      BUILD=1
      ;;
    --keep)
      KEEP=1
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "terminal-native-smoke.sh requires macOS (Darwin)" >&2
  exit 1
fi

HERDR_BIN="${HERDR_BIN:-}"
if [[ -z "$HERDR_BIN" ]]; then
  HERDR_BIN="$(command -v herdr || true)"
fi
if [[ -z "$HERDR_BIN" || ! -x "$HERDR_BIN" ]]; then
  echo "herdr CLI not found; set HERDR_BIN or install Herdr with wax" >&2
  exit 1
fi

SHARDLANE_BIN="${SHARDLANE_BIN:-$ROOT_DIR/target/debug/shardlane}"
if ((BUILD)); then
  cargo build --locked -p shardlane --bin shardlane
fi
if [[ ! -x "$SHARDLANE_BIN" ]]; then
  echo "Shardlane binary not found: $SHARDLANE_BIN (pass --build or set SHARDLANE_BIN)" >&2
  exit 1
fi

RUNTIME_DIR="$(mktemp -d /tmp/shardlane-terminal-smoke.XXXXXX)"
SMOKE_HOME="$RUNTIME_DIR/home"
SOCKET_PATH="$RUNTIME_DIR/herdr.sock"
TRACE_LOG="$RUNTIME_DIR/shardlane-lag.log"
SERVER_LOG="$RUNTIME_DIR/herdr-server.log"
APP_LOG="$RUNTIME_DIR/shardlane.log"
mkdir -p "$SMOKE_HOME/.config/herdr"

SERVER_PID=""
APP_PID=""

cleanup() {
  local status=$?
  # INT/TERM also cause EXIT after this function returns; disarm all three
  # traps before terminating so the exact runtime directory is cleaned once.
  trap - EXIT INT TERM
  set +e
  if [[ -n "$APP_PID" ]] && kill -0 "$APP_PID" 2>/dev/null; then
    kill -TERM "$APP_PID" 2>/dev/null
  fi
  if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill -TERM "$SERVER_PID" 2>/dev/null
  fi
  [[ -z "$APP_PID" ]] || wait "$APP_PID" 2>/dev/null
  [[ -z "$SERVER_PID" ]] || wait "$SERVER_PID" 2>/dev/null
  if ((KEEP)); then
    echo "kept runtime: $RUNTIME_DIR"
  else
    rm -rf "$RUNTIME_DIR"
  fi
  exit "$status"
}
trap cleanup EXIT INT TERM

HERDR_DIR="$(dirname -- "$HERDR_BIN")"
SMOKE_PATH="$HERDR_DIR:${PATH:-/usr/bin:/bin}"

(
  export HOME="$SMOKE_HOME"
  export HERDR_SOCKET_PATH="$SOCKET_PATH"
  export PATH="$SMOKE_PATH"
  exec "$HERDR_BIN" server
) >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!

socket_ready=0
for _ in {1..100}; do
  if [[ -e "$SOCKET_PATH" ]]; then
    socket_ready=1
    break
  fi
  sleep 0.05
done
if (( ! socket_ready )); then
  echo "Herdr socket did not appear: $SOCKET_PATH" >&2
  tail -80 "$SERVER_LOG" >&2 || true
  exit 1
fi

(
  export HOME="$SMOKE_HOME"
  export HERDR_SOCKET_PATH="$SOCKET_PATH"
  export PATH="$SMOKE_PATH"
  export SHARDLANE_TERMINAL_TRACE=1
  export SHARDLANE_SCROLL_DEBUG=1
  export SHARDLANE_LAG_LOG_PATH="$TRACE_LOG"
  exec "$SHARDLANE_BIN"
) >"$APP_LOG" 2>&1 &
APP_PID=$!

cat <<EOF
Terminal native smoke is ready.
app_pid=$APP_PID
server_pid=$SERVER_PID
runtime_dir=$RUNTIME_DIR
home=$SMOKE_HOME
socket=$SOCKET_PATH
app_log=$APP_LOG
trace_log=$TRACE_LOG

Manual Shardlane checks:
  1. Open the hosted Herdr TUI and hold a printable key; repeat with Ctrl/Alt/Cmd chords.
  2. Exercise slow/fast/momentum scroll, click, drag selection, and an actual IME candidate commit.
  3. Keep the same isolated server alive for the Ghostty comparison below.

Ghostty A/B command (same HOME/socket, no Shardlane process involved):
  # `open` itself does not reliably propagate shell environment into a
  # LaunchServices-launched app, so pass the isolated values through `/usr/bin/env`
  # inside Ghostty's `-e` command as well.
  open -na /Applications/Ghostty.app --args -e /usr/bin/env \\
    HOME="$SMOKE_HOME" HERDR_SOCKET_PATH="$SOCKET_PATH" PATH="$SMOKE_PATH" "$HERDR_BIN"

Trace analysis after exit (the parser never prints terminal text):
  python3 "$ROOT_DIR/scripts/analyze-terminal-trace.py" "$TRACE_LOG" --pid "$APP_PID"
EOF

app_status=0
wait "$APP_PID" || app_status=$?
exit "$app_status"
