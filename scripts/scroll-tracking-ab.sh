#!/usr/bin/env bash
# -----------------------------------------------------------------------------
# [INPUT]: macOS + herdr CLI + target/debug/shardlane (or SHARDLANE_BIN),
#          scripts/keyrepeat-evpost.swift (compiled on the fly with swiftc, includes the scroll injection command),
#          python3 + Pillow (screen-recording frame differencing), ffmpeg/ffprobe, osascript (System Events)
# [OUTPUT]: A repeatable scored measurement of hosted Herdr TUI scroll tracking:
#          under multi-screen scrollback (100 screens by default), deterministic wheel bursts
#          (up/down phases) are jointly scored against screen-recording frame differencing:
#          visible update rate / interval distribution / dropped-frame rate + scroll→present latency.
#          Each phase produces score(0-100) and PASS/WARN/FAIL; report compares across labels.
# [POS]: Terminal performance forensics harness (scroll-tracking line); owns no product runtime logic;
#        isolates HOME + socket and never touches the user's default session
# -----------------------------------------------------------------------------
#
# Scoring rubric (per phase): the user-visible property of scrolling is
# "no long freezes + continuous displacement", not per-vsync alignment with input
# (the herdr TUI's content repaint cadence of ~30Hz is a runtime boundary, and the
# window system splits synthetic wheel events into continuous events faster than
# vsync, so absolute thresholds would misjudge the boundary cadence as dropped frames). Therefore:
#   stall = visible update interval > 50ms (≈3 vsyncs of visible freeze);
#   score = clamp(100 - stall_ratio*60 - max(0,(25 - ups))*2, 0, 100)
#   verdict: PASS>=80, WARN>=55, otherwise FAIL.
# scroll→present latency includes the TUI round trip (report → PTY → Herdr repaint → read-back → extraction → on-screen),
# and is not item-by-item comparable with Ghostty's local scrollback scrolling (no subprocess round trip);
# it is only used for same-architecture before/after comparison and absolute-value recording.

set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
RUNTIME_ROOT="${SCROLL_RUNTIME_ROOT:-/tmp/shardlane-scroll-ab}"
EVPOST_SRC="$ROOT_DIR/keyrepeat-evpost.swift"
APP_NAME="Shardlane"
DISPLAY_PT_W=1800
# Main display point width (screencapture video pixels / this value = scale factor)
RECORD_SECONDS=4
DEFAULT_SAMPLES=3
DEFAULT_SCREENS=100      # 100 screens × 40 rows ≈ 4000 rows of scrollback
DEFAULT_EVENTS=45
DEFAULT_INTERVAL_US=30000
DEFAULT_AMOUNT=3         # line mode: 3 lines per event (mouse_scroll_lines default)
SEED_ROWS_PER_SCREEN=40

usage() {
  cat <<'EOF'
Usage:
  scroll-tracking-ab.sh run <label> [--samples N] [--screens N] [--pixel]
                             [--events N] [--interval-us US] [--amount V]
  scroll-tracking-ab.sh report <label>...

Defaults: samples=3 screens=100 line-mode amount=3 events=45 interval=30000us.
--pixel uses continuous (precision) point deltas — faithful trackpad emulation;
amount is then points per event (e.g. 18) and interval ~8000us.

Environment:
  SHARDLANE_BIN   default $ROOT_DIR/../target/debug/shardlane
  HERDR_BIN       default herdr on PATH (or ~/.local/bin/herdr)
EOF
}

log() { echo "[$(date +%H:%M:%S)][scroll-ab] $*"; }
die() { echo "[$(date +%H:%M:%S)][scroll-ab] FAIL: $*" >&2; exit 1; }

herdr_bin() {
  local bin="${HERDR_BIN:-$(command -v herdr || true)}"
  [[ -z "$bin" ]] && [[ -x "$HOME/.local/bin/herdr" ]] && bin="$HOME/.local/bin/herdr"
  [[ -n "$bin" && -x "$bin" ]] || die "herdr CLI not found"
  echo "$bin"
}

shardlane_bin() {
  local bin="${SHARDLANE_BIN:-$ROOT_DIR/../target/debug/shardlane}"
  [[ -x "$bin" ]] || die "shardlane binary not found: $bin"
  echo "$bin"
}

