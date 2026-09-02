#!/usr/bin/env bash
# -----------------------------------------------------------------------------
# [INPUT]: bind key, --driver computer-use|native, and isolated Herdr/tmux
#          prerequisites; native driver additionally requires explicit global
#          input opt-in through ui-driver-guard.sh.
# [OUTPUT]: multiplexer acceptance evidence (JSONL + human summary) against
#           tmux ground truth; --prepare exposes a Computer Use/MCP session.
# [POS]: real-app acceptance boundary for the hosted TUI and mux adapters;
#        it owns only disposable test processes and never runtime state.
# [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
# -----------------------------------------------------------------------------
# Usage: scripts/mux-acceptance.sh [--driver computer-use|native] [--prepare]
#                              [--keep] [bind-key]
#   computer-use (default) prepares an isolated session for an Agent to drive
#   through mcp__node_repl__js + @oai/sky, then waits for PASS/FAIL marker.
#   native is a real-device fallback and must be explicitly authorized.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
KEEP=0
BIND_KEY="tmux:default"
DRIVER="${SHARDLANE_UI_DRIVER:-computer-use}"
PREPARE=0
ACCEPTANCE_TIMEOUT_SEC="${SHARDLANE_ACCEPTANCE_TIMEOUT_SEC:-900}"
while (($# > 0)); do
  case "$1" in
    --keep) KEEP=1; shift ;;
    --prepare) PREPARE=1; shift ;;
    --driver=computer-use|--driver=computer_use|--driver=cu) DRIVER="computer-use"; shift ;;
    --driver=native|--driver=cg-event|--driver=cg_event) DRIVER="native"; shift ;;
    --driver)
      (($# >= 2)) || { echo "FAIL setup: --driver requires computer-use or native" >&2; exit 2; }
      DRIVER="$2"; shift 2 ;;
    --help|-h)
      sed -n '1,28p' "$0"; exit 0 ;;
    --*)
      echo "FAIL setup: unknown option '$1'" >&2; exit 2 ;;
    *) BIND_KEY="$1"; shift ;;
  esac
done

case "$DRIVER" in
  computer-use|computer_use|cu) DRIVER="computer-use" ;;
  native|cg-event|cg_event) DRIVER="native" ;;
esac

[[ "$ACCEPTANCE_TIMEOUT_SEC" =~ ^[0-9]+$ && "$ACCEPTANCE_TIMEOUT_SEC" -gt 0 ]] || {
  echo "FAIL setup: SHARDLANE_ACCEPTANCE_TIMEOUT_SEC must be a positive integer" >&2
  exit 2
}

export SHARDLANE_UI_DRIVER="$DRIVER"
source "$ROOT/scripts/ui-driver-guard.sh"
case "$DRIVER" in
  computer-use)
    require_computer_use_ui_driver || exit $?
    if (( !PREPARE )); then
      cat >&2 <<'EOF'
Computer Use is the safe default. Add --prepare to start an isolated session
for an Agent to drive through mcp__node_repl__js + @oai/sky; this shell cannot
invoke the node_repl MCP itself. No global mouse/keyboard events were sent.
EOF
      exit 64
    fi
    ;;
  native)
    require_native_ui_driver || exit $?
    ;;
  *)
    echo "FAIL setup: unknown UI driver '$DRIVER'" >&2
    exit 2
    ;;
esac

if [[ -z "${HERDR_BIN:-}" ]]; then
  HERDR_BIN="$(command -v herdr || true)"
  [[ -n "$HERDR_BIN" ]] || HERDR_BIN="$HOME/.local/bin/herdr"
fi
[[ -x "$HERDR_BIN" ]] || { echo "FAIL setup: herdr CLI not at $HERDR_BIN" >&2; exit 1; }
command -v tmux >/dev/null || { echo "FAIL setup: tmux missing" >&2; exit 1; }
SHARDLANE_BIN="${SHARDLANE_BIN:-$ROOT/target/debug/shardlane}"
[[ -x "$SHARDLANE_BIN" ]] || { echo "FAIL setup: Shardlane binary not at $SHARDLANE_BIN" >&2; exit 1; }

