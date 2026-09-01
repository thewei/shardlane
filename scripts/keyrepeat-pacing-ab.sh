#!/usr/bin/env bash
# -----------------------------------------------------------------------------
# [INPUT]: macOS + herdr CLI + target/debug/shardlane (or SHARDLANE_BIN),
#          scripts/keyrepeat-evpost.swift (compiled on the fly with swiftc into the event injection tool),
#          python3 + Pillow (screen-recording frame differencing), ffmpeg/ffprobe, osascript (System Events)
# [OUTPUT]: A repeatable scored measurement of long-press rendering cadence (key-repeat pacing):
#          setup/snap establish a seeded isolated environment; run executes N consecutive sampling
#          rounds within a single app session (deterministic navigation to Terminal+composer → esc/gg reset →
#          screen recording → 30Hz autorepeat burst); each round produces visible-state count / update rate /
#          interval distribution / dropped-frame rate, summarized into score(0-100) with a PASS/WARN/FAIL verdict;
#          report compares across labels
# [POS]: Terminal performance forensics harness (the pacing line of the latency-evidence playbook);
#        owns no product runtime logic; isolates HOME + socket and never touches the user's default session
# -----------------------------------------------------------------------------

set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
RUNTIME_DIR="${PACING_RUNTIME_DIR:-/tmp/shardlane-pacing-ab}"
HOME_DIR="$RUNTIME_DIR/home"
SEED_DIR="$RUNTIME_DIR/seed-home"
APP_DIR="$RUNTIME_DIR/app"
SOCKET_PATH="$RUNTIME_DIR/herdr.sock"
LAG_LOG="$RUNTIME_DIR/shardlane-lag.log"
RESULTS_DIR="$RUNTIME_DIR/results"
VIDEOS_DIR="$RUNTIME_DIR/videos"
SAMPLES_DIR="$RUNTIME_DIR/samples"
EVPOST="$RUNTIME_DIR/evpost"
APP_NAME="Shardlane"
# Main display point width (screencapture video pixels / this value = scale factor)
DISPLAY_PT_W=1800
# 30Hz × 45 presses: faithful emulation of the macOS "fast" key repeat rate
KEYCODE_J=38
KEYCODE_ESC=53
KEYCODE_G=5
KEYCODE_BACKSPACE=51
KEYCOUNT=45
KEYINTERVAL_US=30000
INPUT_PERIOD_MS=33   # nominal input period, scoring baseline
RECORD_SECONDS=4
DEFAULT_SAMPLES=3
BACKSPACE_FILL=60    # filler characters typed first in backspace mode

usage() {
  cat <<'EOF'
Usage:
  keyrepeat-pacing-ab.sh setup                        # one-time: fresh isolated environment, wait for the operator (CUA) to build Project+Terminal+nvim
  keyrepeat-pacing-ab.sh snap                         # snapshot the current prepared HOME as the seed
  keyrepeat-pacing-ab.sh run <label> [--samples N] [--profile]
                                                      # N consecutive sampling rounds in a single session (default 3), outputs score and verdict
                          [--mode nav|insert|backspace]
                                                      # nav=j scrolling in normal mode (default); insert=type characters repeatedly in insert mode;
                                                      # backspace=type filler first in insert mode, then burst backspace
  keyrepeat-pacing-ab.sh report <label>...            # compare score/verdict/key metrics across labels

Environment:
  SHARDLANE_BIN   default $ROOT_DIR/target/debug/shardlane
  HERDR_BIN       default herdr on PATH (or ~/.local/bin/herdr)

Scoring rubric (per round): input_period≈33ms; slip = visible update interval > 45ms (≈1 dropped vsync);
  updates/s = visible state update rate within the active span;
  score = clamp(100 - slip_ratio*60 - max(0,(25-updates/s))*2, 0, 100);
  verdict: PASS>=80, WARN>=55, otherwise FAIL.
EOF
}

log() { echo "[$(date +%H:%M:%S)][pacing-ab] $*"; }
die() { echo "[$(date +%H:%M:%S)][pacing-ab] FAIL: $*" >&2; exit 1; }

