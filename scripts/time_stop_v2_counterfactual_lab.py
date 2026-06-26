#!/usr/bin/env python3
"""Offline TimeStop V2 counterfactual exit lab.

This script is research-only. It reads durable shadow/probe lifecycle evidence
and compact shadow exit replay records, then estimates whether a TimeStop V2
candidate would have improved the replayed economic outcome. It never writes to
runtime log directories and must not be imported by runtime decision code.
"""

from __future__ import annotations

import argparse
import csv
import json
import math
import statistics
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable


SCHEMA = "time_stop_v2_counterfactual_exit_v1"
POSITION_RECORD = "time_stop_v2_counterfactual_position"
WINDOW_RECORD = "time_stop_v2_window"
EXIT_REPLAY_SCHEMA = "shadow_exit_replay_v1"

TARGET = "TARGET"
STOP = "STOP"
TIMEOUT = "TIMEOUT"
UNKNOWN = "UNKNOWN"
TSV2_EXIT = "TSV2_EXIT"

EXACT_LEVELS = "exact_levels"
PATH_APPROX = "path_approx"
UNSUPPORTED_NON_EXACT_LEVEL = "unsupported_non_exact_level"

RECOMMEND_NO_WINDOWS = "TIMESTOP_V2_NO_WINDOWS"
RECOMMEND_DATA_BLOCKED = "TIMESTOP_V2_DATA_QUALITY_BLOCKED"
RECOMMEND_PROMISING = "TIMESTOP_V2_COUNTERFACTUAL_PROMISING"
RECOMMEND_TOO_MANY_CUTS = "TIMESTOP_V2_TOO_MANY_TARGET_CUTS"
RECOMMEND_NO_BENEFIT = "TIMESTOP_V2_NO_ECONOMIC_BENEFIT"
RECOMMEND_NEEDS_MORE_DATA = "TIMESTOP_V2_NEEDS_MORE_DATA"

VERDICT_REJECTED_FOR_RUNTIME = "REJECTED_FOR_RUNTIME"
VERDICT_INCONCLUSIVE_RESEARCH = "INCONCLUSIVE_RESEARCH"
VERDICT_PROMISING_OFFLINE_ONLY = "PROMISING_OFFLINE_ONLY"
VERDICT_ELIGIBLE_FOR_SHADOW_CLOSE_ONLY_PLAN = "ELIGIBLE_FOR_SHADOW_CLOSE_ONLY_PLAN"

DEFAULT_COST_BPS = [0, 50, 100, 150, 200]
DEFAULT_NEGATIVE_CONTROL_SCOPE = "shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2"
NOHARM_SELECTION_COST_BPS = 100


@dataclass(frozen=True)
class ExactKey:
    run_id: str
    session_id: str
    pool_id: str
    base_mint: str
    entry_ts_ms: int


@dataclass(frozen=True)
class FallbackKey:
    run_id: str
    pool_id: str
    base_mint: str


@dataclass
class LoadStats:
    path: str
    rows: int = 0
    malformed_rows: int = 0
    malformed_examples: list[str] = field(default_factory=list)

    def add_malformed(self, error: str) -> None:
        self.malformed_rows += 1
        if len(self.malformed_examples) < 5:
            self.malformed_examples.append(error)


@dataclass
class ExitReplayPosition:
    row: dict[str, Any]
    exact_key: ExactKey
    fallback_key: FallbackKey


@dataclass
class LifecyclePosition:
    source_lifecycle: str
    group_key: str
    run_id: str = ""
    session_id: str = ""
    pool_id: str = ""
    base_mint: str = ""
    entry_ts_ms: int | None = None
    entry_ts_ms_source: str = "unavailable"
    candidate_id: str | None = None
    position_id: str | None = None
    terminal_row: dict[str, Any] | None = None
    windows: list[dict[str, Any]] = field(default_factory=list)

    def exact_key(self) -> ExactKey | None:
        if (
            self.run_id
            and self.session_id
            and self.pool_id
            and self.base_mint
            and self.entry_ts_ms is not None
        ):
            return ExactKey(
                self.run_id,
                self.session_id,
                self.pool_id,
                self.base_mint,
                self.entry_ts_ms,
            )
        return None

    def fallback_key(self) -> FallbackKey | None:
        if self.run_id and self.pool_id and self.base_mint:
            return FallbackKey(self.run_id, self.pool_id, self.base_mint)
        return None


@dataclass
class Tsv2Derived:
    has_windows: bool = False
    window_count: int = 0
    has_candidate: bool = False
    first_candidate_window: dict[str, Any] | None = None
    first_candidate_age_ms: int | None = None
    first_candidate_status: str | None = None
    first_candidate_subreason: str | None = None
    failed_windows_at_candidate: int | None = None
    status_sequence_before_candidate: list[str] = field(default_factory=list)
    candidate_class: str = "no_candidate"


@dataclass
class BaselineResult:
    result: str
    exit_age_ms: int
    pnl_bps: int
    result_quality: str
    pnl_quality: str


def utc_now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def int_or_none(value: Any) -> int | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value
    if isinstance(value, float) and math.isfinite(value) and value.is_integer():
        return int(value)
    if isinstance(value, str):
        stripped = value.strip()
        if stripped and stripped.lstrip("-").isdigit():
            return int(stripped)
    return None


def float_or_none(value: Any) -> float | None:
    if isinstance(value, bool) or value is None:
        return None
    try:
        parsed = float(value)
    except (TypeError, ValueError):
        return None
    return parsed if math.isfinite(parsed) else None


def pct_to_bps(value: Any) -> int | None:
    parsed = float_or_none(value)
    if parsed is None:
        return None
    return int(round(parsed * 100.0))


def parse_candidate_ts_ms(candidate_id: Any) -> int | None:
    if not isinstance(candidate_id, str):
        return None
    suffix = candidate_id.rsplit("_", 1)[-1]
    if suffix.isdigit():
        return int(suffix)
    return None


def parse_int_list(raw: str) -> list[int]:
    values: list[int] = []
    for item in raw.split(","):
        item = item.strip()
        if item:
            values.append(int(item))
    if not values:
        raise argparse.ArgumentTypeError("empty integer list")
    return values


def median_int(values: list[int]) -> float:
    return float(statistics.median(values)) if values else 0.0


def mean_int(values: list[int]) -> float:
    return float(sum(values) / len(values)) if values else 0.0


def safe_div(numerator: float, denominator: float) -> float:
    return numerator / denominator if denominator else 0.0


def wilson_lower_bound(successes: int, total: int, z: float = 1.959963984540054) -> float:
    if total <= 0:
        return 0.0
    phat = successes / total
    denom = 1.0 + (z * z / total)
    center = phat + (z * z / (2.0 * total))
    margin = z * math.sqrt((phat * (1.0 - phat) + z * z / (4.0 * total)) / total)
    return max(0.0, (center - margin) / denom)


def read_jsonl(path: Path) -> tuple[list[dict[str, Any]], LoadStats]:
    stats = LoadStats(str(path))
    rows: list[dict[str, Any]] = []
    if not path.exists():
        return rows, stats
    with path.open("r", encoding="utf-8", errors="ignore") as handle:
        for line_no, line in enumerate(handle, start=1):
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError as exc:
                stats.add_malformed(f"line={line_no} error={exc}")
                continue
            if isinstance(row, dict):
                stats.rows += 1
                rows.append(row)
    return rows, stats


def read_json_objects(path: Path) -> tuple[list[dict[str, Any]], LoadStats]:
    stats = LoadStats(str(path))
    rows: list[dict[str, Any]] = []
    if not path.exists():
        return rows, stats
    decoder = json.JSONDecoder()
    with path.open("r", encoding="utf-8", errors="ignore") as handle:
        for line_no, line in enumerate(handle, start=1):
            text = line.strip()
            if not text:
                continue
            try:
                row = json.loads(text)
            except json.JSONDecodeError:
                index = 0
                length = len(text)
                decoded_any = False
                while index < length:
                    while index < length and text[index].isspace():
                        index += 1
                    if index >= length:
                        break
                    try:
                        row, next_index = decoder.raw_decode(text, index)
                    except json.JSONDecodeError as exc:
                        stats.add_malformed(f"line={line_no} offset={index} error={exc}")
                        break
                    index = next_index
                    decoded_any = True
                    if isinstance(row, dict):
                        stats.rows += 1
                        rows.append(row)
                if not decoded_any:
                    continue
                continue
            if isinstance(row, dict):
                stats.rows += 1
                rows.append(row)
    return rows, stats


def base_mint_from(row: dict[str, Any]) -> str:
    return str(row.get("base_mint") or row.get("mint_id") or "")


def normalized_entry_ts_ms(row: dict[str, Any]) -> tuple[int | None, str]:
    explicit = int_or_none(row.get("entry_ts_ms"))
    if explicit is not None:
        return explicit, "explicit"
    ts_ms = int_or_none(row.get("timestamp_ms"))
    age_ms = int_or_none(row.get("time_stop_v2_position_age_ms"))
    if ts_ms is not None and age_ms is not None:
        return ts_ms - age_ms, "window_timestamp_minus_age"
    duration_ms = int_or_none(row.get("duration_ms"))
    if ts_ms is not None and duration_ms is not None:
        return ts_ms - duration_ms, "closed_minus_duration"
    candidate_ts = parse_candidate_ts_ms(row.get("candidate_id"))
    if candidate_ts is not None:
        return candidate_ts, "candidate_id"
    return None, "unavailable"


def lifecycle_group_key(source: str, row: dict[str, Any]) -> str:
    position_id = row.get("position_id")
    if isinstance(position_id, str) and position_id:
        return f"{source}:position:{position_id}"
    candidate_id = row.get("candidate_id")
    if isinstance(candidate_id, str) and candidate_id:
        return f"{source}:candidate:{candidate_id}"
    entry_ts_ms, _ = normalized_entry_ts_ms(row)
    return (
        f"{source}:identity:{row.get('run_id') or ''}:"
        f"{row.get('session_id') or ''}:{row.get('pool_id') or ''}:"
        f"{base_mint_from(row)}:{entry_ts_ms or ''}"
    )


def update_lifecycle_identity(pos: LifecyclePosition, row: dict[str, Any]) -> None:
    pos.run_id = pos.run_id or str(row.get("run_id") or "")
    pos.session_id = pos.session_id or str(row.get("session_id") or "")
    pos.pool_id = pos.pool_id or str(row.get("pool_id") or "")
    pos.base_mint = pos.base_mint or base_mint_from(row)
    if pos.candidate_id is None and isinstance(row.get("candidate_id"), str):
        pos.candidate_id = row["candidate_id"]
    if pos.position_id is None and isinstance(row.get("position_id"), str):
        pos.position_id = row["position_id"]
    entry_ts_ms, source = normalized_entry_ts_ms(row)
    if pos.entry_ts_ms is None and entry_ts_ms is not None:
        pos.entry_ts_ms = entry_ts_ms
        pos.entry_ts_ms_source = source


def is_terminal_row(row: dict[str, Any]) -> bool:
    record_type = row.get("record_type")
    if record_type in {"position_closed", "probe_position_closed"}:
        return True
    return "close_reason" in row and ("final_pnl" in row or "final_pnl_pct" in row)


