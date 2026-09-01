#!/usr/bin/env bash
# scripts/verify.sh — Shardlane tiered verification: fast refactor loop.
#
# [INPUT]: Depends on cargo and the repository Cargo.lock; no external services, no Git writes
# [OUTPUT]: Provides five verification tiers: check | unit [FILTER] | history | fast | full
# [POS]: Fast verification entry point for edits/refactors; the full tier == AGENTS.md static gates (same source as CI)
#
# Usage:
#   scripts/verify.sh check               # Fastest: compile check only (run repeatedly while moving code around)
#   scripts/verify.sh unit [FILTER]       # Targeted unit tests; FILTER defaults to herdr::tests
#   scripts/verify.sh unit request_param  # Example: 015 request-shape contract tests
#   scripts/verify.sh history             # All shardlane-history crate tests
#   scripts/verify.sh fast                # fmt + clippy (both crates in this repo) + all unit tests
#   scripts/verify.sh full                # AGENTS.md four gates + git diff --check
set -euo pipefail
cd "$(dirname "$0")/.."

cmd="${1:-fast}"
case "$cmd" in
  check)
    # Minimal refactor/move loop: only proves "it still compiles".
    cargo check --locked --workspace -q
    ;;
  unit)
    # Targeted unit tests: compile only the shardlane crate and run tests matching FILTER.
    filter="${2:-herdr::tests}"
    cargo test --locked -p shardlane "$filter"
    ;;
  history)
    # shardlane-history crate tests in isolation (small crate, second-scale loop).
    cargo test --locked -p shardlane-history
    ;;
  fast)
    # Pre-commit loop: format + clippy for both crates + all unit tests (no full build).
    cargo fmt -- --check
    cargo clippy --locked -p shardlane -p shardlane-history --all-targets -- -D warnings
    cargo test --locked -p shardlane
    cargo test --locked -p shardlane-history
    ;;
  full)
    # Completion gates: identical to AGENTS.md / CI (checks.yml).
    cargo fmt -- --check
    cargo clippy --locked --workspace --all-targets -- -D warnings
    cargo test --locked --workspace
    cargo build --locked --workspace
    git diff --check
    git diff --cached --check
    ;;
  *)
    echo "unknown tier: $cmd (use check|unit|history|fast|full)" >&2
    exit 2
    ;;
esac
