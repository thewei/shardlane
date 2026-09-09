#!/usr/bin/env bash
set -euo pipefail

# [INPUT]: Cargo workspace, cargo-bundle 0.11.0, optional herdr-mobile repository; Universal 2 requires both Rust macOS targets
# [OUTPUT]: A signed and verified Shardlane.app, optionally copied to ~/Applications; the bundle icon is force-rebuilt from assets/app-icon via iconutil, and the Universal 2 artifact contains both arm64 and x86_64
# [POS]: macOS packaging/install orchestration entry point; identity configuration remains solely owned by Cargo.toml, signing happens only after the Universal merge and the icon rebuild

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

PROFILE=""
PROFILE_EXPLICIT=0
TARGET_TRIPLE=""
UNIVERSAL=0
INSTALL_APP=0
WITH_MOBILE_WEB=0
MOBILE_ROOT=""
SIGN_IDENTITY="-"

usage() {
  cat <<'USAGE'
Usage: scripts/package-macos.sh [options]

Options:
  --release                 Build the release bundle (default when --install is specified).
  --debug                   Build the unoptimized debug bundle.
  --target TRIPLE           Build for a specific Rust target (for example aarch64-apple-darwin).
  --universal               Build arm64 and x86_64, then create one Universal 2 app (implies both targets).
  --with-mobile-web         Build/copy ../herdr-mobile/dist into the app bundle.
  --mobile-root PATH        Use PATH as the herdr-mobile repository (implies --with-mobile-web).
  --sign IDENTITY            Use this codesign identity (default: ad-hoc '-').
  --install                 Copy the verified app to ${SHARDLANE_INSTALL_ROOT:-~/Applications} (defaults to release).
  -h, --help                Show this help.

The package name, app name, bundle identifier, and icon are configured in
Cargo.toml [package.metadata.bundle].
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --release)
      PROFILE="release"
      PROFILE_EXPLICIT=1
      shift
      ;;
    --debug)
      PROFILE="debug"
      PROFILE_EXPLICIT=1
      shift
      ;;
    --target)
      [[ $# -ge 2 ]] || { echo "Error: --target requires a Rust target triple." >&2; exit 2; }
      TARGET_TRIPLE="$2"
      shift 2
      ;;
    --universal)
      UNIVERSAL=1
      shift
      ;;
    --with-mobile-web)
      WITH_MOBILE_WEB=1
      shift
      ;;
    --mobile-root)
      [[ $# -ge 2 ]] || { echo "Error: --mobile-root requires a path." >&2; exit 2; }
      MOBILE_ROOT="$2"
      WITH_MOBILE_WEB=1
      shift 2
      ;;
    --sign)
      [[ $# -ge 2 ]] || { echo "Error: --sign requires a codesign identity." >&2; exit 2; }
      SIGN_IDENTITY="$2"
      shift 2
      ;;
    --install)
      INSTALL_APP=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ "$PROFILE_EXPLICIT" -eq 0 ]]; then
  if [[ "$INSTALL_APP" -eq 1 ]]; then
    PROFILE="release"
  else
    PROFILE="debug"
  fi
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "Error: macOS packaging requires Darwin (cargo-bundle --format osx)." >&2
  exit 1
fi

command -v cargo >/dev/null || { echo "Error: cargo is required." >&2; exit 1; }
command -v cargo-bundle >/dev/null || {
  echo "Error: cargo-bundle 0.11.0 is required. Install with:" >&2
  echo "  cargo install cargo-bundle --version 0.11.0 --locked" >&2
  exit 1
}
command -v rustc >/dev/null || { echo "Error: rustc is required." >&2; exit 1; }
command -v codesign >/dev/null || { echo "Error: codesign is required." >&2; exit 1; }
command -v file >/dev/null || { echo "Error: file is required." >&2; exit 1; }
command -v lipo >/dev/null || { echo "Error: lipo is required for Mach-O verification." >&2; exit 1; }
if [[ "$UNIVERSAL" -eq 1 ]]; then
  if [[ -n "$TARGET_TRIPLE" ]]; then
    echo "Error: --universal cannot be combined with --target." >&2
    exit 2
  fi
fi

if [[ "$WITH_MOBILE_WEB" -eq 1 ]]; then
  if [[ -z "$MOBILE_ROOT" ]]; then
    MOBILE_ROOT="$REPO_ROOT/../herdr-mobile"
  fi
  if [[ ! -f "$MOBILE_ROOT/package.json" ]]; then
    echo "Error: herdr-mobile repository not found at $MOBILE_ROOT." >&2
    echo "Pass --mobile-root /path/to/herdr-mobile or omit --with-mobile-web." >&2
    exit 1
  fi
fi

ARM_TARGET="aarch64-apple-darwin"
INTEL_TARGET="x86_64-apple-darwin"
if [[ "$UNIVERSAL" -eq 1 ]]; then
  BUNDLE_TARGET="$ARM_TARGET"
else
  BUNDLE_TARGET="$TARGET_TRIPLE"
