#!/usr/bin/env bash
set -euo pipefail

# [INPUT]: Cargo workspace, cargo-bundle, optional herdr-mobile repository (pnpm)
# [OUTPUT]: One command that runs gate checks → Universal 2 Mobile Web static bundle → release .app install → dist/ ZIP+SHA256
# [POS]: The one-click release entry point for scripts/; composes package-macos.sh/archive-macos.sh/bundle-web.sh,
#        and does not duplicate app identity (the sole identity remains in Cargo.toml [package.metadata.bundle])

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

SKIP_TESTS=0
NO_INSTALL=0
NO_ARCHIVE=0
BUILD_WEB=0
WITHOUT_WEB=0
UNIVERSAL=1
SIGN_IDENTITY=""
MOBILE_ROOT=""

usage() {
  cat <<'USAGE'
Usage: scripts/release-macos.sh [options]

One-click release: static gates -> Mobile Web bundle -> signed release .app
(install to ~/Applications) -> dist/ ZIP + SHA-256.

Options:
  --skip-tests         Skip cargo fmt/clippy/test gates (still builds).
  --no-install         Do not copy the app to ${SHARDLANE_INSTALL_ROOT:-~/Applications}.
  --no-archive         Do not produce dist/ ZIP + checksum.
  --without-web        Package without the herdr-mobile static bundle.
  --universal          Build one arm64+x86_64 Universal 2 app (default).
  --arm64              Explicitly use the arm64-only fallback when Universal 2 is unavailable.
  --build-web          Force `pnpm export:web` even if dist/ already exists.
  --mobile-root PATH   herdr-mobile repository (default: ../herdr-mobile).
  --sign IDENTITY      codesign identity (default: ad-hoc '-').
  -h, --help           Show this help.

Examples:
  # Everyday one-click: gates + embed Mobile Web + install + archive
  scripts/release-macos.sh

  # Fastest local-install-only path (skips gates and archive)
  scripts/release-macos.sh --skip-tests --no-archive
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-tests) SKIP_TESTS=1; shift ;;
    --no-install) NO_INSTALL=1; shift ;;
    --no-archive) NO_ARCHIVE=1; shift ;;
    --without-web) WITHOUT_WEB=1; shift ;;
    --universal) UNIVERSAL=1; shift ;;
    --arm64) UNIVERSAL=0; shift ;;
    --build-web) BUILD_WEB=1; shift ;;
    --mobile-root)
      [[ $# -ge 2 ]] || { echo "Error: --mobile-root requires a path." >&2; exit 2; }
      MOBILE_ROOT="$2"; shift 2 ;;
    --sign)
      [[ $# -ge 2 ]] || { echo "Error: --sign requires a codesign identity." >&2; exit 2; }
      SIGN_IDENTITY="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "Error: macOS release requires Darwin." >&2
  exit 1
fi

command -v cargo >/dev/null || { echo "Error: cargo is required." >&2; exit 1; }
command -v cargo-bundle >/dev/null || {
  echo "Error: cargo-bundle 0.11.0 is required. Install with:" >&2
  echo "  cargo install cargo-bundle --version 0.11.0 --locked" >&2
  exit 1
}

MOBILE_ROOT="${MOBILE_ROOT:-$REPO_ROOT/../herdr-mobile}"
HAVE_MOBILE_WEB=0
if [[ "$WITHOUT_WEB" -eq 0 ]]; then
  if [[ -f "$MOBILE_ROOT/package.json" ]]; then
    HAVE_MOBILE_WEB=1
  else
    echo "Warning: herdr-mobile not found at $MOBILE_ROOT; packaging WITHOUT Mobile Web." >&2
    echo "         Pass --mobile-root PATH or clone the repo next to herdr-client." >&2
  fi
fi

# ---- 1/4 Static gates -------------------------------------------------------
if [[ "$SKIP_TESTS" -eq 0 ]]; then
  echo "==> [1/4] Static gates (fmt/clippy/test)…"
  cargo fmt -- --check
  SDKROOT="$(xcrun --show-sdk-path)" cargo clippy --locked --workspace --all-targets -- -D warnings
  cargo test --locked --workspace
else
  echo "==> [1/4] Static gates skipped (--skip-tests)."
fi

# ---- 2/4 Mobile Web static bundle -------------------------------------------
if [[ "$HAVE_MOBILE_WEB" -eq 1 ]]; then
  WEB_INDEX="$MOBILE_ROOT/dist/index.html"
  if [[ "$BUILD_WEB" -eq 1 || ! -f "$WEB_INDEX" ]]; then
    echo "==> [2/4] Exporting Mobile Web (pnpm export:web)…"
    command -v pnpm >/dev/null || {
      echo "Error: pnpm is required to build Mobile Web (dist/index.html missing)." >&2
      exit 1
    }
    (
      cd "$MOBILE_ROOT"
      [[ -d node_modules ]] || pnpm install
      pnpm export:web
    )
    test -f "$WEB_INDEX" || {
      echo "Error: pnpm export:web did not produce dist/index.html." >&2
      exit 1
    }
  else
    echo "==> [2/4] Mobile Web: reusing $MOBILE_ROOT/dist (use --build-web to force)."
  fi
else
  echo "==> [2/4] Mobile Web skipped."
fi

# ---- 3/4 Package + install --------------------------------------------------
echo "==> [3/4] Packaging release .app…"
PACKAGE_ARGS=(--release)
ARM_TARGET="aarch64-apple-darwin"
if [[ "$UNIVERSAL" -eq 1 ]]; then
  PACKAGE_ARGS+=(--universal)
else
  PACKAGE_ARGS+=(--target "$ARM_TARGET")
fi
[[ "$NO_INSTALL" -eq 0 ]] && PACKAGE_ARGS+=(--install)
[[ -n "$SIGN_IDENTITY" ]] && PACKAGE_ARGS+=(--sign "$SIGN_IDENTITY")
if [[ "$HAVE_MOBILE_WEB" -eq 1 ]]; then
  PACKAGE_ARGS+=(--mobile-root "$MOBILE_ROOT")
fi
scripts/package-macos.sh "${PACKAGE_ARGS[@]}"

# ---- 4/4 Release archive ----------------------------------------------------
if [[ "$NO_ARCHIVE" -eq 0 ]]; then
  if [[ "$UNIVERSAL" -eq 1 ]]; then
    APP_PATH="$REPO_ROOT/target/aarch64-apple-darwin/release/bundle/osx/Shardlane.app"
    ARCHIVE_ARCHITECTURE="universal2"
  else
    APP_PATH="$REPO_ROOT/target/$ARM_TARGET/release/bundle/osx/Shardlane.app"
    ARCHIVE_ARCHITECTURE="aarch64"
  fi
  test -d "$APP_PATH" || { echo "Error: expected $APP_PATH for archiving." >&2; exit 1; }
  echo "==> [4/4] Archiving to dist/…"
  ARCHIVE_ARGS=(--app "$APP_PATH" --output-dir "$REPO_ROOT/dist" --architecture "$ARCHIVE_ARCHITECTURE")
  scripts/archive-macos.sh "${ARCHIVE_ARGS[@]}"
else
  echo "==> [4/4] Archive skipped (--no-archive)."
fi

echo
echo "Release ready:"
[[ "$NO_INSTALL" -eq 0 ]] && echo "  App installed : ${SHARDLANE_INSTALL_ROOT:-$HOME/Applications}/Shardlane.app"
if [[ "$NO_ARCHIVE" -eq 0 ]]; then
  if [[ "$UNIVERSAL" -eq 1 ]]; then
    echo "  Archive       : dist/Shardlane-macos-universal2.zip (+ .sha256)"
  else
    echo "  Archive       : dist/Shardlane-macos-aarch64.zip (+ .sha256)"
  fi
fi
echo "  Note          : ad-hoc signing unless --sign was given; notarization is credential-gated."
