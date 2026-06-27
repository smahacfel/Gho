#!/usr/bin/env python3
"""Evaluate compact shadow exit replay records for target/stop grids.

The input is `shadow_exit_replay_v1.jsonl`. This script is offline-only and
must not be used by runtime decision code.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import statistics
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable


EXACT_LEVELS = "exact_levels"
PATH_APPROX = "path_approx"
PATH_PREV_TIMEOUT = "path_prev_timeout"
TARGET = "Target"
STOP_LOSS = "StopLoss"
TIME_STOP = "TimeStop"

MATRIX_TARGET = "TARGET"
MATRIX_STOP = "STOP"
MATRIX_TIMEOUT = "TIMEOUT"
MATRIX_UNAVAILABLE = "UNAVAILABLE"

DEFAULT_TARGETS_BPS = [100, 200, 300, 400, 500, 700, 1000, 1500, 2000, 3000, 5000, 6000, 7500, 10000]
DEFAULT_STOPS_BPS = [-100, -200, -300, -500, -700, -1000, -1500, -2000, -3000, -5000, -6000]
DEFAULT_MAX_HOLD_MS = [10000, 15000, 20000, 30000, 40000, 60000, 90000, 120000]
DEFAULT_ROUNDTRIP_COST_BPS = [0, 50, 100, 150, 200]
POSITION_SIZE_SOL = 0.25

REQUIRED_KEYS = {
    "schema",
    "run_id",
    "session_id",
    "pool_id",
    "base_mint",
    "entry_ts_ms",
    "horizon_ms",
    "levels_bps",
    "first_hit_ms",
    "path_bps",
    "quality",
    "truncated",
}


@dataclass(frozen=True)
class ReplayRecord:
    raw: dict[str, Any]
    order_index: int
    run_id: str
    session_id: str
    pool_id: str
    base_mint: str
    entry_ts_ms: int
    horizon_ms: int
    quality: str
    truncated: bool
    levels_bps: frozenset[int]
    first_hit_ms: dict[int, int]
    path_bps: tuple[tuple[int, int], ...]


@dataclass(frozen=True)
class CellOutcome:
    label: str
    pnl_bps: int | None
    source: str


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
    """Legacy target/stop evaluator retained for compatibility.

    This mode intentionally preserves the original semantics: no max-hold
    dimension and TIME_STOP uses `last_pnl_bps`.
    """
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


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def utc_now_iso() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def parse_path_bps(raw: Any) -> tuple[tuple[int, int], ...] | None:
    if not isinstance(raw, list):
        return None
    points: list[tuple[int, int]] = []
    previous_age: int | None = None
    for point in raw:
        if not isinstance(point, (list, tuple)) or len(point) != 2:
            return None
        age_ms = finite_int(point[0])
        pnl_bps = finite_int(point[1])
        if age_ms is None or pnl_bps is None or age_ms < 0:
            return None
        if previous_age is not None and age_ms < previous_age:
            return None
        previous_age = age_ms
        points.append((age_ms, pnl_bps))
    if not points:
        return None
    return tuple(points)


def parse_levels(raw: Any) -> frozenset[int] | None:
    if not isinstance(raw, list):
        return None
    values: set[int] = set()
    for item in raw:
        value = finite_int(item)
        if value is None or value == 0:
            return None
        values.add(value)
    return frozenset(values) if values else None


def parse_first_hit(raw: Any) -> dict[int, int] | None:
    if not isinstance(raw, dict):
        return None
    parsed: dict[int, int] = {}
    for key, value in raw.items():
        try:
            level = int(key)
        except (TypeError, ValueError):
            return None
        age_ms = finite_int(value)
        if age_ms is None or age_ms < 0:
            return None
        parsed[level] = age_ms
    return parsed


def load_records(path: Path) -> tuple[list[ReplayRecord], dict[str, Any]]:
    records: list[ReplayRecord] = []
    controls: dict[str, Any] = {
        "total_records": 0,
        "malformed_json_records": 0,
        "non_object_records": 0,
        "missing_required_records": 0,
        "damaged_records": 0,
        "qualified_records": 0,
        "valid_path_bps_records": 0,
        "records_with_parseable_levels_bps": 0,
        "common_levels_bps_all_parseable_records": [],
        "quality_counts": Counter(),
        "truncated_counts": Counter(),
        "horizon_ms_counts": Counter(),
        "damage_reasons": Counter(),
        "duplicate_keys": [],
    }
    key_counts: Counter[tuple[str, str, str, str, int]] = Counter()
    common_parseable_levels: set[int] | None = None

    with path.open(encoding="utf-8") as fh:
        for line_no, line in enumerate(fh, start=1):
            if not line.strip():
                continue
            controls["total_records"] += 1
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                controls["malformed_json_records"] += 1
                controls["damage_reasons"]["invalid_json"] += 1
                continue
            if not isinstance(row, dict):
                controls["non_object_records"] += 1
                controls["damage_reasons"]["non_object"] += 1
                continue

            missing = sorted(REQUIRED_KEYS.difference(row))
            if missing:
                controls["missing_required_records"] += 1
                controls["damage_reasons"][f"missing:{','.join(missing)}"] += 1

            quality = str(row.get("quality") or "missing")
            truncated = bool(row.get("truncated")) if isinstance(row.get("truncated"), bool) else None
            controls["quality_counts"][quality] += 1
            controls["truncated_counts"][str(truncated)] += 1

            horizon_ms = finite_int(row.get("horizon_ms"))
            if horizon_ms is not None:
                controls["horizon_ms_counts"][horizon_ms] += 1

            levels = parse_levels(row.get("levels_bps"))
            first_hit = parse_first_hit(row.get("first_hit_ms"))
            path_bps = parse_path_bps(row.get("path_bps"))
            if levels is not None:
                controls["records_with_parseable_levels_bps"] += 1
                if common_parseable_levels is None:
                    common_parseable_levels = set(levels)
                else:
                    common_parseable_levels.intersection_update(levels)
            if path_bps is not None:
                controls["valid_path_bps_records"] += 1

            entry_ts_ms = finite_int(row.get("entry_ts_ms"))
            damaged = False
            if row.get("schema") != "shadow_exit_replay_v1":
                controls["damage_reasons"]["schema_not_shadow_exit_replay_v1"] += 1
                damaged = True
            if quality != "clean":
                controls["damage_reasons"][f"quality:{quality}"] += 1
                damaged = True
            if truncated is not False:
                controls["damage_reasons"][f"truncated:{truncated}"] += 1
                damaged = True
            if levels is None:
                controls["damage_reasons"]["invalid_levels_bps"] += 1
                damaged = True
            if first_hit is None:
                controls["damage_reasons"]["invalid_first_hit_ms"] += 1
                damaged = True
            if path_bps is None:
                controls["damage_reasons"]["invalid_path_bps"] += 1
                damaged = True
            if entry_ts_ms is None:
                controls["damage_reasons"]["invalid_entry_ts_ms"] += 1
                damaged = True
            if horizon_ms is None:
                controls["damage_reasons"]["invalid_horizon_ms"] += 1
                damaged = True

            run_id = str(row.get("run_id") or "")
            session_id = str(row.get("session_id") or "")
            pool_id = str(row.get("pool_id") or "")
            base_mint = str(row.get("base_mint") or "")
            key_entry_ts = entry_ts_ms if entry_ts_ms is not None else -1
            key = (run_id, session_id, pool_id, base_mint, key_entry_ts)
            key_counts[key] += 1

            if damaged:
                controls["damaged_records"] += 1
                continue

            records.append(
                ReplayRecord(
                    raw=row,
                    order_index=line_no,
                    run_id=run_id,
                    session_id=session_id,
                    pool_id=pool_id,
                    base_mint=base_mint,
                    entry_ts_ms=entry_ts_ms or 0,
                    horizon_ms=horizon_ms or 0,
                    quality=quality,
                    truncated=False,
                    levels_bps=levels or frozenset(),
                    first_hit_ms=first_hit or {},
                    path_bps=path_bps or tuple(),
                )
            )

    controls["duplicate_keys"] = [
        {
            "run_id": key[0],
            "session_id": key[1],
            "pool_id": key[2],
            "base_mint": key[3],
            "entry_ts_ms": key[4],
            "count": count,
        }
        for key, count in key_counts.items()
        if count > 1
    ]
    controls["qualified_records"] = len(records)
    controls["common_levels_bps_all_parseable_records"] = (
        sorted(common_parseable_levels) if common_parseable_levels is not None else []
    )
    controls["quality_counts"] = dict(sorted(controls["quality_counts"].items()))
    controls["truncated_counts"] = dict(sorted(controls["truncated_counts"].items()))
    controls["horizon_ms_counts"] = dict(sorted(controls["horizon_ms_counts"].items()))
    controls["damage_reasons"] = dict(sorted(controls["damage_reasons"].items()))
    return records, controls


def common_levels(records: list[ReplayRecord]) -> list[int]:
    if not records:
        return []
    levels = set(records[0].levels_bps)
    for record in records[1:]:
        levels.intersection_update(record.levels_bps)
    return sorted(levels)


def path_prev_pnl(record: ReplayRecord, max_hold_ms: int) -> int | None:
    selected: int | None = None
    for age_ms, pnl_bps in record.path_bps:
        if age_ms <= max_hold_ms:
            selected = pnl_bps
        else:
            break
    return selected


def simulate_record(
    record: ReplayRecord,
    target_bps: int,
    stop_bps: int,
    max_hold_ms: int,
) -> CellOutcome:
    if target_bps not in record.levels_bps or stop_bps not in record.levels_bps:
        return CellOutcome(MATRIX_UNAVAILABLE, None, "missing_exact_level")

    target_hit = record.first_hit_ms.get(target_bps)
    stop_hit = record.first_hit_ms.get(stop_bps)
    if target_hit is not None and target_hit > max_hold_ms:
        target_hit = None
    if stop_hit is not None and stop_hit > max_hold_ms:
        stop_hit = None

    if target_hit is not None and stop_hit is not None:
        if target_hit < stop_hit:
            return CellOutcome(MATRIX_TARGET, target_bps, EXACT_LEVELS)
        return CellOutcome(MATRIX_STOP, stop_bps, EXACT_LEVELS)
    if target_hit is not None:
        return CellOutcome(MATRIX_TARGET, target_bps, EXACT_LEVELS)
    if stop_hit is not None:
        return CellOutcome(MATRIX_STOP, stop_bps, EXACT_LEVELS)

    timeout_pnl = path_prev_pnl(record, max_hold_ms)
    if timeout_pnl is None:
        return CellOutcome(MATRIX_UNAVAILABLE, None, "no_path_point_before_max_hold")
    return CellOutcome(MATRIX_TIMEOUT, timeout_pnl, PATH_PREV_TIMEOUT)


def percentile(values: list[int], pct: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    if len(ordered) == 1:
        return float(ordered[0])
    rank = (len(ordered) - 1) * pct
    low = math.floor(rank)
    high = math.ceil(rank)
    if low == high:
        return float(ordered[low])
    weight = rank - low
    return float(ordered[low] * (1.0 - weight) + ordered[high] * weight)


def profit_factor(values: list[int]) -> float | None:
    positive = sum(value for value in values if value > 0)
    negative = abs(sum(value for value in values if value < 0))
    if negative == 0:
        return None
    return positive / negative


def metrics_from_values(
    *,
    target_bps: int,
    stop_bps: int,
    max_hold_ms: int,
    total_candidate_count: int,
    outcomes: list[CellOutcome],
) -> dict[str, Any]:
    pnl_values = [outcome.pnl_bps for outcome in outcomes if outcome.pnl_bps is not None]
    eligible_count = len(pnl_values)
    counts = Counter(outcome.label for outcome in outcomes if outcome.label != MATRIX_UNAVAILABLE)
    timeout_values = [
        outcome.pnl_bps
        for outcome in outcomes
        if outcome.label == MATRIX_TIMEOUT and outcome.pnl_bps is not None
    ]
    positive_count = sum(1 for value in pnl_values if value > 0)
    negative_count = sum(1 for value in pnl_values if value < 0)
    positive_timeout_count = sum(1 for value in timeout_values if value > 0)
    negative_timeout_count = sum(1 for value in timeout_values if value < 0)
    pf = profit_factor(pnl_values)
    return {
        "target_bps": target_bps,
        "stop_bps": stop_bps,
        "max_hold_ms": max_hold_ms,
        "eligible_count": eligible_count,
        "excluded_count": total_candidate_count - eligible_count,
        "target_count": counts[MATRIX_TARGET],
        "stop_count": counts[MATRIX_STOP],
        "timeout_count": counts[MATRIX_TIMEOUT],
        "target_rate": counts[MATRIX_TARGET] / eligible_count if eligible_count else 0.0,
        "stop_rate": counts[MATRIX_STOP] / eligible_count if eligible_count else 0.0,
        "timeout_rate": counts[MATRIX_TIMEOUT] / eligible_count if eligible_count else 0.0,
        "positive_timeout_count": positive_timeout_count,
        "negative_timeout_count": negative_timeout_count,
        "positive_timeout_rate": positive_timeout_count / counts[MATRIX_TIMEOUT] if counts[MATRIX_TIMEOUT] else 0.0,
        "negative_timeout_rate": negative_timeout_count / counts[MATRIX_TIMEOUT] if counts[MATRIX_TIMEOUT] else 0.0,
        "avg_pnl_bps": mean(pnl_values),
        "median_pnl_bps": median(pnl_values),
        "sum_pnl_bps": sum(pnl_values),
        "p10_pnl_bps": percentile(pnl_values, 0.10),
        "p25_pnl_bps": percentile(pnl_values, 0.25),
        "p75_pnl_bps": percentile(pnl_values, 0.75),
        "p90_pnl_bps": percentile(pnl_values, 0.90),
        "min_pnl_bps": min(pnl_values) if pnl_values else 0,
        "max_pnl_bps": max(pnl_values) if pnl_values else 0,
        "positive_result_count": positive_count,
        "negative_result_count": negative_count,
        "win_rate": positive_count / eligible_count if eligible_count else 0.0,
        "loss_rate": negative_count / eligible_count if eligible_count else 0.0,
        "profit_factor": pf,
        "expected_gross_pnl_sol_at_0_25_sol": (mean(pnl_values) / 10_000.0) * POSITION_SIZE_SOL,
        "median_gross_pnl_sol_at_0_25_sol": (median(pnl_values) / 10_000.0) * POSITION_SIZE_SOL,
    }


def evaluate_matrix(
    records: list[ReplayRecord],
    targets_bps: list[int],
    stops_bps: list[int],
    max_hold_ms_values: list[int],
) -> tuple[list[dict[str, Any]], dict[tuple[int, int, int], list[CellOutcome]]]:
    matrix: list[dict[str, Any]] = []
    outcomes_by_cell: dict[tuple[int, int, int], list[CellOutcome]] = {}
    total = len(records)
    for max_hold_ms in max_hold_ms_values:
        for target_bps in targets_bps:
            for stop_bps in stops_bps:
                outcomes = [
                    simulate_record(record, target_bps, stop_bps, max_hold_ms)
                    for record in records
                ]
                key = (target_bps, stop_bps, max_hold_ms)
                outcomes_by_cell[key] = outcomes
                matrix.append(
                    metrics_from_values(
                        target_bps=target_bps,
                        stop_bps=stop_bps,
                        max_hold_ms=max_hold_ms,
                        total_candidate_count=total,
                        outcomes=outcomes,
                    )
                )
    return matrix, outcomes_by_cell


def row_sort_key(row: dict[str, Any]) -> tuple[float, float, float, float, int]:
    pf = row.get("profit_factor")
    pf_value = float(pf) if isinstance(pf, (int, float)) else -1.0
    return (
        float(row["avg_pnl_bps"]),
        float(row["median_pnl_bps"]),
        pf_value,
        -float(row["stop_rate"]),
        int(row["eligible_count"]),
    )


def top_rows(matrix: list[dict[str, Any]], limit: int = 20) -> list[dict[str, Any]]:
    return sorted(matrix, key=row_sort_key, reverse=True)[:limit]


def split_terciles(records: list[ReplayRecord]) -> dict[str, list[ReplayRecord]]:
    ordered = sorted(records, key=lambda record: (record.entry_ts_ms, record.order_index))
    n = len(ordered)
    first = n // 3
    second = (2 * n) // 3
    return {
        "early": ordered[:first],
        "middle": ordered[first:second],
        "late": ordered[second:],
    }


def evaluate_stability(
    records: list[ReplayRecord],
    top20: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    terciles = split_terciles(records)
    rows: list[dict[str, Any]] = []
    for rank, cell in enumerate(top20, start=1):
        target = int(cell["target_bps"])
        stop = int(cell["stop_bps"])
        hold = int(cell["max_hold_ms"])
        positive_sum_terciles = 0
        positive_avg_terciles = 0
        for name, subset in terciles.items():
            outcomes = [simulate_record(record, target, stop, hold) for record in subset]
            metrics = metrics_from_values(
                target_bps=target,
                stop_bps=stop,
                max_hold_ms=hold,
                total_candidate_count=len(subset),
                outcomes=outcomes,
            )
            if metrics["sum_pnl_bps"] > 0:
                positive_sum_terciles += 1
            if metrics["avg_pnl_bps"] > 0:
                positive_avg_terciles += 1
            rows.append(
                {
                    "rank": rank,
                    "tercile": name,
                    "stable": "",
                    **metrics,
                }
            )

        stable = bool(cell["sum_pnl_bps"] > 0 and positive_sum_terciles >= 2 and positive_avg_terciles >= 2)
        for row in rows[-3:]:
            row["stable"] = stable
            row["positive_sum_terciles"] = positive_sum_terciles
            row["positive_avg_terciles"] = positive_avg_terciles
    return rows


def pareto_frontier(matrix: list[dict[str, Any]]) -> list[dict[str, Any]]:
    frontier: list[dict[str, Any]] = []
    for row in matrix:
        dominated = False
        for other in matrix:
            if row is other:
                continue
            better_or_equal = (
                other["avg_pnl_bps"] >= row["avg_pnl_bps"]
                and other["median_pnl_bps"] >= row["median_pnl_bps"]
                and other["stop_rate"] <= row["stop_rate"]
                and other["timeout_rate"] <= row["timeout_rate"]
            )
            strictly_better = (
                other["avg_pnl_bps"] > row["avg_pnl_bps"]
                or other["median_pnl_bps"] > row["median_pnl_bps"]
                or other["stop_rate"] < row["stop_rate"]
                or other["timeout_rate"] < row["timeout_rate"]
            )
            if better_or_equal and strictly_better:
                dominated = True
                break
        if not dominated:
            frontier.append(row)
    return sorted(frontier, key=row_sort_key, reverse=True)


MATRIX_FIELDNAMES = [
    "target_bps",
    "stop_bps",
    "max_hold_ms",
    "eligible_count",
    "excluded_count",
    "target_count",
    "stop_count",
    "timeout_count",
    "target_rate",
    "stop_rate",
    "timeout_rate",
    "positive_timeout_count",
    "negative_timeout_count",
    "positive_timeout_rate",
    "negative_timeout_rate",
    "avg_pnl_bps",
    "median_pnl_bps",
    "sum_pnl_bps",
    "p10_pnl_bps",
    "p25_pnl_bps",
    "p75_pnl_bps",
    "p90_pnl_bps",
    "min_pnl_bps",
    "max_pnl_bps",
    "positive_result_count",
    "negative_result_count",
    "win_rate",
    "loss_rate",
    "profit_factor",
    "expected_gross_pnl_sol_at_0_25_sol",
    "median_gross_pnl_sol_at_0_25_sol",
]


def csv_value(value: Any) -> Any:
    if value is None:
        return ""
    return value


def write_dict_csv(path: Path, rows: list[dict[str, Any]], fieldnames: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames, extrasaction="ignore")
        writer.writeheader()
        for row in rows:
            writer.writerow({name: csv_value(row.get(name)) for name in fieldnames})


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


def cost_adjusted_rows(
    top20: list[dict[str, Any]],
    outcomes_by_cell: dict[tuple[int, int, int], list[CellOutcome]],
    costs_bps: list[int],
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for rank, cell in enumerate(top20, start=1):
        key = (int(cell["target_bps"]), int(cell["stop_bps"]), int(cell["max_hold_ms"]))
        base_values = [out.pnl_bps for out in outcomes_by_cell[key] if out.pnl_bps is not None]
        for cost_bps in costs_bps:
            adjusted = [value - cost_bps for value in base_values]
            rows.append(
                {
                    "rank": rank,
                    "target_bps": key[0],
                    "stop_bps": key[1],
                    "max_hold_ms": key[2],
                    "roundtrip_cost_bps": cost_bps,
                    "eligible_count": len(adjusted),
                    "avg_pnl_bps_after_cost": mean(adjusted),
                    "median_pnl_bps_after_cost": median(adjusted),
                    "sum_pnl_bps_after_cost": sum(adjusted),
                    "profit_factor_after_cost": profit_factor(adjusted),
                    "expected_pnl_sol_at_0_25_sol_after_cost": (mean(adjusted) / 10_000.0) * POSITION_SIZE_SOL,
                    "median_pnl_sol_at_0_25_sol_after_cost": (median(adjusted) / 10_000.0) * POSITION_SIZE_SOL,
                }
            )
    return rows


def write_heatmaps(
    output_dir: Path,
    matrix: list[dict[str, Any]],
    targets: list[int],
    stops: list[int],
    max_hold_ms_values: list[int],
) -> list[str]:
    paths: list[str] = []
    by_hold: dict[int, dict[tuple[int, int], dict[str, Any]]] = defaultdict(dict)
    for row in matrix:
        by_hold[int(row["max_hold_ms"])][(int(row["target_bps"]), int(row["stop_bps"]))] = row
    for hold in max_hold_ms_values:
        for metric in ("avg_pnl_bps", "median_pnl_bps", "profit_factor", "stop_rate", "timeout_rate"):
            path = output_dir / f"target_stop_hold_heatmap_{metric}_max_hold_{hold}.csv"
            with path.open("w", encoding="utf-8", newline="") as fh:
                writer = csv.writer(fh)
                writer.writerow(["target_bps\\stop_bps", *stops])
                for target in targets:
                    out_row: list[Any] = [target]
                    for stop in stops:
                        value = by_hold[hold].get((target, stop), {}).get(metric)
                        out_row.append(csv_value(value))
                    writer.writerow(out_row)
            paths.append(str(path))
    return paths


def markdown_table(rows: list[dict[str, Any]], columns: list[str], limit: int | None = None) -> str:
    selected = rows[:limit] if limit is not None else rows
    if not selected:
        return "_brak danych_"
    lines = [
        "| " + " | ".join(columns) + " |",
        "| " + " | ".join("---" for _ in columns) + " |",
    ]
    for row in selected:
        values = []
        for column in columns:
            value = row.get(column)
            if isinstance(value, float):
                values.append(f"{value:.6g}")
            elif value is None:
                values.append("")
            else:
                values.append(str(value))
        lines.append("| " + " | ".join(values) + " |")
    return "\n".join(lines)


def write_report(
    path: Path,
    *,
    input_path: Path,
    metadata: dict[str, Any],
    controls: dict[str, Any],
    targets: list[int],
    stops: list[int],
    max_holds: list[int],
    matrix: list[dict[str, Any]],
    top20: list[dict[str, Any]],
    stability: list[dict[str, Any]],
    frontier: list[dict[str, Any]],
    cost_rows: list[dict[str, Any]],
    heatmap_paths: list[str],
    legacy_comparison: dict[str, Any] | None,
) -> None:
    by_avg = sorted(matrix, key=lambda row: row["avg_pnl_bps"], reverse=True)
    by_median = sorted(matrix, key=lambda row: row["median_pnl_bps"], reverse=True)
    by_pf = sorted(
        matrix,
        key=lambda row: (
            row["profit_factor"] if isinstance(row["profit_factor"], (int, float)) else -1,
            row["avg_pnl_bps"],
        ),
        reverse=True,
    )
    report = f"""# Target x StopLoss x max_hold matrix report