fi

build_target() {
  local target="$1"
  local -a build_args=(build --locked)
  if [[ "$PROFILE" == "release" ]]; then
    build_args+=(--release)
  fi
  if [[ -n "$target" ]]; then
    local target_libdir
    target_libdir="$(rustc --print target-libdir --target "$target" 2>/dev/null || true)"
    if [[ -z "$target_libdir" || ! -d "$target_libdir" || -z "$(find "$target_libdir" -maxdepth 1 -name 'libstd-*.rlib' -print -quit 2>/dev/null)" ]]; then
      echo "Error: Rust standard library for $target is unavailable to $(rustc --version)." >&2
      echo "Install it with 'rustup target add $target' (or use a toolchain that owns this target)." >&2
      exit 1
    fi
    build_args+=(--target "$target")
    echo "Building Shardlane ($PROFILE, $target)…"
  else
    echo "Building Shardlane ($PROFILE, host target)…"
  fi
  SDKROOT="$(xcrun --show-sdk-path)" cargo "${build_args[@]}"
}

if [[ "$UNIVERSAL" -eq 1 ]]; then
  build_target "$ARM_TARGET"
  build_target "$INTEL_TARGET"
else
  build_target "$TARGET_TRIPLE"
fi

BUNDLE_ARGS=(bundle --format osx)
if [[ "$PROFILE" == "release" ]]; then
  BUNDLE_ARGS+=(--release)
fi
if [[ -n "$BUNDLE_TARGET" ]]; then
  BUNDLE_ARGS+=(--target "$BUNDLE_TARGET")
fi

if [[ "$UNIVERSAL" -eq 1 ]]; then
  echo "Creating Shardlane.app (arm64 bundle template)…"
else
  echo "Creating Shardlane.app…"
fi
SDKROOT="$(xcrun --show-sdk-path)" cargo "${BUNDLE_ARGS[@]}"

if [[ -n "$BUNDLE_TARGET" ]]; then
  BUNDLE_DIR="$REPO_ROOT/target/$BUNDLE_TARGET/$PROFILE/bundle/osx"
else
  BUNDLE_DIR="$REPO_ROOT/target/$PROFILE/bundle/osx"
fi
APP_PATH="$(find "$BUNDLE_DIR" -maxdepth 1 -type d -name '*.app' -print -quit 2>/dev/null || true)"
if [[ -z "$APP_PATH" || ! -d "$APP_PATH" ]]; then
  echo "Error: cargo-bundle did not produce an .app under $BUNDLE_DIR." >&2
  exit 1
fi

EXECUTABLE="$APP_PATH/Contents/MacOS/shardlane"

if [[ "$UNIVERSAL" -eq 1 ]]; then
  ARM_EXECUTABLE="$REPO_ROOT/target/$ARM_TARGET/$PROFILE/shardlane"
  INTEL_EXECUTABLE="$REPO_ROOT/target/$INTEL_TARGET/$PROFILE/shardlane"
  test -x "$ARM_EXECUTABLE" || {
    echo "Error: missing arm64 executable $ARM_EXECUTABLE." >&2
    exit 1
  }
  test -x "$INTEL_EXECUTABLE" || {
    echo "Error: missing x86_64 executable $INTEL_EXECUTABLE." >&2
    exit 1
  }
  echo "Creating Universal 2 executable…"
  LIPO_OUTPUT="$EXECUTABLE.universal"
  rm -f "$LIPO_OUTPUT"
  lipo -create "$ARM_EXECUTABLE" "$INTEL_EXECUTABLE" -output "$LIPO_OUTPUT"
  chmod +x "$LIPO_OUTPUT"
  mv "$LIPO_OUTPUT" "$EXECUTABLE"
fi

if [[ "$WITH_MOBILE_WEB" -eq 1 ]]; then
  echo "Embedding Mobile Web from ${MOBILE_ROOT}…"
  "$SCRIPT_DIR/bundle-web.sh" "$MOBILE_ROOT" --app "$APP_PATH"
fi

# Notes/Bookmarks/Annotation were deleted (2026-08-27 TUI-only convergence):
# no built-in web editor/bridge assets are packaged anymore; Mobile Web is the only optional web bundle.

