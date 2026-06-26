#!/usr/bin/env python3
"""Evaluate compact shadow exit replay records for target/stop grids.

The input is `shadow_exit_replay_v1.jsonl`. This script is offline-only and
must not be used by runtime decision code.
"""

from __future__ import annotations

import argparse
import csv
import json
import statistics
import sys
from pathlib import Path
from typing import Any, Iterable


EXACT_LEVELS = "exact_levels"
PATH_APPROX = "path_approx"
TARGET = "Target"
STOP_LOSS = "StopLoss"
TIME_STOP = "TimeStop"


def parse_bps_list(raw: str) -> list[int]:
    values: list[int] = []
    for item in raw.split(","):
        item = item.strip()
        if not item:
            continue
        values.append(int(item))
    if not values:
        raise argparse.ArgumentTypeError("empty bps list")
    return values


def iter_jsonl(path: Path) -> Iterable[dict[str, Any]]:
    with path.open(encoding="utf-8") as fh:
        for line_no, line in enumerate(fh, start=1):
            if not line.strip():
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError as exc:
                raise SystemExit(f"{path}:{line_no}: invalid JSON: {exc}") from exc
            if isinstance(row, dict):
                yield row


def finite_int(value: Any) -> int | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value
    if isinstance(value, float) and value.is_integer():
        return int(value)
    return None


def first_hit_exact(row: dict[str, Any], level_bps: int) -> int | None:
    first_hit_ms = row.get("first_hit_ms")
    if not isinstance(first_hit_ms, dict):
        return None
    return finite_int(first_hit_ms.get(str(level_bps)))


def path_first_hit(row: dict[str, Any], level_bps: int) -> int | None:
    path = row.get("path_bps")
    if not isinstance(path, list):
        return None
    for point in path:
        if not isinstance(point, (list, tuple)) or len(point) != 2:
            continue
        age_ms = finite_int(point[0])
        pnl_bps = finite_int(point[1])
        if age_ms is None or pnl_bps is None:
            continue
        if level_bps > 0 and pnl_bps >= level_bps:
            return age_ms
        if level_bps < 0 and pnl_bps <= level_bps:
            return age_ms
    return None


def classify_exit(
    row: dict[str, Any],
    target_bps: int,
    stop_bps: int,
    *,
    result_quality: str,
) -> tuple[str, int] | None:
    last_pnl_bps = finite_int(row.get("last_pnl_bps"))
    if last_pnl_bps is None:
        return None

    if result_quality == EXACT_LEVELS:
        target_ts = first_hit_exact(row, target_bps)
        stop_ts = first_hit_exact(row, stop_bps)
    else:
        target_ts = path_first_hit(row, target_bps)
        stop_ts = path_first_hit(row, stop_bps)

    if target_ts is not None and stop_ts is not None:
        if target_ts < stop_ts:
            return TARGET, target_bps
        return STOP_LOSS, stop_bps
    if target_ts is not None:
        return TARGET, target_bps
    if stop_ts is not None:
        return STOP_LOSS, stop_bps
    return TIME_STOP, last_pnl_bps


def median(values: list[int]) -> float:
    if not values:
        return 0.0
    return float(statistics.median(values))


def mean(values: list[int]) -> float:
    if not values:
        return 0.0
    return float(sum(values) / len(values))


def evaluate(
    rows: list[dict[str, Any]],
    targets_bps: list[int],
    stops_bps: list[int],
) -> list[dict[str, Any]]:
    output: list[dict[str, Any]] = []
    for target_bps in targets_bps:
        for stop_bps in stops_bps:
            counts = {TARGET: 0, STOP_LOSS: 0, TIME_STOP: 0}
            pnl_values: list[int] = []
            total = 0
            exact_rows = 0
            approx_rows = 0

            for row in rows:
                levels = row.get("levels_bps")
                levels_set = set(levels) if isinstance(levels, list) else set()
                result_quality = (
                    EXACT_LEVELS
                    if target_bps in levels_set and stop_bps in levels_set
                    else PATH_APPROX
                )
                classified = classify_exit(
                    row,
                    target_bps,
                    stop_bps,
                    result_quality=result_quality,
                )
                if classified is None:
                    continue
                label, pnl_bps = classified
                counts[label] += 1
                pnl_values.append(pnl_bps)
                total += 1
                if result_quality == EXACT_LEVELS:
                    exact_rows += 1
                else:
                    approx_rows += 1

            quality = EXACT_LEVELS if approx_rows == 0 else PATH_APPROX
            output.append(
                {
                    "target_bps": target_bps,
                    "stop_bps": stop_bps,
                    "result_quality": quality,
                    "total": total,
                    "target_count": counts[TARGET],
                    "stop_count": counts[STOP_LOSS],
                    "timestop_count": counts[TIME_STOP],
                    "target_rate": counts[TARGET] / total if total else 0.0,
                    "stop_rate": counts[STOP_LOSS] / total if total else 0.0,
                    "timestop_rate": counts[TIME_STOP] / total if total else 0.0,
                    "avg_pnl_bps": mean(pnl_values),
                    "median_pnl_bps": median(pnl_values),
                    "sum_pnl_bps": sum(pnl_values),
                    "exact_rows": exact_rows,
                    "path_approx_rows": approx_rows,
                }
            )
    return output


def write_csv(rows: list[dict[str, Any]], output: Path | None) -> None:
    fieldnames = [
        "target_bps",
        "stop_bps",
        "result_quality",
        "total",
        "target_count",
        "stop_count",
        "timestop_count",
        "target_rate",
        "stop_rate",
        "timestop_rate",
        "avg_pnl_bps",
        "median_pnl_bps",
        "sum_pnl_bps",
        "exact_rows",
        "path_approx_rows",
    ]
    if output is None:
        fh = sys.stdout
        close = False
    else:
        output.parent.mkdir(parents=True, exist_ok=True)
        fh = output.open("w", encoding="utf-8", newline="")
        close = True
    try:
        writer = csv.DictWriter(fh, fieldnames=fieldnames)
        writer.writeheader()
        for row in rows:
            writer.writerow(row)
    finally:
        if close:
            fh.close()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--targets-bps", required=True, type=parse_bps_list)
    parser.add_argument("--stops-bps", required=True, type=parse_bps_list)
    parser.add_argument("--output", type=Path)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    rows = list(iter_jsonl(args.input))
    result_rows = evaluate(rows, args.targets_bps, args.stops_bps)
    write_csv(result_rows, args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
