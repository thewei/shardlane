#!/usr/bin/env python3
"""
[INPUT]: ``check``/``mcp-snippet`` command options and the repository root.
[OUTPUT]: JSON or text capability inventory, plus a read-only Computer Use MCP
          probe snippet; no UI action is performed by this executable.
[POS]: Agent-facing entry point for quickly selecting a safe acceptance driver.
[PROTOCOL]: Update this header on change, then check CLAUDE.md.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.acceptance.capabilities import (  # noqa: E402
    Availability,
    local_capability_report,
    mcp_probe_script,
)


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Shardlane no-GUI acceptance capability probe")
    sub = parser.add_subparsers(dest="command", required=True)

    check = sub.add_parser("check", help="inventory local tools and guarded drivers")
    check.add_argument("--driver", help="computer-use (default) or native")
    check.add_argument("--root", type=Path, default=ROOT)
    check.add_argument(
        "--mcp-export",
        action="append",
        default=None,
        help="resolved export from the read-only node_repl probe (repeatable)",
    )
    check.add_argument("--json", action="store_true", help="print machine-readable JSON")
    check.add_argument(
        "--strict",
        action="store_true",
        help="return non-zero unless every reported capability is available",
    )

    snippet = sub.add_parser("mcp-snippet", help="print a read-only node_repl Computer Use probe")
    snippet.add_argument("--json", action="store_true", help="wrap the snippet in JSON")
    return parser


def _check(args: argparse.Namespace) -> int:
    report = local_capability_report(args.root, driver=args.driver, mcp_exports=args.mcp_export)
    if args.json:
        print(json.dumps(report.as_dict(), ensure_ascii=False, indent=2))
    else:
        print(f"driver={report.driver} overall={report.overall.value}")
        for item in report.results:
            print(f"[{item.status.value}] {item.name}: {item.detail}")
    if any(item.status is Availability.UNAVAILABLE for item in report.results):
        return 1
    if args.strict and report.overall is not Availability.AVAILABLE:
        return 1
    return 0


def _mcp_snippet(args: argparse.Namespace) -> int:
    script = mcp_probe_script()
    if args.json:
        print(json.dumps({"transport": "mcp__node_repl__js", "code": script}, ensure_ascii=False))
    else:
        print(script)
    return 0


def main() -> int:
    args = _parser().parse_args()
    if args.command == "check":
        return _check(args)
    return _mcp_snippet(args)


if __name__ == "__main__":
    raise SystemExit(main())