## 1. Snapshot i kontrola danych

- input_snapshot: `{input_path}`
- snapshot_timestamp_utc: `{metadata['snapshot_timestamp_utc']}`
- snapshot_size_bytes: `{metadata['snapshot_size_bytes']}`
- snapshot_sha256: `{metadata['snapshot_sha256']}`
- total_records: `{controls['total_records']}`
- qualified_records_for_main_matrix: `{controls['qualified_records']}`
- valid_path_bps_records: `{controls['valid_path_bps_records']}`
- malformed_json_records: `{controls['malformed_json_records']}`
- missing_required_records: `{controls['missing_required_records']}`
- damaged_or_excluded_records: `{controls['damaged_records']}`
- duplicate_key_count: `{len(controls['duplicate_keys'])}`

quality distribution:
```json
{json.dumps(controls['quality_counts'], indent=2, sort_keys=True)}
```

truncated distribution:
```json
{json.dumps(controls['truncated_counts'], indent=2, sort_keys=True)}
```

horizon_ms distribution:
```json
{json.dumps(controls['horizon_ms_counts'], indent=2, sort_keys=True)}
```

missing/damaged reasons:
```json
{json.dumps(controls['damage_reasons'], indent=2, sort_keys=True)}
```

Duplicate key check uses `(run_id, session_id, pool_id, base_mint, entry_ts_ms)`.

