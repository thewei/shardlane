#!/usr/bin/env bash
#
# Build the newest Shardlane from this working tree and replace the installed
# copy the user runs day to day, restarting the running instance. Each stage
# gates the next: vet -> build -> artifact checks -> quit & replace -> relaunch
# -> liveness. No git state is ever touched.
#
# Platform split (2026-10-16, per user): on macOS the flow installs the
# Shardlane.app bundle (default ~/Applications/Shardlane.app, override with
# SHARDLANE_INSTALL_DST); on Linux it installs the MyGo-generated
# user-local build (default ~/.local/shardlane.app + applications-menu entry,
# override with SHARDLANE_INSTALL_APP_DIR).
set -euo pipefail

step() { printf '\n==> %s\n' "$*"; }
die() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

REPO="$(git rev-parse --show-toplevel 2>/dev/null)" || die "run from inside the herdr-client repository"
PB=/usr/libexec/PlistBuddy

bundle_id() { "$PB" -c "Print :CFBundleIdentifier" "$1/Contents/Info.plist" 2>/dev/null; }

# ---------------------------------------------------------------- Linux ----
# Installs the MyGo user-local Linux build through the installer that
# `go tool mygo build` generates next to the binary — mature ownership: the
# framework's installer owns staging, whole-directory replacement and the
# desktop entry; this script only gates and drives it. There is no bundle
# identity file on Linux, so the macOS identity guard does not apply; the
# generated target name (~/.local/shardlane.app) is the identity.
#
# Quit scope is wider than the target: instances may run day to day straight
# from a build/ output directory (the pre-install habit). Those are this
# repository's own build products, never user-authored state, so they are
# replaced too — gracefully: TERM, 10 s grace, then abort. Never SIGKILL: it
# would skip the app's session save.
linux_pids_in_scope() {
  local pid exe app_dir="$1"
  for pid in $(pgrep -x shardlane 2>/dev/null || true); do
    exe="$(readlink "/proc/$pid/exe" 2>/dev/null || true)"
    exe="${exe% (deleted)}"
    case "$exe" in
      "$app_dir"/*|"$REPO"/build/*) echo "$pid" ;;
    esac
  done
}

if [ "$(uname -s)" = Linux ]; then
  BUILD_OUT="$REPO/build/linux-amd64"
  APP_DIR="${SHARDLANE_INSTALL_APP_DIR:-$HOME/.local/shardlane.app}"
  DESKTOP_FILE="${XDG_DATA_HOME:-$HOME/.local/share}/applications/shardlane.desktop"

  case "$(uname -m)" in
    x86_64|amd64) LINUX_TARGET=linux-amd64 ;;
    aarch64|arm64) LINUX_TARGET=linux-arm64 ;;
    *) die "no Shardlane build for $(uname -m)" ;;
  esac

  step "1/7 vet"
  cd "$REPO"
  GOTOOLCHAIN=auto go tool mygo vet .

  step "2/7 build (working tree — uncommitted changes included)"
  dirty="$(git status --porcelain | wc -l | tr -d ' ')"
  if [ "$dirty" -gt 0 ]; then
    echo "note: building with $dirty uncommitted file(s)"
  fi
  GOTOOLCHAIN=go1.27.1 go tool mygo build

  step "3/7 verify the built artifacts"
  [ -x "$BUILD_OUT/shardlane" ] || die "build produced no executable at $BUILD_OUT/shardlane"
  [ -f "$BUILD_OUT/install.sh" ] || die "build produced no installer at $BUILD_OUT/install.sh"
  ARCHIVE="$(ls "$BUILD_OUT"/shardlane-*-$LINUX_TARGET.tar.gz 2>/dev/null | head -1 || true)"
  [ -n "$ARCHIVE" ] || die "build produced no $LINUX_TARGET archive in $BUILD_OUT"
  LINUX_VER="$(basename "$ARCHIVE")"
  LINUX_VER="${LINUX_VER#shardlane-}"; LINUX_VER="${LINUX_VER%%-*}"
  echo "built: shardlane $LINUX_VER ($ARCHIVE)"

  step "4/7 preflight the install target"
  if [ -d "$APP_DIR" ]; then
    echo "target: $APP_DIR (replacing)"
  else
    echo "target: $APP_DIR (fresh install)"
  fi

  step "5/7 quit instances running from the target or an old build directory"
  if [ -n "$(linux_pids_in_scope "$APP_DIR")" ]; then
    for pid in $(linux_pids_in_scope "$APP_DIR"); do kill -TERM "$pid" 2>/dev/null || true; done
    for _ in $(seq 1 10); do
      [ -z "$(linux_pids_in_scope "$APP_DIR")" ] && break
      sleep 1
    done
    if [ -n "$(linux_pids_in_scope "$APP_DIR")" ]; then
      die "Shardlane (pid $(linux_pids_in_scope "$APP_DIR" | tr '\n' ' ')) still running — quit it manually and rerun (no SIGKILL by design: it skips the session save)"
    fi
    echo "previous instance exited"
  else
    echo "not running from the target or build/"
  fi

  step "6/7 install via the generated installer"
  sh "$BUILD_OUT/install.sh" || die "the generated installer failed — nothing was replaced by this script"
  [ -x "$APP_DIR/shardlane" ] || die "installer reported success but $APP_DIR/shardlane is missing"
  [ -f "$DESKTOP_FILE" ] || die "installer left no applications-menu entry at $DESKTOP_FILE"
  echo "installed: $APP_DIR/shardlane"

  step "7/7 launch and verify"
  if [ -z "${WAYLAND_DISPLAY:-}" ] && [ -z "${DISPLAY:-}" ]; then
    echo
    echo "done: Shardlane $LINUX_VER installed at $APP_DIR — no display session detected, launch it from the applications menu"
    exit 0
  fi
  before="$(linux_pids_in_scope "$APP_DIR" | tr '\n' ' ')"
  setsid -f gtk-launch shardlane >/dev/null 2>&1 || die "gtk-launch shardlane failed"
  PID=""
  for _ in $(seq 1 10); do
    for pid in $(linux_pids_in_scope "$APP_DIR"); do
      case " $before " in *" $pid "*) ;; *) PID="$pid" ;; esac
    done
    [ -n "$PID" ] && break
    sleep 1
  done
  [ -n "$PID" ] || die "app did not start"
  sleep 2
  kill -0 "$PID" 2>/dev/null || die "app exited right after launching"
  sleep 3
  kill -0 "$PID" 2>/dev/null || die "app exited after a few seconds"
  echo
  echo "done: Shardlane $LINUX_VER installed at $APP_DIR and running (pid $PID)"
  exit 0
fi

# --------------------------------------------------------------- macOS ----
[ "$(uname -s)" = Darwin ] || die "unsupported platform $(uname -s) — this script installs the macOS app and the Linux build only"

SRC="$REPO/build/darwin-arm64/Shardlane.app"
DST="${SHARDLANE_INSTALL_DST:-$HOME/Applications/Shardlane.app}"

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

running_pids() { pgrep -x Shardlane || true; }

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