def load_lifecycle_positions(
    shadow_lifecycle_path: Path,
    probe_lifecycle_path: Path | None,
) -> tuple[list[LifecyclePosition], list[LoadStats]]:
    positions: dict[str, LifecyclePosition] = {}
    stats_all: list[LoadStats] = []
    for source, path in (
        ("shadow", shadow_lifecycle_path),
        ("probe", probe_lifecycle_path),
    ):
        if path is None:
            continue
        rows, stats = read_json_objects(path)
        stats_all.append(stats)
        for row in rows:
            record_type = row.get("record_type")
            if record_type != WINDOW_RECORD and not is_terminal_row(row):
                continue
            group_key = lifecycle_group_key(source, row)
            pos = positions.setdefault(group_key, LifecyclePosition(source, group_key))
            update_lifecycle_identity(pos, row)
            if record_type == WINDOW_RECORD:
                pos.windows.append(row)
            elif is_terminal_row(row):
                current_ts = int_or_none(row.get("timestamp_ms")) or 0
                previous_ts = (
                    int_or_none(pos.terminal_row.get("timestamp_ms"))
                    if pos.terminal_row is not None
                    else None
                )
                if pos.terminal_row is None or current_ts >= (previous_ts or 0):
                    pos.terminal_row = row
    return list(positions.values()), stats_all


def load_exit_replay_positions(path: Path) -> tuple[list[ExitReplayPosition], LoadStats]:
    rows, stats = read_jsonl(path)
    positions: list[ExitReplayPosition] = []
    for row in rows:
        if row.get("schema") != EXIT_REPLAY_SCHEMA:
            continue
        entry_ts_ms = int_or_none(row.get("entry_ts_ms"))
        run_id = str(row.get("run_id") or "")
        session_id = str(row.get("session_id") or "")
        pool_id = str(row.get("pool_id") or "")
        base_mint = str(row.get("base_mint") or "")
        if not (run_id and session_id and pool_id and base_mint and entry_ts_ms is not None):
            stats.add_malformed("exit_replay_missing_identity")
            continue
        exact = ExactKey(run_id, session_id, pool_id, base_mint, entry_ts_ms)
        fallback = FallbackKey(run_id, pool_id, base_mint)
        positions.append(ExitReplayPosition(row=row, exact_key=exact, fallback_key=fallback))
    return positions, stats