focus_window() {
  local attempt fpid app_pid="$1"
  for attempt in 1 2 3; do
    osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $app_pid) to true" >/dev/null 2>&1 || true
    sleep 0.4
    fpid="$(osascript -e 'tell application "System Events" to get unix id of first process whose frontmost is true' 2>/dev/null || echo 0)"
    [[ "$fpid" == "$app_pid" ]] && return 0
    log "focus attempt $attempt failed (frontmost=$fpid)"
  done
  die "could not focus app (frontmost=$fpid)"
}

cmd_run() {
  local label="$1"; shift
  local samples=$DEFAULT_SAMPLES screens=$DEFAULT_SCREENS pixel=0
  local events=$DEFAULT_EVENTS interval_us=$DEFAULT_INTERVAL_US amount=$DEFAULT_AMOUNT
  while (($# > 0)); do
    case "$1" in
      --samples) samples="$2"; shift 2 ;;
      --screens) screens="$2"; shift 2 ;;
      --pixel) pixel=1; shift ;;
      --events) events="$2"; shift 2 ;;
      --interval-us) interval_us="$2"; shift 2 ;;
      --amount) amount="$2"; shift 2 ;;
      *) usage >&2; die "unknown run option: $1" ;;
    esac
  done

  local runtime_dir="$RUNTIME_ROOT/$label"
  rm -rf "$runtime_dir"
  local home_dir="$runtime_dir/home"
  local socket_path="$runtime_dir/herdr.sock"
  local lag_log="$runtime_dir/shardlane-lag.log"
  local videos_dir="$runtime_dir/videos"
  mkdir -p "$home_dir/.config/herdr" "$videos_dir" "$runtime_dir/results"
  if [[ ! -x "$runtime_dir/evpost" || "$runtime_dir/evpost" -ot "$EVPOST_SRC" ]]; then
    swiftc -O "$EVPOST_SRC" -o "$runtime_dir/evpost"
  fi
  local evpost="$runtime_dir/evpost"

  # .app wrapper (screencapture video permission and stable window identity)
  local app_dir="$runtime_dir/app"
  mkdir -p "$app_dir/$APP_NAME.app/Contents/MacOS"
  cat > "$app_dir/$APP_NAME.app/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>dev.shardlane.app.scrollab</string>
  <key>CFBundleName</key><string>Shardlane</string>
  <key>CFBundleExecutable</key><string>Shardlane</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.0.0-scroll-ab</string>