BIN="$ROOT/target/debug"
if [[ "$DRIVER" == "native" ]]; then
  command -v swiftc >/dev/null || { echo "FAIL setup: swiftc missing" >&2; exit 1; }
  mkdir -p "$BIN"
  if [[ ! -x "$BIN/evpost" || "$ROOT/scripts/keyrepeat-evpost.swift" -nt "$BIN/evpost" ]]; then
    /usr/bin/swiftc -O -o "$BIN/evpost" "$ROOT/scripts/keyrepeat-evpost.swift" || exit 1
  fi
  if [[ ! -f "$ROOT/scripts/vision-ocr.swift" ]]; then
    echo "FAIL setup: scripts/vision-ocr.swift missing" >&2; exit 1
  fi
  if [[ ! -x "$BIN/vocr" || "$ROOT/scripts/vision-ocr.swift" -nt "$BIN/vocr" ]]; then
    /usr/bin/swiftc -O -o "$BIN/vocr" "$ROOT/scripts/vision-ocr.swift" || exit 1
  fi
  EVPOST="$BIN/evpost"; VOCR="$BIN/vocr"
fi

# Computer Use needs a stable bundle identity: launch the packaged .app whose
# Contents/MacOS/shardlane IS the dev binary, never a stale packaged copy
# (docs/ui-acceptance-testing.md §5 pitfall 1: same bundle id, two processes —
# the second window never comes on screen). Build the bundle from the dev
# binary when it is missing or older than the binary.
if [[ "$DRIVER" == "computer-use" ]]; then
  APP_BUNDLE="$BIN/bundle/osx/Shardlane.app"
  APP_EXEC="$APP_BUNDLE/Contents/MacOS/shardlane"
  if [[ ! -x "$APP_EXEC" || "$SHARDLANE_BIN" -nt "$APP_EXEC" ]]; then
    echo "[setup] packaging dev binary into $APP_BUNDLE"
    mkdir -p "$APP_BUNDLE/Contents/MacOS" "$APP_BUNDLE/Contents/Resources"
    cp "$SHARDLANE_BIN" "$APP_EXEC"
    chmod +x "$APP_EXEC"
    ICONSET="$BIN/bundle/osx/shardlane.iconset"
    mkdir -p "$ICONSET"
    cp "$ROOT/assets/app-icon/shardlane-16.png" "$ICONSET/icon_16x16.png"
    cp "$ROOT/assets/app-icon/shardlane-16@2x.png" "$ICONSET/icon_16x16@2x.png"
    cp "$ROOT/assets/app-icon/shardlane-32.png" "$ICONSET/icon_32x32.png"
    cp "$ROOT/assets/app-icon/shardlane-32@2x.png" "$ICONSET/icon_32x32@2x.png"
    cp "$ROOT/assets/app-icon/shardlane-128.png" "$ICONSET/icon_128x128.png"
    cp "$ROOT/assets/app-icon/shardlane-128@2x.png" "$ICONSET/icon_128x128@2x.png"
    cp "$ROOT/assets/app-icon/shardlane-256.png" "$ICONSET/icon_256x256.png"
    cp "$ROOT/assets/app-icon/shardlane-256@2x.png" "$ICONSET/icon_256x256@2x.png"
    cp "$ROOT/assets/app-icon/shardlane-512.png" "$ICONSET/icon_512x512.png"
    cp "$ROOT/assets/app-icon/shardlane-512@2x.png" "$ICONSET/icon_512x512@2x.png"
    iconutil -c icns "$ICONSET" -o "$APP_BUNDLE/Contents/Resources/shardlane.icns"
    cat > "$APP_BUNDLE/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key><string>en</string>
    <key>CFBundleExecutable</key><string>shardlane</string>
    <key>CFBundleIconFile</key><string>shardlane</string>
    <key>CFBundleIdentifier</key><string>dev.shardlane.app</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleName</key><string>Shardlane</string>
    <key>CFBundleDisplayName</key><string>Shardlane</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
    <key>CFBundleCategory</key><string>public.app-category.developer-tools</string>
