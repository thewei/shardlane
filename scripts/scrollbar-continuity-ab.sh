#!/usr/bin/env bash
# -----------------------------------------------------------------------------
# [INPUT]: macOS + herdr CLI + target/debug/shardlane (or SHARDLANE_BIN),
#          scripts/keyrepeat-evpost.swift (compiled on the fly with swiftc into the event injection tool),
#          python3 + Pillow (scrollbar-column pixel continuity scoring), osascript (System Events),
#          and scripts/ui-driver-guard.sh (explicit native global-input authorization)
# [OUTPUT]: A repeatable scored measurement of hosted Herdr TUI vertical scrollbar rendering continuity:
#          a single run command starts server+app in a fresh isolated HOME+socket, clicks the
#          Terminal segment with evpost + seeds seq 400 into the composer, captures N consecutive
#          window samples, locates the scrollbar column on the right edge of each and counts
#          background gaps (segment count / longest gap / coverage), then summarizes into
#          score(0-100) with a PASS/WARN/FAIL verdict; report compares across labels
# [POS]: Terminal presentation forensics harness (scrollbar-continuity line); owns no product runtime logic;
#        isolates HOME + socket and never touches the user's default session
# -----------------------------------------------------------------------------
#
# Scoring rubric: within the vertical ink span of the scrollbar column, the background-gap
# count gap_count and the longest gap max_gap_px directly measure segmentation artifacts
# (a healthy render should have 0 gaps; flex-stretch artifacts with a 1px bottom seam per row
# produce a characteristic one-gap-every-2-3-rows distribution).
#   score = clamp(100 - gap_count*2.5 - max_gap_px*10, 0, 100)
#   verdict: PASS>=85, WARN>=60, otherwise FAIL.

set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
# This harness posts global mouse/keyboard events. Require an explicit
# real-device opt-in; Computer Use/MCP is the preferred app-scoped driver.
# shellcheck source=scripts/ui-driver-guard.sh
source "$ROOT_DIR/scripts/ui-driver-guard.sh"
RUNTIME_ROOT="${SCROLLBAR_RUNTIME_ROOT:-/tmp/shardlane-scrollbar-ab}"
EVPOST_SRC="$ROOT_DIR/scripts/keyrepeat-evpost.swift"
# Fixed New Task layout: window-relative coordinates (pt) of the Terminal segment button and command composer
SEGMENT_TERMINAL_REL_X=770
SEGMENT_TERMINAL_REL_Y=76
COMPOSER_REL_X=721
COMPOSER_REL_Y=749
SEED_COMMAND="seq 400"
DEFAULT_SAMPLES=3

usage() {
  cat <<'EOF'
Usage:
  scrollbar-continuity-ab.sh run <label> [--samples N]   # fresh isolated environment, N scored samples (default 3)
  scrollbar-continuity-ab.sh report <label>...           # compare score/verdict/gap metrics across labels

Environment:
  SHARDLANE_BIN   default $ROOT_DIR/target/debug/shardlane
  HERDR_BIN       default herdr on PATH (or ~/.local/bin/herdr)
EOF
}

log() { echo "[$(date +%H:%M:%S)][scrollbar-ab] $*"; }
die() { echo "[$(date +%H:%M:%S)][scrollbar-ab] FAIL: $*" >&2; exit 1; }

herdr_bin() {
  local bin="${HERDR_BIN:-$(command -v herdr || true)}"
  [[ -z "$bin" ]] && [[ -x "$HOME/.local/bin/herdr" ]] && bin="$HOME/.local/bin/herdr"
  [[ -n "$bin" && -x "$bin" ]] || die "herdr CLI not found"
  echo "$bin"
}

shardlane_bin() {
  local bin="${SHARDLANE_BIN:-$ROOT_DIR/target/debug/shardlane}"
  [[ -x "$bin" ]] || die "shardlane binary not found: $bin"
  echo "$bin"
}

ensure_evpost() {
  local dir="$1"
  if [[ ! -x "$dir/evpost" || "$dir/evpost" -ot "$EVPOST_SRC" ]]; then
    log "compiling evpost..."
    swiftc -O "$EVPOST_SRC" -o "$dir/evpost"
  fi
}

