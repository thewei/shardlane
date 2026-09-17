#!/usr/bin/env bash
#
# [INPUT]: cargo build 产物（target/<triple>/release/shardlane）、仓库 LICENSE、
#           tar/sha256sum；可选 --triple 交叉目标
# [OUTPUT]: dist/Shardlane-linux-<arch>.tar.gz + .sha256（二进制 + README + LICENSE）
# [POS]: scripts 的 Linux 发布打包入口，release.yml linux-x86_64 作业调用；
#        与 archive-macos.sh 平级，只编排产物不做业务
# [PROTOCOL]: Update scripts/CLAUDE.md on change, then check /CLAUDE.md.
set -euo pipefail

TRIPLE="x86_64-unknown-linux-gnu"
while [ $# -gt 0 ]; do
    case "$1" in
        --target) TRIPLE="$2"; shift 2 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done
case "$TRIPLE" in
    *x86_64*) ARCH="x86_64" ;;
    *aarch64*) ARCH="aarch64" ;;
    *) ARCH="unknown" ;;
esac

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$REPO_ROOT/target/$TRIPLE/release/shardlane"
[ -x "$BIN" ] || { echo "missing release binary: $BIN" >&2; exit 1; }

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/Shardlane-linux-$ARCH"
cp "$BIN" "$STAGE/Shardlane-linux-$ARCH/shardlane"
cp "$REPO_ROOT/LICENSE" "$STAGE/Shardlane-linux-$ARCH/LICENSE"
cat > "$STAGE/Shardlane-linux-$ARCH/README-linux.txt" <<'README'
Shardlane (Linux x86_64)
========================

Run:
  ./shardlane

Runtime requirements:
  - A Vulkan-capable GPU driver (the UI renders through Vulkan).
  - A Wayland or X11 desktop session.
  - Herdr runtime (https://herdr.dev): install with
      curl -fsS https://herdr.dev/install.sh | sh
    Shardlane discovers the herdr CLI on PATH and drives the local runtime.

First launch tips:
  - Wayland is the default session; X11 works too.
  - Without Vulkan the window cannot start - update your driver first.
README

DIST="$REPO_ROOT/dist"
mkdir -p "$DIST"
TARBALL="$DIST/Shardlane-linux-$ARCH.tar.gz"
tar -czf "$TARBALL" -C "$STAGE" "Shardlane-linux-$ARCH"
if command -v sha256sum >/dev/null 2>&1; then
    (cd "$DIST" && sha256sum "$(basename "$TARBALL")" > "$(basename "$TARBALL").sha256")
else
    (cd "$DIST" && shasum -a 256 "$(basename "$TARBALL")" > "$(basename "$TARBALL").sha256")
fi
echo "packaged: $TARBALL"
