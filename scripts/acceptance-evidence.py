#!/usr/bin/env python3
"""
[INPUT]: an evidence JSONL path plus bounded phase/status/message fields.
[OUTPUT]: append-only, schema-versioned acceptance events and a compact summary.
[POS]: the shared evidence ledger for native and Computer Use acceptance runs;
       it records control-plane facts only and never terminal/user text.
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import math
import os
import pathlib
import re
import sys
from collections import Counter


SCHEMA_VERSION = 1
STATUSES = {"pass", "fail", "skip", "info"}
DRIVERS = {"computer-use", "native"}
MAX_MESSAGE = 240
MAX_METRIC_KEY = 48
METRIC_KEY = re.compile(r"^[A-Za-z][A-Za-z0-9_.-]*$")


def bounded(value: str, field: str) -> str:
    value = value.strip()
    if not value or len(value) > MAX_MESSAGE or any(ord(char) < 0x20 for char in value):
        raise ValueError(f"{field} must be non-empty, single-line, and <= {MAX_MESSAGE} chars")
    return value


def record(args: argparse.Namespace) -> int:
    driver = bounded(args.driver, "driver")
    if driver not in DRIVERS:
        raise ValueError(f"driver must be one of {sorted(DRIVERS)}")
    phase = bounded(args.phase, "phase")
    status = bounded(args.status, "status")
    message = bounded(args.message, "message")
    if status not in STATUSES:
        raise ValueError(f"status must be one of {sorted(STATUSES)}")

    metrics: dict[str, float | str] = {}
    for item in args.metric:
        key, separator, value = item.partition("=")
        if (
            not separator
            or not key
            or not value
            or len(key) > MAX_METRIC_KEY
            or METRIC_KEY.fullmatch(key) is None
        ):
            raise ValueError(f"invalid metric (expected key=value): {item!r}")
        try:
            numeric = float(value)
        except ValueError:
            metrics[key] = bounded(value, f"metric {key}")
        else:
            if not math.isfinite(numeric):
                raise ValueError(f"metric {key} must be finite")
            metrics[key] = numeric

    path = pathlib.Path(args.path)
    path.parent.mkdir(parents=True, exist_ok=True)
    event = {
        "schema": SCHEMA_VERSION,
        "ts": dt.datetime.now(dt.timezone.utc).isoformat(timespec="milliseconds"),
        "driver": driver,
        "phase": phase,
        "status": status,
        "message": message,
    }
    if metrics:
        event["metrics"] = metrics
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(event, ensure_ascii=False, separators=(",", ":")) + "\n")
    os.chmod(path, 0o600)
    return 0


def summary(args: argparse.Namespace) -> int:
    path = pathlib.Path(args.path)
    counts: Counter[str] = Counter()
    phases: set[str] = set()
    phase_statuses: dict[str, set[str]] = {}
    invalid = 0
    if path.exists():
        for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            try:
                event = json.loads(line)
                if event.get("schema") != SCHEMA_VERSION:
                    raise ValueError("schema")
                status = event["status"]
                phase = event["phase"]
                driver = event["driver"]
                message = event["message"]
                timestamp = event["ts"]
                parsed_timestamp = dt.datetime.fromisoformat(timestamp)
                if parsed_timestamp.tzinfo is None:
                    raise ValueError("timestamp")
                if (
                    status not in STATUSES
                    or driver not in DRIVERS
                    or not isinstance(phase, str)
                    or not phase
                    or len(phase) > MAX_MESSAGE
                    or any(ord(char) < 0x20 for char in phase)
                    or not isinstance(message, str)
                    or not message
                    or len(message) > MAX_MESSAGE
                    or any(ord(char) < 0x20 for char in message)
                ):
                    raise ValueError("fields")
                metrics = event.get("metrics", {})
                if not isinstance(metrics, dict):
                    raise ValueError("metrics")
                for key, value in metrics.items():
                    if not isinstance(key, str) or len(key) > MAX_METRIC_KEY or METRIC_KEY.fullmatch(key) is None:
                        raise ValueError("metric key")
                    if not isinstance(value, (int, float, str)) or isinstance(value, float) and not math.isfinite(value):
                        raise ValueError("metric value")
                counts[status] += 1
                phases.add(phase)
                phase_statuses.setdefault(phase, set()).add(status)
            except (ValueError, KeyError, TypeError, json.JSONDecodeError):
                print(f"evidence: invalid line {line_number}", file=sys.stderr)
                invalid += 1
    else:
        print(f"evidence: missing {path}", file=sys.stderr)
        invalid += 1

    missing = [
        phase
        for phase in args.require_phase
        if "pass" not in phase_statuses.get(phase, set())
    ]
    if missing:
        print(f"evidence: required phases without pass: {','.join(missing)}", file=sys.stderr)
    ordered = " ".join(f"{status}={counts[status]}" for status in sorted(STATUSES))
    print(f"evidence: {ordered} phases={len(phases)} invalid={invalid} missing={len(missing)}")
    return 1 if invalid or missing else 0


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description="Shardlane acceptance evidence ledger")
    sub = root.add_subparsers(dest="command", required=True)

    record_parser = sub.add_parser("record", help="append one control-plane event")
    record_parser.add_argument("path")
    record_parser.add_argument("--driver", required=True)
    record_parser.add_argument("--phase", required=True)
    record_parser.add_argument("--status", required=True)
    record_parser.add_argument("--message", required=True)
    record_parser.add_argument("--metric", action="append", default=[])
    record_parser.set_defaults(function=record)

    summary_parser = sub.add_parser("summary", help="validate and summarize JSONL")
    summary_parser.add_argument("path")
    summary_parser.add_argument(
        "--require-phase", action="append", default=[], help="phase that must contain a pass event"
    )
    summary_parser.set_defaults(function=summary)
    return root


def main() -> int:
    args = parser().parse_args()
    try:
        return args.function(args)
    except ValueError as error:
        print(f"evidence: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