focus_window() {
  local attempt fpid app_pid="$1"
  for attempt in 1 2 3; do
    osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $app_pid) to true" >/dev/null 2>&1 || true
    sleep 0.4
    fpid="$(osascript -e 'tell application "System Events" to get unix id of first process whose frontmost is true' 2>/dev/null || echo 0)"
    [[ "$fpid" == "$app_pid" ]] && return 0
  done
  die "could not focus app (frontmost=$fpid)"
}

score_sample() {
  # python: <sample.png> -> single-line JSON result
  python3 - "$1" <<'PYEOF'
import json, sys
from PIL import Image

img = Image.open(sys.argv[1]).convert("RGB")
w, h = img.size
px = img.load()

def ink(p, bg):
    return abs(p[0]-bg[0]) + abs(p[1]-bg[1]) + abs(p[2]-bg[2]) > 60

# Background sampling: blank column in the middle of the content area
bg = px[w // 4, h // 2]

# Within the right-edge 12% region, find the column with the largest vertical ink total
# (the scrollbar track); inset 12 device pixels to avoid window border/corner-shadow columns.
# Anchor by "total ink rows" rather than "longest consecutive run": segmentation artifacts
# chop consecutive runs into pieces but do not change the track column's ink total.
x_start = int(w * 0.88)
x_end = w - 12
best = (0, -1, 0, 0)  # (total_ink, x, y0, y1)
for x in range(x_start, x_end):
    total = 0
    y0 = y1 = -1
    for y in range(int(h * 0.08), int(h * 0.95)):
        if ink(px[x, y], bg):
            total += 1
            if y0 < 0:
                y0 = y
            y1 = y
    if total > best[0]:
        best = (total, x, y0, y1)
total, x, y0, y1 = best
if y0 < 0 or (y1 - y0 + 1) < 100:
    print(json.dumps({"found": False}))
    sys.exit(0)

gaps = []
y = y0
while y <= y1:
    if not ink(px[x, y], bg):
        s = y
        while y <= y1 and not ink(px[x, y], bg):
            y += 1
        gaps.append(y - s)
    else:
        y += 1
gap_count = len(gaps)
max_gap = max(gaps) if gaps else 0
coverage = 1.0 - sum(gaps) / (y1 - y0 + 1)
score = max(0.0, min(100.0, 100 - gap_count * 2.5 - max_gap * 10))
verdict = "PASS" if score >= 85 else ("WARN" if score >= 60 else "FAIL")
print(json.dumps({
    "found": True, "x": x, "span": [y0, y1], "gap_count": gap_count,
    "max_gap_px": max_gap, "coverage": round(coverage, 4),
    "score": round(score, 1), "verdict": verdict,
}))
PYEOF
}

cmd_run() {
  require_native_ui_driver
  require_global_capture
  local label="$1"; shift
  local samples="$DEFAULT_SAMPLES"
  while (($# > 0)); do
    case "$1" in
      --samples) samples="$2"; shift 2 ;;
      *) usage >&2; die "unknown run option: $1" ;;
    esac
  done
  [[ "$samples" =~ ^[0-9]+$ && "$samples" -ge 1 ]] || die "--samples must be a positive integer"

  local runtime_dir="$RUNTIME_ROOT/$label"
  rm -rf "$runtime_dir"
  local home_dir="$runtime_dir/home"
  local socket_path="$runtime_dir/herdr.sock"
  mkdir -p "$home_dir/.config/herdr" "$runtime_dir/samples" "$runtime_dir/results"
  ensure_evpost "$runtime_dir"

  local herdr; herdr="$(herdr_bin)"
  HOME="$home_dir" HERDR_SOCKET_PATH="$socket_path" \
    nohup "$herdr" server > "$runtime_dir/herdr-server.log" 2>&1 &
  local server_pid=$!
  for _ in {1..100}; do [[ -e "$socket_path" ]] && break; sleep 0.05; done
  [[ -e "$socket_path" ]] || { tail -20 "$runtime_dir/herdr-server.log" >&2; die "herdr socket missing"; }

  local app_pid=""
  HOME="$home_dir" HERDR_SOCKET_PATH="$socket_path" \
    SHARDLANE_LAG_LOG_PATH="$runtime_dir/shardlane-lag.log" \
    nohup "$(shardlane_bin)" > "$runtime_dir/shardlane-app.log" 2>&1 &
  app_pid=$!
  local bounds=""
  for _ in {1..60}; do
    bounds="$("$runtime_dir/evpost" bounds "$app_pid" 2>/dev/null || true)"
    [[ -n "$bounds" ]] && break
    sleep 0.25
  done
  [[ -n "$bounds" ]] || die "app window never appeared"
  read -r wx wy ww wh <<< "$bounds"
  log "app pid=$app_pid window=[$bounds]"

  focus_window "$app_pid"
  # A Project is a precondition for the Terminal segment to appear; create one via the isolated socket, then click the segment
  HOME="$home_dir" HERDR_SOCKET_PATH="$socket_path" "$herdr" workspace create \
    --label scrollbar-ab --focus >/dev/null 2>&1 || true
  sleep 1
  "$runtime_dir/evpost" click $((wx + SEGMENT_TERMINAL_REL_X)) $((wy + SEGMENT_TERMINAL_REL_Y)) >/dev/null
  sleep 1.2
  # The hosted TUI surface mounts only after the composer submits a command; switch to the ABC input source first to avoid IME
  "$runtime_dir/evpost" click $((wx + COMPOSER_REL_X)) $((wy + COMPOSER_REL_Y)) >/dev/null
  sleep 0.6
  local prev_src
  prev_src="$("$runtime_dir/evpost" src get 2>/dev/null || true)"
  "$runtime_dir/evpost" src set com.apple.keylayout.ABC >/dev/null 2>&1 || true
  sleep 0.3
  osascript -e "tell application \"System Events\" to keystroke \"$SEED_COMMAND\"" >/dev/null 2>&1
  sleep 0.3
  osascript -e 'tell application "System Events" to key code 36' >/dev/null 2>&1
  [[ -n "$prev_src" && "$prev_src" != "com.apple.keylayout.ABC" ]] \
    && "$runtime_dir/evpost" src set "$prev_src" >/dev/null 2>&1 || true
  sleep 2.5

  local scores=() s result
  for s in $(seq 1 "$samples"); do
    local shot="$runtime_dir/samples/s${s}.png"
    screencapture -x -o -R "$wx,$wy,$ww,$wh" "$shot"
    result="$(score_sample "$shot")"
    if [[ "$result" != *'"found": true'* ]]; then
      kill -TERM "$app_pid" "$server_pid" 2>/dev/null || true
      die "sample s$s: scrollbar column not found (saved $shot for inspection)"
    fi
    scores+=("$(python3 -c 'import json,sys; print(json.load(sys.stdin)["score"])' <<<"$result")")
    log "sample s$s: $result"
  done

  local summary
  summary="$(python3 - "$label" "${scores[@]}" <<'PYEOF'
import json, statistics, sys
label = sys.argv[1]
scores = [float(v) for v in sys.argv[2:]]
median = statistics.median(scores)
worst = min(scores)
verdict = "PASS" if worst >= 85 else ("WARN" if worst >= 60 else "FAIL")
print(json.dumps({"label": label, "samples": scores, "median": median, "worst": worst, "verdict": verdict}))
PYEOF
)"
  echo "$summary" > "$runtime_dir/results/summary.json"
  log "summary: $summary"

  kill -TERM "$app_pid" "$server_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  wait "$server_pid" 2>/dev/null || true
}

cmd_report() {
  python3 - "$RUNTIME_ROOT" "$@" <<'PYEOF'
import json, os, sys
root = sys.argv[1]
print(f"{'label':<16} {'median':>7} {'worst':>7}  verdict")
for label in sys.argv[2:]:
    path = os.path.join(root, label, "results", "summary.json")
    if not os.path.exists(path):
        print(f"{label:<16} {'-':>7} {'-':>7}  MISSING")
        continue
    data = json.load(open(path))
    print(f"{label:<16} {data['median']:>7.1f} {data['worst']:>7.1f}  {data['verdict']}")
PYEOF
}

case "${1:-}" in
  run) shift; cmd_run "$@" ;;
  report) shift; cmd_report "$@" ;;
  --help|-h|"" ) usage ;;
  *) usage >&2; die "unknown command: $1" ;;
esac
