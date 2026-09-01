#!/usr/bin/env bash
# -----------------------------------------------------------------------------
# [INPUT]: vendored gpui (vendor/gpui, = upstream pristine + patches applied),
#          patch files vendor/patches/gpui/*.patch (the sole delta source of truth),
#          crate source origins (registry src/cache or the static.crates.io CDN),
#          Cargo.lock (the gpui version resolved for the workspace),
#          crepuscularity-gpui's exact `gpui = "=X.Y.Z"` pin
# [OUTPUT]: Sustainable upstream/downstream sync for vendored gpui: status (baseline/resolved version/patch consistency),
#          export-patch (regenerate the patch file from the current vendor delta; non-whitelisted deltas error out),
#          verify (pristine+patch round-trip == vendor),
#          sync [--version V] (fetch pristine → apply patches → swap in → build).
#          The version only changes when crepuscularity-gpui's pin is upgraded; this script never unpins.
# [POS]: vendor/gpui upstream/downstream update mechanism (maintenance companion of the latency-evidence playbook);
#        only orchestrates pristine/patch/build verification; owns no gpui upstream code and no product logic
# -----------------------------------------------------------------------------

set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
VENDOR="$ROOT_DIR/vendor/gpui"
PATCH_DIR="$ROOT_DIR/vendor/patches/gpui"
META="$VENDOR/PACING-PATCH.md"
REGISTRY_SRC_GLOB="$HOME/.cargo/registry/src/*/gpui-VERSION"
REGISTRY_CACHE_GLOB="$HOME/.cargo/registry/cache/*/gpui-VERSION.crate"
CRATES_IO="https://static.crates.io/crates/gpui"
# Files the patch is allowed to touch (relative to the vendor root). Any other delta = unregistered hand edits; error out.
PATCH_WHITELIST=("src/window.rs" "PACING-PATCH.md")

usage() {
  cat <<'EOF'
Usage:
  update-vendored-gpui.sh status              # baseline version / resolved version / pin constraint / patch consistency
  update-vendored-gpui.sh export-patch        # regenerate the patch file from the current vendor delta (whitelist validated)
  update-vendored-gpui.sh verify              # pristine(base)+patch round-trip is byte-identical to vendor
  update-vendored-gpui.sh sync [--version V]  # fetch pristine (default: Cargo.lock resolved version) → apply patches → swap in → cargo build

Workflow convention (when a wrapper upgrade brings a new gpui pin):
  1. Bump the crepuscularity-gpui version; 2. run this script's sync --version <new pin>; 3. if the patch conflicts,
     first check whether upstream already fixed the target behavior (if fixed, just delete the patch file and return to pristine);
     otherwise merge manually into the patches file and sync again; 4. cargo test + scripts/keyrepeat-pacing-ab.sh run gpui-<ver>
     --samples 3, and compare against the old label; 5. after all gates pass, commit vendor/gpui + patches + lock.
EOF
}

log() { echo "[update-gpui] $*"; }
die() { echo "[update-gpui] FAIL: $*" >&2; exit 1; }

base_version() {
  rg -m1 '^upstream-version: ' "$META" | rg -o '[0-9]+\.[0-9]+\.[0-9]+'
}

resolved_version() {
  rg -A1 '^name = "gpui"$' "$ROOT_DIR/Cargo.lock" | rg '^version = ' | rg -o '[0-9]+\.[0-9]+\.[0-9]+'
}

pin_version() {
  local wrapper
  wrapper="$(ls -d "$HOME"/.cargo/registry/src/*/crepuscularity-gpui-*/ 2>/dev/null | sort -V | tail -1)"
  [[ -n "$wrapper" ]] || { echo "unknown"; return; }
  rg -A2 '^\[dependencies\.gpui\]' "$wrapper/Cargo.toml" | rg '^version = ' | rg -o '[0-9]+\.[0-9]+\.[0-9]+' || echo "unknown"
}

fetch_pristine() {
  local ver="$1" dest="$2" d c
  rm -rf "$dest"; mkdir -p "$dest"
  for d in $HOME/.cargo/registry/src/*/gpui-"$ver"; do
    [[ -d "$d" ]] && { cp -R "$d/." "$dest/"; log "pristine $ver from registry src"; return 0; }
  done
  for c in $HOME/.cargo/registry/cache/*/gpui-"$ver".crate; do
    [[ -f "$c" ]] && { tar xzf "$c" -C "$dest" --strip-components 1; log "pristine $ver from registry cache"; return 0; }
  done
  log "downloading $ver from crates.io CDN..."
  curl -fsSL "$CRATES_IO/gpui-$ver.crate" | tar xz -C "$dest" --strip-components 1
}

# File list of the current vendor relative to the pristine baseline (modified/deleted/added, sorted and deduplicated by relative path)
delta_files() {
  local base="$1"
  diff -rq "$base" "$VENDOR" 2>/dev/null | sed -E \
    -e 's/^Files (.+) and (.+) differ$/\2/' \
    -e 's/^Only in (.+): (.+)$/\1\/\2/' \
  | sed -E "s#$base/##; s#$VENDOR/##" | sort -u
}

patch_files_ok() {
  local base="$1"
  local bad=0 f
  while IFS= read -r f; do
    [[ -z "$f" ]] && continue
    local ok=0
    for w in "${PATCH_WHITELIST[@]}"; do [[ "$f" == "$w" ]] && ok=1; done
    if [[ "$ok" != 1 ]]; then echo "  unexpected delta: $f"; bad=1; fi
  done < <(delta_files "$base")
  return $bad
}

