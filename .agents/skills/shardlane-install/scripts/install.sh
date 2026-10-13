#!/usr/bin/env bash
#
# Build the newest Shardlane.app from this working tree and replace the
# installed copy the user runs day to day, restarting the running instance.
# Each stage gates the next: vet -> build -> bundle checks -> identity-guarded
# quit & replace -> relaunch -> liveness. No git state is ever touched.
#
# Override the install target with SHARDLANE_INSTALL_DST (default:
# ~/Applications/Shardlane.app).
set -euo pipefail

step() { printf '\n==> %s\n' "$*"; }
die() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

REPO="$(git rev-parse --show-toplevel 2>/dev/null)" || die "run from inside the herdr-client repository"
SRC="$REPO/build/darwin-arm64/Shardlane.app"
DST="${SHARDLANE_INSTALL_DST:-$HOME/Applications/Shardlane.app}"
PB=/usr/libexec/PlistBuddy

bundle_id() { "$PB" -c "Print :CFBundleIdentifier" "$1/Contents/Info.plist" 2>/dev/null; }
running_pids() { pgrep -x Shardlane || true; }
# pids_on_target lists running Shardlane pids whose executable lives inside
# the install target — only the instance being replaced gets restarted.
pids_on_target() {
  local pid path
  for pid in $(running_pids); do
    path="$(ps -o command= -p "$pid" 2>/dev/null || true)"
    case "$path" in
      "$DST"/*) echo "$pid" ;;
    esac
  done
}

step "1/7 vet"
cd "$REPO"
GOTOOLCHAIN=auto go tool mygo vet .

step "2/7 build (working tree — uncommitted changes included)"
dirty="$(git status --porcelain | wc -l | tr -d ' ')"
if [ "$dirty" -gt 0 ]; then
  echo "note: building with $dirty uncommitted file(s)"
fi
GOTOOLCHAIN=go1.27.1 go tool mygo build
[ -x "$SRC/Contents/MacOS/Shardlane" ] || die "build produced no executable at $SRC"

step "3/7 verify the built bundle"
plutil -lint "$SRC/Contents/Info.plist" >/dev/null || die "built Info.plist is invalid"
BUILD_ID="$(bundle_id "$SRC")" || die "cannot read the built bundle identifier"
BUILD_VER="$("$PB" -c 'Print :CFBundleShortVersionString' "$SRC/Contents/Info.plist")"
echo "built: $BUILD_ID $BUILD_VER"

step "4/7 preflight the install target"
if [ -d "$DST" ]; then
  DST_ID="$(bundle_id "$DST")" || die "existing bundle at $DST is unreadable"
  if [ "$DST_ID" != "$BUILD_ID" ]; then
    die "identity mismatch: installed $DST_ID != built $BUILD_ID — refusing to overwrite"
  fi
  echo "target: $DST ($DST_ID)"
else
  echo "target: $DST (fresh install)"
fi

step "5/7 quit the instance running from the target"
if [ -n "$(pids_on_target)" ]; then
  osascript -e 'tell application "Shardlane" to quit' >/dev/null 2>&1 || true
  for _ in $(seq 1 10); do
    [ -z "$(pids_on_target)" ] && break
    sleep 1
  done
  if [ -n "$(pids_on_target)" ]; then
    for pid in $(pids_on_target); do kill -TERM "$pid" 2>/dev/null || true; done
    sleep 3
  fi
  if [ -n "$(pids_on_target)" ]; then
    die "Shardlane (pid $(pids_on_target | tr '\n' ' ')) still running — quit it manually and rerun (no SIGKILL by design: it skips the session save)"
  fi
  echo "previous instance exited"
else
  echo "not running from $DST"
fi

step "6/7 replace"
mkdir -p "$(dirname "$DST")"
rm -rf "$DST"
ditto "$SRC" "$DST"
plutil -lint "$DST/Contents/Info.plist" >/dev/null || die "installed Info.plist is invalid — source bundle kept at $SRC"

step "7/7 launch and verify"
open "$DST"
sleep 3
[ -n "$(running_pids)" ] || die "app did not start"
PID="$(running_pids | head -1)"
sleep 4
[ -n "$(running_pids)" ] || die "app exited right after launching"
echo
echo "done: Shardlane $BUILD_ID $BUILD_VER installed at $DST and running (pid $PID)"
