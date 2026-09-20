#!/usr/bin/env bash
#
# [INPUT]: 依赖 zig 工具链、git、llvm-nm/nm、strings/llvm-strings（缺失时退化为
#           grep -aoE）、任一 sha256 工具（shasum/sha256sum/openssl）、
#           ghostty-org/ghostty 上游仓库；目标三元组映射表内置（Rust triple -> Zig target）
# [OUTPUT]: 产出 vendor/ghostty-vt/lib/<rust-triple>/libghostty-vt.a（windows-msvc 为
#           ghostty-vt.lib），并按 PIN.md 协议追加来源记录；ABI 门禁失败即拒绝入库
# [POS]: scripts 的 vendored 终端语义层生产入口；多平台 release 工作流的库来源，
#        与 build.rs 的 <triple> 目录选择协议一一对应
# [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
#
# Produce one per-target vendored libghostty-vt static archive from upstream
# Ghostty (build.zig emit-lib-vt), verify the exported symbol set against the
# baseline archive, and record provenance in vendor/ghostty-vt/PIN.md.
#
# Usage:
#   scripts/vendor-ghostty-vt.sh --triple <rust-triple> --ghostty-ref <tag|commit>
#       [--zig <path-to-zig>] [--allow-layout-drift] [--skip-symbols]
#
# Supported triples (Rust -> Zig):
#   aarch64-apple-darwin        -> aarch64-macos
#   x86_64-apple-darwin         -> x86_64-macos
#   x86_64-unknown-linux-gnu    -> x86_64-linux-gnu
#   x86_64-pc-windows-msvc      -> x86_64-windows-msvc
#
# The ABI gate: every exported ghostty_* symbol present in the baseline
# archive (lib/aarch64-apple-darwin/libghostty-vt.a, 162-symbol set) must
# exist in the produced archive. Struct-layout JSON embedded in both binaries
# is compared too; differences require --allow-layout-drift and are recorded
# in the PIN entry.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
VENDOR_ROOT="$REPO_ROOT/vendor/ghostty-vt"
BASELINE_LIB="$VENDOR_ROOT/lib/aarch64-apple-darwin/libghostty-vt.a"
PIN_FILE="$VENDOR_ROOT/PIN.md"
GHOSTTY_REPO="${GHOSTTY_VT_REPO:-https://github.com/ghostty-org/ghostty.git}"

TRIPLE=""
GHOSTTY_REF=""
ZIG_BIN="zig"
ALLOW_LAYOUT_DRIFT=0
SKIP_SYMBOLS=0

usage() { grep -E "^#" "$0" | sed -n "s/^# \{0,1\}//p" | head -40; exit 0; }
while [ $# -gt 0 ]; do
    case "$1" in
        --triple) TRIPLE="$2"; shift 2 ;;
        --ghostty-ref) GHOSTTY_REF="$2"; shift 2 ;;
        --zig) ZIG_BIN="$2"; shift 2 ;;
        --allow-layout-drift) ALLOW_LAYOUT_DRIFT=1; shift ;;
        --skip-symbols) SKIP_SYMBOLS=1; shift ;;
        -h|--help) usage ;;
        *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

[ -n "$TRIPLE" ] || { echo "--triple is required" >&2; exit 2; }
[ -n "$GHOSTTY_REF" ] || { echo "--ghostty-ref is required (explicit provenance per PIN.md doctrine)" >&2; exit 2; }

case "$TRIPLE" in
    aarch64-apple-darwin)     ZIG_TARGET="aarch64-macos" ;;
    x86_64-apple-darwin)      ZIG_TARGET="x86_64-macos" ;;
    x86_64-unknown-linux-gnu) ZIG_TARGET="x86_64-linux-gnu" ;;
    x86_64-pc-windows-msvc)   ZIG_TARGET="x86_64-windows-msvc" ;;
    *) echo "unsupported triple: $TRIPLE" >&2; exit 2 ;;
esac