## 2. Semantyka replay PnL

`first_hit_ms` is used only for exact levels present in every qualified record. Target/StopLoss hits are exact barrier hits. Ties are conservative: STOP wins when target and stop are hit in the same millisecond.

TIMEOUT uses `path_prev_timeout`: the last `path_bps` point with `age_ms <= max_hold_ms`. The evaluator never uses a future path point and never converts missing timeout evidence into `0`.

Gross PnL is computed directly from replay `pnl_bps`. The replay record contains price-path deltas (`entry_price`, observed price-derived `pnl_bps`) and does not carry explicit Pump.fun fee, priority fee, Jito tip, or realized execution-cost fields. Therefore this report calls the primary result gross PnL, not net PnL. Cost sensitivity is reported separately.

## 3. Wspolne exact levels

- targets used: `{targets}`
- stops used: `{stops}`
- max_hold_ms grid: `{max_holds}`
- combination_count: `{len(matrix)}`

## 4. Pelna macierz

Full CSV/JSON artifacts contain the full matrix:
- `target_stop_hold_matrix_exact.csv`
- `target_stop_hold_matrix_exact.json`

Heatmap CSV files are written separately per `max_hold_ms` and metric. Count: `{len(heatmap_paths)}`.

## 5. Najlepsze kombinacje wedlug avg, median i profit factor