</dict>
</plist>
PLIST
    codesign --force -s - "$APP_BUNDLE" >/dev/null 2>&1 || true
  fi
  [[ -x "$APP_EXEC" ]] || { echo "FAIL setup: bundle executable missing at $APP_EXEC" >&2; exit 1; }
  # Kill stale instances sharing the bundle id so the fresh window is visible.
  STALE_PIDS="$(pgrep -f 'Shardlane.app/Contents/MacOS/shardlane' || true)"
  if [[ -n "$STALE_PIDS" ]]; then
    echo "[setup] killing stale Shardlane instances: $STALE_PIDS"
    kill $STALE_PIDS 2>/dev/null || true
    sleep 1
  fi
  SHARDLANE_BIN="$APP_EXEC"
fi

RUNTIME=$(mktemp -d /tmp/shardlane-mux-accept.XXXXXX)
mkdir -p "$RUNTIME/home/.config/herdr"
export HOME="$RUNTIME/home" HERDR_SOCKET_PATH="$RUNTIME/herdr.sock"
export PATH="$HOME/.local/bin:$PATH"
export SHARDLANE_LAG_LOG_PATH="$RUNTIME/lag.log"
RESULT=0
APP_PID=""; HERDR_PID=""
CURRENT_PHASE="setup"
EVIDENCE="$RUNTIME/evidence.jsonl"
EVIDENCE_SUMMARY_DONE=0
note() { echo "[$1] $2"; }
record() {
  "$ROOT/scripts/acceptance-evidence.py" record "$EVIDENCE" \
    --driver "$DRIVER" --phase "$CURRENT_PHASE" --status "$1" --message "$2" >/dev/null
}
phase() { CURRENT_PHASE="$1"; }
pass() { note "PASS" "$*"; record pass "$*"; }
fail() { note "FAIL" "$*"; record fail "$*"; RESULT=1; }
summarize_evidence() {
  (( EVIDENCE_SUMMARY_DONE )) && return 0
  if ! "$ROOT/scripts/acceptance-evidence.py" summary "$EVIDENCE" \
    --require-phase runtime \
    --require-phase bind \
    --require-phase surface \
    --require-phase input \
    --require-phase resize \
    --require-phase menu \
    --require-phase switch; then
    note "FAIL" "evidence: ledger validation failed"
    RESULT=1
  fi
  EVIDENCE_SUMMARY_DONE=1
}
cleanup() {
  trap - EXIT INT TERM
  [[ -f "$EVIDENCE" ]] && summarize_evidence
  [[ -n "${APP_PID:-}" ]] && kill "$APP_PID" 2>/dev/null
  [[ -n "${HERDR_PID:-}" ]] && kill "$HERDR_PID" 2>/dev/null
  tmux -S "${TMUX_SOCK:-/nonexistent}" kill-server 2>/dev/null
  sleep 1
  if ((KEEP)) || ((RESULT)); then
    echo "runtime kept for debugging: $RUNTIME (RESULT=$RESULT)"
  else
    rm -rf "$RUNTIME"
  fi
  exit "$RESULT"
}
trap cleanup EXIT INT TERM

write_manifest() {
  {
    printf 'SCHEMA_VERSION=1\n'
    printf 'DRIVER=%q\n' "$DRIVER"
    printf 'BIND_INSTANCE=%q\n' "$BIND_KEY"
    printf 'RUNTIME_DIR=%q\n' "$RUNTIME"
    printf 'APP_PID=%q\n' "$APP_PID"
    printf 'APP_PATH=%q\n' "$SHARDLANE_BIN"
    printf 'HERDR_PID=%q\n' "$HERDR_PID"
    printf 'HERDR_SOCKET_PATH=%q\n' "$HERDR_SOCKET_PATH"
    printf 'TMUX_SOCKET_PATH=%q\n' "$TMUX_SOCK"
    printf 'TMUX_TARGET=%q\n' "demo-proj:0.0"
    printf 'LAG_LOG=%q\n' "$SHARDLANE_LAG_LOG_PATH"
    printf 'EVIDENCE=%q\n' "$EVIDENCE"
    printf 'ACCEPTANCE_TIMEOUT_SEC=%q\n' "$ACCEPTANCE_TIMEOUT_SEC"
    printf 'PASS_MARKER=%q\n' "$RUNTIME/PASS"
    printf 'FAIL_MARKER=%q\n' "$RUNTIME/FAIL"
  } > "$RUNTIME/manifest.env"
}