command -v "$ZIG_BIN" >/dev/null 2>&1 || { echo "zig not found (tried: $ZIG_BIN)" >&2; exit 1; }
command -v git >/dev/null 2>&1 || { echo "git not found" >&2; exit 1; }

# nm that reads Mach-O, ELF, and COFF. Prefer llvm-nm; fall back to nm.
NM_BIN=""
for candidate in llvm-nm "$(command -v llvm-nm || true)" "$( [ "$(uname)" = "Darwin" ] && xcrun --find llvm-nm 2>/dev/null || true )" nm; do
    if [ -n "$candidate" ] && command -v "$candidate" >/dev/null 2>&1; then NM_BIN="$candidate"; break; fi
done
[ -n "$NM_BIN" ] || { echo "no nm/llvm-nm available for ABI verification" >&2; exit 1; }

symbols_of() {
    # Exported (defined) symbols only; keep the ghostty_* API surface. Normalize
    # the Mach-O leading underscore away so ELF/COFF candidate archives compare
    # equal to the Mach-O baseline.
    "$NM_BIN" -g --defined-only "$1" 2>/dev/null \
        | awk '{print $3}' \
        | grep -E "^_?ghostty_" | sed 's/^_//' | sort -u
}

hash256() {
    # Portable stdin sha256 (Git Bash ships sha256sum, macOS shasum, both may be absent).
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 | awk '{print $1}'
    elif command -v sha256sum >/dev/null 2>&1; then
        sha256sum | awk '{print $1}'
    else
        openssl dgst -sha256 | awk '{print $NF}'
    fi
}

layout_fingerprint() {
    # The archive embeds self-describing struct-layout JSON. Extract the
    # printable segments that carry Ghostty struct layout facts and hash them,
    # so drift across Ghostty snapshots is detectable without parsing Zig.
    if command -v strings >/dev/null 2>&1; then
        strings -n 24 "$1" 2>/dev/null
    elif command -v llvm-strings >/dev/null 2>&1; then
        llvm-strings -n 24 "$1" 2>/dev/null
    else
        grep -aoE "[[:print:]]{24,}" "$1" 2>/dev/null
    fi \
        | grep -E "Ghostty(Style|GridRef|Selection|TerminalOptions|RenderStateColors)" \
        | grep -E "offset|size" | sort -u | hash256
}

WORK="$(mktemp -d)"
trap "rm -rf "$WORK"" EXIT
echo "==> cloning $GHOSTTY_REPO at $GHOSTTY_REF"
git clone --filter=blob:none --no-checkout "$GHOSTTY_REPO" "$WORK/ghostty" >/dev/null 2>&1
git -C "$WORK/ghostty" checkout --detach "$GHOSTTY_REF" >/dev/null 2>&1
GHOSTTY_SHA="$(git -C "$WORK/ghostty" rev-parse HEAD)"
echo "==> ghostty commit: $GHOSTTY_SHA"

echo "==> zig build -Demit-lib-vt=true -Dtarget=$ZIG_TARGET -Doptimize=ReleaseFast"
git -C "$WORK/ghostty" submodule update --init --depth 1 >/dev/null 2>&1 || true
(cd "$WORK/ghostty" && "$ZIG_BIN" build -Demit-lib-vt=true -Demit-exe=false \
    -Doptimize=ReleaseFast -Dtarget="$ZIG_TARGET" --prefix "$WORK/prefix")

if [ "$TRIPLE" = "x86_64-pc-windows-msvc" ]; then
    ARTIFACT="$(find "$WORK" -type f \( -name "ghostty-vt.lib" -o -name "ghostty-vt-static.lib" -o -name "libghostty-vt.a" \) | head -1)"
    OUT_NAME="ghostty-vt.lib"
else
    ARTIFACT="$(find "$WORK" -type f -name "libghostty-vt.a" | head -1)"
    OUT_NAME="libghostty-vt.a"
