#!/usr/bin/env python3
"""Summarize TimeStop V2 observe-only lifecycle evidence.

The script is intentionally read-only. It inspects shadow_lifecycle JSONL files
and reports how many counterfactual TimeStop V2 windows were emitted, how many
positions became V2 candidates, and how those candidates later closed.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8") as handle:
        for line_no, line in enumerate(handle, start=1):
            line = line.strip()
            if not line:
                continue
            try:
                value = json.loads(line)
            except json.JSONDecodeError as exc:
                raise SystemExit(f"{path}:{line_no}: invalid JSON: {exc}") from exc
            if isinstance(value, dict):
                rows.append(value)
    return rows


def summarize(path: Path) -> dict[str, Any]:
    rows = read_jsonl(path)
    window_rows = [row for row in rows if row.get("record_type") == "time_stop_v2_window"]
    terminal_rows = [row for row in rows if row.get("record_type") == "position_closed"]
    candidate_windows = [row for row in window_rows if row.get("time_stop_v2_candidate") is True]
    candidate_positions = {
        row.get("position_id")
        for row in candidate_windows
        if isinstance(row.get("position_id"), str)
    }
    terminal_candidate_rows = [
        row for row in terminal_rows if row.get("position_id") in candidate_positions
    ]

    return {
        "path": str(path),
        "exists": path.exists(),
        "rows": len(rows),
        "time_stop_v2_window_rows": len(window_rows),
        "time_stop_v2_candidate_window_rows": len(candidate_windows),
        "time_stop_v2_candidate_positions": len(candidate_positions),
        "window_status_counts": dict(
            Counter(row.get("time_stop_v2_status", "missing") for row in window_rows)
        ),
        "window_subreason_counts": dict(
            Counter(row.get("time_stop_v2_subreason", "missing") for row in window_rows)
        ),
        "candidate_subreason_counts": dict(
            Counter(row.get("time_stop_v2_candidate_subreason", "missing") for row in candidate_windows)
        ),
        "terminal_rows": len(terminal_rows),
        "terminal_close_reason_counts": dict(
            Counter(row.get("close_reason", "missing") for row in terminal_rows)
        ),
        "terminal_candidate_rows": len(terminal_candidate_rows),
        "terminal_candidate_close_reason_counts": dict(
            Counter(row.get("close_reason", "missing") for row in terminal_candidate_rows)
        ),
    }


def merge_summaries(summaries: list[dict[str, Any]]) -> dict[str, Any]:
    merged: dict[str, Any] = {
        "rows": 0,
        "time_stop_v2_window_rows": 0,
        "time_stop_v2_candidate_window_rows": 0,
        "time_stop_v2_candidate_positions": 0,
        "terminal_rows": 0,
        "terminal_candidate_rows": 0,
        "window_status_counts": Counter(),
        "window_subreason_counts": Counter(),
        "candidate_subreason_counts": Counter(),
        "terminal_close_reason_counts": Counter(),
        "terminal_candidate_close_reason_counts": Counter(),
    }
    for summary in summaries:
        for key in (
            "rows",
            "time_stop_v2_window_rows",
            "time_stop_v2_candidate_window_rows",
            "time_stop_v2_candidate_positions",
            "terminal_rows",
            "terminal_candidate_rows",
        ):
            merged[key] += int(summary.get(key, 0))
        for key in (
            "window_status_counts",
            "window_subreason_counts",
            "candidate_subreason_counts",
            "terminal_close_reason_counts",
            "terminal_candidate_close_reason_counts",
        ):
            merged[key].update(summary.get(key, {}))

    for key, value in list(merged.items()):
        if isinstance(value, Counter):
            merged[key] = dict(value)
    return merged


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--shadow-lifecycle", type=Path, required=True)
    parser.add_argument("--probe-lifecycle", type=Path)
    parser.add_argument("--json", action="store_true", help="emit machine-readable JSON")
    return parser


def main() -> int:
    args = build_parser().parse_args()
    summaries = {"shadow": summarize(args.shadow_lifecycle)}
    if args.probe_lifecycle is not None:
        summaries["probe"] = summarize(args.probe_lifecycle)
    combined = merge_summaries(list(summaries.values()))
    report = {"inputs": summaries, "combined": combined}

    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print(f"combined.rows = {combined['rows']}")
        print(f"combined.time_stop_v2_window_rows = {combined['time_stop_v2_window_rows']}")
        print(
            "combined.time_stop_v2_candidate_positions = "
            f"{combined['time_stop_v2_candidate_positions']}"
        )
        print(f"combined.window_status_counts = {combined['window_status_counts']}")
        print(f"combined.window_subreason_counts = {combined['window_subreason_counts']}")
        print(
            "combined.terminal_candidate_close_reason_counts = "
            f"{combined['terminal_candidate_close_reason_counts']}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