# ---- Phase 1: isolated runtime ----
phase runtime
# The tmux server runs on a DEDICATED socket and the app reaches it through
# SHARDLANE_TMUX_SOCKET — the user's default tmux server is never touched.
export SHARDLANE_TMUX_SOCKET="$RUNTIME/tmux.sock"
TMUX_SOCK="$SHARDLANE_TMUX_SOCKET"
"$HERDR_BIN" server > "$RUNTIME/server.log" 2>&1 & HERDR_PID=$!
for _ in $(seq 1 100); do [[ -e "$RUNTIME/herdr.sock" ]] && break; sleep 0.05; done
[[ -e "$RUNTIME/herdr.sock" ]] || { fail "runtime: herdr socket"; cleanup; }
tmux -S "$TMUX_SOCK" -f /dev/null new-session -d -s demo-proj -c /tmp || { fail "runtime: tmux session"; cleanup; }
tmux -S "$TMUX_SOCK" new-window -t demo-proj -n build -c /tmp
pass "runtime: isolated herdr + tmux demo-proj (2 windows, dedicated socket)"

# ---- Phase 2: bind via the automation seam ----
phase bind
export SHARDLANE_BIND_INSTANCE="$BIND_KEY"
PAUSE="${PAUSE:-0}"
"$SHARDLANE_BIN" > "$RUNTIME/app.log" 2>&1 & APP_PID=$!
if [[ "$DRIVER" == "computer-use" ]]; then
  # The app-scoped Computer Use path does not need a window raise or a global
  # event probe. Wait only for the backend bind acknowledgement and publish a
  # manifest that another Agent can consume through the MCP node_repl tool.
  for _ in $(seq 1 240); do
    if ! kill -0 "$APP_PID" 2>/dev/null; then
      fail "bind $BIND_KEY: app died"
      break
    fi
    if grep -q "bind.ok" "$RUNTIME/lag.log" 2>/dev/null; then
      pass "bind $BIND_KEY: app alive and bind.ok observed"
      break
    fi
    sleep 0.1
  done
  if ! grep -q "bind.ok" "$RUNTIME/lag.log" 2>/dev/null; then
    fail "bind flow never logged bind.ok"
    write_manifest
    cleanup
  fi
  write_manifest
  cat <<EOF

Computer Use session is ready (no CGEvent/osascript input was sent).
manifest=$RUNTIME/manifest.env
app_path=$SHARDLANE_BIN
app_pid=$APP_PID
lag_log=$RUNTIME/lag.log
tmux_socket=$TMUX_SOCK
tmux_target=demo-proj:0.0

Drive the app with mcp__node_repl__js + @oai/sky, targeting app_path. Re-read
sky.get_app_state after every action. Assert behavior with tmux capture-pane,
display-message, and list-panes. When done, create $RUNTIME/PASS or
$RUNTIME/FAIL (keep evidence JSONL in $EVIDENCE), then this process will clean
up its own Herdr/tmux/app processes.
EOF
  waited=0
  while [[ ! -e "$RUNTIME/PASS" && ! -e "$RUNTIME/FAIL" ]]; do
    if ! kill -0 "$APP_PID" 2>/dev/null; then
      fail "Computer Use acceptance app exited before marker"
      cleanup
    fi
    if (( waited >= ACCEPTANCE_TIMEOUT_SEC )); then
      fail "Computer Use acceptance marker timeout (${ACCEPTANCE_TIMEOUT_SEC}s)"
      cleanup
    fi
    sleep 1
    waited=$((waited + 1))
  done
  if [[ -e "$RUNTIME/PASS" && -e "$RUNTIME/FAIL" ]]; then
    fail "Computer Use acceptance created both PASS and FAIL markers"
  elif [[ -e "$RUNTIME/FAIL" ]]; then
    fail "Computer Use acceptance marked FAIL"
  else
    pass "Computer Use acceptance marked PASS"
  fi
  summarize_evidence
  cleanup
fi