</dict>
</plist>
EOF
  cp "$(shardlane_bin)" "$app_dir/$APP_NAME.app/Contents/MacOS/$APP_NAME"

  local herdr; herdr="$(herdr_bin)"
  HOME="$home_dir" HERDR_SOCKET_PATH="$socket_path" \
    nohup "$herdr" server > "$runtime_dir/herdr-server.log" 2>&1 &
  local server_pid=$!
  for _ in {1..100}; do [[ -e "$socket_path" ]] && break; sleep 0.05; done
  [[ -e "$socket_path" ]] || { tail -20 "$runtime_dir/herdr-server.log" >&2; die "herdr socket missing"; }

  : > "$lag_log"
  HOME="$home_dir" HERDR_SOCKET_PATH="$socket_path" \
    SHARDLANE_TERMINAL_TRACE=1 SHARDLANE_LAG_LOG_PATH="$lag_log" \
    nohup "$app_dir/$APP_NAME.app/Contents/MacOS/$APP_NAME" > "$runtime_dir/shardlane-app.log" 2>&1 &
  local app_pid=$!
  local bounds=""
  for _ in {1..60}; do
    bounds="$($evpost bounds "$app_pid" 2>/dev/null || true)"
    [[ -n "$bounds" ]] && break
    sleep 0.25
  done
  [[ -n "$bounds" ]] || die "app window never appeared"
  read -r wx wy ww wh <<< "$bounds"
  log "app pid=$app_pid window=[$bounds]"

  focus_window "$app_pid"
  HOME="$home_dir" HERDR_SOCKET_PATH="$socket_path" "$herdr" workspace create \
    --label scroll-ab --focus >/dev/null 2>&1 || true
  sleep 1
  $evpost click $((wx + 770)) $((wy + 76)) >/dev/null
  sleep 1.2
  $evpost click $((wx + 721)) $((wy + 749)) >/dev/null
  sleep 0.6
  local prev_src seed_cmd seed_lines
  seed_lines=$(( screens * SEED_ROWS_PER_SCREEN ))
  seed_cmd="seq ${seed_lines}"
  prev_src="$($evpost src get 2>/dev/null || true)"
  $evpost src set com.apple.keylayout.ABC >/dev/null 2>&1 || true
  sleep 0.3
  osascript -e "tell application \"System Events\" to keystroke \"$seed_cmd\"" >/dev/null 2>&1
  sleep 0.3
  osascript -e 'tell application "System Events" to key code 36' >/dev/null 2>&1
  [[ -n "$prev_src" && "$prev_src" != "com.apple.keylayout.ABC" ]] \
    && $evpost src set "$prev_src" >/dev/null 2>&1 || true
  local attached=0
  for _ in {1..40}; do
    if rg -q "tui.chrome projection" "$lag_log" 2>/dev/null; then attached=1; break; fi
    sleep 0.25
  done
  [[ "$attached" == 1 ]] || { kill -TERM "$app_pid" "$server_pid" 2>/dev/null || true; die "hosted TUI never attached after seed"; }
  sleep 2.5

  local unit="line"; ((pixel)) && unit="pixel"
  # The pointer must hover over the center of the pane content for wheel events to route to the hosted TUI
  local scroll_x=$((wx + ww / 2)) scroll_y=$((wy + wh * 55 / 100))
  local s sign
  for ((s=1; s<=samples; s++)); do
    for sign in up down; do
      local video="$videos_dir/$label-s${s}-${sign}.mov"
      rm -f "$video"
      ( screencapture -v -V "$RECORD_SECONDS" "$video" & )
      sleep 0.6
      local wheel_amount="$amount"
      # CGEvent line-mode positive values translate to scrolling down in this pipeline (measured 2026-08-31, no-op at the bottom);
      # the up phase (scrolling into history) needs negative values; the down phase (return) sends positive values.
      [[ "$sign" == "up" ]] && wheel_amount="-$amount"
      log "sample $s/$samples [$sign]: ${events}x${wheel_amount}$unit @${interval_us}us..."
      $evpost scroll "$scroll_x" "$scroll_y" "$wheel_amount" "$events" "$interval_us" $unit >/dev/null
      sleep 2.4
      local waited=0
      for _ in {1..24}; do [[ -s "$video" ]] && break; sleep 0.5; waited=$((waited+1)); done
      [[ -s "$video" ]] || { kill -TERM "$app_pid" "$server_pid" 2>/dev/null || true; die "$sign video failed to save"; }
    done
  done

  kill -TERM "$app_pid" "$server_pid" 2>/dev/null || true
  sleep 1
  log "analyzing (video × trace joint scoring)..."
  python3 - "$runtime_dir" "$label" "$samples" "$DISPLAY_PT_W" "$wx" "$wy" "$ww" "$wh" <<'PYEOF'
import os, re, shutil, statistics, subprocess, sys, tempfile
from PIL import Image

runtime, label, samples, disp_w = sys.argv[1], sys.argv[2], int(sys.argv[3]), float(sys.argv[4])
wx, wy, ww, wh = map(int, sys.argv[5:9])
lag_log = os.path.join(runtime, "shardlane-lag.log")
videos_dir = os.path.join(runtime, "videos")
results = os.path.join(runtime, "results")

pat = re.compile(r'\[(\d+\.\d+) pid=\d+ #\d+\] terminal\.trace t_us=\d+ stage=(\S+) (.*)')
scrolls, presents = [], []
for line in open(lag_log, errors="ignore").readlines()[-120000:]:
    m = pat.match(line)
    if not m:
        continue
    t, stage, rest = float(m.group(1)), m.group(2), m.group(3)
    if stage == "ui.scroll" and "route=mouse_report" in rest:
        sm = re.search(r"steps=(-?\d+)", rest)
        if sm and int(sm.group(1)) != 0:
            scrolls.append(t)
    elif stage == "frame.present_request":
        presents.append(t)

clusters, cur = [], []
for t in scrolls:
    if cur and t - cur[-1] > 0.5:
        clusters.append(cur); cur = []
    cur.append(t)
if cur:
    clusters.append(cur)

