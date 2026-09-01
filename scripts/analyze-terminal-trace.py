#!/usr/bin/env python3
"""Summarize SHARDLANE_TERMINAL_TRACE=1 events from shardlane-lag.log.

The parser deliberately ignores user text. It consumes only terminal.trace key/value timing
records and reports stage-local and approximate input-packet end-to-end latency percentiles.
"""

from __future__ import annotations

import argparse
import math
import re
from collections import Counter, defaultdict
from pathlib import Path

FIELD_RE = re.compile(r"([A-Za-z0-9_]+)=([^\s]+)")
# Rust uses the maximum value of the field's integer width as an explicit
# "no input/read has happened" marker in a few opt-in trace fields. They are not
# latency samples and must not dominate percentile output with pseudo-values.
UNSET_DURATIONS = {(1 << 64) - 1, (1 << 128) - 1}


def parse_number(raw: str):
    raw = raw.rstrip(",")
    if raw in {"true", "false"}:
        return None
    try:
        if any(ch in raw for ch in ".eE"):
            return float(raw)
        return int(raw)
    except ValueError:
        return None


def percentile(values: list[float], q: float) -> float:
    if not values:
        return math.nan
    ordered = sorted(values)
    if len(ordered) == 1:
        return ordered[0]
    pos = (len(ordered) - 1) * q
    lo = math.floor(pos)
    hi = math.ceil(pos)
    if lo == hi:
        return ordered[lo]
    weight = pos - lo
    return ordered[lo] * (1.0 - weight) + ordered[hi] * weight


def fmt(value: float) -> str:
    if math.isnan(value):
        return "-"
    if abs(value) >= 1000:
        return f"{value:,.0f}"
    if abs(value) >= 10:
        return f"{value:.1f}"
    return f"{value:.2f}"


def print_metric_table(title: str, metrics: dict[str, list[float]]) -> None:
    rows = [(name, values) for name, values in sorted(metrics.items()) if values]
    if not rows:
        return
    print(f"\n{title}")
    print(f"{'metric':52} {'n':>6} {'p50 us':>10} {'p95 us':>10} {'max us':>10}")
    print("-" * 94)
    for name, values in rows:
        print(
            f"{name:52} {len(values):6d} "
            f"{fmt(percentile(values, 0.50)):>10} "
            f"{fmt(percentile(values, 0.95)):>10} "
            f"{fmt(max(values)):>10}"
        )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "log",
        nargs="?",
        default="/tmp/shardlane-lag.log",
        help="lag log path (default: /tmp/shardlane-lag.log)",
    )
    parser.add_argument(
        "--pid",
        type=int,
        help="only analyze one Shardlane PID (recommended when multiple instances exist)",
    )
    args = parser.parse_args()

    path = Path(args.log)
    if not path.is_file():
        raise SystemExit(f"trace log not found: {path}")

    stage_counts: Counter[str] = Counter()
    local_metrics: dict[str, list[float]] = defaultdict(list)
    packets: dict[int, dict[str, object]] = defaultdict(dict)
    parsed = 0

    with path.open("r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            if args.pid is not None and f"pid={args.pid}" not in line:
                continue
            if "terminal.trace" in line:
                fields = dict(FIELD_RE.findall(line.split("terminal.trace", 1)[1]))
                stage = fields.get("stage")
            else:
                shared_stage = next(
                    (
                        stage
                        for stage in ("write", "read")
                        if f"terminal.shared.{stage}" in line
                    ),
                    None,
                )
                if shared_stage is None:
                    continue
                fields = dict(
                    FIELD_RE.findall(line.split(f"terminal.shared.{shared_stage}", 1)[1])
                )
                stage = f"shared.{shared_stage}"
            if not stage:
                continue
            parsed += 1
            stage_counts[stage] += 1

            numeric = {key: parse_number(value) for key, value in fields.items()}
            for key, value in numeric.items():
                if key == "t_us" or not key.endswith("_us") or value is None:
                    continue
                if value in UNSET_DURATIONS:
                    continue
                local_metrics[f"{stage}.{key}"].append(float(value))

            input_id = numeric.get("input_id")
            t_us = numeric.get("t_us")
            if not isinstance(input_id, int) or input_id <= 0 or not isinstance(t_us, int):
                continue
            packet = packets[input_id]
            if stage in {"pty.enqueue", "shared.enqueue"}:
                packet.setdefault("enqueue_us", t_us)
                packet.setdefault("kind", fields.get("kind", "unknown"))
            elif stage in {"pty.write", "shared.write"}:
                packet.setdefault("write_us", t_us)
            elif stage in {"pty.read", "shared.read"}:
                packet.setdefault("read_us", t_us)
            elif stage == "frame.extract":
                packet.setdefault("extract_us", t_us)
            elif stage == "frame.present_request":
                packet.setdefault("present_us", t_us)
            elif stage == "pane.render_build":
                packet.setdefault("pane_render_us", t_us)

    if parsed == 0:
        raise SystemExit(
            "no terminal.trace records found; launch Shardlane with SHARDLANE_TERMINAL_TRACE=1"
        )

    print(f"Terminal trace: {path}")
    if args.pid is not None:
        print(f"PID filter: {args.pid}")
    print(f"Events parsed: {parsed}")
    print("Stage counts:")
    for stage, count in sorted(stage_counts.items()):
        print(f"  {stage:28} {count}")

    print_metric_table("Stage-local timings", local_metrics)

    end_to_end: dict[str, list[float]] = defaultdict(list)
    by_kind: dict[str, dict[str, list[float]]] = defaultdict(lambda: defaultdict(list))
    targets = {
        "write_us": "enqueue_to_write",
        "read_us": "enqueue_to_read",
        "extract_us": "enqueue_to_frame_extract",
        "present_us": "enqueue_to_present_request",
        "pane_render_us": "enqueue_to_pane_render",
    }
    for packet in packets.values():
        enqueue = packet.get("enqueue_us")
        if not isinstance(enqueue, int):
            continue
        kind = str(packet.get("kind", "unknown"))
        for field, name in targets.items():
            target = packet.get(field)
            if not isinstance(target, int) or target < enqueue:
                continue
            delta = float(target - enqueue)
            end_to_end[name].append(delta)
            by_kind[kind][name].append(delta)

    print_metric_table("Approximate input packet end-to-end timings", end_to_end)
    for kind in sorted(by_kind):
        print_metric_table(f"Input kind: {kind}", by_kind[kind])

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