sleep 12
kill -0 "$APP_PID" 2>/dev/null && pass "bind $BIND_KEY: app alive" || fail "bind $BIND_KEY: app died"
grep -q "bind.ok" "$RUNTIME/lag.log" && pass "bind flow reached the ok arm (lag bind.ok)" \
                                      || fail "bind flow never logged bind.ok"
write_manifest

front() {
  osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $APP_PID) to true" >/dev/null 2>&1
  # Activation can be silently ignored while the user works; a failed front
  # poisons every downstream assertion (input lands in someone else's app).
  local current tries=0
  while (( tries < 5 )); do
    current=$(osascript -e "tell application \"System Events\" to get unix id of (first application process whose frontmost is true)" 2>/dev/null)
    [[ "$current" == "$APP_PID" ]] && return 0
    osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $APP_PID) to true" >/dev/null 2>&1
    sleep 1
    tries=$((tries + 1))
  done
  echo "[WARN] frontmost is $current, wanted $APP_PID after $tries retries"+  return 1
}
wait_window() {
  for _ in $(seq 1 20); do
    [[ -n "$($EVPOST bounds "$APP_PID" 2>/dev/null)" ]] && return 0
    sleep 0.5
  done
  return 1
}

# ---- Phase 3: switch to the Terminal surface + attach ----
phase surface
front; wait_window || { fail "surface: app window never appeared"; cleanup; }
read -r WX WY WW WH <<< "$($EVPOST bounds "$APP_PID")"
# Backend-aware landing (2026-09-02): a bind without agent capability lands
# directly on the hosted work surface — no sidebar navigation is needed, and
# synthetic clicks cannot drive GPUI's custom on_click regions anyway.
sleep 3
if [[ "${PAUSE:-}" == "1" ]]; then
  echo "[PAUSE] inspect now ($RUNTIME)"; sleep 60
fi
ATTACH_CHILD=$(pgrep -f "$TMUX_SOCK.*(attach|attach-session)|tmux.*-S[[:space:]]*$TMUX_SOCK" | head -1)
if [[ -n "$ATTACH_CHILD" ]]; then
  pass "attach: tmux attach child process is running"
else
  fail "attach: no tmux attach child under the app"
  if [[ "${SHARDLANE_ALLOW_GLOBAL_CAPTURE:-0}" == "1" ]]; then
    screencapture -x -R "$WX,$WY,$WW,$WH" "$RUNTIME/attach-fail.png" 2>/dev/null
  else
    note "INFO" "attach: skipped shared-display screenshot (set SHARDLANE_ALLOW_GLOBAL_CAPTURE=1 to opt in)"
    record info "attach: shared-display screenshot skipped"
  fi
fi

# Ground truth lives on the CURRENT window (the one the attach client views);
# other windows of the session are never resized by the client.
CURRENT_TARGET="demo-proj"
cur_pane_width() {
  tmux -S "$TMUX_SOCK" display-message -p -t "$CURRENT_TARGET" '#{pane_width}'
}

# ---- Phase 4: input echo (ground truth = tmux capture-pane) ----
phase input
CX=$((WX + WW / 2)); CY=$((WY + WH / 2))
front
 front || fail "input: app could not be made frontmost; keystrokes would land elsewhere"
"$EVPOST" click "$CX" "$CY" >/dev/null
sleep 2
MARKER="mux-accept-$RANDOM"
osascript -e "tell application \"System Events\" to keystroke \"echo $MARKER\"" >/dev/null 2>&1
sleep 0.6
osascript -e 'tell application "System Events" to key code 36' >/dev/null 2>&1
ECHO_OK=0
for _ in $(seq 1 30); do
  if tmux -S "$TMUX_SOCK" capture-pane -t "$CURRENT_TARGET" -p 2>/dev/null | grep -q "$MARKER"; then ECHO_OK=1; break; fi
  sleep 0.5
done
((ECHO_OK)) && pass "input: marker echoed through the tmux attach child" \
               || fail "input: marker never reached the attach client's current window"