ensure_tooling() {
  mkdir -p "$RUNTIME_DIR" "$RESULTS_DIR" "$VIDEOS_DIR" "$SAMPLES_DIR"
  if [[ ! -x "$EVPOST" || "$EVPOST" -ot "$ROOT_DIR/scripts/keyrepeat-evpost.swift" ]]; then
    log "compiling evpost..."
    swiftc -O "$ROOT_DIR/scripts/keyrepeat-evpost.swift" -o "$EVPOST"
  fi
  command -v ffmpeg >/dev/null || die "ffmpeg not found"
  python3 -c 'import PIL' 2>/dev/null || die "python3 Pillow not found"
}

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

ensure_server() {
  if [[ -e "$SOCKET_PATH" ]]; then
    log "reusing isolated herdr server (socket present)"
    return
  fi
  local bin; bin="$(herdr_bin)"
  HOME="$HOME_DIR" HERDR_SOCKET_PATH="$SOCKET_PATH" PATH="$PATH" \
    nohup "$bin" server > "$RUNTIME_DIR/herdr-server.log" 2>&1 &
  for _ in {1..100}; do [[ -e "$SOCKET_PATH" ]] && break; sleep 0.05; done
  [[ -e "$SOCKET_PATH" ]] || { tail -20 "$RUNTIME_DIR/herdr-server.log" >&2; die "herdr socket missing"; }
  log "started isolated herdr server"
}

launch_app() {
  local bin; bin="$(shardlane_bin)"
  : > "$LAG_LOG"
  HOME="$HOME_DIR" HERDR_SOCKET_PATH="$SOCKET_PATH" \
    SHARDLANE_TERMINAL_TRACE=1 SHARDLANE_SCROLL_DEBUG=1 \
    SHARDLANE_LAG_LOG_PATH="$LAG_LOG" \
    nohup "$APP_DIR/$APP_NAME.app/Contents/MacOS/$APP_NAME" > "$RUNTIME_DIR/shardlane-app.log" 2>&1 &
  APP_PID=$!
  WINDOW_BOUNDS=""
  for _ in {1..60}; do
    WINDOW_BOUNDS="$("$EVPOST" bounds "$APP_PID" 2>/dev/null || true)"
    [[ -n "$WINDOW_BOUNDS" ]] && break
    sleep 0.25
  done
  [[ -n "$WINDOW_BOUNDS" ]] || die "app window never appeared"
  log "app pid=$APP_PID window=[$WINDOW_BOUNDS]"
}

focus_window() {
  local attempt fpid
  for attempt in 1 2 3; do
    osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $APP_PID) to true" >/dev/null 2>&1 || true
    sleep 0.4
    fpid="$(osascript -e 'tell application "System Events" to get unix id of first process whose frontmost is true' 2>/dev/null || echo 0)"
    [[ "$fpid" == "$APP_PID" ]] && return 0
    log "focus attempt $attempt failed (frontmost=$fpid)"
  done
  die "could not focus probe app (frontmost=$fpid)"
}

# The terminal surface must genuinely hold keyboard focus: send a probe 'j'; text_submit must appear in the trace.
# Key injection uses System Events (full AppKit pipeline); evpost's HID-level keydown gets dropped in the
# "frontmost but not key window" state, so it cannot serve as the acceptance criterion.
focus_terminal_surface() {
  local wx wy ww wh attempt before after
  read -r wx wy ww wh <<< "$WINDOW_BOUNDS"
  for attempt in 1 2 3 4; do
    "$EVPOST" click $((wx+ww/2)) $((wy+wh/2)) >/dev/null
    sleep 0.6
    before=$(rg -c "ui.text_submit" "$LAG_LOG" 2>/dev/null || echo 0)
    osascript -e 'tell application "System Events" to keystroke "j"' >/dev/null
    sleep 0.8
    after=$(rg -c "ui.text_submit" "$LAG_LOG" 2>/dev/null || echo 0)
    if [[ "$after" -gt "$before" ]]; then
      log "terminal surface focused (attempt $attempt, text_submit $before->$after)"
      return 0
    fi
    log "focus probe $attempt failed ($before->$after), refocusing window"
    focus_window
  done
  die "terminal surface never accepted keyboard input"
}

reset_cursor() {
  # esc + g g: clear any mode residue/chord state and return to the top of the file
  osascript -e 'tell application "System Events" to key code 53' >/dev/null; sleep 0.2
  osascript -e 'tell application "System Events" to keystroke "g"' >/dev/null; sleep 0.08
  osascript -e 'tell application "System Events" to keystroke "g"' >/dev/null; sleep 0.35
}