Top by avg:
{markdown_table(by_avg, ['target_bps', 'stop_bps', 'max_hold_ms', 'eligible_count', 'avg_pnl_bps', 'median_pnl_bps', 'profit_factor', 'stop_rate', 'timeout_rate'], 10)}

Top by median:
{markdown_table(by_median, ['target_bps', 'stop_bps', 'max_hold_ms', 'eligible_count', 'avg_pnl_bps', 'median_pnl_bps', 'profit_factor', 'stop_rate', 'timeout_rate'], 10)}

Top by profit factor:
{markdown_table(by_pf, ['target_bps', 'stop_bps', 'max_hold_ms', 'eligible_count', 'avg_pnl_bps', 'median_pnl_bps', 'profit_factor', 'stop_rate', 'timeout_rate'], 10)}

## 6. Pareto frontier

Pareto objective: maximize `avg_pnl_bps` and `median_pnl_bps`, minimize `stop_rate` and `timeout_rate`.

{markdown_table(frontier, ['target_bps', 'stop_bps', 'max_hold_ms', 'eligible_count', 'avg_pnl_bps', 'median_pnl_bps', 'stop_rate', 'timeout_rate', 'profit_factor'], 30)}

## 7. Stabilnosc tercyli

Top 20 from the full sample was re-evaluated on chronological terciles. A cell is not marked stable when positive result comes from only one tercile.