make_git_repo() {
  local dir="$1" msg="$2"
  git -C "$dir" init -q
  git -C "$dir" add -A
  git -C "$dir" -c user.email=vendor@local -c user.name=vendor commit -qm "$msg"
}

cmd_status() {
  [[ -f "$META" ]] || die "missing $META"
  local base resolved pin
  base="$(base_version)"; resolved="$(resolved_version)"; pin="$(pin_version)"
  echo "vendored base version : $base"
  echo "Cargo.lock resolved   : $resolved"
  echo "wrapper pin (exact)   : $pin"
  [[ "$base" == "$resolved" ]] || echo "  !! vendor base != resolved version — run: $0 sync"
  local tmp; tmp="$(mktemp -d)"
  fetch_pristine "$base" "$tmp/pristine"
  if patch_files_ok "$tmp/pristine"; then
    echo "patch delta           : clean (whitelisted files only)"
  else
    echo "patch delta           : UNEXPECTED FILES (see above) — hand edits are not registered; export-patch will fail"
  fi
  rm -rf "$tmp"
}

cmd_export_patch() {
  local base; base="$(base_version)"
  local tmp; tmp="$(mktemp -d)"
  fetch_pristine "$base" "$tmp/pristine"
  if ! patch_files_ok "$tmp/pristine"; then
    rm -rf "$tmp"
    die "vendor contains unregistered deltas (see list above). Integrate or revert them, then export."
  fi
  mkdir -p "$tmp/repo"
  cp -R "$tmp/pristine/." "$tmp/repo/"
  make_git_repo "$tmp/repo" "base"
  rsync -a --exclude PACING-PATCH.md "$VENDOR/" "$tmp/repo/" 2>/dev/null || cp -R "$VENDOR/." "$tmp/repo/"
  git -C "$tmp/repo" add -A
  mkdir -p "$PATCH_DIR"
  git -C "$tmp/repo" diff --cached --src-prefix=a/ --dst-prefix=b/ > "$PATCH_DIR/pacing-re-present.patch"
  local count; count=$(rg -c '^diff --git' "$PATCH_DIR/pacing-re-present.patch" || echo 0)
  log "exported $count file(s) to $PATCH_DIR/pacing-re-present.patch"
  rm -rf "$tmp"
}

cmd_verify() {
  local base; base="$(base_version)"
  local tmp; tmp="$(mktemp -d)"
  fetch_pristine "$base" "$tmp/check"
  ( cd "$tmp/check" && patch -p1 --forward --silent < "$PATCH_DIR/pacing-re-present.patch" )
  cp "$VENDOR/PACING-PATCH.md" "$tmp/check/PACING-PATCH.md"
  if diff -rq "$tmp/check" "$VENDOR" >/dev/null 2>&1; then
    log "verify OK: pristine($base) + patch == vendor (byte-identical)"
  else
    diff -rq "$tmp/check" "$VENDOR" | head -10
    die "round-trip mismatch"
  fi
  rm -rf "$tmp"
}

cmd_sync() {
  local ver="" explicit=0
  while (($# > 0)); do
    case "$1" in
      --version) ver="$2"; explicit=1; shift 2 ;;
      *) shift ;;
    esac
  done
  [[ -n "$ver" ]] || ver="$(resolved_version)"
  [[ -f "$PATCH_DIR/pacing-re-present.patch" ]] || die "patch file missing (run export-patch first)"
  log "syncing vendor to gpui $ver (explicit=$explicit)..."

  local tmp; tmp="$(mktemp -d)"
  fetch_pristine "$ver" "$tmp/new"
  if ( cd "$tmp/new" && patch -p1 --forward --silent < "$PATCH_DIR/pacing-re-present.patch" ); then
    log "patch applied cleanly"
  else
    ls "$tmp/new"/*.rej 2>/dev/null | head -5 || true
    rm -rf "$tmp"
    die "patch did not apply cleanly to $ver. Check whether upstream already fixed the behavior (drop the patch) or merge manually into $PATCH_DIR/, then rerun."
  fi
  # Documentation travels with the vendor (not inside the patch file)
  cp "$VENDOR/PACING-PATCH.md" "$tmp/new/PACING-PATCH.md"
  rm -rf "$VENDOR"
  mv "$tmp/new" "$VENDOR"
  rm -rf "$tmp"
  # Refresh the meta baseline version line
  python3 - "$META" "$ver" <<'PYEOF'
import sys, re
p, ver = sys.argv[1], sys.argv[2]
s = open(p).read()
if re.search(r'^upstream-version: ', s, re.M):
    s = re.sub(r'^upstream-version: .*$', f'upstream-version: {ver}', s, flags=re.M)
else:
    s = f"upstream-version: {ver}\n" + s
open(p, "w").write(s)
PYEOF
  log "building..."
  ( cd "$ROOT_DIR" && cargo build -p shardlane --bin shardlane )
  log "vendor synced to $ver and build OK."
  cat <<'EOF'
Next steps (do not skip):
  1. cargo test --locked --workspace
  2. scripts/keyrepeat-pacing-ab.sh run gpui-VER --samples 3   # compare with the old label via `report`
  3. If upstream already fixed the target behavior (scoring no longer depends on the patch), delete the patch file + sync back to pristine to shrink the vendor deviation
  4. All gates + commit vendor/gpui, vendor/patches, Cargo.lock
EOF
}

case "${1:-}" in
  status) shift; cmd_status "$@" ;;
  export-patch) shift; cmd_export_patch "$@" ;;
  verify) shift; cmd_verify "$@" ;;
  sync) shift; cmd_sync "$@" ;;
  *) usage; exit 2 ;;
esac