# Mode preconditions: establish the input context outside the measurement window (insert enters insert mode;
# backspace additionally types filler characters so the whole backspace burst has deletable content). nav has none.
prepare_mode() {
  case "$MODE" in
    insert|backspace)
      osascript -e 'tell application "System Events" to keystroke "i"' >/dev/null; sleep 0.3
      if [[ "$MODE" == "backspace" ]]; then
        local filler
        filler="$(printf 'j%.0s' $(seq 1 "$BACKSPACE_FILL"))"
        osascript -e "tell application \"System Events\" to keystroke \"$filler\"" >/dev/null
        sleep 0.4
      fi
      ;;
  esac
}

# 30Hz autorepeat burst (evpost HID path; 100% delivery once the focus self-check passes)
burst_key() {
  "$EVPOST" key "$1" "$2" "$KEYINTERVAL_US" >/dev/null
}

mode_keycode() {
  case "$MODE" in
    backspace) echo "$KEYCODE_BACKSPACE" ;;
    *) echo "$KEYCODE_J" ;;
  esac
}

# Trace stage for each mode's input cluster: nav/insert printable characters go through the AppKit text path
# (ui.text_submit); backspace is a named key and goes through the Ghostty encoding path (shared.enqueue kind=key).
mode_key_stage() {
  case "$MODE" in
    backspace) echo "shared.enqueue" ;;
    *) echo "ui.text_submit" ;;
  esac
}

cmd_setup() {
  ensure_tooling
  rm -rf "$HOME_DIR" "$APP_DIR"
  mkdir -p "$HOME_DIR/.config/herdr" "$APP_DIR/$APP_NAME.app/Contents/MacOS" "$RUNTIME_DIR/proj"
  cat > "$APP_DIR/$APP_NAME.app/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>dev.shardlane.app.pacingab</string>
  <key>CFBundleName</key><string>Shardlane</string>
  <key>CFBundleExecutable</key><string>Shardlane</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.0.0-pacing-ab</string>
</dict>
</plist>
EOF
  cp "$(shardlane_bin)" "$APP_DIR/$APP_NAME.app/Contents/MacOS/$APP_NAME"
  [[ -f "$RUNTIME_DIR/proj/package.json" ]] || \
    printf '{\n  "name": "pacing-probe",\n  "version": "1.0.0"\n}\n' > "$RUNTIME_DIR/proj/package.json"
  ensure_server
  launch_app
  focus_window
  cat <<EOF

=== Manual preparation phase (CUA/human, once only) ===
Complete in the probe window:
  1. Add Project -> choose $RUNTIME_DIR/proj
  2. Terminal segment -> type in the composer: nvim package.json
  3. Confirm nvim shows package.json
Then: touch $RUNTIME_DIR/PREPARED
EOF
  rm -f "$RUNTIME_DIR/PREPARED"
  while [[ ! -f "$RUNTIME_DIR/PREPARED" ]]; do sleep 1; done
  cmd_snap
}

cmd_snap() {
  [[ -d "$HOME_DIR" ]] || die "no live HOME to snapshot (run setup first)"
  rm -rf "$SEED_DIR"
  cp -R "$HOME_DIR" "$SEED_DIR"
  # Fixed New Task page layout: window-relative coordinates (pt) of the Terminal segment button and command composer
  cat > "$RUNTIME_DIR/seed-meta.txt" <<'EOF'
SEGMENT_TERMINAL_REL=770 76
COMPOSER_REL=750 748
SEED_OK=1
EOF
  log "seeded HOME snapshot + meta saved"
}