{markdown_table(stability, ['rank', 'tercile', 'stable', 'target_bps', 'stop_bps', 'max_hold_ms', 'eligible_count', 'avg_pnl_bps', 'median_pnl_bps', 'sum_pnl_bps', 'positive_sum_terciles'], 60)}

## 8. Wyniki dla pozycji 0.25 SOL

Gross SOL columns use `pnl_bps / 10000 * 0.25`.

{markdown_table(top20, ['target_bps', 'stop_bps', 'max_hold_ms', 'avg_pnl_bps', 'expected_gross_pnl_sol_at_0_25_sol', 'median_gross_pnl_sol_at_0_25_sol'], 20)}

## 9. Wrazliwosc na koszty

Cost sensitivity subtracts a separate `roundtrip_cost_bps` from each gross position result. These are not mixed with gross columns.

{markdown_table(cost_rows, ['rank', 'target_bps', 'stop_bps', 'max_hold_ms', 'roundtrip_cost_bps', 'avg_pnl_bps_after_cost', 'median_pnl_bps_after_cost', 'profit_factor_after_cost'], 80)}

## 10. Ograniczenia i brakujace dane

- Results are offline-only and derived from a frozen snapshot.
- `quality != clean`, `truncated != false`, malformed, or structurally damaged records are excluded from the main matrix.
- The matrix uses only common exact `levels_bps`; no barrier interpolation from `path_bps` is used.
- TIMEOUT PnL depends on compressed `path_bps`, so it is marked `path_prev_timeout`.
- No selector score, Gatekeeper outcome, alpha, XGBoost, lifecycle close reason, or TimeStop V2 records are used.
- Gross replay PnL is not proven net of Pump.fun fees, priority fee, Jito tip, or live execution costs.
"""
    if legacy_comparison is not None:
        report += f"""

