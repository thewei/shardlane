#!/usr/bin/env bash
set -euo pipefail

# [INPUT]: herdr-mobile package.json/dist, optional --app Shardlane.app path
# [OUTPUT]: target/mobile-web or Shardlane.app/Contents/Resources/mobile-web
# [POS]: Cross-repository Mobile Web static artifact bridge; owns no Web business logic or Remote API
#
# Bundle the herdr-mobile Web export into the Mac App resource path.
#
# Usage:
#   scripts/bundle-web.sh                           # default: sibling herdr-mobile repo
#   scripts/bundle-web.sh /path/to/herdr-mobile     # explicit source
#
# Modes:
#   Development: copies dist/ to target/mobile-web/ and prints the env var to set.
#   Packaging:   pass --app <path/to/Shardlane.app> to copy into Resources/mobile-web/.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

DEFAULT_MOBILE_ROOT="$REPO_ROOT/../herdr-mobile"
MOBILE_ROOT="${SHARDLANE_MOBILE_ROOT:-$DEFAULT_MOBILE_ROOT}"
APP_PATH=""
MOBILE_ROOT_EXPLICIT=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --app)
      [[ $# -ge 2 ]] || { echo "Error: --app requires a path." >&2; exit 2; }
      APP_PATH="$2"
      shift 2
      ;;
    --)
      shift
      break
      ;;
    -*)
      echo "Unknown option: $1" >&2
      exit 2
      ;;
    *)
      if [[ "$MOBILE_ROOT_EXPLICIT" -eq 1 ]]; then
        echo "Error: mobile root was provided more than once." >&2
        exit 2
      fi
      MOBILE_ROOT="$1"
      MOBILE_ROOT_EXPLICIT=1
      shift
      ;;
  esac
done

if [[ -z "$MOBILE_ROOT" || ! -f "$MOBILE_ROOT/package.json" ]]; then
  echo "Error: herdr-mobile repo not found. Pass the path as first argument." >&2
  echo "  scripts/bundle-web.sh /path/to/herdr-mobile" >&2
  exit 1
fi

DIST="$MOBILE_ROOT/dist"

if [[ ! -d "$DIST" ]]; then
  echo "Building Web export…"
  (cd "$MOBILE_ROOT" && pnpm export:web)
fi

if [[ ! -f "$DIST/index.html" ]]; then
  echo "Error: dist/index.html not found after export." >&2
  exit 1
fi

if [[ -n "$APP_PATH" ]]; then
  DEST="$APP_PATH/Contents/Resources/mobile-web"
  echo "Packaging: copying dist/ → $DEST"
  rm -rf "$DEST"
  cp -R "$DIST" "$DEST"
  echo "Done. $(du -sh "$DEST" | cut -f1) bundled into Shardlane.app"
else
  DEST="$REPO_ROOT/target/mobile-web"
  echo "Development: copying dist/ → $DEST"
  rm -rf "$DEST"
  cp -R "$DIST" "$DEST"
  echo ""
  echo "Set this env var to use the Web bundle in development:"
  echo "  export SHARDLANE_WEB_BUNDLE=$DEST"
  echo ""
  echo "Or launch with:"
  echo "  SHARDLANE_WEB_BUNDLE=$DEST cargo run --bin shardlane"
  echo ""
  echo "$(du -sh "$DEST" | cut -f1) bundled."
fi