# Joint analysis: video (visible-state ground truth) × trace (pipeline stages); the i-th video corresponds to the i-th burst cluster.
# Scoring uses the measured input period (median interval between input events); slip = visible interval > 1.35 × measured period.
analyze_all() {
  local res="$1" label="$2" samples="$3"
  python3 - "$res" "$label" "$samples" "$VIDEOS_DIR" "$LAG_LOG" "$WINDOW_BOUNDS" "$DISPLAY_PT_W" "$(mode_key_stage)" <<'PYEOF'
import subprocess, sys, os, tempfile, shutil, re
from PIL import Image
res, label, samples = sys.argv[1], sys.argv[2], int(sys.argv[3])
videos_dir, lag_log, bounds, disp_w = sys.argv[4], sys.argv[5], sys.argv[6], float(sys.argv[7])
key_stage = sys.argv[8]
wx, wy, ww, wh = map(int, bounds.split())

# ---- trace: burst clusters + measured per-cluster input period and present stats ----
pat = re.compile(r'\[(\d+\.\d+) pid=\d+ #\d+\] terminal\.trace t_us=\d+ stage=(\S+) (.*)')
keys=[]; presents=[]; sig_wake=[]; r2d=[]; last_sig=None
for line in open(lag_log, errors="ignore").readlines()[-80000:]:
    m=pat.match(line)
    if not m: continue
    t=float(m.group(1)); stage=m.group(2); rest=m.group(3)
    if key_stage=="shared.enqueue":
        # Named keys such as backspace go through the Ghostty encoding path: shared.enqueue kind=key with bytes>0
        if stage=="shared.enqueue" and "kind=key" in rest:
            bm=re.search(r"bytes=(\d+)", rest)
            if bm and int(bm.group(1))>0: keys.append(t)
    elif stage=="ui.text_submit": keys.append(t)
    elif stage=="frame.present_request": presents.append(t)
    elif stage=="vt.drain":
        mm=re.search(r"read_to_drain_us=(\d+)", rest)
        if mm: r2d.append(int(mm.group(1)))
    elif stage=="poll.wake_signal" and "queued" in rest: last_sig=t
    elif stage=="poll.wake" and "trigger=Output" in rest:
        if last_sig is not None:
            sig_wake.append((t-last_sig)*1000); last_sig=None
clusters=[]; cur=[]
for t in keys:
    if cur and t-cur[-1] > 0.5: clusters.append(cur); cur=[]
    cur.append(t)
if cur: clusters.append(cur)
clusters=[c for c in clusters if len(c) >= 30][-samples:]

def med(v):
    v=sorted(v); return v[len(v)//2] if v else 0

lines=[]
scores=[]; verdicts=[]
for s in range(1, samples+1):
    video=os.path.join(videos_dir, f"{label}-s{s}.mov")
    name=f"s{s}"
    if not os.path.exists(video):
        lines.append(f"[{name}] MISSING VIDEO"); continue
    # video -> visible state sequence
    tmp=tempfile.mkdtemp(prefix="pacing-frames-")
    pr=subprocess.run(["ffprobe","-v","error","-select_streams","v:0","-show_entries","stream=width","-of","csv=p=0",video],capture_output=True,text=True)
    frame_w=int(pr.stdout.strip()); scale=frame_w/disp_w
    crop=[int(v*scale) for v in (wx,wy,ww,wh)]
    subprocess.run(["ffmpeg","-v","error","-i",video,"-vf",
      f"crop={crop[2]}:{crop[3]}:{crop[0]}:{crop[1]},format=gray,scale=880:719",
      os.path.join(tmp,"f_%04d.png")],check=True)
    files=sorted(os.listdir(tmp))
    state=None; times=[]; idx=0
    for f in files:
        px=list(Image.open(os.path.join(tmp,f)).convert("L").getdata()); idx+=1
        if state is None: state=px; continue
        d=sum(1 for a,b in zip(state,px) if abs(a-b)>24)/len(px)*100.0
        if d>0.02: times.append(idx); state=px
    shutil.rmtree(tmp, ignore_errors=True)
    # matching trace cluster (in time order)
    cl = clusters[s-1] if s-1 < len(clusters) else []
    if not cl or len(times)<3:
        lines.append(f"[{name}] insufficient data (states={len(times)} trace_keys={len(cl)})"); continue
    period_ms = (med([cl[i+1]-cl[i] for i in range(len(cl)-1)]))*1000.0
    gaps=sorted((times[i+1]-times[i])*1000.0/60.0 for i in range(len(times)-1))
    n=len(gaps); p50=gaps[n//2]; p90=gaps[int(n*0.9)-1]; mx=gaps[-1]
    span=times[-1]-times[0]+1
    ups=len(times)*60.0/span
    slips=sum(1 for g in gaps if g > period_ms*1.35)
    slip_ratio=slips/n
    score=int(max(0.0,min(100.0, 100 - slip_ratio*60 - max(0.0,(0.8*1000.0/period_ms - ups))*2)))
    verdict="PASS" if score>=80 else ("WARN" if score>=55 else "FAIL")
    scores.append(score); verdicts.append(verdict)
    kp=sorted((min(p for p in presents if p>=k)-k)*1000 for k in cl if any(p>=k for p in presents))
    lines += [f"[{name}] input={len(cl)}keys@{period_ms:.0f}ms visible={len(times)} ups={ups:.1f} gap p50={p50:.0f} p90={p90:.0f} max={mx:.0f} slip={slips}/{n}"]
    if kp:
        lines.append(f"[{name}] key->present_ms p50={kp[len(kp)//2]:.1f} max={kp[-1]:.1f}")
    lines.append(f"[{name}] score={score} verdict={verdict}")
sw=sorted(sig_wake); r2=sorted(r2d)
if sw:
    n=len(sw); lines.append(f"[trace] wake_to_loop_ms p50={sw[n//2]:.1f} p95={sw[int(n*0.95)-1]:.1f} max={sw[-1]:.1f} n={n}")
if r2:
    n=len(r2); lines.append(f"[trace] read_to_drain_us p50={r2[n//2]} p95={r2[int(n*0.95)-1]} max={r2[-1]}")
if scores:
    med_score=sorted(scores)[len(scores)//2]
    worst=min(verdicts, key=["PASS","WARN","FAIL"].index)
    lines.append(f"scores={','.join(map(str,scores))}")
    lines.append(f"median_score={med_score}")
    lines.append(f"verdict={worst}")
    print("\n".join(lines))
open(res,"a").write("\n".join(lines)+"\n")
PYEOF
}

cmd_setup() {
  ensure_tooling
  rm -rf "$HOME_DIR" "$APP_DIR"
  mkdir -p "$HOME_DIR/.config/herdr" "$APP_DIR/$APP_NAME.app/Contents/MacOS" "$RUNTIME_DIR/proj"
  cat > "$APP_DIR/$APP_NAME.app/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>dev.shardlane.app.pacingab</string>
  <key>CFBundleName</key><string>Shardlane</string>
  <key>CFBundleExecutable</key><string>Shardlane</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.0.0-pacing-ab</string>
</dict>
</plist>
EOF
  cp "$(shardlane_bin)" "$APP_DIR/$APP_NAME.app/Contents/MacOS/$APP_NAME"
  [[ -f "$RUNTIME_DIR/proj/package.json" ]] || \
    printf '{\n  "name": "pacing-probe",\n  "version": "1.0.0"\n}\n' > "$RUNTIME_DIR/proj/package.json"
  ensure_server
  launch_app
  focus_window
  cat <<EOF

=== Manual preparation phase (CUA/human, once only) ===
Complete in the probe window:
  1. Add Project -> choose $RUNTIME_DIR/proj
  2. Terminal segment -> type in the composer: nvim package.json
  3. Confirm nvim shows package.json
Then: touch $RUNTIME_DIR/PREPARED
EOF
  rm -f "$RUNTIME_DIR/PREPARED"
  while [[ ! -f "$RUNTIME_DIR/PREPARED" ]]; do sleep 1; done
  cmd_snap
}

cmd_snap() {
  [[ -d "$HOME_DIR" ]] || die "no live HOME to snapshot (run setup first)"
  rm -rf "$SEED_DIR"
  cp -R "$HOME_DIR" "$SEED_DIR"
  # Fixed New Task page layout: window-relative coordinates (pt) of the Terminal segment button and command composer
  cat > "$RUNTIME_DIR/seed-meta.txt" <<'EOF'
SEGMENT_TERMINAL_REL=770 76
COMPOSER_REL=750 748
SEED_OK=1
EOF
  log "seeded HOME snapshot + meta saved"
}

analyze_video() {
  local video="$1" bounds="$2" out="$3" sample_name="$4"
  read -r wx wy ww wh <<< "$bounds"
  python3 - "$video" "$wx" "$wy" "$ww" "$wh" "$DISPLAY_PT_W" "$out" "$sample_name" "$KEYCOUNT" "$INPUT_PERIOD_MS" <<'PYEOF'
import subprocess, sys, os, tempfile, shutil
from PIL import Image
video = sys.argv[1]
wx, wy, ww, wh, disp_w = map(int, sys.argv[2:7])
out, name, keycount, period_ms = sys.argv[7], sys.argv[8], int(sys.argv[9]), float(sys.argv[10])
tmp = tempfile.mkdtemp(prefix="pacing-frames-")
probe = subprocess.run(["ffprobe","-v","error","-select_streams","v:0","-show_entries","stream=width","-of","csv=p=0",video],capture_output=True,text=True)
frame_w = int(probe.stdout.strip())
scale = frame_w / float(disp_w)
crop = [int(v*scale) for v in (wx, wy, ww, wh)]
subprocess.run(["ffmpeg","-v","error","-i",video,"-vf",
  f"crop={crop[2]}:{crop[3]}:{crop[0]}:{crop[1]},format=gray,scale=880:719",
  os.path.join(tmp,"f_%04d.png")], check=True)
files = sorted(os.listdir(tmp))
state=None; times=[]; idx=0
for f in files:
    px=list(Image.open(os.path.join(tmp,f)).convert("L").getdata()); idx+=1
    if state is None: state=px; continue
    diff=sum(1 for a,b in zip(state,px) if abs(a-b)>24)/len(px)*100.0
    if diff>0.02:
        times.append(idx); state=px
shutil.rmtree(tmp, ignore_errors=True)
fps=60.0
lines=[f"[{name}] video_frames={len(files)} visible_states={len(times)} (input {keycount})"]
verdict="FAIL"; score=0
if len(times)>2:
    gaps=sorted((times[i+1]-times[i])*1000.0/fps for i in range(len(times)-1))
    n=len(gaps)
    p50,p90,mx = gaps[n//2], gaps[int(n*0.9)-1], gaps[-1]
    span = times[-1]-times[0]+1
    ups = len(times)*fps/span
    slips = sum(1 for g in gaps if g > period_ms*1.35)
    slip_ratio = slips/n
    score = int(max(0.0, min(100.0, 100 - slip_ratio*60 - max(0.0,(25-ups))*2)))
    verdict = "PASS" if score>=80 else ("WARN" if score>=55 else "FAIL")
    lines += [f"[{name}] gap_ms p50={p50:.0f} p90={p90:.0f} max={mx:.0f}",
              f"[{name}] updates_per_s={ups:.1f} slips={slips}/{n} score={score} verdict={verdict}"]
open(out,"a").write("\n".join(lines)+"\n")
print("\n".join(lines))
PYEOF
}

analyze_trace() {
  local out="$1"
  python3 - "$LAG_LOG" "$out" "$KEYCOUNT" <<'PYEOF'
import re, sys
pat = re.compile(r'\[(\d+\.\d+) pid=\d+ #\d+\] terminal\.trace t_us=\d+ stage=(\S+) (.*)')
keys=[]; presents=[]; sig_wake=[]; r2d=[]; last_sig=None
for line in open(sys.argv[1], errors="ignore").readlines()[-80000:]:
    m=pat.match(line)
    if not m: continue
    t=float(m.group(1)); stage=m.group(2); rest=m.group(3)
    if stage=="ui.text_submit": keys.append(t)
    elif stage=="frame.present_request": presents.append(t)
    elif stage=="vt.drain":
        mm=re.search(r"read_to_drain_us=(\d+)", rest)
        if mm: r2d.append(int(mm.group(1)))
    elif stage=="poll.wake_signal" and "queued" in rest: last_sig=t
    elif stage=="poll.wake" and "trigger=Output" in rest:
        if last_sig is not None:
            sig_wake.append((t-last_sig)*1000); last_sig=None
# Split into burst clusters: adjacent keys >0.5s apart start a new cluster
clusters=[]; cur=[keys[0]] if keys else []
for t in keys[1:]:
    if t-cur[-1] > 0.5:
        clusters.append(cur); cur=[]
    cur.append(t)
if cur: clusters.append(cur)
out_lines=[]
si=0
for c in clusters:
    if len(c) < int(sys.argv[3])*0.7: continue
    si+=1
    t0,t1 = c[0]-0.2, c[-1]+0.3
    bw=[p for p in presents if t0<=p<=t1]
    lat=[(min(p for p in bw if p>=k)-k)*1000 for k in c if any(p>=k for p in bw)]
    pg=sorted((bw[i+1]-bw[i])*1000 for i in range(len(bw)-1))
    def fmt(v): return f"{v:.1f}"
    line=f"[trace s{si}] keys={len(c)} presents={len(bw)}"
    if lat:
        lat.sort(); n=len(lat)
        line+=f" key->present p50={fmt(lat[n//2])} p95={fmt(lat[int(n*0.95)-1])} max={fmt(lat[-1])}"
    if pg:
        n=len(pg)
        line+=f" present_gap p50={fmt(pg[n//2])} max={fmt(pg[-1])}"
    out_lines.append(line)
sw=sorted(sig_wake); r2=sorted(r2d)
if sw:
    n=len(sw); out_lines.append(f"[trace] wake_to_loop_ms p50={sw[n//2]:.1f} p95={sw[int(n*0.95)-1]:.1f} max={sw[-1]:.1f} n={n}")
if r2:
    n=len(r2); out_lines.append(f"[trace] read_to_drain_us p50={r2[n//2]} p95={r2[int(n*0.95)-1]} max={r2[-1]}")
open(sys.argv[2],"a").write("\n".join(out_lines)+"\n")
print("\n".join(out_lines))
PYEOF
}

cmd_run() {
  [[ $# -ge 1 ]] || { usage; exit 2; }
  local label="$1"; shift
  local samples=$DEFAULT_SAMPLES profile=0 mode=nav interval_us=$KEYINTERVAL_US
  while (($# > 0)); do
    case "$1" in
      --samples) samples="$2"; shift 2 ;;
      --profile) profile=1; shift ;;
      --mode) mode="$2"; shift 2 ;;
      --interval-us) interval_us="$2"; shift 2 ;;
      *) shift ;;
    esac
  done
  case "$mode" in nav|insert|backspace) ;; *) die "unknown mode: $mode" ;; esac
  MODE="$mode"
  KEYINTERVAL_US="$interval_us"
  [[ -d "$SEED_DIR" ]] || die "no seed (run setup first)"
  ensure_tooling
  pkill -f "$APP_DIR/$APP_NAME.app" 2>/dev/null || true
  sleep 1
  ensure_server
  rm -rf "$HOME_DIR"; cp -R "$SEED_DIR" "$HOME_DIR"
  launch_app
  focus_window
  # Keyboard injection requires an ASCII pass-through layout: save the current input source, use ABC for the whole run, restore at the end
  SAVED_SOURCE="$("$EVPOST" src get)"
  log "input source: $SAVED_SOURCE -> com.apple.keylayout.ABC"
  "$EVPOST" src set com.apple.keylayout.ABC || die "cannot select ABC layout"
  # Deterministic navigation: New Task page -> Terminal segment -> type the nvim command in the composer (new tab each time)
  local seg_rel composer_rel wx wy ww wh relx rely
  seg_rel="$(rg 'SEGMENT_TERMINAL_REL=([0-9]+ [0-9]+)' -or '$1' "$RUNTIME_DIR/seed-meta.txt")"
  composer_rel="$(rg 'COMPOSER_REL=([0-9]+ [0-9]+)' -or '$1' "$RUNTIME_DIR/seed-meta.txt")"
  read -r wx wy ww wh <<< "$WINDOW_BOUNDS"
  read -r relx rely <<< "$seg_rel"
  "$EVPOST" click $((wx+relx)) $((wy+rely)) >/dev/null
  sleep 0.6
  read -r relx rely <<< "$composer_rel"
  "$EVPOST" click $((wx+relx)) $((wy+rely)) >/dev/null
  sleep 0.4
  osascript -e 'tell application "System Events" to keystroke "nvim package.json"' >/dev/null
  sleep 0.3
  osascript -e 'tell application "System Events" to key code 36' >/dev/null
  local attached=0
  for _ in {1..40}; do
    if rg -q "tui.chrome projection" "$LAG_LOG" 2>/dev/null; then attached=1; break; fi
    sleep 0.25
  done
  [[ "$attached" == 1 ]] || die "hosted TUI never attached after nvim launch"
  sleep 2.5
  focus_terminal_surface
  local res="$RESULTS_DIR/$label.txt"
  {
    echo "label=$label"
    echo "samples=$samples"
    echo "window=$WINDOW_BOUNDS"
    echo "app_pid=$APP_PID"
    echo "shardlane_bin=$(shardlane_bin)"
    echo "mode=$MODE"
    echo "started=$(date +%s)"
  } > "$res"

  local s
  for ((s=1; s<=samples; s++)); do
    local video="$VIDEOS_DIR/$label-s$s.mov"
    rm -f "$video"
    reset_cursor
    prepare_mode
    focus_window
    ( screencapture -v -V "$RECORD_SECONDS" "$video" & )
    sleep 0.6
    if [[ "$profile" == 1 && "$s" == 1 ]]; then
      sample "$APP_PID" 3 -file "$SAMPLES_DIR/$label.txt" >/dev/null 2>&1 &
      SAMPLE_PID=$!
    fi
    log "sample $s/$samples: burst..."
    burst_key "$(mode_keycode)" "$KEYCOUNT"
    sleep 2.4
    for _ in {1..24}; do [[ -s "$video" ]] && break; sleep 0.5; done
    [[ -s "$video" ]] || die "screencapture failed to save $video"
    log "sample $s/$samples: recorded"
  done
  [[ "$profile" == 1 ]] && wait "${SAMPLE_PID:-}" 2>/dev/null || true
  kill -TERM "$APP_PID" 2>/dev/null || true
  sleep 1
  if [[ -n "${SAVED_SOURCE:-}" && "$SAVED_SOURCE" != "com.apple.keylayout.ABC" ]]; then
    "$EVPOST" src set "$SAVED_SOURCE" >/dev/null 2>&1 || true
    log "input source restored: $SAVED_SOURCE"
  fi
  log "analyzing (video × trace joint scoring)..."
  analyze_all "$res" "$label" "$samples"
  # Summarize verdict: take the median of per-round scores
  python3 - "$res" <<'PYEOF'
import sys, re
scores=[int(m.group(1)) for m in re.finditer(r"score=(\d+)", open(sys.argv[1]).read())]
verdicts=re.findall(r"verdict=(\w+)", open(sys.argv[1]).read())
if scores:
    scores.sort()
    med=scores[len(scores)//2]
    worst=min(verdicts, key=["PASS","WARN","FAIL"].index) if verdicts else "FAIL"
    with open(sys.argv[1],"a") as f:
        f.write(f"scores={','.join(map(str,scores))}\nmedian_score={med}\nverdict={worst}\n")
    print(f"scores={scores} median={med} verdict={worst}")
PYEOF
  log "result written: $res"
}

cmd_report() {
  python3 - "$RESULTS_DIR" "$@" <<'PYEOF'
import sys, os
d=sys.argv[1]; labels=sys.argv[2:]
rows=[]
for lb in labels:
    p=os.path.join(d, lb+".txt")
    if not os.path.exists(p): print(f"(missing {lb})"); continue
    kv={}
    for line in open(p):
        if "=" in line:
            k,v=line.strip().split("=",1)
            try: kv[k]=float(v)
            except ValueError: kv[k]=v
    m=re.search
    import re
    txt=open(p).read()
    kv["verdict_str"]= (re.search(r"^verdict=(\w+)", txt, re.M) or [None,"?"])[1] if re.search(r"^verdict=(\w+)", txt, re.M) else "?"
    kv["median_score"]= float(re.search(r"^median_score=(\d+)", txt, re.M).group(1)) if re.search(r"^median_score=(\d+)", txt, re.M) else -1
    rows.append((lb,kv))
import re
cols=[("median_score","score"),("verdict_str","verdict"),
      ("visible_updates_per_s","upd/s"),
      ("visible_gap_p50_ms","gap50"),("visible_gap_p90_ms","gap90"),("visible_gap_max_ms","gapMax"),
      ("key_to_present_ms p50","k2p50"),("key_to_present_ms max","k2max"),
      ("wake_to_loop_ms p50","wake50"),("read_to_drain_us p50","r2d50")]
def get(kv,key):
    for k,v in kv.items():
        if k.startswith(key): return v
    return "-"
print("metric".ljust(10)+" ".join(lb[:13].rjust(15) for lb,_ in rows))
for key,short in cols:
    line=short.ljust(10)
    for _,kv in rows:
        v=get(kv,key)
        line += (f"{v:.1f}".rjust(15) if isinstance(v,float) else str(v).rjust(15))
    print(line)
PYEOF
}

case "${1:-}" in
  setup) shift; cmd_setup "$@" ;;
  snap) shift; cmd_snap "$@" ;;
  run) shift; cmd_run "$@" ;;
  report) shift; cmd_report "$@" ;;
  *) usage; exit 2 ;;
esac