fi
[ -n "$ARTIFACT" ] || { echo "emit-lib-vt produced no static archive" >&2; exit 1; }
echo "==> artifact: $ARTIFACT ($(wc -c < "$ARTIFACT" | tr -d " ") bytes)"

# --- ABI gate: baseline symbol coverage ---
SYMBOL_VERDICT="skipped"
if [ "$SKIP_SYMBOLS" -eq 0 ]; then
    [ -f "$BASELINE_LIB" ] || { echo "baseline archive missing: $BASELINE_LIB" >&2; exit 1; }
    symbols_of "$BASELINE_LIB" > "$WORK/baseline.syms" || {
        echo "ABI gate: could not extract baseline symbols with $NM_BIN" >&2
        exit 1
    }
    [ -s "$WORK/baseline.syms" ] || {
        echo "ABI gate: baseline symbol extraction is empty ($NM_BIN vs Mach-O baseline)" >&2
        exit 1
    }
    symbols_of "$ARTIFACT" > "$WORK/candidate.syms"
    # grep -Fxvf instead of comm: equivalent for sorted unique sets and present
    # in Git Bash, where comm is not guaranteed.
    MISSING="$(grep -Fxvf "$WORK/candidate.syms" "$WORK/baseline.syms" || true)"
    if [ -n "$MISSING" ]; then
        echo "ABI gate FAILED: baseline symbols missing from produced archive:" >&2
        echo "$MISSING" >&2
        exit 1
    fi
    NEW_COUNT="$(wc -l < "$WORK/candidate.syms" | tr -d " ")"
    BASE_COUNT="$(wc -l < "$WORK/baseline.syms" | tr -d " ")"
    SYMBOL_VERDICT="ok baseline=$BASE_COUNT candidate=$NEW_COUNT"
    echo "==> ABI symbols: $SYMBOL_VERDICT"
fi

# --- Layout drift gate ---
BASE_FP="$(layout_fingerprint "$BASELINE_LIB")"
CAND_FP="$(layout_fingerprint "$ARTIFACT")"
LAYOUT_VERDICT="match"
if [ "$BASE_FP" != "$CAND_FP" ]; then
    if [ "$ALLOW_LAYOUT_DRIFT" -eq 1 ]; then
        LAYOUT_VERDICT="drift-allowed (baseline=$BASE_FP candidate=$CAND_FP)"
        echo "==> layout drift recorded: $LAYOUT_VERDICT"
    else
        echo "struct-layout JSON drifted; re-verify the 21 extern bindings in" >&2
        echo "crates/herdr-gui/src/ghostty.rs, then rerun with --allow-layout-drift" >&2
        exit 1
    fi
fi

# --- Install + record ---
OUT_DIR="$VENDOR_ROOT/lib/$TRIPLE"
mkdir -p "$OUT_DIR"
cp "$ARTIFACT" "$OUT_DIR/$OUT_NAME"

{
    echo ""
    echo "## $TRIPLE — produced $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo ""
    echo "- Source: $GHOSTTY_REPO @ $GHOSTTY_SHA (ref: $GHOSTTY_REF)"
    echo "- Zig target: $ZIG_TARGET ($("$ZIG_BIN" version))"
    echo "- Build: zig build -Demit-lib-vt=true -Demit-exe=false -Doptimize=ReleaseFast -Dtarget=$ZIG_TARGET"
    echo "- Artifact: lib/$TRIPLE/$OUT_NAME ($(wc -c < "$OUT_DIR/$OUT_NAME" | tr -d " ") bytes)"
    echo "- ABI: $SYMBOL_VERDICT; layout: $LAYOUT_VERDICT"
    echo "- Command: $0 --triple $TRIPLE --ghostty-ref $GHOSTTY_REF${ALLOW_LAYOUT_DRIFT:+ --allow-layout-drift}"
} >> "$PIN_FILE"
echo "==> installed $OUT_DIR/$OUT_NAME and appended the PIN.md entry"