## 11. Porownanie z dotychczasowym evaluatorem

Cell `+6000/-6000/120000` was compared against the legacy evaluator on the same snapshot.

```json
{json.dumps(legacy_comparison, indent=2, sort_keys=True)}
```
"""
    path.write_text(report, encoding="utf-8")


def run_matrix(args: argparse.Namespace) -> int:
    records, controls = load_records(args.input)
    common = common_levels(records)
    common_set = set(common)
    targets = [value for value in (args.targets_bps or DEFAULT_TARGETS_BPS) if value in common_set and value > 0]
    stops = [value for value in (args.stops_bps or DEFAULT_STOPS_BPS) if value in common_set and value < 0]
    max_holds = args.max_hold_ms or DEFAULT_MAX_HOLD_MS
    costs = args.roundtrip_cost_bps if args.roundtrip_cost_bps is not None else DEFAULT_ROUNDTRIP_COST_BPS

    matrix, outcomes_by_cell = evaluate_matrix(records, targets, stops, max_holds)
    top20 = top_rows(matrix, 20)
    stability = evaluate_stability(records, top20)
    frontier = pareto_frontier(matrix)
    cost_rows = cost_adjusted_rows(top20, outcomes_by_cell, costs)

    output_dir = args.matrix_output_dir
    output_dir.mkdir(parents=True, exist_ok=True)
    matrix_csv = output_dir / "target_stop_hold_matrix_exact.csv"
    matrix_json = output_dir / "target_stop_hold_matrix_exact.json"
    top20_csv = output_dir / "target_stop_hold_top20.csv"
    stability_csv = output_dir / "target_stop_hold_stability.csv"
    cost_csv = output_dir / "target_stop_hold_cost_sensitivity.csv"
    report_md = output_dir / "TARGET_STOP_HOLD_MATRIX_REPORT.md"

    write_dict_csv(matrix_csv, matrix, MATRIX_FIELDNAMES)
    write_dict_csv(top20_csv, top20, MATRIX_FIELDNAMES)
    write_dict_csv(stability_csv, stability, ["rank", "tercile", "stable", "positive_sum_terciles", "positive_avg_terciles", *MATRIX_FIELDNAMES])
    write_dict_csv(
        cost_csv,
        cost_rows,
        [
            "rank",
            "target_bps",
            "stop_bps",
            "max_hold_ms",
            "roundtrip_cost_bps",
            "eligible_count",
            "avg_pnl_bps_after_cost",
            "median_pnl_bps_after_cost",
            "sum_pnl_bps_after_cost",
            "profit_factor_after_cost",
            "expected_pnl_sol_at_0_25_sol_after_cost",
            "median_pnl_sol_at_0_25_sol_after_cost",
        ],
    )
    heatmap_paths = write_heatmaps(output_dir, matrix, targets, stops, max_holds)

    legacy = evaluate([record.raw for record in records], [6000], [-6000])[0] if 6000 in common_set and -6000 in common_set else None
    matrix_6000 = next(
        (
            row
            for row in matrix
            if row["target_bps"] == 6000 and row["stop_bps"] == -6000 and row["max_hold_ms"] == 120000
        ),
        None,
    )
    legacy_comparison = None
    if legacy is not None and matrix_6000 is not None:
        legacy_comparison = {
            "legacy_total": legacy["total"],
            "legacy_target_count": legacy["target_count"],
            "legacy_stop_count": legacy["stop_count"],
            "legacy_timestop_count": legacy["timestop_count"],
            "legacy_avg_pnl_bps": legacy["avg_pnl_bps"],
            "legacy_median_pnl_bps": legacy["median_pnl_bps"],
            "legacy_sum_pnl_bps": legacy["sum_pnl_bps"],
            "matrix_eligible_count": matrix_6000["eligible_count"],
            "matrix_target_count": matrix_6000["target_count"],
            "matrix_stop_count": matrix_6000["stop_count"],
            "matrix_timeout_count": matrix_6000["timeout_count"],
            "matrix_avg_pnl_bps": matrix_6000["avg_pnl_bps"],
            "matrix_median_pnl_bps": matrix_6000["median_pnl_bps"],
            "matrix_sum_pnl_bps": matrix_6000["sum_pnl_bps"],
            "matches_counts_and_pnl": (
                legacy["total"] == matrix_6000["eligible_count"]
                and legacy["target_count"] == matrix_6000["target_count"]
                and legacy["stop_count"] == matrix_6000["stop_count"]
                and legacy["timestop_count"] == matrix_6000["timeout_count"]
                and legacy["sum_pnl_bps"] == matrix_6000["sum_pnl_bps"]
            ),
        }

    metadata = {
        "input_path": str(args.input),
        "snapshot_timestamp_utc": args.snapshot_timestamp_utc or utc_now_iso(),
        "snapshot_size_bytes": args.input.stat().st_size,
        "snapshot_sha256": sha256_file(args.input),
        "common_levels_bps": common,
        "targets_bps": targets,
        "stops_bps": stops,
        "max_hold_ms": max_holds,
        "roundtrip_cost_bps": costs,
        "combination_count": len(matrix),
        "generated_at_utc": utc_now_iso(),
        "semantic_notes": {
            "barrier_hits": EXACT_LEVELS,
            "timeout_pnl": PATH_PREV_TIMEOUT,
            "gross_not_net": True,
            "fee_cost_fields_proven_present": False,
        },
        "controls": controls,
        "top20": top20,
        "pareto_frontier": frontier,
        "cost_sensitivity_top20": cost_rows,
        "heatmap_paths": heatmap_paths,
        "legacy_6000_minus6000_120000_comparison": legacy_comparison,
    }
    matrix_json.write_text(
        json.dumps({"metadata": metadata, "matrix": matrix}, indent=2, sort_keys=True),
        encoding="utf-8",
    )
    write_report(
        report_md,
        input_path=args.input,
        metadata=metadata,
        controls=controls,
        targets=targets,
        stops=stops,
        max_holds=max_holds,
        matrix=matrix,
        top20=top20,
        stability=stability,
        frontier=frontier,
        cost_rows=cost_rows,
        heatmap_paths=heatmap_paths,
        legacy_comparison=legacy_comparison,
    )

    print(json.dumps({
        "snapshot_count": controls["total_records"],
        "snapshot_sha256": metadata["snapshot_sha256"],
        "qualified_records": controls["qualified_records"],
        "common_levels_bps": common,
        "targets_bps": targets,
        "stops_bps": stops,
        "combination_count": len(matrix),
        "top5_by_avg": top20[:5],
        "output_dir": str(output_dir),
    }, indent=2, sort_keys=True))
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--targets-bps", type=parse_bps_list)
    parser.add_argument("--stops-bps", type=parse_bps_list)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--matrix-output-dir", type=Path)
    parser.add_argument("--max-hold-ms", type=parse_bps_list)
    parser.add_argument("--roundtrip-cost-bps", type=parse_bps_list)
    parser.add_argument("--snapshot-timestamp-utc")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    if args.matrix_output_dir is not None:
        return run_matrix(args)
    if args.targets_bps is None or args.stops_bps is None:
        raise SystemExit("--targets-bps and --stops-bps are required unless --matrix-output-dir is used")
    rows = list(iter_jsonl(args.input))
    result_rows = evaluate(rows, args.targets_bps, args.stops_bps)
    write_csv(result_rows, args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
