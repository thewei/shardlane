#!/usr/bin/env bash
# scripts/verify.sh — Shardlane tiered verification: fast refactor loop.
#
# [INPUT]: cargo/Cargo.lock for Rust tiers; swiftc/python3 for the UI harness
#          tier; no external services, no Git writes
# [OUTPUT]: Provides six verification tiers: check | unit [FILTER] | history | ui | fast | full
# [POS]: Fast verification entry point for edits/refactors; the full tier == AGENTS.md static gates (same source as CI)
# [PROTOCOL]: Update this header on change, then check CLAUDE.md.
#
# Usage:
#   scripts/verify.sh check               # Fastest: compile check only (run repeatedly while moving code around)
#   scripts/verify.sh unit [FILTER]       # Targeted unit tests; FILTER defaults to herdr::tests
#   scripts/verify.sh unit request_param  # Example: 015 request-shape contract tests
#   scripts/verify.sh history             # All shardlane-history crate tests
#   scripts/verify.sh ui                  # UI harness syntax/type/capability/evidence checks (no GUI actions)
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
  ui)
    # Validate the real-app acceptance chain without launching or focusing a GUI.
    for script in scripts/*.sh; do bash -n "$script"; done
    swiftc -typecheck scripts/keyrepeat-evpost.swift
    swiftc -typecheck scripts/vision-ocr.swift
    swiftc -typecheck scripts/render-app-icon.swift
    python3 -B - <<'PY'
import ast
import json
from pathlib import Path

for path in (
    Path("scripts/acceptance-evidence.py"),
    Path("scripts/acceptance-capabilities.py"),
    Path("scripts/acceptance/__init__.py"),
    Path("scripts/acceptance/capabilities.py"),
    Path("scripts/acceptance/assertions.py"),
    Path("scripts/tests/test_acceptance_capabilities.py"),
    Path("scripts/tests/test_acceptance_assertions.py"),
    Path("scripts/tests/test_acceptance_evidence.py"),
):
    ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
json.loads(Path(".agents/skills/ui-acceptance-testing/evals/evals.json").read_text(encoding="utf-8"))
PY
    python3 -B -m unittest discover -s scripts/tests -p 'test_*.py' -v
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
    echo "unknown tier: $cmd (use check|unit|history|ui|fast|full)" >&2
    exit 2
    ;;
esac