# ---- Bundle icon: force-rebuild from the repo ladder (single source of truth) ----
# cargo-bundle's generated ICNS has drifted from assets/app-icon before; rebuild
# Contents/Resources/Shardlane.icns from the reviewed ladder via iconutil so the
# packaged icon always equals the artwork in the repository.
ICONSET_DIR="$BUNDLE_DIR/Shardlane.iconset"
mkdir -p "$ICONSET_DIR"
LADDER_DIR="$REPO_ROOT/assets/app-icon"
cp "$LADDER_DIR/shardlane-16.png"     "$ICONSET_DIR/icon_16x16.png"
cp "$LADDER_DIR/shardlane-16@2x.png"  "$ICONSET_DIR/icon_16x16@2x.png"
cp "$LADDER_DIR/shardlane-32.png"     "$ICONSET_DIR/icon_32x32.png"
cp "$LADDER_DIR/shardlane-32@2x.png"  "$ICONSET_DIR/icon_32x32@2x.png"
cp "$LADDER_DIR/shardlane-128.png"    "$ICONSET_DIR/icon_128x128.png"
cp "$LADDER_DIR/shardlane-128@2x.png" "$ICONSET_DIR/icon_128x128@2x.png"
cp "$LADDER_DIR/shardlane-256.png"    "$ICONSET_DIR/icon_256x256.png"
cp "$LADDER_DIR/shardlane-256@2x.png" "$ICONSET_DIR/icon_256x256@2x.png"
cp "$LADDER_DIR/shardlane-512.png"    "$ICONSET_DIR/icon_512x512.png"
cp "$LADDER_DIR/shardlane-512@2x.png" "$ICONSET_DIR/icon_512x512@2x.png"
DEST_ICNS="$APP_PATH/Contents/Resources/Shardlane.icns"
iconutil -c icns -o "$DEST_ICNS" "$ICONSET_DIR" || {
  echo "Error: iconutil could not rebuild $DEST_ICNS" >&2
  exit 1
}
rm -rf "$ICONSET_DIR"
echo "Rebuilt bundle icon from assets/app-icon ladder: $DEST_ICNS"

PLIST="$APP_PATH/Contents/Info.plist"
ICON="$(find "$APP_PATH/Contents/Resources" -maxdepth 1 -type f -name '*.icns' -print -quit 2>/dev/null || true)"
test -x "$EXECUTABLE" || { echo "Error: missing executable $EXECUTABLE" >&2; exit 1; }
test -n "$ICON" || { echo "Error: no ICNS icon was generated." >&2; exit 1; }

expected_arch=""
case "$BUNDLE_TARGET" in
  aarch64-apple-darwin) expected_arch="arm64" ;;
  x86_64-apple-darwin) expected_arch="x86_64" ;;
esac

has_arch() {
  local arch_list="$1"
  local expected="$2"
  [[ " $arch_list " == *" $expected "* ]]
}

verify_macho_architectures() {
  local path kind arch_list
  while IFS= read -r -d '' path; do
    kind="$(file -b "$path")"
    if [[ "$kind" == *Mach-O* ]]; then
      arch_list="$(lipo -archs "$path" 2>/dev/null)" || {
        echo "Error: unable to inspect Mach-O architectures: $path" >&2
        return 1
      }
      if [[ "$UNIVERSAL" -eq 1 ]]; then
        has_arch "$arch_list" arm64 || {
          echo "Error: Universal 2 bundle member is missing arm64: $path ($arch_list)" >&2
          return 1
        }
        has_arch "$arch_list" x86_64 || {
          echo "Error: Universal 2 bundle member is missing x86_64: $path ($arch_list)" >&2
          return 1
        }
      elif [[ -n "$expected_arch" ]]; then
        has_arch "$arch_list" "$expected_arch" || {
          echo "Error: bundle member has unexpected architecture: $path ($arch_list)" >&2
          return 1
        }
      fi
    fi
  done < <(find "$APP_PATH" -type f -print0)
}

verify_macho_architectures
if [[ "$UNIVERSAL" -eq 1 ]]; then
  ARCHITECTURES="$(lipo -archs "$EXECUTABLE")"
  has_arch "$ARCHITECTURES" arm64 || { echo "Error: executable is missing arm64: $ARCHITECTURES" >&2; exit 1; }
  has_arch "$ARCHITECTURES" x86_64 || { echo "Error: executable is missing x86_64: $ARCHITECTURES" >&2; exit 1; }
  echo "Universal 2 architectures: $ARCHITECTURES"
fi

echo "Signing app bundle ($SIGN_IDENTITY)…"
codesign --force --deep --sign "$SIGN_IDENTITY" "$APP_PATH"

test "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$PLIST")" = "dev.shardlane.app" || {
  echo "Error: unexpected CFBundleIdentifier in $PLIST." >&2
  exit 1
}
plutil -lint "$PLIST" >/dev/null
codesign --verify --deep --strict "$APP_PATH"
verify_macho_architectures
if [[ "$WITH_MOBILE_WEB" -eq 1 ]]; then
  test -f "$APP_PATH/Contents/Resources/mobile-web/index.html" || {
    echo "Error: Mobile Web bundle is missing index.html." >&2
    exit 1
  }
fi

echo "Verified: $APP_PATH"
echo "Icon: $ICON"

if [[ "$INSTALL_APP" -eq 1 ]]; then
  INSTALL_ROOT="${SHARDLANE_INSTALL_ROOT:-$HOME/Applications}"
  INSTALL_PATH="$INSTALL_ROOT/Shardlane.app"
  mkdir -p "$INSTALL_ROOT"
  rm -rf "$INSTALL_PATH"
  ditto "$APP_PATH" "$INSTALL_PATH"
  echo "Installed: $INSTALL_PATH"
fi