def med(v):
    return sorted(v)[len(v) // 2] if v else 0.0

def visible_states(video):
    tmp = tempfile.mkdtemp(prefix="scroll-frames-")
    pr = subprocess.run(["ffprobe","-v","error","-select_streams","v:0","-show_entries","stream=width","-of","csv=p=0",video],capture_output=True,text=True)
    frame_w = int(pr.stdout.strip())
    scale = frame_w / disp_w
    crop = [int(v * scale) for v in (wx, wy, ww, wh)]
    subprocess.run(["ffmpeg","-v","error","-i",video,"-vf",
      f"crop={crop[2]}:{crop[3]}:{crop[0]}:{crop[1]},format=gray,scale=880:719",
      os.path.join(tmp,"f_%04d.png")], check=True)
    files = sorted(os.listdir(tmp))
    state = None; times = []; idx = 0
    for f in files:
        px = list(Image.open(os.path.join(tmp, f)).convert("L").getdata()); idx += 1
        if state is None:
            state = px; continue
        d = sum(1 for a, b in zip(state, px) if abs(a - b) > 24) / len(px) * 100.0
        if d > 0.02:
            times.append(idx); state = px
    shutil.rmtree(tmp, ignore_errors=True)
    return times

lines = []
scores, verdicts = [], []
for s in range(1, samples + 1):
    for phase, idx in (("up", (s - 1) * 2), ("down", (s - 1) * 2 + 1)):
        name = f"s{s}-{phase}"
        video = os.path.join(videos_dir, f"{label}-{name}.mov")
        cl = clusters[idx] if idx < len(clusters) else []
        if not cl or not os.path.exists(video):
            lines.append(f"[{name}] insufficient data (trace_events={len(cl)} video={os.path.exists(video)})")
            continue
        times = visible_states(video)
        if len(times) < 3:
            lines.append(f"[{name}] insufficient visible states={len(times)} trace_events={len(cl)}")
            continue
        period_ms = med([cl[i + 1] - cl[i] for i in range(len(cl) - 1)]) * 1000.0
        gaps = sorted((times[i + 1] - times[i]) * 1000.0 / 60.0 for i in range(len(times) - 1))
        n = len(gaps)
        span = times[-1] - times[0] + 1
        ups = len(times) * 60.0 / span
        stalls = sum(1 for g in gaps if g > 50.0)
        stall_ratio = stalls / n
        score = int(max(0.0, min(100.0, 100 - stall_ratio * 60 - max(0.0, (25 - ups)) * 2)))
        verdict = "PASS" if score >= 80 else ("WARN" if score >= 55 else "FAIL")
        lat = sorted((min(p for p in presents if p >= k) - k) * 1000 for k in cl if any(p >= k for p in presents))
        scores.append(score); verdicts.append(verdict)
        lines.append(f"[{name}] input={len(cl)}events@{period_ms:.0f}ms visible={len(times)} ups={ups:.1f} gap_p50={gaps[n//2]:.0f} gap_max={gaps[-1]:.0f} stall={stalls}/{n}")
        if lat:
            lines.append(f"[{name}] scroll->present_ms p50={lat[len(lat)//2]:.1f} p95={lat[int(len(lat)*0.95)-1]:.1f} max={lat[-1]:.1f}")
        lines.append(f"[{name}] score={score} verdict={verdict}")

if scores:
    lines.append(f"scores={','.join(map(str, scores))}")
    lines.append(f"median_score={sorted(scores)[len(scores)//2]}")
    lines.append(f"verdict={min(verdicts, key=['PASS','WARN','FAIL'].index)}")
print("\n".join(lines))
with open(os.path.join(results, f"{label}.txt"), "a") as f:
    f.write("\n".join(lines) + "\n")
PYEOF
  log "result written: $runtime_dir/results/$label.txt"
}

cmd_report() {
  python3 - "$RUNTIME_ROOT" "$@" <<'PYEOF'
import os, re, sys
root = sys.argv[1]
print(f"{'label':<16} {'median':>7} {'worst':>7}  verdict")
for label in sys.argv[2:]:
    path = os.path.join(root, label, "results", f"{label}.txt")
    if not os.path.exists(path):
        print(f"{label:<16} {'-':>7} {'-':>7}  MISSING")
        continue
    text = open(path).read()
    scores = [int(m) for m in re.findall(r"score=(\d+)", text)]
    verdicts = re.findall(r"verdict=(PASS|WARN|FAIL)", text)
    if not scores:
        print(f"{label:<16} {'-':>7} {'-':>7}  NO-DATA")
        continue
    med = sorted(scores)[len(scores)//2]
    worst = min(verdicts, key=["PASS","WARN","FAIL"].index)
    print(f"{label:<16} {med:>7} {min(scores):>7}  {worst}")
PYEOF
}

case "${1:-}" in
  run) shift; cmd_run "$@" ;;
  report) shift; cmd_report "$@" ;;
  --help|-h|"") usage ;;
  *) usage >&2; die "unknown command: $1" ;;
esac
