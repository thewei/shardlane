#!/usr/bin/env bash
set -euo pipefail

# [INPUT]: An already-built Shardlane.app, optional archive directory, and a Rust/macOS architecture label
# [OUTPUT]: A publicly distributable ZIP plus a SHA-256 checksum file in the same directory; Universal 2 is uniformly named universal2
# [POS]: macOS release archive boundary; does not build, sign, or notarize, but re-verifies the bundle architecture

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

APP_PATH=""
OUTPUT_DIR="$REPO_ROOT/dist"
ARCHITECTURE="$(uname -m)"

usage() {
  cat <<'USAGE'
Usage: scripts/archive-macos.sh --app PATH [options]

Options:
  --app PATH                Existing .app bundle to archive (required).
  --output-dir PATH         Destination directory (default: ./dist).
  --architecture LABEL     Rust/macOS architecture (default: host architecture; universal2 is supported).
  -h, --help               Show this help.

The archive name is derived from the app bundle name:
  <AppName>-macos-<architecture>.zip (Universal 2: <AppName>-macos-universal2.zip)

The checksum file contains only the archive basename, so this works from the
output directory:
  shasum -a 256 -c <AppName>-macos-<architecture>.zip.sha256
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --app)
      [[ $# -ge 2 ]] || { echo "Error: --app requires a path." >&2; exit 2; }
      APP_PATH="$2"
      shift 2
      ;;
    --output-dir)
      [[ $# -ge 2 ]] || { echo "Error: --output-dir requires a path." >&2; exit 2; }
      OUTPUT_DIR="$2"
      shift 2
      ;;
    --architecture)
      [[ $# -ge 2 ]] || { echo "Error: --architecture requires a label." >&2; exit 2; }
      ARCHITECTURE="$2"
      shift 2
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

if [[ -z "$APP_PATH" ]]; then
  echo "Error: --app is required." >&2
  usage >&2
  exit 2
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "Error: macOS app archiving requires Darwin." >&2
  exit 1
fi

case "$ARCHITECTURE" in
  aarch64|arm64|aarch64-apple-darwin)
    ARCHITECTURE="aarch64"
    ;;
  x86_64|x86-64|x86_64-apple-darwin)
    ARCHITECTURE="x86_64"
    ;;
  universal|universal2|universal-2|universal_2)
    ARCHITECTURE="universal2"
    ;;
  ''|*[!A-Za-z0-9._-]*)
    echo "Error: invalid architecture label: $ARCHITECTURE" >&2
    exit 2
    ;;
esac

if [[ ! -d "$APP_PATH" || "$APP_PATH" != *.app ]]; then
  echo "Error: --app must point to an existing .app bundle: $APP_PATH" >&2
  exit 1
fi

command -v codesign >/dev/null || { echo "Error: codesign is required." >&2; exit 1; }
command -v ditto >/dev/null || { echo "Error: ditto is required." >&2; exit 1; }
command -v shasum >/dev/null || { echo "Error: shasum is required." >&2; exit 1; }
command -v file >/dev/null || { echo "Error: file is required." >&2; exit 1; }
command -v lipo >/dev/null || { echo "Error: lipo is required." >&2; exit 1; }

APP_PATH="$(cd "$(dirname "$APP_PATH")" && pwd)/$(basename "$APP_PATH")"
APP_NAME="$(basename "$APP_PATH")"
APP_NAME="${APP_NAME%.app}"
ARCHIVE_NAME="${APP_NAME}-macos-${ARCHITECTURE}.zip"
ARCHIVE_PATH="$OUTPUT_DIR/$ARCHIVE_NAME"
CHECKSUM_NAME="$ARCHIVE_NAME.sha256"
CHECKSUM_PATH="$OUTPUT_DIR/$CHECKSUM_NAME"

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
      if [[ "$ARCHITECTURE" == "universal2" ]]; then
        has_arch "$arch_list" arm64 || {
          echo "Error: Universal 2 bundle member is missing arm64: $path ($arch_list)" >&2
          return 1
        }
        has_arch "$arch_list" x86_64 || {
          echo "Error: Universal 2 bundle member is missing x86_64: $path ($arch_list)" >&2
          return 1
        }
      fi
    fi
  done < <(find "$APP_PATH" -type f -print0)
}

echo "Verifying app signature: $APP_PATH"
codesign --verify --deep --strict "$APP_PATH"
verify_macho_architectures

mkdir -p "$OUTPUT_DIR"
rm -f "$ARCHIVE_PATH" "$CHECKSUM_PATH"

echo "Creating archive: $ARCHIVE_PATH"
ditto -c -k --sequesterRsrc --keepParent "$APP_PATH" "$ARCHIVE_PATH"

(
  cd "$OUTPUT_DIR"
  shasum -a 256 "$ARCHIVE_NAME" > "$CHECKSUM_NAME"
)

echo "Created: $ARCHIVE_PATH"
echo "Checksum: $CHECKSUM_PATH"