def sort_windows(windows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return sorted(
        windows,
        key=lambda row: (
            int_or_none(row.get("time_stop_v2_window_index")) if row.get("time_stop_v2_window_index") is not None else 10**12,
            int_or_none(row.get("time_stop_v2_position_age_ms")) if row.get("time_stop_v2_position_age_ms") is not None else 10**12,
            int_or_none(row.get("time_stop_v2_scheduled_check_ms")) if row.get("time_stop_v2_scheduled_check_ms") is not None else 10**12,
            int_or_none(row.get("timestamp_ms")) or 0,
        ),
    )


def derive_tsv2(windows: list[dict[str, Any]]) -> Tsv2Derived:
    ordered = sort_windows(windows)
    derived = Tsv2Derived(has_windows=bool(ordered), window_count=len(ordered))
    candidate_index: int | None = None
    for idx, row in enumerate(ordered):
        if row.get("time_stop_v2_candidate") is True:
            candidate_index = idx
            derived.first_candidate_window = row
            derived.has_candidate = True
            break
    if candidate_index is None:
        return derived

    candidate = ordered[candidate_index]
    before = ordered[: candidate_index + 1]
    statuses = [str(row.get("time_stop_v2_status") or "missing") for row in before]
    subreason = str(candidate.get("time_stop_v2_subreason") or "")
    status = str(candidate.get("time_stop_v2_status") or "")
    derived.first_candidate_age_ms = int_or_none(candidate.get("time_stop_v2_position_age_ms"))
    derived.first_candidate_status = status or None
    derived.first_candidate_subreason = subreason or None
    derived.failed_windows_at_candidate = int_or_none(candidate.get("time_stop_v2_failed_windows"))
    derived.status_sequence_before_candidate = statuses
    derived.candidate_class = classify_candidate(candidate, before)
    return derived


def classify_candidate(candidate: dict[str, Any], before: list[dict[str, Any]]) -> str:
    status = str(candidate.get("time_stop_v2_status") or "").lower()
    subreason = str(candidate.get("time_stop_v2_subreason") or "").lower()
    if status == "stale_or_insufficient" or any(
        token in subreason for token in ("missing", "invalid", "stale")
    ):
        return "stale_data_no_action"

    failed_statuses = [
        str(row.get("time_stop_v2_status") or "").lower()
        for row in before
        if str(row.get("time_stop_v2_status") or "").lower() != "alive"
    ]
    if failed_statuses and all(status == "heartbeat" for status in failed_statuses):
        return "heartbeat_only_candidate"

    volume = float_or_none(candidate.get("time_stop_v2_volume_delta_sol_window"))
    price_delta = abs(float_or_none(candidate.get("time_stop_v2_price_delta_pct_window")) or 0.0)
    mcap_delta = abs(float_or_none(candidate.get("time_stop_v2_mcap_delta_pct_window")) or 0.0)
    bonding_delta = abs(float_or_none(candidate.get("time_stop_v2_bonding_delta_pct_window")) or 0.0)
    if (
        volume is not None
        and volume > 0.0
        and status != "alive"
        and price_delta < 3.0
        and mcap_delta < 3.0
        and bonding_delta < 0.75
    ):
        return "no_progress_with_volume_candidate"
    if any(status.startswith("weak") or status == "weak" for status in failed_statuses):
        return "weak_no_progress_candidate"
    return "mixed_failed_vitality_candidate"


def normalize_close_reason(value: Any) -> str:
    if value is None:
        return UNKNOWN
    raw = str(value).strip().lower().replace("-", "_").replace(" ", "_")
    if raw in {"target", "take_profit", "takeprofit"}:
        return TARGET
    if raw in {"stop", "stoploss", "stop_loss"}:
        return STOP
    if raw in {"timeout", "time_stop", "timestop"}:
        return TIMEOUT
    return UNKNOWN


def terminal_context(pos: LifecyclePosition | None) -> dict[str, Any]:
    if pos is None or pos.terminal_row is None:
        return {
            "actual_terminal_reason": UNKNOWN,
            "actual_close_age_ms": None,
            "actual_final_pnl_bps": None,
            "actual_final_pnl_pct": None,
        }
    row = pos.terminal_row
    final_pct = float_or_none(row.get("final_pnl_pct"))
    close_age_ms = int_or_none(row.get("duration_ms"))
    if close_age_ms is None:
        ts_ms = int_or_none(row.get("timestamp_ms"))
        if ts_ms is not None and pos.entry_ts_ms is not None:
            close_age_ms = max(0, ts_ms - pos.entry_ts_ms)
    final_bps = int(round(final_pct * 100.0)) if final_pct is not None else None
    return {
        "actual_terminal_reason": normalize_close_reason(row.get("close_reason")),
        "actual_lifecycle_close_reason_raw": row.get("close_reason"),
        "actual_close_age_ms": close_age_ms,
        "actual_final_pnl_bps": final_bps,
        "actual_final_pnl_pct": final_pct,
    }


def path_points(row: dict[str, Any]) -> list[tuple[int, int]]:
    cached = row.get("_path_points_cache")
    if isinstance(cached, list):
        return cached
    points: list[tuple[int, int]] = []
    raw = row.get("path_bps")
    if not isinstance(raw, list):
        return points
    for point in raw:
        if not isinstance(point, (list, tuple)) or len(point) != 2:
            continue
        age_ms = int_or_none(point[0])
        pnl_bps = int_or_none(point[1])
        if age_ms is not None and pnl_bps is not None:
            points.append((age_ms, pnl_bps))
    points.sort(key=lambda item: item[0])
    row["_path_points_cache"] = points
    return points


def last_path_pnl_at_or_before(row: dict[str, Any], age_ms: int) -> tuple[int | None, str]:
    selected: int | None = None
    for point_age_ms, pnl_bps in path_points(row):
        if point_age_ms <= age_ms:
            selected = pnl_bps
        else:
            break
    return selected, "path_prev" if selected is not None else "unavailable"


def first_path_hit(row: dict[str, Any], level_bps: int, max_hold_ms: int) -> int | None:
    for age_ms, pnl_bps in path_points(row):
        if age_ms > max_hold_ms:
            break
        if level_bps > 0 and pnl_bps >= level_bps:
            return age_ms
        if level_bps < 0 and pnl_bps <= level_bps:
            return age_ms
    return None


def first_hit_exact(row: dict[str, Any], level_bps: int, max_hold_ms: int) -> int | None:
    first_hit = row.get("first_hit_ms")
    if not isinstance(first_hit, dict):
        return None
    value = int_or_none(first_hit.get(str(level_bps)))
    if value is None or value > max_hold_ms:
        return None
    return value


def simulate_baseline(
    row: dict[str, Any],
    target_bps: int,
    stop_bps: int,
    max_hold_ms: int,
) -> BaselineResult | None:
    levels = row.get("levels_bps")
    levels_set = set(levels) if isinstance(levels, list) else set()
    use_exact = target_bps in levels_set and stop_bps in levels_set
    if use_exact:
        target_ts = first_hit_exact(row, target_bps, max_hold_ms)
        stop_ts = first_hit_exact(row, stop_bps, max_hold_ms)
        quality = EXACT_LEVELS
    else:
        target_ts = first_path_hit(row, target_bps, max_hold_ms)
        stop_ts = first_path_hit(row, stop_bps, max_hold_ms)
        quality = PATH_APPROX

    if target_ts is not None and stop_ts is not None:
        if target_ts < stop_ts:
            return BaselineResult(TARGET, target_ts, target_bps, quality, quality)
        return BaselineResult(STOP, stop_ts, stop_bps, quality, quality)
    if target_ts is not None:
        return BaselineResult(TARGET, target_ts, target_bps, quality, quality)
    if stop_ts is not None:
        return BaselineResult(STOP, stop_ts, stop_bps, quality, quality)

    pnl_at_hold, pnl_quality = last_path_pnl_at_or_before(row, max_hold_ms)
    if pnl_at_hold is None:
        last_pnl = int_or_none(row.get("last_pnl_bps"))
        close_age_ms = int_or_none(row.get("close_age_ms"))
        if last_pnl is not None and close_age_ms is not None and close_age_ms <= max_hold_ms:
            pnl_at_hold = last_pnl
            pnl_quality = "last_pnl_before_max_hold"
    if pnl_at_hold is None:
        return None
    return BaselineResult(TIMEOUT, max_hold_ms, pnl_at_hold, quality, pnl_quality)


def simulate_baseline_cached(
    row: dict[str, Any],
    target_bps: int,
    stop_bps: int,
    max_hold_ms: int,
) -> BaselineResult | None:
    cache = row.setdefault("_baseline_result_cache", {})
    if not isinstance(cache, dict):
        cache = {}
        row["_baseline_result_cache"] = cache
    key = (target_bps, stop_bps, max_hold_ms)
    if key not in cache:
        cache[key] = simulate_baseline(row, target_bps, stop_bps, max_hold_ms)
    return cache[key]


def candidate_pnl(
    exit_replay: dict[str, Any],
    candidate: dict[str, Any] | None,
    candidate_age_ms: int | None,
) -> tuple[int | None, str, int | None, int | None]:
    if candidate is None or candidate_age_ms is None:
        return None, "unavailable", None, None
    from_tsv2 = pct_to_bps(candidate.get("time_stop_v2_price_delta_pct_from_entry"))
    from_path, _ = last_path_pnl_at_or_before(exit_replay, candidate_age_ms)
    if from_tsv2 is not None:
        return from_tsv2, "tsv2_window", from_tsv2, from_path
    if from_path is not None:
        return from_path, "path_prev", from_tsv2, from_path
    return None, "unavailable", from_tsv2, from_path


def after_candidate_path_stats(
    row: dict[str, Any],
    candidate_age_ms: int | None,
    target_bps: int,
) -> dict[str, Any]:
    if candidate_age_ms is None:
        return {
            "mfe_after_candidate_bps_path_approx": None,
            "mae_after_candidate_bps_path_approx": None,
            "target_after_candidate": None,
        }
    values = [pnl for age, pnl in path_points(row) if age >= candidate_age_ms]
    target_after = any(age > candidate_age_ms and pnl >= target_bps for age, pnl in path_points(row))
    return {
        "mfe_after_candidate_bps_path_approx": max(values) if values else None,
        "mae_after_candidate_bps_path_approx": min(values) if values else None,
        "target_after_candidate": target_after,
    }


def alive_within(windows: list[dict[str, Any]], candidate_age_ms: int | None, window_ms: int) -> bool | None:
    if candidate_age_ms is None:
        return None
    upper = candidate_age_ms + window_ms
    for row in sort_windows(windows):
        age = int_or_none(row.get("time_stop_v2_position_age_ms"))
        if age is None or age <= candidate_age_ms or age > upper:
            continue
        if str(row.get("time_stop_v2_status") or "").lower() == "alive":
            return True
    return False


def build_indexes(
    lifecycle_positions: list[LifecyclePosition],
) -> tuple[dict[ExactKey, LifecyclePosition], dict[FallbackKey, list[LifecyclePosition]], Counter[str]]:
    exact: dict[ExactKey, LifecyclePosition] = {}
    fallback: dict[FallbackKey, list[LifecyclePosition]] = defaultdict(list)
    entry_sources: Counter[str] = Counter()
    for pos in lifecycle_positions:
        entry_sources[pos.entry_ts_ms_source] += 1
        key = pos.exact_key()
        if key is not None:
            exact.setdefault(key, pos)
        fkey = pos.fallback_key()
        if fkey is not None:
            fallback[fkey].append(pos)
    return exact, fallback, entry_sources


def join_lifecycle(
    replay_positions: list[ExitReplayPosition],
    lifecycle_positions: list[LifecyclePosition],
) -> tuple[dict[int, tuple[LifecyclePosition | None, str]], dict[str, Any]]:
    exact_index, fallback_index, entry_sources = build_indexes(lifecycle_positions)
    joined: dict[int, tuple[LifecyclePosition | None, str]] = {}
    exact_count = 0
    fallback_unique_count = 0
    unmatched_exit = 0
    duplicate_fallback = 0
    for idx, replay in enumerate(replay_positions):
        pos = exact_index.get(replay.exact_key)
        if pos is not None:
            joined[idx] = (pos, "exact")
            exact_count += 1
            continue
        candidates = fallback_index.get(replay.fallback_key, [])
        if len(candidates) == 1:
            joined[idx] = (candidates[0], "fallback_unique")
            fallback_unique_count += 1
        elif len(candidates) > 1:
            joined[idx] = (None, "fallback_duplicate_ambiguous")
            duplicate_fallback += 1
        else:
            joined[idx] = (None, "unmatched_exit_replay")
            unmatched_exit += 1

    matched_lifecycle_ids = {id(pos) for pos, _ in joined.values() if pos is not None}
    unmatched_lifecycle = sum(1 for pos in lifecycle_positions if id(pos) not in matched_lifecycle_ids)
    return joined, {
        "exact_join_count": exact_count,
        "fallback_unique_join_count": fallback_unique_count,
        "unmatched_exit_replay_count": unmatched_exit,
        "unmatched_lifecycle_position_count": unmatched_lifecycle,
        "duplicate_fallback_key_count": duplicate_fallback,
        "entry_ts_ms_source_counts": dict(entry_sources),
    }


def actual_classification(
    active_exit_eligible: bool,
    candidate_pnl_bps: int | None,
    baseline: BaselineResult | None,
    candidate_before_baseline: bool | None,
    candidate_class: str,
) -> tuple[str, int | None]:
    if candidate_class == "stale_data_no_action":
        return "stale_excluded", None
    if not active_exit_eligible:
        return "no_candidate" if candidate_class == "no_candidate" else "not_active_exit_eligible", None
    if baseline is None or candidate_pnl_bps is None or candidate_before_baseline is not True:
        return "not_active_exit_eligible", None
    delta = candidate_pnl_bps - baseline.pnl_bps
    if baseline.result == STOP and delta > 0:
        return "saved_stop", delta
    if baseline.result == TARGET:
        return "cut_target", delta
    if baseline.result == TIMEOUT and delta > 0:
        return "timeout_improved", delta
    if delta < 0:
        return "harmful_exit", delta
    if delta > 0:
        return "beneficial_exit", delta
    return "neutral_exit", delta


def matrix_row(
    positions: list[dict[str, Any]],
    target_bps: int,
    stop_bps: int,
    max_hold_ms: int,
) -> dict[str, Any]:
    baseline_counts = Counter()
    tsv2_counts = Counter()
    baseline_pnls: list[int] = []
    tsv2_pnls: list[int] = []
    deltas: list[int] = []
    counters = Counter()

    for pos in positions:
        replay = pos.get("_exit_replay_row")
        if not isinstance(replay, dict):
            continue
        baseline = simulate_baseline_cached(replay, target_bps, stop_bps, max_hold_ms)
        if baseline is None:
            counters["unsupported_rows"] += 1
            continue
        counters["exact_rows" if baseline.result_quality == EXACT_LEVELS else "path_approx_rows"] += 1
        baseline_counts[baseline.result] += 1
        baseline_pnls.append(baseline.pnl_bps)
        candidate_age = int_or_none(pos.get("first_candidate_age_ms"))
        candidate_pnl_bps = int_or_none(pos.get("candidate_pnl_bps"))
        active = bool(pos.get("active_exit_eligible"))
        candidate_before = (
            active
            and candidate_age is not None
            and candidate_pnl_bps is not None
            and candidate_age <= baseline.exit_age_ms
            and candidate_age <= max_hold_ms
        )
        if active and pos.get("candidate_class") == "stale_data_no_action":
            counters["stale_candidates_excluded"] += 1
        if candidate_before:
            tsv2_counts[TSV2_EXIT] += 1
            tsv2_pnls.append(candidate_pnl_bps)
            delta = candidate_pnl_bps - baseline.pnl_bps
            deltas.append(delta)
            if baseline.result == TARGET:
                counters["targets_cut_by_tsv2"] += 1
            if baseline.result == STOP and delta > 0:
                counters["stops_saved_by_tsv2"] += 1
            if baseline.result == TIMEOUT:
                counters["timeouts_cut_by_tsv2"] += 1
            if delta < 0:
                counters["harmful_tsv2_exits"] += 1
            elif delta > 0:
                counters["beneficial_tsv2_exits"] += 1
            else:
                counters["neutral_tsv2_exits"] += 1
        else:
            tsv2_counts[baseline.result] += 1
            tsv2_pnls.append(baseline.pnl_bps)

    total = len(baseline_pnls)
    return {
        "target_bps": target_bps,
        "stop_bps": stop_bps,
        "max_hold_ms": max_hold_ms,
        "total_positions": total,
        "eligible_positions": sum(1 for pos in positions if pos.get("active_exit_eligible")),
        "baseline_target_count": baseline_counts[TARGET],
        "baseline_stop_count": baseline_counts[STOP],
        "baseline_timeout_count": baseline_counts[TIMEOUT],
        "baseline_avg_pnl_bps": mean_int(baseline_pnls),
        "baseline_median_pnl_bps": median_int(baseline_pnls),
        "baseline_sum_pnl_bps": sum(baseline_pnls),
        "tsv2_target_count": tsv2_counts[TARGET],
        "tsv2_stop_count": tsv2_counts[STOP],
        "tsv2_timeout_count": tsv2_counts[TIMEOUT],
        "tsv2_exit_count": tsv2_counts[TSV2_EXIT],
        "tsv2_avg_pnl_bps": mean_int(tsv2_pnls),
        "tsv2_median_pnl_bps": median_int(tsv2_pnls),
        "tsv2_sum_pnl_bps": sum(tsv2_pnls),
        "pnl_delta_sum_bps": sum(deltas),
        "pnl_delta_avg_bps": mean_int(deltas),
        "targets_cut_by_tsv2": counters["targets_cut_by_tsv2"],
        "stops_saved_by_tsv2": counters["stops_saved_by_tsv2"],
        "timeouts_cut_by_tsv2": counters["timeouts_cut_by_tsv2"],
        "harmful_tsv2_exits": counters["harmful_tsv2_exits"],
        "beneficial_tsv2_exits": counters["beneficial_tsv2_exits"],
        "neutral_tsv2_exits": counters["neutral_tsv2_exits"],
        "stale_candidates_excluded": counters["stale_candidates_excluded"],
        "path_approx_rows": counters["path_approx_rows"],
        "exact_rows": counters["exact_rows"],
        "unsupported_rows": counters["unsupported_rows"],
    }


def assign_chronological_terciles(records: list[dict[str, Any]]) -> None:
    replay_records = [
        row for row in records
        if row.get("has_exit_replay") and row.get("entry_ts_ms") is not None
    ]
    replay_records.sort(
        key=lambda row: (
            int_or_none(row.get("entry_ts_ms")) or 0,
            str(row.get("run_id") or ""),
            str(row.get("session_id") or ""),
            str(row.get("pool_id") or ""),
            str(row.get("base_mint") or ""),
        )
    )
    total = len(replay_records)
    for index, row in enumerate(replay_records):
        ratio = index / total if total else 0.0
        if ratio < 1 / 3:
            split = "train"
        elif ratio < 2 / 3:
            split = "validation"
        else:
            split = "holdout"
        row["_chronological_split"] = split


def action_class_for_delta(baseline: BaselineResult, delta_bps: int) -> str:
    if baseline.result == STOP and delta_bps > 0:
        return "saved_stop"
    if baseline.result == TARGET:
        return "cut_target"
    if baseline.result == TIMEOUT and delta_bps > 0:
        return "timeout_improved"
    if delta_bps < 0:
        return "harmful_exit"
    if delta_bps > 0:
        return "beneficial_exit"
    return "neutral_exit"


def cell_action_rows(
    records: list[dict[str, Any]],
    target_bps: int,
    stop_bps: int,
    max_hold_ms: int,
    *,
    roundtrip_cost_bps: int = 0,
) -> list[dict[str, Any]]:
    actions: list[dict[str, Any]] = []
    for row in records:
        replay = row.get("_exit_replay_row")
        join_quality = str(row.get("join_quality") or "missing")
        candidate_class = str(row.get("candidate_class") or "missing")
        base = {
            "run_id": row.get("run_id"),
            "session_id": row.get("session_id"),
            "pool_id": row.get("pool_id"),
            "base_mint": row.get("base_mint"),
            "entry_ts_ms": row.get("entry_ts_ms"),
            "segment": row.get("_chronological_split") or "unassigned",
            "target_bps": target_bps,
            "stop_bps": stop_bps,
            "max_hold_ms": max_hold_ms,
            "roundtrip_cost_bps": roundtrip_cost_bps,
            "join_quality": join_quality,
            "candidate_class": candidate_class,
            "has_exit_replay": bool(row.get("has_exit_replay")),
            "has_tsv2_windows": bool(row.get("has_tsv2_windows")),
            "has_candidate": bool(row.get("has_candidate")),
            "active_exit_eligible_lifecycle": bool(row.get("active_exit_eligible")),
        }
        if not isinstance(replay, dict):
            actions.append(
                {
                    **base,
                    "supported": False,
                    "action_taken": False,
                    "classification": "no_exit_replay",
                    "exclusion_reason": "lifecycle_without_exit_replay",
                    "baseline_result": UNKNOWN,
                    "baseline_result_quality": "unavailable",
                    "baseline_pnl_bps": None,
                    "tsv2_pnl_bps": None,
                    "delta_bps": None,
                    "baseline_pnl_after_cost_bps": None,
                    "tsv2_pnl_after_cost_bps": None,
                    "delta_after_cost_bps": None,
                }
            )
            continue

        baseline = simulate_baseline_cached(replay, target_bps, stop_bps, max_hold_ms)
        if baseline is None:
            actions.append(
                {
                    **base,
                    "supported": False,
                    "action_taken": False,
                    "classification": "unsupported_replay",
                    "exclusion_reason": "baseline_unavailable",
                    "baseline_result": UNKNOWN,
                    "baseline_result_quality": "unavailable",
                    "baseline_pnl_bps": None,
                    "tsv2_pnl_bps": None,
                    "delta_bps": None,
                    "baseline_pnl_after_cost_bps": None,
                    "tsv2_pnl_after_cost_bps": None,
                    "delta_after_cost_bps": None,
                }
            )
            continue

        candidate_age = int_or_none(row.get("first_candidate_age_ms"))
        candidate_pnl_bps = int_or_none(row.get("candidate_pnl_bps"))
        candidate_before_baseline = (
            bool(row.get("active_exit_eligible"))
            and candidate_age is not None
            and candidate_pnl_bps is not None
            and candidate_age <= baseline.exit_age_ms
            and candidate_age <= max_hold_ms
        )

        action_taken = False
        exclusion_reason = ""
        if candidate_class == "stale_data_no_action":
            exclusion_reason = "stale_data_no_action"
        elif join_quality in {"fallback_duplicate_ambiguous", "unmatched_exit_replay"}:
            exclusion_reason = join_quality
        elif not row.get("has_tsv2_windows"):
            exclusion_reason = "no_tsv2_windows"
        elif not row.get("has_candidate"):
            exclusion_reason = "no_candidate"
        elif not row.get("active_exit_eligible"):
            exclusion_reason = "not_active_exit_eligible"
        elif candidate_age is None:
            exclusion_reason = "missing_candidate_age"
        elif candidate_pnl_bps is None:
            exclusion_reason = "missing_candidate_pnl"
        elif candidate_age > max_hold_ms:
            exclusion_reason = "candidate_after_max_hold"
        elif candidate_age > baseline.exit_age_ms:
            exclusion_reason = "candidate_after_baseline_exit"
        elif candidate_before_baseline:
            action_taken = True
        else:
            exclusion_reason = "not_active_exit_eligible"

        if action_taken:
            tsv2_pnl = candidate_pnl_bps if candidate_pnl_bps is not None else baseline.pnl_bps
            delta = int(tsv2_pnl - baseline.pnl_bps)
            classification = action_class_for_delta(baseline, delta)
        else:
            tsv2_pnl = baseline.pnl_bps
            delta = 0
            classification = "no_active_exit"

        actions.append(
            {
                **base,
                "supported": True,
                "action_taken": action_taken,
                "classification": classification,
                "exclusion_reason": exclusion_reason,
                "baseline_result": baseline.result,
                "baseline_exit_age_ms": baseline.exit_age_ms,
                "baseline_result_quality": baseline.result_quality,
                "baseline_pnl_quality": baseline.pnl_quality,
                "baseline_pnl_bps": baseline.pnl_bps,
                "tsv2_pnl_bps": tsv2_pnl,
                "delta_bps": delta,
                "baseline_pnl_after_cost_bps": baseline.pnl_bps - roundtrip_cost_bps,
                "tsv2_pnl_after_cost_bps": tsv2_pnl - roundtrip_cost_bps,
                "delta_after_cost_bps": delta,
                "candidate_age_ms": candidate_age,
                "candidate_pnl_bps": candidate_pnl_bps,
                "candidate_before_baseline_exit": candidate_before_baseline,
            }
        )
    return actions


def max_consecutive_harmful_actions(actions: list[dict[str, Any]]) -> int:
    ordered = sorted(
        [row for row in actions if row.get("action_taken")],
        key=lambda row: (
            int_or_none(row.get("entry_ts_ms")) or 0,
            str(row.get("run_id") or ""),
            str(row.get("session_id") or ""),
            str(row.get("pool_id") or ""),
            str(row.get("base_mint") or ""),
        ),
    )
    best = 0
    current = 0
    for row in ordered:
        if row.get("classification") in {"cut_target", "harmful_exit"}:
            current += 1
            best = max(best, current)
        else:
            current = 0
    return best


def summarize_action_rows(actions: list[dict[str, Any]], *, prefix: str = "") -> dict[str, Any]:
    supported = [row for row in actions if row.get("supported")]
    deltas = [int(row.get("delta_after_cost_bps") or 0) for row in supported]
    baseline_pnls = [int(row.get("baseline_pnl_after_cost_bps") or 0) for row in supported]
    tsv2_pnls = [int(row.get("tsv2_pnl_after_cost_bps") or 0) for row in supported]
    counts = Counter(str(row.get("classification") or "missing") for row in actions)
    baseline_counts = Counter(str(row.get("baseline_result") or UNKNOWN) for row in supported)
    exclusion_counts = Counter(str(row.get("exclusion_reason") or "none") for row in actions)
    quality_counts = Counter(str(row.get("baseline_result_quality") or "unavailable") for row in supported)

    beneficial_classes = {"saved_stop", "timeout_improved", "beneficial_exit"}
    harmful_classes = {"cut_target", "harmful_exit"}
    beneficial_rows = [row for row in supported if row.get("classification") in beneficial_classes]
    harmful_rows = [row for row in supported if row.get("classification") in harmful_classes]
    neutral_rows = [row for row in supported if row.get("classification") == "neutral_exit"]
    action_rows = [row for row in supported if row.get("action_taken")]

    saved_stop_rows = [row for row in supported if row.get("classification") == "saved_stop"]
    target_cut_rows = [row for row in supported if row.get("classification") == "cut_target"]
    timeout_improved_rows = [row for row in supported if row.get("classification") == "timeout_improved"]
    generic_beneficial_rows = [row for row in supported if row.get("classification") == "beneficial_exit"]

    beneficial_count = len(beneficial_rows)
    harmful_count = len(harmful_rows)
    precision_denominator = beneficial_count + harmful_count
    saved_stop_bps = sum(max(0, int(row.get("delta_after_cost_bps") or 0)) for row in saved_stop_rows)
    timeout_improved_bps = sum(max(0, int(row.get("delta_after_cost_bps") or 0)) for row in timeout_improved_rows)
    generic_beneficial_bps = sum(max(0, int(row.get("delta_after_cost_bps") or 0)) for row in generic_beneficial_rows)
    target_cut_damage_bps = sum(max(0, -int(row.get("delta_after_cost_bps") or 0)) for row in target_cut_rows)
    harmful_damage_bps = sum(max(0, -int(row.get("delta_after_cost_bps") or 0)) for row in harmful_rows)
    gross_saved_damage_bps = saved_stop_bps + timeout_improved_bps + generic_beneficial_bps
    target_cut_count_guard_limit = len(saved_stop_rows) + 0.10 * len(timeout_improved_rows)
    target_cut_damage_guard_pass = target_cut_damage_bps <= 0.25 * gross_saved_damage_bps if gross_saved_damage_bps else target_cut_damage_bps == 0
    target_cut_count_guard_pass = len(target_cut_rows) <= target_cut_count_guard_limit

    out = {
        "supported_rows": len(supported),
        "unsupported_rows": len(actions) - len(supported),
        "action_taken_count": len(action_rows),
        "no_action_count": len(supported) - len(action_rows),
        "baseline_target_count": baseline_counts[TARGET],
        "baseline_stop_count": baseline_counts[STOP],
        "baseline_timeout_count": baseline_counts[TIMEOUT],
        "baseline_sum_after_cost_bps": sum(baseline_pnls),
        "baseline_avg_after_cost_bps": mean_int(baseline_pnls),
        "baseline_median_after_cost_bps": median_int(baseline_pnls),
        "tsv2_sum_after_cost_bps": sum(tsv2_pnls),
        "tsv2_avg_after_cost_bps": mean_int(tsv2_pnls),
        "tsv2_median_after_cost_bps": median_int(tsv2_pnls),
        "delta_sum_bps": sum(deltas),
        "delta_avg_bps": mean_int(deltas),
        "delta_median_bps": median_int(deltas),
        "action_delta_avg_bps": mean_int([int(row.get("delta_after_cost_bps") or 0) for row in action_rows]),
        "action_delta_median_bps": median_int([int(row.get("delta_after_cost_bps") or 0) for row in action_rows]),
        "beneficial_exit_count": beneficial_count,
        "harmful_exit_count": harmful_count,
        "neutral_exit_count": len(neutral_rows),
        "exit_action_precision": safe_div(beneficial_count, precision_denominator),
        "exit_action_precision_denominator": precision_denominator,
        "exit_action_precision_wilson95_lower": wilson_lower_bound(beneficial_count, precision_denominator),
        "saved_stop_count": len(saved_stop_rows),
        "saved_stop_damage_bps": saved_stop_bps,
        "target_cut_count": len(target_cut_rows),
        "target_cut_damage_bps": target_cut_damage_bps,
        "timeout_improved_count": len(timeout_improved_rows),
        "timeout_improved_bps": timeout_improved_bps,
        "generic_beneficial_count": len(generic_beneficial_rows),
        "generic_beneficial_bps": generic_beneficial_bps,
        "gross_saved_damage_bps": gross_saved_damage_bps,
        "harmful_damage_bps": harmful_damage_bps,
        "target_cut_damage_guard_pass": target_cut_damage_guard_pass,
        "target_cut_count_guard_pass": target_cut_count_guard_pass,
        "target_cut_count_guard_limit": target_cut_count_guard_limit,
        "stale_no_action_exclusions": exclusion_counts["stale_data_no_action"],
        "no_candidate_exclusions": exclusion_counts["no_candidate"],
        "not_active_exit_eligible_exclusions": exclusion_counts["not_active_exit_eligible"],
        "candidate_after_baseline_exclusions": exclusion_counts["candidate_after_baseline_exit"],
        "candidate_after_max_hold_exclusions": exclusion_counts["candidate_after_max_hold"],
        "ambiguous_unjoined_exclusions": exclusion_counts["fallback_duplicate_ambiguous"] + exclusion_counts["unmatched_exit_replay"],
        "lifecycle_without_exit_replay_exclusions": counts["no_exit_replay"],
        "exact_rows": quality_counts[EXACT_LEVELS],
        "path_approx_rows": quality_counts[PATH_APPROX],
        "baseline_unavailable_rows": counts["unsupported_replay"],
        "max_consecutive_harmful_actions": max_consecutive_harmful_actions(actions),
        "classification_counts": json.dumps(dict(sorted(counts.items())), sort_keys=True),
        "exclusion_counts": json.dumps(dict(sorted(exclusion_counts.items())), sort_keys=True),
    }
    if prefix:
        return {f"{prefix}{key}": value for key, value in out.items()}
    return out


def build_noharm_tables(
    records: list[dict[str, Any]],
    targets_bps: list[int],
    stops_bps: list[int],
    max_hold_values: list[int],
    costs_bps: list[int],
) -> tuple[list[dict[str, Any]], list[dict[str, Any]], list[dict[str, Any]]]:
    summary_rows: list[dict[str, Any]] = []
    cost_rows: list[dict[str, Any]] = []
    stability_rows: list[dict[str, Any]] = []
    for target_bps in targets_bps:
        for stop_bps in stops_bps:
            for max_hold_ms in max_hold_values:
                cost100_actions = cell_action_rows(
                    records,
                    target_bps,
                    stop_bps,
                    max_hold_ms,
                    roundtrip_cost_bps=NOHARM_SELECTION_COST_BPS,
                )
                cost100_summary = summarize_action_rows(cost100_actions)
                base_keys = {
                    "target_bps": target_bps,
                    "stop_bps": stop_bps,
                    "max_hold_ms": max_hold_ms,
                }
                summary_rows.append(
                    {
                        **base_keys,
                        **{f"cost100_{key}": value for key, value in cost100_summary.items()},
                    }
                )
                for cost in costs_bps:
                    actions = cell_action_rows(records, target_bps, stop_bps, max_hold_ms, roundtrip_cost_bps=cost)
                    metrics = summarize_action_rows(actions)
                    cost_rows.append(
                        {
                            **base_keys,
                            "roundtrip_cost_bps": cost,
                            **metrics,
                        }
                    )
                for segment in ("train", "validation", "holdout"):
                    segment_actions = [row for row in cost100_actions if row.get("segment") == segment]
                    metrics = summarize_action_rows(segment_actions)
                    stability_rows.append(
                        {
                            **base_keys,
                            "segment": segment,
                            "roundtrip_cost_bps": NOHARM_SELECTION_COST_BPS,
                            **metrics,
                        }
                    )
    return summary_rows, cost_rows, stability_rows


def choose_noharm_best(summary_rows: list[dict[str, Any]]) -> dict[str, Any] | None:
    if not summary_rows:
        return None
    return max(
        summary_rows,
        key=lambda row: (
            float(row.get("cost100_delta_sum_bps") or 0.0),
            float(row.get("cost100_exit_action_precision_wilson95_lower") or 0.0),
            float(row.get("cost100_exit_action_precision") or 0.0),
            -float(row.get("cost100_target_cut_damage_bps") or 0.0),
            int(row.get("target_bps") or 0),
            int(row.get("stop_bps") or 0),
            int(row.get("max_hold_ms") or 0),
        ),
    )


def build_grid_neighborhood(
    summary_rows: list[dict[str, Any]],
    targets_bps: list[int],
    stops_bps: list[int],
    max_hold_values: list[int],
    best: dict[str, Any] | None,
) -> list[dict[str, Any]]:
    if best is None:
        return []
    by_key = {
        (int(row["target_bps"]), int(row["stop_bps"]), int(row["max_hold_ms"])): row
        for row in summary_rows
    }
    target_idx = targets_bps.index(int(best["target_bps"]))
    stop_idx = stops_bps.index(int(best["stop_bps"]))
    hold_idx = max_hold_values.index(int(best["max_hold_ms"]))
    output: list[dict[str, Any]] = []
    for ti in range(max(0, target_idx - 1), min(len(targets_bps), target_idx + 2)):
        for si in range(max(0, stop_idx - 1), min(len(stops_bps), stop_idx + 2)):
            for hi in range(max(0, hold_idx - 1), min(len(max_hold_values), hold_idx + 2)):
                key = (targets_bps[ti], stops_bps[si], max_hold_values[hi])
                row = by_key.get(key)
                if row is None:
                    continue
                output.append(
                    {
                        "target_bps": key[0],
                        "stop_bps": key[1],
                        "max_hold_ms": key[2],
                        "is_best": key == (int(best["target_bps"]), int(best["stop_bps"]), int(best["max_hold_ms"])),
                        "cost100_delta_sum_bps": row.get("cost100_delta_sum_bps"),
                        "cost100_delta_avg_bps": row.get("cost100_delta_avg_bps"),
                        "cost100_delta_median_bps": row.get("cost100_delta_median_bps"),
                        "cost100_exit_action_precision": row.get("cost100_exit_action_precision"),
                        "cost100_exit_action_precision_wilson95_lower": row.get("cost100_exit_action_precision_wilson95_lower"),
                        "cost100_beneficial_exit_count": row.get("cost100_beneficial_exit_count"),
                        "cost100_harmful_exit_count": row.get("cost100_harmful_exit_count"),
                        "cost100_target_cut_damage_bps": row.get("cost100_target_cut_damage_bps"),
                        "cost100_gross_saved_damage_bps": row.get("cost100_gross_saved_damage_bps"),
                        "positive_delta": float(row.get("cost100_delta_sum_bps") or 0.0) > 0,
                    }
                )
    return output


def build_position_records(
    replay_positions: list[ExitReplayPosition],
    lifecycle_positions: list[LifecyclePosition],
    joined: dict[int, tuple[LifecyclePosition | None, str]],
    target_bps: int,
    stop_bps: int,
    max_hold_ms: int,
    resurrection_windows_ms: list[int],
) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    matched_lifecycle_ids: set[int] = set()

    for idx, replay in enumerate(replay_positions):
        lifecycle, join_quality = joined.get(idx, (None, "unmatched_exit_replay"))
        if lifecycle is not None:
            matched_lifecycle_ids.add(id(lifecycle))
        tsv2 = derive_tsv2(lifecycle.windows if lifecycle is not None else [])
        terminal = terminal_context(lifecycle)
        candidate_before_lifecycle = None
        if tsv2.first_candidate_age_ms is not None and terminal.get("actual_close_age_ms") is not None:
            candidate_before_lifecycle = tsv2.first_candidate_age_ms <= terminal["actual_close_age_ms"]

        pnl, pnl_source, pnl_tsv2, pnl_path = candidate_pnl(
            replay.row, tsv2.first_candidate_window, tsv2.first_candidate_age_ms
        )
        active_exit_eligible = (
            tsv2.has_candidate
            and candidate_before_lifecycle is True
            and tsv2.candidate_class != "stale_data_no_action"
        )
        baseline = simulate_baseline_cached(replay.row, target_bps, stop_bps, max_hold_ms)
        candidate_before_baseline = (
            baseline is not None
            and tsv2.first_candidate_age_ms is not None
            and tsv2.first_candidate_age_ms <= baseline.exit_age_ms
        )
        classification, delta_vs_baseline = actual_classification(
            active_exit_eligible,
            pnl,
            baseline,
            candidate_before_baseline,
            tsv2.candidate_class,
        )
        delta_vs_actual = (
            pnl - terminal["actual_final_pnl_bps"]
            if pnl is not None and terminal.get("actual_final_pnl_bps") is not None
            else None
        )
        path_stats = after_candidate_path_stats(replay.row, tsv2.first_candidate_age_ms, target_bps)
        resurrection = {
            f"alive_within_{window_ms}ms_after_candidate": alive_within(
                lifecycle.windows if lifecycle is not None else [],
                tsv2.first_candidate_age_ms,
                window_ms,
            )
            for window_ms in resurrection_windows_ms
        }
        record = {
            "schema": SCHEMA,
            "record_type": POSITION_RECORD,
            "run_id": replay.exact_key.run_id,
            "session_id": replay.exact_key.session_id,
            "pool_id": replay.exact_key.pool_id,
            "base_mint": replay.exact_key.base_mint,
            "entry_ts_ms": replay.exact_key.entry_ts_ms,
            "source_lifecycle": lifecycle.source_lifecycle if lifecycle is not None else None,
            "join_quality": join_quality,
            "has_exit_replay": True,
            "has_tsv2_windows": tsv2.has_windows,
            "tsv2_window_count": tsv2.window_count,
            "has_candidate": tsv2.has_candidate,
            "first_candidate_age_ms": tsv2.first_candidate_age_ms,
            "first_candidate_status": tsv2.first_candidate_status,
            "first_candidate_subreason": tsv2.first_candidate_subreason,
            "failed_windows_at_candidate": tsv2.failed_windows_at_candidate,
            "status_sequence_before_candidate": tsv2.status_sequence_before_candidate,
            "candidate_class": tsv2.candidate_class,
            "active_exit_eligible": active_exit_eligible,
            "candidate_pnl_bps": pnl,
            "candidate_pnl_source": pnl_source,
            "candidate_pnl_bps_from_tsv2_window": pnl_tsv2,
            "candidate_pnl_bps_from_path_prev": pnl_path,
            "actual_terminal_reason": terminal.get("actual_terminal_reason"),
            "actual_lifecycle_close_reason_raw": terminal.get("actual_lifecycle_close_reason_raw"),
            "actual_close_age_ms": terminal.get("actual_close_age_ms"),
            "actual_final_pnl_bps": terminal.get("actual_final_pnl_bps"),
            "actual_final_pnl_pct": terminal.get("actual_final_pnl_pct"),
            "baseline_target_bps": target_bps,
            "baseline_stop_bps": stop_bps,
            "baseline_max_hold_ms": max_hold_ms,
            "baseline_barrier_result": baseline.result if baseline else UNKNOWN,
            "baseline_barrier_exit_age_ms": baseline.exit_age_ms if baseline else None,
            "baseline_barrier_pnl_bps": baseline.pnl_bps if baseline else None,
            "baseline_result_quality": baseline.result_quality if baseline else "unavailable",
            "candidate_before_terminal": candidate_before_lifecycle,
            "candidate_before_baseline_exit": candidate_before_baseline,
            "delta_vs_actual_bps": delta_vs_actual,
            "delta_vs_baseline_bps": delta_vs_baseline,
            **path_stats,
            **resurrection,
            "actual_classification": classification,
            "_exit_replay_row": replay.row,
        }
        records.append(record)

    for lifecycle in lifecycle_positions:
        if id(lifecycle) in matched_lifecycle_ids:
            continue
        tsv2 = derive_tsv2(lifecycle.windows)
        terminal = terminal_context(lifecycle)
        records.append(
            {
                "schema": SCHEMA,
                "record_type": POSITION_RECORD,
                "run_id": lifecycle.run_id,
                "session_id": lifecycle.session_id,
                "pool_id": lifecycle.pool_id,
                "base_mint": lifecycle.base_mint,
                "entry_ts_ms": lifecycle.entry_ts_ms,
                "source_lifecycle": lifecycle.source_lifecycle,
                "join_quality": "lifecycle_without_exit_replay",
                "has_exit_replay": False,
                "has_tsv2_windows": tsv2.has_windows,
                "tsv2_window_count": tsv2.window_count,
                "has_candidate": tsv2.has_candidate,
                "first_candidate_age_ms": tsv2.first_candidate_age_ms,
                "first_candidate_status": tsv2.first_candidate_status,
                "first_candidate_subreason": tsv2.first_candidate_subreason,
                "failed_windows_at_candidate": tsv2.failed_windows_at_candidate,
                "status_sequence_before_candidate": tsv2.status_sequence_before_candidate,
                "candidate_class": tsv2.candidate_class,
                "active_exit_eligible": False,
                "candidate_pnl_bps": None,
                "candidate_pnl_source": "unavailable",
                "candidate_pnl_bps_from_tsv2_window": None,
                "candidate_pnl_bps_from_path_prev": None,
                **terminal,
                "baseline_target_bps": target_bps,
                "baseline_stop_bps": stop_bps,
                "baseline_max_hold_ms": max_hold_ms,
                "baseline_barrier_result": UNKNOWN,
                "baseline_barrier_exit_age_ms": None,
                "baseline_barrier_pnl_bps": None,
                "baseline_result_quality": "unavailable",
                "candidate_before_terminal": None,
                "candidate_before_baseline_exit": None,
                "delta_vs_actual_bps": None,
                "delta_vs_baseline_bps": None,
                "mfe_after_candidate_bps_path_approx": None,
                "mae_after_candidate_bps_path_approx": None,
                "target_after_candidate": None,
                **{f"alive_within_{window_ms}ms_after_candidate": None for window_ms in resurrection_windows_ms},
                "actual_classification": "no_exit_replay",
            }
        )

    records.sort(
        key=lambda row: (
            str(row.get("run_id") or ""),
            str(row.get("session_id") or ""),
            str(row.get("pool_id") or ""),
            str(row.get("base_mint") or ""),
            row.get("entry_ts_ms") if row.get("entry_ts_ms") is not None else 10**18,
            str(row.get("source_lifecycle") or ""),
        )
    )
    return records


def public_record(row: dict[str, Any]) -> dict[str, Any]:
    return {key: value for key, value in row.items() if not key.startswith("_")}


def recommendation(report: dict[str, Any]) -> str:
    coverage = report["coverage"]
    if coverage["positions_with_tsv2_windows"] == 0:
        return RECOMMEND_NO_WINDOWS
    if coverage["positions_with_tsv2_windows_rate"] < 0.2:
        return RECOMMEND_DATA_BLOCKED
    summary = report["actual_counterfactual_summary"]
    eligible = summary["active_exit_eligible_positions"]
    if eligible < 50:
        return RECOMMEND_NEEDS_MORE_DATA
    cut_rate = summary["targets_cut_by_tsv2"] / eligible if eligible else 0.0
    if cut_rate > 0.25:
        return RECOMMEND_TOO_MANY_CUTS
    if summary["delta_sum_bps"] <= 0:
        return RECOMMEND_NO_BENEFIT
    if summary["beneficial_exit_count"] > summary["harmful_exit_count"]:
        return RECOMMEND_PROMISING
    return RECOMMEND_NO_BENEFIT


def build_report(
    scope: str,
    input_paths: dict[str, str],
    records: list[dict[str, Any]],
    matrix: list[dict[str, Any]],
    join_quality: dict[str, Any],
    load_stats: list[LoadStats],
    target_bps: int,
    stop_bps: int,
    max_hold_ms: int,
    resurrection_windows_ms: list[int],
) -> dict[str, Any]:
    simulated_positions = len(records)
    exit_replay_positions = sum(1 for row in records if row.get("has_exit_replay"))
    with_windows = sum(1 for row in records if row.get("has_tsv2_windows"))
    candidates = sum(1 for row in records if row.get("has_candidate"))
    candidate_before_terminal = sum(1 for row in records if row.get("candidate_before_terminal") is True)
    stale_candidates = sum(1 for row in records if row.get("candidate_class") == "stale_data_no_action")
    status_counts = Counter()
    subreason_counts = Counter()
    class_counts = Counter(str(row.get("candidate_class") or "missing") for row in records)
    terminal_counts = Counter(str(row.get("actual_terminal_reason") or UNKNOWN) for row in records)
    classification_counts = Counter(str(row.get("actual_classification") or "missing") for row in records)
    deltas = [int(row["delta_vs_baseline_bps"]) for row in records if row.get("delta_vs_baseline_bps") is not None]
    for row in records:
        status = row.get("first_candidate_status")
        subreason = row.get("first_candidate_subreason")
        if status is not None:
            status_counts[str(status)] += 1
        if subreason is not None:
            subreason_counts[str(subreason)] += 1

    resurrection_summary: dict[str, Any] = {}
    for window_ms in resurrection_windows_ms:
        key = f"alive_within_{window_ms}ms_after_candidate"
        values = [row.get(key) for row in records if row.get("has_candidate")]
        total = len(values)
        alive = sum(1 for value in values if value is True)
        resurrection_summary[key] = {
            "candidate_rows": total,
            "alive_count": alive,
            "alive_rate": alive / total if total else 0.0,
        }

    report: dict[str, Any] = {
        "scope": scope,
        "generated_at": utc_now_iso(),
        "input_paths": input_paths,
        "default_policy": {
            "target_bps": target_bps,
            "stop_bps": stop_bps,
            "max_hold_ms": max_hold_ms,
        },
        "position_counts": {
            "simulated_positions": simulated_positions,
            "positions_with_exit_replay": exit_replay_positions,
            "lifecycle_only_positions": simulated_positions - exit_replay_positions,
        },
        "coverage": {
            "simulated_positions": simulated_positions,
            "positions_with_exit_replay": exit_replay_positions,
            "positions_with_tsv2_windows": with_windows,
            "positions_with_tsv2_windows_rate": with_windows / simulated_positions if simulated_positions else 0.0,
            "candidate_positions": candidates,
            "candidate_positions_rate_over_windows": candidates / with_windows if with_windows else 0.0,
            "candidate_before_terminal": candidate_before_terminal,
            "candidate_before_terminal_rate": candidate_before_terminal / candidates if candidates else 0.0,
            "stale_only_candidate_rate": stale_candidates / candidates if candidates else 0.0,
        },
        "join_quality": join_quality,
        "status_distribution": dict(status_counts),
        "subreason_distribution": dict(subreason_counts),
        "candidate_class_distribution": dict(class_counts),
        "candidate_before_terminal_counts": {
            "true": candidate_before_terminal,
            "false": sum(1 for row in records if row.get("candidate_before_terminal") is False),
            "unknown": sum(1 for row in records if row.get("candidate_before_terminal") is None),
        },
        "actual_terminal_reason_distribution": dict(terminal_counts),
        "actual_counterfactual_summary": {
            "active_exit_eligible_positions": sum(1 for row in records if row.get("active_exit_eligible")),
            "classification_counts": dict(classification_counts),
            "saved_stop_count": classification_counts["saved_stop"],
            "targets_cut_by_tsv2": classification_counts["cut_target"],
            "timeout_improved_count": classification_counts["timeout_improved"],
            "beneficial_exit_count": classification_counts["beneficial_exit"] + classification_counts["saved_stop"] + classification_counts["timeout_improved"],
            "harmful_exit_count": classification_counts["harmful_exit"] + classification_counts["cut_target"],
            "neutral_exit_count": classification_counts["neutral_exit"],
            "delta_sum_bps": sum(deltas),
            "delta_avg_bps": mean_int(deltas),
            "delta_median_bps": median_int(deltas),
        },
        "matrix_summary": matrix,
        "resurrection_summary": resurrection_summary,
        "data_quality_summary": {
            "load_stats": [
                {
                    "path": stat.path,
                    "rows": stat.rows,
                    "malformed_rows": stat.malformed_rows,
                    "malformed_examples": stat.malformed_examples,
                }
                for stat in load_stats
            ],
        },
    }
    report["recommendation"] = recommendation(report)
    return report


def write_json(path: Path, data: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_jsonl(path: Path, rows: Iterable[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(public_record(row), sort_keys=False))
            handle.write("\n")


def write_csv(path: Path, rows: list[dict[str, Any]], fieldnames: list[str] | None = None) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if fieldnames is None:
        fieldnames = []
        for row in rows:
            for key in row:
                if key not in fieldnames:
                    fieldnames.append(key)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames)
        writer.writeheader()
        for row in rows:
            writer.writerow(row)


def pct(value: float) -> str:
    return f"{value * 100.0:.2f}%"


def write_markdown(path: Path, report: dict[str, Any]) -> None:
    coverage = report["coverage"]
    join = report["join_quality"]
    summary = report["actual_counterfactual_summary"]
    matrix = report["matrix_summary"]
    lines = [
        "# TimeStop V2 Counterfactual Report",
        "",
        "This is observe-only counterfactual evidence. It is not active-exit proof and not a production promotion.",
        "",
        "## Scope and Inputs",
        f"- scope: `{report['scope']}`",
        f"- generated_at: `{report['generated_at']}`",
        f"- recommendation: `{report['recommendation']}`",
        "",
        "## Coverage",
        f"- simulated_positions: `{coverage['simulated_positions']}`",
        f"- positions_with_exit_replay: `{coverage['positions_with_exit_replay']}`",
        f"- positions_with_tsv2_windows: `{coverage['positions_with_tsv2_windows']}` ({pct(coverage['positions_with_tsv2_windows_rate'])})",
        f"- candidate_positions: `{coverage['candidate_positions']}`",
        f"- candidate_before_terminal: `{coverage['candidate_before_terminal']}`",
        f"- stale_only_candidate_rate: `{pct(coverage['stale_only_candidate_rate'])}`",
        "",
        "## Join Quality",
        f"- exact_join_count: `{join['exact_join_count']}`",
        f"- fallback_unique_join_count: `{join['fallback_unique_join_count']}`",
        f"- unmatched_exit_replay_count: `{join['unmatched_exit_replay_count']}`",
        f"- unmatched_lifecycle_position_count: `{join['unmatched_lifecycle_position_count']}`",
        f"- duplicate_fallback_key_count: `{join['duplicate_fallback_key_count']}`",
        "",
        "## TimeStop V2 Status Distribution",
        "```json",
        json.dumps(report["status_distribution"], indent=2, sort_keys=True),
        "```",
        "",
        "## Candidate Class Distribution",
        "```json",
        json.dumps(report["candidate_class_distribution"], indent=2, sort_keys=True),
        "```",
        "",
        "## Candidate Before Terminal Outcome",
        "```json",
        json.dumps(report["candidate_before_terminal_counts"], indent=2, sort_keys=True),
        "```",
        "",
        "## Counterfactual Economics vs Baseline Barrier",
        f"- active_exit_eligible_positions: `{summary['active_exit_eligible_positions']}`",
        f"- saved_stop_count: `{summary['saved_stop_count']}`",
        f"- targets_cut_by_tsv2: `{summary['targets_cut_by_tsv2']}`",
        f"- timeout_improved_count: `{summary['timeout_improved_count']}`",
        f"- delta_sum_bps: `{summary['delta_sum_bps']}`",
        f"- delta_avg_bps: `{summary['delta_avg_bps']:.2f}`",
        "",
        "## Matrix: Baseline vs With TimeStop V2",
        "| target_bps | stop_bps | max_hold_ms | total | baseline TARGET/STOP/TIMEOUT | TSV2 exits | pnl_delta_sum_bps | exact/path |",
        "|---:|---:|---:|---:|---|---:|---:|---|",
    ]
    for row in matrix:
        lines.append(
            "| {target_bps} | {stop_bps} | {max_hold_ms} | {total_positions} | "
            "{baseline_target_count}/{baseline_stop_count}/{baseline_timeout_count} | "
            "{tsv2_exit_count} | {pnl_delta_sum_bps} | {exact_rows}/{path_approx_rows} |".format(
                **row
            )
        )
    lines.extend(
        [
            "",
            "## False-Close Accounting",
            "```json",
            json.dumps(summary["classification_counts"], indent=2, sort_keys=True),
            "```",
            "",
            "## Resurrection Checks",
            "```json",
            json.dumps(report["resurrection_summary"], indent=2, sort_keys=True),
            "```",
            "",
            "## Stale/Missing-Data Safety",
            "- `stale_data_no_action` candidates are excluded from active-exit eligibility.",
            "- Missing or stale TimeStop V2 evidence can support data-quality diagnosis only.",
            "",
            "## Recommendation",
            f"`{report['recommendation']}`",
            "",
        ]
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines), encoding="utf-8")


def inspect_scope_coverage(root: Path, scope: str, resurrection_windows_ms: list[int]) -> dict[str, Any]:
    base = root / "logs" / "shadow_run" / scope
    paths = {
        "shadow_exit_replay": base / "shadow_exit_replay_v1.jsonl",
        "shadow_lifecycle": base / "shadow_lifecycle.jsonl",
        "probe_shadow_lifecycle": base / "probe_shadow_lifecycle.jsonl",
    }
    replay_positions, replay_stats = load_exit_replay_positions(paths["shadow_exit_replay"])
    lifecycle_positions, lifecycle_stats = load_lifecycle_positions(
        paths["shadow_lifecycle"],
        paths["probe_shadow_lifecycle"],
    )
    joined, join_quality = join_lifecycle(replay_positions, lifecycle_positions)
    records = build_position_records(
        replay_positions,
        lifecycle_positions,
        joined,
        6000,
        -6000,
        120000,
        resurrection_windows_ms,
    )
    return {
        "scope": scope,
        "input_paths": {key: str(value) for key, value in paths.items()},
        "positions": len(records),
        "positions_with_exit_replay": sum(1 for row in records if row.get("has_exit_replay")),
        "positions_with_tsv2_windows": sum(1 for row in records if row.get("has_tsv2_windows")),
        "candidate_positions": sum(1 for row in records if row.get("has_candidate")),
        "stale_data_no_action_candidates": sum(1 for row in records if row.get("candidate_class") == "stale_data_no_action"),
        "join_quality": join_quality,
        "load_stats": [
            {
                "path": stat.path,
                "rows": stat.rows,
                "malformed_rows": stat.malformed_rows,
            }
            for stat in [replay_stats, *lifecycle_stats]
        ],
    }


def row_for_cost(
    cost_rows: list[dict[str, Any]],
    target_bps: int,
    stop_bps: int,
    max_hold_ms: int,
    cost_bps: int,
) -> dict[str, Any] | None:
    for row in cost_rows:
        if (
            int(row["target_bps"]) == target_bps
            and int(row["stop_bps"]) == stop_bps
            and int(row["max_hold_ms"]) == max_hold_ms
            and int(row["roundtrip_cost_bps"]) == cost_bps
        ):
            return row
    return None


def rows_for_variant(rows: list[dict[str, Any]], target_bps: int, stop_bps: int, max_hold_ms: int) -> list[dict[str, Any]]:
    return [
        row for row in rows
        if int(row["target_bps"]) == target_bps
        and int(row["stop_bps"]) == stop_bps
        and int(row["max_hold_ms"]) == max_hold_ms
    ]


def evaluate_noharm_verdict(
    best: dict[str, Any] | None,
    cost_rows: list[dict[str, Any]],
    stability_rows: list[dict[str, Any]],
    neighborhood_rows: list[dict[str, Any]],
    coverage: dict[str, Any],
    negative_control: dict[str, Any] | None,
) -> tuple[str, list[str], list[str]]:
    blockers: list[str] = []
    shadow_close_blockers: list[str] = ["requires minimum two independent TSV2 scopes; only one full TSV2-window scope is available"]
    if best is None:
        return VERDICT_REJECTED_FOR_RUNTIME, ["no grid rows"], shadow_close_blockers

    target_bps = int(best["target_bps"])
    stop_bps = int(best["stop_bps"])
    max_hold_ms = int(best["max_hold_ms"])
    cost100 = row_for_cost(cost_rows, target_bps, stop_bps, max_hold_ms, 100)
    cost200 = row_for_cost(cost_rows, target_bps, stop_bps, max_hold_ms, 200)
    if coverage.get("positions_with_tsv2_windows", 0) <= 0:
        blockers.append("main scope has no TimeStop V2 windows")
    if coverage.get("positions_with_exit_replay", 0) <= 0:
        blockers.append("main scope has no exit replay rows")
    if negative_control and negative_control.get("positions_with_tsv2_windows", 0) != 0:
        blockers.append("R48/R2 negative control unexpectedly has TSV2 windows")
    if cost100 is None or cost200 is None:
        blockers.append("cost100/cost200 rows missing")
    else:
        if float(cost100["delta_sum_bps"]) <= 0:
            blockers.append("cost100_delta_sum_bps <= 0")
        if float(cost200["delta_sum_bps"]) <= 0:
            blockers.append("cost200_delta_sum_bps <= 0")
        if float(cost100["delta_avg_bps"]) <= 0:
            blockers.append("cost100_delta_avg_bps <= 0")
        if float(cost100["delta_median_bps"]) < 0:
            blockers.append("cost100_delta_median_bps < 0")
        if float(cost100["exit_action_precision"]) < 0.70:
            blockers.append("exit_action_precision < 0.70")
        if float(cost100["exit_action_precision_wilson95_lower"]) < 0.65:
            blockers.append("Wilson lower bound 95% < 0.65")
        if not bool(cost100["target_cut_damage_guard_pass"]):
            blockers.append("target_cut_damage_bps > 25% gross_saved_damage_bps")
        if not bool(cost100["target_cut_count_guard_pass"]):
            blockers.append("target_cut_count exceeds saved_stop_count + 10% timeout_improved_count")
        denominator = float(cost100["exit_action_precision_denominator"] or 0.0)
        stale_exclusions = float(cost100["stale_no_action_exclusions"] or 0.0)
        ambiguous = float(cost100["ambiguous_unjoined_exclusions"] or 0.0)
        if denominator and stale_exclusions / denominator > 0.05:
            blockers.append("stale/no-action exclusions are too large relative to precision denominator")
        if ambiguous > 0:
            blockers.append("ambiguous/unjoined exclusions are non-zero")

    selected_stability = rows_for_variant(stability_rows, target_bps, stop_bps, max_hold_ms)
    positive_segment_sum = 0.0
    total_positive_delta = 0.0
    max_segment_positive = 0.0
    for row in selected_stability:
        segment = row["segment"]
        delta_sum = float(row["delta_sum_bps"])
        precision = float(row["exit_action_precision"])
        harmful = int(row["harmful_exit_count"])
        if precision < 0.60:
            blockers.append(f"{segment}: action precision < 0.60")
        if delta_sum <= 0:
            blockers.append(f"{segment}: delta_sum_bps <= 0")
        if harmful <= 0:
            pass
        positive_segment_sum += max(0.0, delta_sum)
        max_segment_positive = max(max_segment_positive, max(0.0, delta_sum))
    if cost100 is not None:
        total_positive_delta = max(0.0, float(cost100["delta_sum_bps"]))
    if total_positive_delta > 0 and max_segment_positive / total_positive_delta > 0.60:
        blockers.append("one chronological tercile contributes >60% of total positive delta")
    if positive_segment_sum <= 0:
        blockers.append("no positive chronological segment contribution")

    if neighborhood_rows and not all(bool(row.get("positive_delta")) for row in neighborhood_rows):
        blockers.append("grid-neighborhood contains non-positive adjacent variants")
    if not neighborhood_rows:
        blockers.append("grid-neighborhood rows missing")

    if blockers:
        if cost100 is None or float(cost100.get("delta_sum_bps", 0.0)) <= 0:
            return VERDICT_REJECTED_FOR_RUNTIME, blockers, shadow_close_blockers
        return VERDICT_INCONCLUSIVE_RESEARCH, blockers, shadow_close_blockers
    return VERDICT_PROMISING_OFFLINE_ONLY, blockers, shadow_close_blockers


def markdown_table(rows: list[dict[str, Any]], columns: list[str], limit: int | None = None) -> str:
    selected = rows[:limit] if limit is not None else rows
    lines = ["| " + " | ".join(columns) + " |", "| " + " | ".join("---" for _ in columns) + " |"]
    for row in selected:
        values = []
        for column in columns:
            value = row.get(column, "")
            if isinstance(value, float):
                value = f"{value:.6g}"
            values.append(str(value))
        lines.append("| " + " | ".join(values) + " |")
    return "\n".join(lines)


def write_noharm_markdown(
    path: Path,
    *,
    scope: str,
    input_paths: dict[str, str],
    coverage: dict[str, Any],
    negative_control: dict[str, Any] | None,
    resurrection_summary: dict[str, Any],
    best: dict[str, Any] | None,
    cost_rows: list[dict[str, Any]],
    stability_rows: list[dict[str, Any]],
    neighborhood_rows: list[dict[str, Any]],
    verdict_value: str,
    blockers: list[str],
    shadow_close_blockers: list[str],
    output_files: dict[str, Path],
) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    display_verdict = verdict_value
    if verdict_value == VERDICT_INCONCLUSIVE_RESEARCH:
        display_verdict = "INCONCLUSIVE_RESEARCH / REJECTED_FOR_RUNTIME"
    lines = [
        "# TimeStop V2 No-Harm / Action-Precision Proof A1",
        "",
        f"Generated UTC: `{utc_now_iso()}`",
        f"Scope: `{scope}`",
        f"Final verdict: `{display_verdict}`",
        "No basis for runtime change.",
        "No basis for shadow_close_only plan.",
        "Positive action precision is blocked by target-cut guard.",
        "",
        "## PR-ORG-A0 Closure",
        "",
        "`DONE / REJECTED_FOR_RUNTIME / INCONCLUSIVE_RESEARCH / KEEP_AS_NEGATIVE_EVIDENCE`",
        "",
        "Do not continue ORG-A0 as PR-ORG-A0b, C6/C7/C8, more R48/R2 threshold tuning, organic hard gates, selector reranker, `alpha_31100`, XGBoost, Gatekeeper BUY/REJECT change, or `shadow_close_only` based on ORG-A0.",
        "",
        "Reason: ORG-A0 showed that the F5/C1 positive avg came from a sparse right tail, not a stable organic edge. After removing top 5%, S1_F5 and C1 are negative; C1 does not beat F5 on holdout; C2-C5 have 0% Target on holdout; all cost-adjusted medians are negative.",
        "",
        "## Scope Boundaries",
        "",
        "- Offline/read-only proof only.",
        "- No Gatekeeper runtime change.",
        "- No BUY/REJECT change.",
        "- No `v25_confidence`, V3, selector runtime, `alpha_31100`, TX builder, sender, Jito path, live execution, or existing log mutation.",
        "- This proof evaluates only `exit_action_precision = beneficial_exit / (beneficial_exit + harmful_exit)`.",
        "- It does not use or report entry target precision as an acceptance metric.",
        "",
        "## Inputs",
        "",
        "```json",
        json.dumps(input_paths, indent=2, sort_keys=True),
        "```",
        "",
        "## Coverage and Join Quality",
        "",
        "```json",
        json.dumps(coverage, indent=2, sort_keys=True, default=str),
        "```",
        "",
        "## R48/R2 Negative Coverage Control",
        "",
        "R48/R2 is used only as a no-window coverage control.",
        "",
        "```json",
        json.dumps(negative_control or {}, indent=2, sort_keys=True, default=str),
        "```",
        "",
        "## Resurrection Checks",
        "",
        "```json",
        json.dumps(resurrection_summary, indent=2, sort_keys=True, default=str),
        "```",
        "",
    ]
    if best is None:
        lines.extend(["## Best Variant", "", "No grid variant available.", ""])
    else:
        target_bps = int(best["target_bps"])
        stop_bps = int(best["stop_bps"])
        max_hold_ms = int(best["max_hold_ms"])
        selected_costs = [
            row for row in cost_rows
            if int(row["target_bps"]) == target_bps
            and int(row["stop_bps"]) == stop_bps
            and int(row["max_hold_ms"]) == max_hold_ms
        ]
        selected_stability = [
            row for row in stability_rows
            if int(row["target_bps"]) == target_bps
            and int(row["stop_bps"]) == stop_bps
            and int(row["max_hold_ms"]) == max_hold_ms
        ]
        lines.extend(
            [
                "## Best Variant",
                "",
                f"- target_bps: `{target_bps}`",
                f"- stop_bps: `{stop_bps}`",
                f"- max_hold_ms: `{max_hold_ms}`",
                f"- selection: max `cost100_delta_sum_bps`, then Wilson lower bound, action precision, and lower target-cut damage.",
                "",
                "## Cost Sensitivity for Best Variant",
                "",
                markdown_table(
                    selected_costs,
                    [
                        "roundtrip_cost_bps",
                        "supported_rows",
                        "action_taken_count",
                        "delta_sum_bps",
                        "delta_avg_bps",
                        "delta_median_bps",
                        "exit_action_precision",
                        "exit_action_precision_wilson95_lower",
                        "beneficial_exit_count",
                        "harmful_exit_count",
                        "target_cut_count",
                        "target_cut_damage_bps",
                        "saved_stop_count",
                        "saved_stop_damage_bps",
                        "timeout_improved_count",
                        "timeout_improved_bps",
                        "stale_no_action_exclusions",
                        "no_candidate_exclusions",
                        "ambiguous_unjoined_exclusions",
                        "exact_rows",
                        "path_approx_rows",
                    ],
                ),
                "",
                "## Chronological Stability for Best Variant",
                "",
                markdown_table(
                    selected_stability,
                    [
                        "segment",
                        "supported_rows",
                        "action_taken_count",
                        "delta_sum_bps",
                        "delta_avg_bps",
                        "exit_action_precision",
                        "exit_action_precision_wilson95_lower",
                        "beneficial_exit_count",
                        "harmful_exit_count",
                        "max_consecutive_harmful_actions",
                    ],
                ),
                "",
                "## Grid-Neighborhood Stability",
                "",
                markdown_table(
                    neighborhood_rows,
                    [
                        "target_bps",
                        "stop_bps",
                        "max_hold_ms",
                        "is_best",
                        "cost100_delta_sum_bps",
                        "cost100_delta_avg_bps",
                        "cost100_exit_action_precision",
                        "cost100_exit_action_precision_wilson95_lower",
                        "positive_delta",
                    ],
                ),
                "",
            ]
        )
    lines.extend(
        [
            "## Verdict",
            "",
            f"`{display_verdict}`",
            "",
            "No basis for runtime change.",
            "No basis for shadow_close_only plan.",
            "Positive action precision is blocked by target-cut guard.",
            "",
            "Runtime blockers:",
        ]
    )
    if blockers:
        lines.extend(f"- {blocker}" for blocker in blockers)
    else:
        lines.append("- none for `PROMISING_OFFLINE_ONLY`; this is still not runtime approval.")
    lines.extend(
        [
            "",
            "Shadow-close-only blockers:",
        ]
    )
    lines.extend(f"- {blocker}" for blocker in shadow_close_blockers)
    lines.extend(
        [
            "",
            "## Output Files",
            "",
            markdown_table(
                [{"artifact": key, "path": str(value)} for key, value in output_files.items()],
                ["artifact", "path"],
            ),
            "",
        ]
    )
    path.write_text("\n".join(lines), encoding="utf-8")


def resolve_paths(args: argparse.Namespace) -> tuple[str, dict[str, Path], Path]:
    root = args.root.resolve()
    scope = args.scope or "explicit_paths"
    base = root / "logs" / "shadow_run" / scope
    paths = {
        "shadow_exit_replay": args.shadow_exit_replay or base / "shadow_exit_replay_v1.jsonl",
        "shadow_lifecycle": args.shadow_lifecycle or base / "shadow_lifecycle.jsonl",
        "probe_shadow_lifecycle": args.probe_shadow_lifecycle or base / "probe_shadow_lifecycle.jsonl",
    }
    output_dir = args.output_dir or root / "reports" / "selector" / scope
    return scope, paths, output_dir


def run_lab(args: argparse.Namespace) -> dict[str, Any]:
    scope, paths, output_dir = resolve_paths(args)
    replay_positions, replay_stats = load_exit_replay_positions(paths["shadow_exit_replay"])
    lifecycle_positions, lifecycle_stats = load_lifecycle_positions(
        paths["shadow_lifecycle"], paths.get("probe_shadow_lifecycle")
    )
    joined, join_quality = join_lifecycle(replay_positions, lifecycle_positions)

    target_bps_values = args.targets_bps or [args.target_bps]
    stop_bps_values = args.stops_bps or [args.stop_bps]
    max_hold_values = args.max_hold_ms
    default_target = target_bps_values[0]
    default_stop = stop_bps_values[0]
    default_max_hold = max_hold_values[0]
    records = build_position_records(
        replay_positions,
        lifecycle_positions,
        joined,
        default_target,
        default_stop,
        default_max_hold,
        args.resurrection_windows_ms,
    )
    assign_chronological_terciles(records)
    matrix = [
        matrix_row(records, target_bps, stop_bps, max_hold_ms)
        for target_bps in target_bps_values
        for stop_bps in stop_bps_values
        for max_hold_ms in max_hold_values
    ]
    report = build_report(
        scope,
        {name: str(path) for name, path in paths.items()},
        records,
        matrix,
        join_quality,
        [replay_stats, *lifecycle_stats],
        default_target,
        default_stop,
        default_max_hold,
        args.resurrection_windows_ms,
    )
    noharm_summary_rows, noharm_cost_rows, noharm_stability_rows = build_noharm_tables(
        records,
        target_bps_values,
        stop_bps_values,
        max_hold_values,
        args.roundtrip_cost_bps,
    )
    noharm_best = choose_noharm_best(noharm_summary_rows)
    noharm_neighborhood_rows = build_grid_neighborhood(
        noharm_summary_rows,
        target_bps_values,
        stop_bps_values,
        max_hold_values,
        noharm_best,
    )
    negative_control = (
        inspect_scope_coverage(args.root.resolve(), args.negative_control_scope, args.resurrection_windows_ms)
        if args.negative_control_scope
        else None
    )
    noharm_coverage = {
        "scope": scope,
        "positions": report["coverage"]["simulated_positions"],
        "positions_with_exit_replay": report["coverage"]["positions_with_exit_replay"],
        "positions_with_tsv2_windows": report["coverage"]["positions_with_tsv2_windows"],
        "candidate_positions": report["coverage"]["candidate_positions"],
        "stale_data_no_action_candidates": sum(1 for row in records if row.get("candidate_class") == "stale_data_no_action"),
        "join_quality": join_quality,
        "exact_join_rate_over_exit_replay": safe_div(
            float(join_quality.get("exact_join_count", 0)),
            float(report["coverage"]["positions_with_exit_replay"]),
        ),
    }
    noharm_verdict, noharm_blockers, shadow_close_blockers = evaluate_noharm_verdict(
        noharm_best,
        noharm_cost_rows,
        noharm_stability_rows,
        noharm_neighborhood_rows,
        noharm_coverage,
        negative_control,
    )
    report["noharm_proof_a1"] = {
        "verdict": noharm_verdict,
        "blockers": noharm_blockers,
        "shadow_close_blockers": shadow_close_blockers,
        "best_variant": noharm_best,
        "negative_control": negative_control,
    }
    write_jsonl(output_dir / "time_stop_v2_counterfactual_exit_v1.jsonl", records)
    write_json(output_dir / "time_stop_v2_counterfactual_report_v1.json", report)
    write_markdown(output_dir / "TIME_STOP_V2_COUNTERFACTUAL_REPORT.md", report)
    noharm_output_files = {
        "summary": output_dir / "time_stop_v2_noharm_summary_v1.csv",
        "cost_sensitivity": output_dir / "time_stop_v2_noharm_cost_sensitivity_v1.csv",
        "stability": output_dir / "time_stop_v2_noharm_stability_v1.csv",
        "grid_neighborhood": output_dir / "time_stop_v2_noharm_grid_neighborhood_v1.csv",
        "report": output_dir / "TIME_STOP_V2_NOHARM_PROOF_A1.md",
    }
    write_csv(noharm_output_files["summary"], noharm_summary_rows)
    write_csv(noharm_output_files["cost_sensitivity"], noharm_cost_rows)
    write_csv(noharm_output_files["stability"], noharm_stability_rows)
    write_csv(noharm_output_files["grid_neighborhood"], noharm_neighborhood_rows)
    write_noharm_markdown(
        noharm_output_files["report"],
        scope=scope,
        input_paths={name: str(path) for name, path in paths.items()},
        coverage=noharm_coverage,
        negative_control=negative_control,
        resurrection_summary=report["resurrection_summary"],
        best=noharm_best,
        cost_rows=noharm_cost_rows,
        stability_rows=noharm_stability_rows,
        neighborhood_rows=noharm_neighborhood_rows,
        verdict_value=noharm_verdict,
        blockers=noharm_blockers,
        shadow_close_blockers=shadow_close_blockers,
        output_files=noharm_output_files,
    )
    return report


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--scope")
    parser.add_argument("--shadow-exit-replay", type=Path)
    parser.add_argument("--shadow-lifecycle", type=Path)
    parser.add_argument("--probe-shadow-lifecycle", type=Path)
    parser.add_argument("--output-dir", type=Path)
    parser.add_argument("--negative-control-scope", default=DEFAULT_NEGATIVE_CONTROL_SCOPE)
    target_group = parser.add_mutually_exclusive_group(required=True)
    target_group.add_argument("--target-bps", type=int)
    target_group.add_argument("--targets-bps", type=parse_int_list)
    stop_group = parser.add_mutually_exclusive_group(required=True)
    stop_group.add_argument("--stop-bps", type=int)
    stop_group.add_argument("--stops-bps", type=parse_int_list)
    parser.add_argument("--max-hold-ms", type=parse_int_list, required=True)
    parser.add_argument("--roundtrip-cost-bps", type=parse_int_list, default=DEFAULT_COST_BPS)
    parser.add_argument("--resurrection-windows-ms", type=parse_int_list, default=[4000, 8000, 12000])
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = run_lab(args)
    print(
        json.dumps(
            {
                "scope": report["scope"],
                "recommendation": report["recommendation"],
                "positions_with_exit_replay": report["coverage"]["positions_with_exit_replay"],
                "positions_with_tsv2_windows": report["coverage"]["positions_with_tsv2_windows"],
                "candidate_positions": report["coverage"]["candidate_positions"],
                "noharm_verdict": report.get("noharm_proof_a1", {}).get("verdict"),
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