# ---- Phase 5: resize follows through to the tmux grid ----
phase resize
BEFORE_W=$(cur_pane_width)
NEW_W=$((WW - 220)); NEW_H=$((WH - 140))
osascript -e "tell application \"System Events\" to tell (first process whose unix id is $APP_PID) to set size of window 1 to {$NEW_W, $NEW_H}" >/dev/null 2>&1
sleep 3
AFTER_W=$(cur_pane_width)
if [[ -n "$BEFORE_W" && -n "$AFTER_W" && "$AFTER_W" != "$BEFORE_W" ]]; then
  pass "resize: pane width $BEFORE_W → $AFTER_W"
else
  fail "resize: pane width unchanged ($BEFORE_W → $AFTER_W)"
fi

front
# ---- Phase 6: pane operations through the app's native menu bar (AX) ----
phase menu
menu_click() {
  # AX menu press: works with the app frontmost, needs no OCR, no hitbox
  # geometry, and doubles as the accessibility regression (GPUI exposes the
  # native menu bar even though its custom surfaces have no AX elements).
  osascript -e "tell application \"System Events\" to tell (first process whose unix id is $APP_PID) to set frontmost to true" >/dev/null 2>&1
  sleep 0.5
  osascript -e "tell application \"System Events\" to tell (first process whose unix id is $APP_PID) to click menu item \"$1\" of menu \"Terminal\" of menu bar item \"Terminal\" of menu bar 1" >/dev/null 2>&1
}

menu_click "Split Right"
if true; then
  SPLIT_OK=0
  for _ in $(seq 1 16); do
    P=$(tmux -S "$TMUX_SOCK" list-panes -t "$CURRENT_TARGET" | wc -l | tr -d ' ')
    [[ "$P" == "2" ]] && { SPLIT_OK=1; break; }
    sleep 0.5
  done
  ((SPLIT_OK)) && pass "split: pane count 1 → 2" || fail "split: pane count still $P"
else
  fail "split: menu item not found (see menu-ocr-miss.txt)"
fi
# Zoom: window_zoomed_flag 0 → 1
if menu_click "Toggle Pane Zoom"; then
  ZOOM_OK=0
  for _ in $(seq 1 16); do
    Z=$(tmux -S "$TMUX_SOCK" display-message -p -t "$CURRENT_TARGET" '#{window_zoomed_flag}')
    [[ "$Z" == "1" ]] && { ZOOM_OK=1; break; }
    sleep 0.5
  done
  ((ZOOM_OK)) && pass "zoom: window_zoomed_flag 0 → 1" || fail "zoom: flag still $Z"
else
  fail "zoom: menu item not found"
fi
if menu_click "Close Pane"; then
  CLOSE_OK=0
  for _ in $(seq 1 16); do
    P=$(tmux -S "$TMUX_SOCK" list-panes -t "$CURRENT_TARGET" | wc -l | tr -d ' ')
    [[ "$P" == "1" ]] && { CLOSE_OK=1; break; }
    sleep 0.5
  done
  ((CLOSE_OK)) && pass "close: pane count 2 → 1" || fail "close: pane count still $P"
else
  fail "close: menu item not found"
fi

# ---- Phase 7: switch back to Herdr — tmux state untouched ----
phase switch
kill "$APP_PID" 2>/dev/null; sleep 2
export SHARDLANE_BIND_INSTANCE="default"
"$SHARDLANE_BIN" > "$RUNTIME/app2.log" 2>&1 & APP_PID=$!
sleep 12
kill -0 "$APP_PID" 2>/dev/null && pass "herdr switch: app alive bound to herdr/default" || fail "herdr switch: app died"
TMUX_AFTER=$(tmux -S "$TMUX_SOCK" list-windows -t demo-proj | wc -l | tr -d ' ')
check() { [[ "$2" == "$3" ]] && pass "$1" || fail "$1 (want '$2' got '$3')"; }
check "herdr switch: tmux state untouched" "2" "$TMUX_AFTER"
HERDR_TUI=$(pgrep -P "$APP_PID" -x herdr | wc -l | tr -d ' ')
((HERDR_TUI >= 1)) && pass "herdr switch: hosted TUI child spawned (direct children=$HERDR_TUI)" \
                    || fail "herdr switch: no hosted TUI child detected (direct children=$HERDR_TUI)"

echo "== acceptance RESULT=$RESULT (0 = all pass) =="
summarize_evidence
exit "$RESULT"
