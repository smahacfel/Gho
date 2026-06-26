#!/usr/bin/env python3
"""Offline proof for organic pool candidate policy A0.

This script is intentionally research-only. It reads existing decision,
selector, lifecycle, and exit-replay artifacts, then writes derived reports.
It must not be imported by runtime decision code.
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


DEFAULT_SCOPE = "shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2"
DEFAULT_CONFIG_HASH = "8b506cc2b631260ea2f828e5fe1dc15b58c79efa2e4ce7a3cca675e057d87051"
DEFAULT_DECISION_LANE = "v2.2/legacy_live"
DEFAULT_OUTPUT_DATE = "20260626"

MATRIX_TARGET = "TARGET"
MATRIX_STOP = "STOP"
MATRIX_TIMEOUT = "TIMEOUT"
MATRIX_UNAVAILABLE = "UNAVAILABLE"
EXACT_LEVELS = "exact_levels"
PATH_PREV_TIMEOUT = "path_prev_timeout"

DEFAULT_TARGETS_BPS = [100, 200, 300, 400, 500, 700, 1000, 1500, 2000, 3000, 5000, 6000, 7500, 10000]
DEFAULT_STOPS_BPS = [-100, -200, -300, -500, -700, -1000, -1500, -2000, -3000, -5000, -6000]
DEFAULT_MAX_HOLD_MS = [10000, 15000, 20000, 30000, 40000, 60000, 90000, 120000]
DEFAULT_ROUNDTRIP_COST_BPS = [0, 50, 100, 150, 200]
PRIMARY_TARGET_BPS = 6000
PRIMARY_STOP_BPS = -6000
PRIMARY_MAX_HOLD_MS = 120000
SELECTION_COST_BPS = 100

PROFILE_QUANTILES = {
    "loose": {"cap": 0.85, "floor": 0.15},
    "medium": {"cap": 0.75, "floor": 0.25},
    "strict": {"cap": 0.65, "floor": 0.35},
}

OPTIONAL_FIELD_MIN_COVERAGE = 0.80
MIN_INTERESTING_RETAINED_COUNT = 100

FORBIDDEN_INPUT_SUBSTRINGS = (
    "exit",
    "final",
    "pnl",
    "profit",
    "loss",
    "target",
    "stop",
    "timeout",
    "simulation",
    "eval",
    "result",
    "future",
    "after",
    "entry_price",
    "exit_price",
    "mint",
    "pool_id",
    "join_key",
    "timestamp",
    "ts_ms",
    "verdict",
    "confidence",
    "v25_confidence",
    "v3_confidence",
)


@dataclass(frozen=True)
class FieldSpec:
    name: str
    family: str
    paths: tuple[tuple[str, ...], ...]
    decision_time_safe: bool
    used_by_ladder: bool
    note: str


FIELD_SPECS: tuple[FieldSpec, ...] = (
    FieldSpec("buy_count", "traction", (("buy_count",), ("materialized_feature_snapshot", "tx_intel_features", "buy_count"), ("v3_materialized_feature_snapshot", "tx_intel_features", "buy_count")), True, True, "top-level Gatekeeper decision row / MFS tx_intel"),
    FieldSpec("total_tx", "traction", (("total_tx",), ("materialized_feature_snapshot", "tx_intel_features", "tx_count"), ("v3_materialized_feature_snapshot", "tx_intel_features", "tx_count")), True, False, "mapped from tx_intel_features.tx_count when top-level total_tx is absent"),
    FieldSpec("total_volume_sol", "traction", (("total_volume_sol",), ("materialized_feature_snapshot", "tx_intel_features", "total_volume_sol"), ("v3_materialized_feature_snapshot", "tx_intel_features", "total_volume_sol")), True, False, "top-level / MFS tx_intel"),
    FieldSpec("sol_buy_ratio", "traction", (("sol_buy_ratio",), ("materialized_feature_snapshot", "tx_intel_features", "sol_buy_ratio"), ("v3_materialized_feature_snapshot", "tx_intel_features", "sol_buy_ratio")), True, True, "top-level / MFS tx_intel"),
    FieldSpec("buy_ratio", "traction", (("buy_ratio",), ("materialized_feature_snapshot", "tx_intel_features", "buy_ratio"), ("v3_materialized_feature_snapshot", "tx_intel_features", "buy_ratio")), True, False, "top-level / MFS tx_intel"),
    FieldSpec("current_market_cap_sol", "overextension", (("current_market_cap_sol",), ("materialized_feature_snapshot", "account_features", "market_cap_sol"), ("v3_materialized_feature_snapshot", "account_features", "market_cap_sol")), True, True, "decision snapshot account market cap"),
    FieldSpec("bonding_progress_pct", "overextension", (("bonding_progress_pct",), ("materialized_feature_snapshot", "account_features", "bonding_progress"), ("v3_materialized_feature_snapshot", "account_features", "bonding_progress")), True, True, "decision snapshot bonding progress"),
    FieldSpec("price_change_ratio", "overextension", (("price_change_ratio",),), True, True, "top-level Gatekeeper decision row"),
    FieldSpec("max_single_tx_price_impact_pct_observed", "overextension", (("max_single_tx_price_impact_pct_observed",), ("materialized_feature_snapshot", "checkpoint_features", "single_tx_max_price_impact_pct"), ("v3_materialized_feature_snapshot", "checkpoint_features", "single_tx_max_price_impact_pct")), True, True, "observed pre-entry price impact cap"),
    FieldSpec("entry_drift_pct", "overextension", (("pdd_entry_drift_pct",),), True, False, "inventory only when top-level PDD drift exists"),
    FieldSpec("unique_ratio", "organicity", (("unique_ratio",), ("materialized_feature_snapshot", "tx_intel_features", "unique_signer_ratio"), ("v3_materialized_feature_snapshot", "tx_intel_features", "unique_signer_ratio")), True, True, "top-level or tx_intel unique signer ratio"),
    FieldSpec("unique_signers", "organicity", (("unique_signers",), ("materialized_feature_snapshot", "tx_intel_features", "unique_signers"), ("v3_materialized_feature_snapshot", "tx_intel_features", "unique_signers"), ("materialized_feature_snapshot", "organic_broadening", "total_unique_signers"), ("v3_materialized_feature_snapshot", "organic_broadening", "total_unique_signers")), True, False, "inventory signer broadening"),
    FieldSpec("hhi", "organicity", (("hhi",), ("materialized_feature_snapshot", "tx_intel_features", "hhi"), ("v3_materialized_feature_snapshot", "tx_intel_features", "hhi")), True, True, "top-level / tx_intel concentration"),
    FieldSpec("top3_signer_volume_ratio", "organicity", (("top3_signer_volume_ratio",), ("materialized_feature_snapshot", "tx_intel_features", "top3_signer_volume_ratio"), ("v3_materialized_feature_snapshot", "tx_intel_features", "top3_signer_volume_ratio")), True, True, "preferred PR4 ratio-scale field"),
    FieldSpec("top3_volume_pct", "organicity", (("top3_volume_pct",), ("materialized_feature_snapshot", "tx_intel_features", "top3_volume_pct"), ("v3_materialized_feature_snapshot", "tx_intel_features", "top3_volume_pct")), True, False, "legacy compatibility field; scale audited before use"),
    FieldSpec("same_ms_tx_ratio", "organicity", (("same_ms_tx_ratio",), ("materialized_feature_snapshot", "tx_intel_features", "same_ms_tx_ratio"), ("v3_materialized_feature_snapshot", "tx_intel_features", "same_ms_tx_ratio")), True, False, "top-level / tx_intel timing concentration"),
    FieldSpec("avg_cpi_depth_50tx", "execution_toxicity", (("avg_cpi_depth_50tx",), ("materialized_feature_snapshot", "alpha_fingerprint", "avg_cpi_depth_50tx"), ("v3_materialized_feature_snapshot", "alpha_fingerprint", "avg_cpi_depth_50tx")), True, True, "pre-entry alpha fingerprint diagnostic field, not alpha_31100"),
    FieldSpec("compute_unit_cluster_dominance", "execution_toxicity", (("compute_unit_cluster_dominance",), ("materialized_feature_snapshot", "alpha_fingerprint", "compute_unit_cluster_dominance"), ("v3_materialized_feature_snapshot", "alpha_fingerprint", "compute_unit_cluster_dominance")), True, True, "optional toxicity cap when coverage is adequate"),
    FieldSpec("jito_tip_intensity", "execution_toxicity", (("jito_tip_intensity",), ("materialized_feature_snapshot", "alpha_fingerprint", "jito_tip_intensity"), ("v3_materialized_feature_snapshot", "alpha_fingerprint", "jito_tip_intensity")), True, False, "inventory only; not used in A0 ladder"),
    FieldSpec("burst_ratio", "execution_toxicity", (("burst_ratio",), ("materialized_feature_snapshot", "tx_intel_features", "burst_ratio"), ("v3_materialized_feature_snapshot", "tx_intel_features", "burst_ratio")), True, False, "inventory only; not used in A0 ladder"),
    FieldSpec("max_single_sell_impact_pct_observed", "execution_toxicity", (("max_single_sell_impact_pct_observed",), ("materialized_feature_snapshot", "checkpoint_features", "max_single_sell_impact_pct"), ("v3_materialized_feature_snapshot", "checkpoint_features", "max_single_sell_impact_pct")), True, False, "inventory toxicity field"),
    FieldSpec("dev_tx_ratio", "dev_cross_pool_guard", (("dev_tx_ratio",), ("materialized_feature_snapshot", "tx_intel_features", "dev_tx_ratio"), ("v3_materialized_feature_snapshot", "tx_intel_features", "dev_tx_ratio")), True, True, "optional C5 guard"),
    FieldSpec("dev_volume_ratio", "dev_cross_pool_guard", (("dev_volume_ratio",), ("materialized_feature_snapshot", "tx_intel_features", "dev_volume_ratio"), ("v3_materialized_feature_snapshot", "tx_intel_features", "dev_volume_ratio")), True, True, "optional C5 guard"),
    FieldSpec("signer_cross_pool_velocity", "dev_cross_pool_guard", (("signer_cross_pool_velocity",), ("materialized_feature_snapshot", "sybil_resistance", "signer_cross_pool_velocity"), ("v3_materialized_feature_snapshot", "sybil_resistance", "signer_cross_pool_velocity")), True, True, "optional C5 guard when coverage is adequate"),
    FieldSpec("cpv_other_pool_activity", "dev_cross_pool_guard", (("cpv_other_pool_activity",), ("materialized_feature_snapshot", "sybil_resistance", "cpv_other_pool_activity"), ("v3_materialized_feature_snapshot", "sybil_resistance", "cpv_other_pool_activity")), True, True, "optional C5 guard when coverage is adequate"),
    FieldSpec("flipper_presence_ratio", "dev_cross_pool_guard", (("flipper_presence_ratio",), ("materialized_feature_snapshot", "alpha_fingerprint", "flipper_presence_ratio"), ("v3_materialized_feature_snapshot", "alpha_fingerprint", "flipper_presence_ratio")), True, True, "optional C5 guard"),
)


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
    levels_bps: frozenset[int]
    first_hit_ms: dict[int, int]
    path_bps: tuple[tuple[int, int], ...]


@dataclass(frozen=True)
class CellOutcome:
    label: str
    pnl_bps: int | None
    source: str


@dataclass
class CandidateRow:
    replay: ReplayRecord
    decision: dict[str, Any]
    features: dict[str, float | None]
    sources: dict[str, str]
    selector_shadow_score: float | None
    split: str = ""
    timeout_pnl_by_hold: dict[int, int | None] = field(default_factory=dict)


def parse_bps_list(raw: str) -> list[int]:
    output: list[int] = []
    for item in raw.split(","):
        item = item.strip()
        if item:
            output.append(int(item))
    if not output:
        raise argparse.ArgumentTypeError("empty bps list")
    return output


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Offline organic pool candidate policy A0 proof.")
    parser.add_argument("--scope", default=DEFAULT_SCOPE)
    parser.add_argument("--config-hash", default=DEFAULT_CONFIG_HASH)
    parser.add_argument("--decision-lane", default=DEFAULT_DECISION_LANE)
    parser.add_argument("--decision-log", type=Path)
    parser.add_argument("--selector-score", type=Path)
    parser.add_argument("--shadow-lifecycle", type=Path)
    parser.add_argument("--probe-lifecycle", type=Path)
    parser.add_argument("--exit-replay", type=Path)
    parser.add_argument("--output-dir", type=Path)
    parser.add_argument("--report-path", type=Path, default=Path(f"PLANS/AUDYT/RAPORT_ORGANIC_POOL_CANDIDATE_POLICY_A0_{DEFAULT_OUTPUT_DATE}.md"))
    parser.add_argument("--profile", choices=sorted(PROFILE_QUANTILES), default="medium")
    parser.add_argument("--targets-bps", type=parse_bps_list, default=DEFAULT_TARGETS_BPS)
    parser.add_argument("--stops-bps", type=parse_bps_list, default=DEFAULT_STOPS_BPS)
    parser.add_argument("--max-hold-ms", type=parse_bps_list, default=DEFAULT_MAX_HOLD_MS)
    parser.add_argument("--roundtrip-cost-bps", type=parse_bps_list, default=DEFAULT_ROUNDTRIP_COST_BPS)
    return parser.parse_args()


def resolve_paths(args: argparse.Namespace) -> dict[str, Path]:
    decision_base = Path("logs/rollout") / args.scope / "decisions" / args.scope / args.decision_lane / args.config_hash
    shadow_base = Path("logs/shadow_run") / args.scope
    output_dir = args.output_dir or Path("reports/selector") / args.scope
    return {
        "decision_log": args.decision_log or decision_base / "gatekeeper_v2_decisions.jsonl",
        "selector_score": args.selector_score or decision_base / "selector_shadow_score_v1.jsonl",
        "shadow_lifecycle": args.shadow_lifecycle or shadow_base / "shadow_lifecycle.jsonl",
        "probe_lifecycle": args.probe_lifecycle or shadow_base / "probe_shadow_lifecycle.jsonl",
        "exit_replay": args.exit_replay or shadow_base / "shadow_exit_replay_v1.jsonl",
        "output_dir": output_dir,
        "report_path": args.report_path,
    }


def utc_now_iso() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def finite_float(value: Any) -> float | None:
    if isinstance(value, bool) or value is None:
        return None
    if isinstance(value, (int, float)):
        value = float(value)
        return value if math.isfinite(value) else None
    if isinstance(value, str):
        try:
            value_f = float(value)
        except ValueError:
            return None
        return value_f if math.isfinite(value_f) else None
    return None


def finite_int(value: Any) -> int | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value
    if isinstance(value, float) and value.is_integer():
        return int(value)
    return None


def json_records_from_line(line: str) -> Iterable[dict[str, Any]]:
    stripped = line.strip()
    if not stripped:
        return
    try:
        row = json.loads(stripped)
    except json.JSONDecodeError:
        decoder = json.JSONDecoder()
        index = 0
        while index < len(stripped):
            row, next_index = decoder.raw_decode(stripped, index)
            if isinstance(row, dict):
                yield row
            index = next_index
            while index < len(stripped) and stripped[index].isspace():
                index += 1
        return
    if isinstance(row, dict):
        yield row


def iter_jsonl(path: Path) -> Iterable[dict[str, Any]]:
    with path.open(encoding="utf-8") as handle:
        for line in handle:
            yield from json_records_from_line(line)


def nested_get(row: dict[str, Any], path: tuple[str, ...]) -> Any:
    value: Any = row
    for key in path:
        if not isinstance(value, dict):
            return None
        value = value.get(key)
    return value


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
    return tuple(points) if points else None


def load_exit_replay(path: Path) -> tuple[list[ReplayRecord], dict[str, Any]]:
    controls: dict[str, Any] = {
        "total_records": 0,
        "qualified_records": 0,
        "damage_reasons": Counter(),
        "quality_counts": Counter(),
        "truncated_counts": Counter(),
        "horizon_ms_counts": Counter(),
        "duplicate_keys": 0,
    }
    records: list[ReplayRecord] = []
    key_counts: Counter[tuple[str, str, str, str, int]] = Counter()
    for line_no, row in enumerate(iter_jsonl(path), start=1):
        controls["total_records"] += 1
        quality = str(row.get("quality") or "missing")
        truncated = row.get("truncated")
        horizon_ms = finite_int(row.get("horizon_ms"))
        controls["quality_counts"][quality] += 1
        controls["truncated_counts"][str(truncated)] += 1
        if horizon_ms is not None:
            controls["horizon_ms_counts"][horizon_ms] += 1

        levels = parse_levels(row.get("levels_bps"))
        first_hit = parse_first_hit(row.get("first_hit_ms"))
        path_bps = parse_path_bps(row.get("path_bps"))
        entry_ts_ms = finite_int(row.get("entry_ts_ms"))
        run_id = str(row.get("run_id") or "")
        session_id = str(row.get("session_id") or "")
        pool_id = str(row.get("pool_id") or "")
        base_mint = str(row.get("base_mint") or "")
        key = (run_id, session_id, pool_id, base_mint, entry_ts_ms or -1)
        key_counts[key] += 1

        damaged = False
        checks = {
            "schema_not_shadow_exit_replay_v1": row.get("schema") != "shadow_exit_replay_v1",
            f"quality:{quality}": quality != "clean",
            f"truncated:{truncated}": truncated is not False,
            "invalid_levels_bps": levels is None,
            "invalid_first_hit_ms": first_hit is None,
            "invalid_path_bps": path_bps is None,
            "invalid_entry_ts_ms": entry_ts_ms is None,
            "invalid_horizon_ms": horizon_ms is None,
            "missing_base_mint": not base_mint,
            "missing_pool_id": not pool_id,
        }
        for reason, failed in checks.items():
            if failed:
                controls["damage_reasons"][reason] += 1
                damaged = True
        if damaged:
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
                levels_bps=levels or frozenset(),
                first_hit_ms=first_hit or {},
                path_bps=path_bps or tuple(),
            )
        )

    controls["qualified_records"] = len(records)
    controls["duplicate_keys"] = sum(1 for count in key_counts.values() if count > 1)
    controls["quality_counts"] = dict(sorted(controls["quality_counts"].items()))
    controls["truncated_counts"] = dict(sorted(controls["truncated_counts"].items()))
    controls["horizon_ms_counts"] = dict(sorted(controls["horizon_ms_counts"].items()))
    controls["damage_reasons"] = dict(sorted(controls["damage_reasons"].items()))
    return records, controls


def decision_sort_ts(row: dict[str, Any]) -> float:
    for key in ("decision_ts_ms", "end_10s_ts_ms", "ab_t_end_event_ts_ms", "first_seen_ts_ms"):
        value = finite_float(row.get(key))
        if value is not None:
            return value
    return -1.0


def load_decisions(path: Path, target_mints: set[str]) -> tuple[dict[str, dict[str, Any]], dict[str, Any]]:
    by_mint: dict[str, dict[str, Any]] = {}
    controls: dict[str, Any] = {
        "rows_scanned": 0,
        "target_rows_seen": 0,
        "duplicate_target_rows": 0,
        "missing_base_mint": 0,
    }
    for row in iter_jsonl(path):
        controls["rows_scanned"] += 1
        base_mint = row.get("base_mint")
        if not isinstance(base_mint, str) or not base_mint:
            controls["missing_base_mint"] += 1
            continue
        if base_mint not in target_mints:
            continue
        controls["target_rows_seen"] += 1
        existing = by_mint.get(base_mint)
        if existing is not None:
            controls["duplicate_target_rows"] += 1
            if decision_sort_ts(row) <= decision_sort_ts(existing):
                continue
        by_mint[base_mint] = row
    controls["joined_mints"] = len(by_mint)
    controls["missing_target_mints"] = len(target_mints.difference(by_mint))
    return by_mint, controls


def load_selector_scores(path: Path, target_mints: set[str]) -> tuple[dict[str, float], dict[str, Any]]:
    scores: dict[str, float] = {}
    controls = {"rows_scanned": 0, "target_rows_seen": 0, "valid_scores": 0, "missing_or_nonfinite_scores": 0}
    if not path.is_file():
        controls["missing_file"] = True
        return scores, controls
    for row in iter_jsonl(path):
        controls["rows_scanned"] += 1
        base_mint = row.get("base_mint")
        if not isinstance(base_mint, str) or base_mint not in target_mints:
            continue
        controls["target_rows_seen"] += 1
        score = finite_float(row.get("selector_shadow_score"))
        if score is None:
            controls["missing_or_nonfinite_scores"] += 1
            continue
        scores[base_mint] = score
        controls["valid_scores"] += 1
    controls["joined_mints"] = len(scores)
    return scores, controls


def lifecycle_scan(path: Path) -> dict[str, Any]:
    controls = {"path": str(path), "exists": path.is_file(), "rows": 0, "record_type_counts": Counter(), "records_with_final_pnl_pct": 0}
    if not path.is_file():
        return controls
    for row in iter_jsonl(path):
        controls["rows"] += 1
        controls["record_type_counts"][str(row.get("record_type"))] += 1
        if finite_float(row.get("final_pnl_pct")) is not None:
            controls["records_with_final_pnl_pct"] += 1
    controls["record_type_counts"] = dict(controls["record_type_counts"].most_common())
    return controls


def extract_features(row: dict[str, Any]) -> tuple[dict[str, float | None], dict[str, str]]:
    features: dict[str, float | None] = {}
    sources: dict[str, str] = {}
    for spec in FIELD_SPECS:
        value: float | None = None
        source = "unavailable"
        for path in spec.paths:
            candidate = finite_float(nested_get(row, path))
            if candidate is not None:
                value = candidate
                source = ".".join(path)
                break
        features[spec.name] = value
        sources[spec.name] = source
    return features, sources


def validate_candidate_field_names() -> None:
    used = {spec.name for spec in FIELD_SPECS if spec.used_by_ladder}
    allowed_exceptions = {
        "current_market_cap_sol",
        "top3_signer_volume_ratio",
        "top3_volume_pct",
        "max_single_tx_price_impact_pct_observed",
        "max_single_sell_impact_pct_observed",
    }
    for field in sorted(used):
        if field in allowed_exceptions:
            continue
        for token in FORBIDDEN_INPUT_SUBSTRINGS:
            if token in field:
                raise SystemExit(f"Forbidden candidate field name token {token!r} in {field!r}")


def build_candidate_rows(
    replays: list[ReplayRecord],
    decisions: dict[str, dict[str, Any]],
    selector_scores: dict[str, float],
) -> tuple[list[CandidateRow], dict[str, Any]]:
    rows: list[CandidateRow] = []
    controls = {"replay_records": len(replays), "joined_decision_records": 0, "missing_decision_records": 0, "joined_selector_scores": 0}
    for replay in replays:
        decision = decisions.get(replay.base_mint)
        if decision is None:
            controls["missing_decision_records"] += 1
            continue
        features, sources = extract_features(decision)
        score = selector_scores.get(replay.base_mint)
        if score is not None:
            controls["joined_selector_scores"] += 1
        rows.append(CandidateRow(replay=replay, decision=decision, features=features, sources=sources, selector_shadow_score=score))
    controls["joined_decision_records"] = len(rows)
    return rows, controls


def assign_chronological_splits(rows: list[CandidateRow]) -> None:
    ordered = sorted(rows, key=lambda row: (row.replay.entry_ts_ms, row.replay.order_index))
    total = len(ordered)
    for index, row in enumerate(ordered):
        ratio = index / total if total else 0.0
        if ratio < 1 / 3:
            row.split = "train"
        elif ratio < 2 / 3:
            row.split = "validation"
        else:
            row.split = "holdout"


def sort_chronological(rows: list[CandidateRow]) -> list[CandidateRow]:
    return sorted(rows, key=lambda row: (row.replay.entry_ts_ms, row.replay.order_index))


def percentile(values: list[float], pct: float) -> float | None:
    clean = sorted(value for value in values if math.isfinite(value))
    if not clean:
        return None
    if len(clean) == 1:
        return clean[0]
    rank = (len(clean) - 1) * pct
    low = math.floor(rank)
    high = math.ceil(rank)
    if low == high:
        return clean[low]
    weight = rank - low
    return clean[low] * (1.0 - weight) + clean[high] * weight


def finite_feature_values(rows: list[CandidateRow], field: str) -> list[float]:
    return [value for row in rows if (value := row.features.get(field)) is not None]


def coverage(rows: list[CandidateRow], field: str) -> float:
    if not rows:
        return 0.0
    return len(finite_feature_values(rows, field)) / len(rows)


def derive_thresholds(rows: list[CandidateRow], profile: str) -> tuple[dict[str, float], list[dict[str, Any]]]:
    quantiles = PROFILE_QUANTILES[profile]
    train_s1 = [row for row in rows if row.split == "train" and passes_s1(row)]
    threshold_specs = [
        ("current_market_cap_sol", "cap", "C1", True),
        ("bonding_progress_pct", "cap", "C1", True),
        ("price_change_ratio", "cap", "C1", True),
        ("avg_cpi_depth_50tx", "cap", "C2", True),
        ("max_single_tx_price_impact_pct_observed", "cap", "C2", True),
        ("compute_unit_cluster_dominance", "cap", "C2", False),
        ("unique_ratio", "floor", "C3", True),
        ("hhi", "cap", "C4", True),
        ("top3_signer_volume_ratio", "cap", "C4", False),
        ("top3_volume_pct", "cap", "C4", False),
        ("dev_tx_ratio", "cap", "C5", False),
        ("dev_volume_ratio", "cap", "C5", False),
        ("signer_cross_pool_velocity", "cap", "C5", False),
        ("cpv_other_pool_activity", "cap", "C5", False),
        ("flipper_presence_ratio", "cap", "C5", False),
    ]
    thresholds: dict[str, float] = {}
    rows_out: list[dict[str, Any]] = []
    for field, direction, stage, required in threshold_specs:
        cov = coverage(train_s1, field)
        use_field = required or cov >= OPTIONAL_FIELD_MIN_COVERAGE
        if field == "top3_volume_pct" and "top3_signer_volume_ratio" in thresholds:
            use_field = False
        if field == "top3_signer_volume_ratio" and cov < OPTIONAL_FIELD_MIN_COVERAGE:
            use_field = False
        values = finite_feature_values(train_s1, field)
        pct = quantiles["cap"] if direction == "cap" else quantiles["floor"]
        threshold = percentile(values, pct)
        if use_field and threshold is not None:
            thresholds[field] = threshold
        rows_out.append(
            {
                "field": field,
                "stage": stage,
                "direction": direction,
                "profile": profile,
                "quantile": pct,
                "threshold": threshold,
                "train_s1_count": len(train_s1),
                "train_s1_finite_count": len(values),
                "train_s1_coverage": cov,
                "used": field in thresholds,
                "required": required,
                "source": "train_s1_distribution_cut",
            }
        )
    return thresholds, rows_out


def ge(row: CandidateRow, field: str, threshold: float) -> bool:
    value = row.features.get(field)
    return value is not None and value >= threshold


def le(row: CandidateRow, field: str, threshold: float) -> bool:
    value = row.features.get(field)
    return value is not None and value <= threshold


def passes_s1(row: CandidateRow) -> bool:
    return (
        ge(row, "current_market_cap_sol", 30.2)
        and ge(row, "bonding_progress_pct", 36.5)
        and ge(row, "price_change_ratio", 1.012)
        and ge(row, "buy_count", 8.0)
        and ge(row, "sol_buy_ratio", 0.520)
    )


def passes_stage(row: CandidateRow, stage: str, thresholds: dict[str, float]) -> bool:
    if stage == "S0":
        return True
    if not passes_s1(row):
        return False
    if stage == "S1":
        return True
    if not all(le(row, field, thresholds[field]) for field in ("current_market_cap_sol", "bonding_progress_pct", "price_change_ratio") if field in thresholds):
        return False
    if stage == "C1":
        return True
    c2_fields = [field for field in ("avg_cpi_depth_50tx", "max_single_tx_price_impact_pct_observed", "compute_unit_cluster_dominance") if field in thresholds]
    if not all(le(row, field, thresholds[field]) for field in c2_fields):
        return False
    if stage == "C2":
        return True
    if "unique_ratio" in thresholds and not ge(row, "unique_ratio", thresholds["unique_ratio"]):
        return False
    if stage == "C3":
        return True
    if "hhi" in thresholds and not le(row, "hhi", thresholds["hhi"]):
        return False
    top3_field = "top3_signer_volume_ratio" if "top3_signer_volume_ratio" in thresholds else "top3_volume_pct"
    if top3_field in thresholds and not le(row, top3_field, thresholds[top3_field]):
        return False
    if stage == "C4":
        return True
    c5_fields = [field for field in ("dev_tx_ratio", "dev_volume_ratio", "signer_cross_pool_velocity", "cpv_other_pool_activity", "flipper_presence_ratio") if field in thresholds]
    return all(le(row, field, thresholds[field]) for field in c5_fields)


def path_prev_pnl(record: ReplayRecord, max_hold_ms: int) -> int | None:
    selected: int | None = None
    for age_ms, pnl_bps in record.path_bps:
        if age_ms <= max_hold_ms:
            selected = pnl_bps
        else:
            break
    return selected


def precompute_timeout_pnls(rows: list[CandidateRow], max_hold_values: list[int]) -> None:
    for row in rows:
        row.timeout_pnl_by_hold = {
            max_hold_ms: path_prev_pnl(row.replay, max_hold_ms)
            for max_hold_ms in max_hold_values
        }


def simulate_record(record: ReplayRecord, target_bps: int, stop_bps: int, max_hold_ms: int) -> CellOutcome:
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


def simulate_candidate(row: CandidateRow, target_bps: int, stop_bps: int, max_hold_ms: int) -> CellOutcome:
    record = row.replay
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
    timeout_pnl = row.timeout_pnl_by_hold.get(max_hold_ms)
    if timeout_pnl is None:
        timeout_pnl = path_prev_pnl(record, max_hold_ms)
    if timeout_pnl is None:
        return CellOutcome(MATRIX_UNAVAILABLE, None, "no_path_point_before_max_hold")
    return CellOutcome(MATRIX_TIMEOUT, timeout_pnl, PATH_PREV_TIMEOUT)


def outcomes_for_rows(
    rows: list[CandidateRow],
    target_bps: int,
    stop_bps: int,
    max_hold_ms: int,
) -> list[tuple[CandidateRow, CellOutcome]]:
    return [
        (row, simulate_candidate(row, target_bps, stop_bps, max_hold_ms))
        for row in rows
    ]


def mean(values: list[float]) -> float:
    return sum(values) / len(values) if values else 0.0


def median(values: list[float]) -> float:
    return float(statistics.median(values)) if values else 0.0


def profit_factor(values: list[float]) -> float | None:
    positive = sum(value for value in values if value > 0)
    negative = abs(sum(value for value in values if value < 0))
    if negative == 0:
        return None
    return positive / negative


def max_consecutive_losses(values: list[float]) -> int:
    best = 0
    current = 0
    for value in values:
        if value < 0:
            current += 1
            best = max(best, current)
        else:
            current = 0
    return best


def metrics_for_rows(
    rows: list[CandidateRow],
    target_bps: int,
    stop_bps: int,
    max_hold_ms: int,
    *,
    roundtrip_cost_bps: int = 0,
) -> dict[str, Any]:
    outcomes = outcomes_for_rows(rows, target_bps, stop_bps, max_hold_ms)
    return metrics_from_outcomes(
        outcomes,
        input_count=len(rows),
        roundtrip_cost_bps=roundtrip_cost_bps,
    )


def metrics_from_outcomes(
    outcomes: list[tuple[CandidateRow, CellOutcome]],
    *,
    input_count: int,
    roundtrip_cost_bps: int = 0,
) -> dict[str, Any]:
    eligible = [(row, outcome) for row, outcome in outcomes if outcome.pnl_bps is not None]
    pnl_values = [float(outcome.pnl_bps or 0) - roundtrip_cost_bps for _, outcome in eligible]
    counts = Counter(outcome.label for _, outcome in eligible)
    timeout_values = [float(outcome.pnl_bps or 0) - roundtrip_cost_bps for _, outcome in eligible if outcome.label == MATRIX_TIMEOUT]
    chronological_pnls = [
        float(outcome.pnl_bps or 0) - roundtrip_cost_bps
        for _, outcome in eligible
    ]
    return {
        "eligible_count": len(eligible),
        "excluded_count": input_count - len(eligible),
        "target_count": counts[MATRIX_TARGET],
        "stop_count": counts[MATRIX_STOP],
        "timeout_count": counts[MATRIX_TIMEOUT],
        "target_rate": counts[MATRIX_TARGET] / len(eligible) if eligible else 0.0,
        "stop_rate": counts[MATRIX_STOP] / len(eligible) if eligible else 0.0,
        "timeout_rate": counts[MATRIX_TIMEOUT] / len(eligible) if eligible else 0.0,
        "positive_timeout_count": sum(1 for value in timeout_values if value > 0),
        "negative_timeout_count": sum(1 for value in timeout_values if value < 0),
        "negative_timeout_rate": sum(1 for value in timeout_values if value < 0) / len(timeout_values) if timeout_values else 0.0,
        "avg_pnl_bps": mean(pnl_values),
        "median_pnl_bps": median(pnl_values),
        "sum_pnl_bps": sum(pnl_values),
        "profit_factor": profit_factor(pnl_values),
        "max_consecutive_losses": max_consecutive_losses(chronological_pnls),
    }


def stage_rows(rows: list[CandidateRow], thresholds: dict[str, float]) -> dict[str, list[CandidateRow]]:
    stages = ["S0", "S1", "C1", "C2", "C3", "C4", "C5"]
    return {stage: [row for row in rows if passes_stage(row, stage, thresholds)] for stage in stages}


def selector_equal_count_rows(rows: list[CandidateRow], count: int) -> list[CandidateRow]:
    scored = [row for row in rows if row.selector_shadow_score is not None]
    scored.sort(key=lambda row: (row.selector_shadow_score or -math.inf, row.replay.entry_ts_ms), reverse=True)
    return sort_chronological(scored[:count])


def field_inventory(rows: list[CandidateRow]) -> list[dict[str, Any]]:
    output: list[dict[str, Any]] = []
    for spec in FIELD_SPECS:
        values = finite_feature_values(rows, spec.name)
        source_counts = Counter(row.sources.get(spec.name, "unavailable") for row in rows)
        max_value = max(values) if values else None
        semantic_note = spec.note
        if spec.name in {"top3_signer_volume_ratio", "top3_volume_pct"} and max_value is not None:
            semantic_note += "; observed scale=" + ("ratio_0_1" if max_value <= 1.0 else "percent_or_mixed")
        output.append(
            {
                "field": spec.name,
                "family": spec.family,
                "decision_time_safe": spec.decision_time_safe,
                "used_by_ladder": spec.used_by_ladder,
                "finite_count": len(values),
                "coverage": len(values) / len(rows) if rows else 0.0,
                "min": min(values) if values else None,
                "max": max_value,
                "primary_source": source_counts.most_common(1)[0][0] if source_counts else "unavailable",
                "source_counts": json.dumps(dict(source_counts.most_common()), sort_keys=True),
                "note": semantic_note,
            }
        )
    return output


def write_csv(path: Path, rows: list[dict[str, Any]], fieldnames: list[str] | None = None) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if not fieldnames:
        keys: list[str] = []
        for row in rows:
            for key in row:
                if key not in keys:
                    keys.append(key)
        fieldnames = keys
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames)
        writer.writeheader()
        for row in rows:
            writer.writerow(row)


def matrix_rows_for_policy(policy_name: str, policy_kind: str, rows: list[CandidateRow], args: argparse.Namespace) -> list[dict[str, Any]]:
    output: list[dict[str, Any]] = []
    for max_hold_ms in args.max_hold_ms:
        for target_bps in args.targets_bps:
            for stop_bps in args.stops_bps:
                outcomes = outcomes_for_rows(rows, target_bps, stop_bps, max_hold_ms)
                metrics = metrics_from_outcomes(outcomes, input_count=len(rows))
                output.append(
                    {
                        "policy": policy_name,
                        "policy_kind": policy_kind,
                        "target_bps": target_bps,
                        "stop_bps": stop_bps,
                        "max_hold_ms": max_hold_ms,
                        "input_count": len(rows),
                        **metrics,
                    }
                )
    return output


def choose_exit_from_train(rows: list[CandidateRow], args: argparse.Namespace) -> tuple[int, int, int] | None:
    train_rows = [row for row in rows if row.split == "train"]
    if not train_rows:
        return None
    best: tuple[float, float, float, float, int, int, int] | None = None
    for max_hold_ms in args.max_hold_ms:
        for target_bps in args.targets_bps:
            for stop_bps in args.stops_bps:
                metrics = metrics_for_rows(train_rows, target_bps, stop_bps, max_hold_ms, roundtrip_cost_bps=SELECTION_COST_BPS)
                key = (
                    float(metrics["avg_pnl_bps"]),
                    float(metrics["median_pnl_bps"]),
                    float(metrics["profit_factor"] or -1.0),
                    -float(metrics["stop_rate"]),
                    target_bps,
                    stop_bps,
                    max_hold_ms,
                )
                if best is None or key > best:
                    best = key
    if best is None:
        return None
    return (best[4], best[5], best[6])


def add_policy_summary(
    output: list[dict[str, Any]],
    policy_name: str,
    policy_kind: str,
    rows: list[CandidateRow],
    total_rows: int,
    selected_exit: tuple[int, int, int] | None,
) -> None:
    target_bps, stop_bps, max_hold_ms = selected_exit or (PRIMARY_TARGET_BPS, PRIMARY_STOP_BPS, PRIMARY_MAX_HOLD_MS)
    gross = metrics_for_rows(rows, target_bps, stop_bps, max_hold_ms)
    cost100 = metrics_for_rows(rows, target_bps, stop_bps, max_hold_ms, roundtrip_cost_bps=SELECTION_COST_BPS)
    output.append(
        {
            "policy": policy_name,
            "policy_kind": policy_kind,
            "count": len(rows),
            "retained_pct": len(rows) / total_rows if total_rows else 0.0,
            "selected_target_bps": target_bps,
            "selected_stop_bps": stop_bps,
            "selected_max_hold_ms": max_hold_ms,
            "selected_exit_source": f"train_only_cost_{SELECTION_COST_BPS}_avg_then_median_pf_stop",
            **{f"gross_{key}": value for key, value in gross.items()},
            **{f"cost100_{key}": value for key, value in cost100.items()},
        }
    )


def stability_rows_for_policy(policy_name: str, policy_kind: str, rows: list[CandidateRow], total_rows: int, selected_exit: tuple[int, int, int] | None) -> list[dict[str, Any]]:
    target_bps, stop_bps, max_hold_ms = selected_exit or (PRIMARY_TARGET_BPS, PRIMARY_STOP_BPS, PRIMARY_MAX_HOLD_MS)
    output: list[dict[str, Any]] = []
    for split in ("train", "validation", "holdout"):
        split_rows = [row for row in rows if row.split == split]
        gross = metrics_for_rows(split_rows, target_bps, stop_bps, max_hold_ms)
        cost100 = metrics_for_rows(split_rows, target_bps, stop_bps, max_hold_ms, roundtrip_cost_bps=SELECTION_COST_BPS)
        output.append(
            {
                "policy": policy_name,
                "policy_kind": policy_kind,
                "segment": split,
                "count": len(split_rows),
                "retained_pct_of_total": len(split_rows) / total_rows if total_rows else 0.0,
                "target_bps": target_bps,
                "stop_bps": stop_bps,
                "max_hold_ms": max_hold_ms,
                **{f"gross_{key}": value for key, value in gross.items()},
                **{f"cost100_{key}": value for key, value in cost100.items()},
            }
        )
    return output


def cost_sensitivity_rows(policy_name: str, policy_kind: str, rows: list[CandidateRow], args: argparse.Namespace) -> list[dict[str, Any]]:
    output: list[dict[str, Any]] = []
    for max_hold_ms in args.max_hold_ms:
        for target_bps in args.targets_bps:
            for stop_bps in args.stops_bps:
                outcomes = outcomes_for_rows(rows, target_bps, stop_bps, max_hold_ms)
                for cost in args.roundtrip_cost_bps:
                    metrics = metrics_from_outcomes(
                        outcomes,
                        input_count=len(rows),
                        roundtrip_cost_bps=cost,
                    )
                    output.append(
                        {
                            "policy": policy_name,
                            "policy_kind": policy_kind,
                            "target_bps": target_bps,
                            "stop_bps": stop_bps,
                            "max_hold_ms": max_hold_ms,
                            "roundtrip_cost_bps": cost,
                            "input_count": len(rows),
                            **metrics,
                        }
                    )
    return output


def matrix_and_cost_rows_for_policy(
    policy_name: str,
    policy_kind: str,
    rows: list[CandidateRow],
    args: argparse.Namespace,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    matrix_output: list[dict[str, Any]] = []
    cost_output: list[dict[str, Any]] = []
    for max_hold_ms in args.max_hold_ms:
        for target_bps in args.targets_bps:
            for stop_bps in args.stops_bps:
                outcomes = outcomes_for_rows(rows, target_bps, stop_bps, max_hold_ms)
                matrix_metrics = metrics_from_outcomes(outcomes, input_count=len(rows))
                matrix_output.append(
                    {
                        "policy": policy_name,
                        "policy_kind": policy_kind,
                        "target_bps": target_bps,
                        "stop_bps": stop_bps,
                        "max_hold_ms": max_hold_ms,
                        "input_count": len(rows),
                        **matrix_metrics,
                    }
                )
                for cost in args.roundtrip_cost_bps:
                    cost_metrics = metrics_from_outcomes(
                        outcomes,
                        input_count=len(rows),
                        roundtrip_cost_bps=cost,
                    )
                    cost_output.append(
                        {
                            "policy": policy_name,
                            "policy_kind": policy_kind,
                            "target_bps": target_bps,
                            "stop_bps": stop_bps,
                            "max_hold_ms": max_hold_ms,
                            "roundtrip_cost_bps": cost,
                            "input_count": len(rows),
                            **cost_metrics,
                        }
                    )
    return matrix_output, cost_output


def verdict(summary_rows: list[dict[str, Any]], stability_rows: list[dict[str, Any]]) -> tuple[str, list[str]]:
    by_policy = {row["policy"]: row for row in summary_rows}
    s1 = by_policy.get("S1_F5")
    blockers: list[str] = []
    if not s1:
        return "INCONCLUSIVE", ["S1/F5 baseline missing"]
    candidate_rows = [row for row in summary_rows if str(row["policy"]).startswith("C")]
    promising = False
    for row in candidate_rows:
        if row["count"] < MIN_INTERESTING_RETAINED_COUNT:
            blockers.append(f"{row['policy']}: retained cohort too small ({row['count']})")
            continue
        improves_mix = (
            float(row["gross_target_rate"]) >= float(s1["gross_target_rate"])
            and float(row["gross_stop_rate"]) <= float(s1["gross_stop_rate"])
            and float(row["gross_negative_timeout_rate"]) <= float(s1["gross_negative_timeout_rate"])
        )
        improves_cost = (
            float(row["cost100_avg_pnl_bps"]) > float(s1["cost100_avg_pnl_bps"])
            and float(row["cost100_sum_pnl_bps"]) > float(s1["cost100_sum_pnl_bps"])
        )
        policy_stability = [item for item in stability_rows if item["policy"] == row["policy"]]
        stable_segments = sum(1 for item in policy_stability if float(item["cost100_avg_pnl_bps"]) >= 0)
        if improves_mix and improves_cost and stable_segments >= 2:
            promising = True
        else:
            blockers.append(
                f"{row['policy']}: no full F5 beat (mix={improves_mix}, cost100={improves_cost}, nonnegative_segments={stable_segments}/3)"
            )
    if promising:
        return "PROMISING_OFFLINE_ONLY", blockers
    if any(float(row.get("cost100_avg_pnl_bps", 0.0)) > float(s1["cost100_avg_pnl_bps"]) for row in candidate_rows):
        return "INCONCLUSIVE", blockers
    return "REJECTED", blockers


def markdown_table(rows: list[dict[str, Any]], columns: list[str], limit: int | None = None) -> str:
    rows = rows[:limit] if limit is not None else rows
    out = ["| " + " | ".join(columns) + " |", "| " + " | ".join("---" for _ in columns) + " |"]
    for row in rows:
        values = []
        for col in columns:
            value = row.get(col, "")
            if isinstance(value, float):
                value = f"{value:.6g}"
            values.append(str(value))
        out.append("| " + " | ".join(values) + " |")
    return "\n".join(out)


def write_report(
    path: Path,
    *,
    args: argparse.Namespace,
    paths: dict[str, Path],
    controls: dict[str, Any],
    inventory_rows: list[dict[str, Any]],
    threshold_rows: list[dict[str, Any]],
    summary_rows: list[dict[str, Any]],
    stability_rows: list[dict[str, Any]],
    verdict_value: str,
    blockers: list[str],
    output_files: dict[str, Path],
) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    allowed_used = [row for row in inventory_rows if row["used_by_ladder"]]
    lines = [
        "# RAPORT: Organic Pool Candidate Policy A0 Offline Proof",
        "",
        f"Generated UTC: `{utc_now_iso()}`",
        f"Scope: `{args.scope}`",
        f"Decision lane: `{args.decision_lane}`",
        f"Profile: `{args.profile}`",
        f"Final verdict: `{verdict_value}`",
        "",
        "## Scope boundaries",
        "",
        "- Offline/read-only proof only.",
        "- No Gatekeeper BUY/REJECT change.",
        "- No `v25_confidence`, V3 promotion, selector runtime policy, TX builder, sender, Jito path, live execution, or existing log mutation.",
        "- `selector_shadow_score` is used only as equal-count diagnostic baseline.",
        "- `shadow_exit_replay_v1` is used only after candidate cohort selection, for identical Target/Stop/max_hold grids.",
        "- Identifiers and timestamps are used only for join/dedup/sorting, not predictive features.",
        "",
        "## Files checked",
        "",
        markdown_table(
            [
                {
                    "kind": key,
                    "path": str(value),
                    "exists": value.is_file(),
                    "size_bytes": value.stat().st_size if value.is_file() else "",
                }
                for key, value in paths.items()
                if key not in {"output_dir", "report_path"}
            ],
            ["kind", "path", "exists", "size_bytes"],
        ),
        "",
        "## Data controls",
        "",
        "```json",
        json.dumps(controls, indent=2, sort_keys=True, default=str),
        "```",
        "",
        "## Decision-time field inventory",
        "",
        markdown_table(
            allowed_used,
            ["field", "family", "finite_count", "coverage", "primary_source", "note"],
        ),
        "",
        "Full inventory is written to CSV.",
        "",
        "## Candidate ladder",
        "",
        "- `S0`: clean joined `shadow_exit_replay_v1` acted/broad sampler cohort.",
        "- `S1_F5`: `current_market_cap_sol >= 30.2`, `bonding_progress_pct >= 36.5`, `price_change_ratio >= 1.012`, `buy_count >= 8`, `sol_buy_ratio >= 0.520`.",
        "- `C1`: S1 + train-only anti-overextension caps.",
        "- `C2`: C1 + train-only low execution-toxicity caps.",
        "- `C3`: C2 + train-only organic broadening floor.",
        "- `C4`: C3 + train-only concentration guard.",
        "- `C5`: C4 + optional train-only dev/cross-pool guards when decision-time coverage is adequate.",
        "",
        "## Threshold source",
        "",
        "Thresholds are distribution cuts from the chronological train segment only; no final outcome or holdout metric is used for threshold selection.",
        "",
        markdown_table(
            threshold_rows,
            ["field", "stage", "direction", "quantile", "threshold", "train_s1_coverage", "used", "source"],
        ),
        "",
        "## Summary metrics",
        "",
        markdown_table(
            summary_rows,
            [
                "policy",
                "policy_kind",
                "count",
                "retained_pct",
                "selected_target_bps",
                "selected_stop_bps",
                "selected_max_hold_ms",
                "gross_target_rate",
                "gross_stop_rate",
                "gross_timeout_rate",
                "gross_negative_timeout_rate",
                "gross_avg_pnl_bps",
                "gross_median_pnl_bps",
                "gross_sum_pnl_bps",
                "cost100_avg_pnl_bps",
                "cost100_median_pnl_bps",
                "cost100_sum_pnl_bps",
                "cost100_max_consecutive_losses",
            ],
        ),
        "",
        "## Stability",
        "",
        markdown_table(
            stability_rows,
            [
                "policy",
                "segment",
                "count",
                "gross_target_rate",
                "gross_stop_rate",
                "gross_negative_timeout_rate",
                "gross_avg_pnl_bps",
                "cost100_avg_pnl_bps",
                "cost100_sum_pnl_bps",
                "cost100_max_consecutive_losses",
            ],
            limit=60,
        ),
        "",
        "## Exit matrix and cost sensitivity",
        "",
        f"- Full identical Target/Stop/max_hold matrix: `{output_files['exit_matrix']}`",
        f"- Cost sensitivity at `{args.roundtrip_cost_bps}` bps: `{output_files['cost_sensitivity']}`",
        f"- Stability by chronological tercile for train-selected exits: `{output_files['stability']}`",
        "",
        "## Acceptance verdict",
        "",
        f"`{verdict_value}`",
        "",
        "Blockers before runtime:",
    ]
    lines.extend(f"- {blocker}" for blocker in blockers[:40])
    lines.extend(
        [
            "- Runtime gate remains closed: this proof does not recommend Gatekeeper, selector, V3, sender, or live-execution changes.",
            "- Any runtime proposal would require fresh multi-run holdout, non-microscopic retained cohort, typed availability guards, and implementation plan review.",
            "",
            "## Generated outputs",
            "",
            markdown_table([{"artifact": key, "path": str(value)} for key, value in output_files.items()], ["artifact", "path"]),
            "",
        ]
    )
    path.write_text("\n".join(lines), encoding="utf-8")


def main() -> int:
    args = parse_args()
    validate_candidate_field_names()
    paths = resolve_paths(args)
    for required_key in ("decision_log", "exit_replay"):
        if not paths[required_key].is_file():
            raise SystemExit(f"Missing required file: {paths[required_key]}")

    replays, replay_controls = load_exit_replay(paths["exit_replay"])
    target_mints = {record.base_mint for record in replays}
    decisions, decision_controls = load_decisions(paths["decision_log"], target_mints)
    selector_scores, selector_controls = load_selector_scores(paths["selector_score"], target_mints)
    rows, join_controls = build_candidate_rows(replays, decisions, selector_scores)
    assign_chronological_splits(rows)
    rows = sort_chronological(rows)
    precompute_timeout_pnls(rows, args.max_hold_ms)

    thresholds, threshold_rows = derive_thresholds(rows, args.profile)
    stages = stage_rows(rows, thresholds)
    total_rows = len(rows)

    policies: list[tuple[str, str, list[CandidateRow]]] = []
    for stage, stage_data in stages.items():
        name = "S1_F5" if stage == "S1" else stage
        policies.append((name, "candidate_ladder", stage_data))
    for name, _, data in list(policies):
        if name == "S0" or not data:
            continue
        selector_data = selector_equal_count_rows(rows, len(data))
        policies.append((f"SEL_EQ_{name}", "diagnostic_selector_shadow_score_equal_count", selector_data))

    summary_rows: list[dict[str, Any]] = []
    stability_rows: list[dict[str, Any]] = []
    exit_matrix_rows: list[dict[str, Any]] = []
    cost_rows: list[dict[str, Any]] = []
    selected_exits: dict[str, tuple[int, int, int] | None] = {}

    for policy_name, policy_kind, policy_rows in policies:
        selected_exit = choose_exit_from_train(policy_rows, args)
        selected_exits[policy_name] = selected_exit
        add_policy_summary(summary_rows, policy_name, policy_kind, policy_rows, total_rows, selected_exit)
        stability_rows.extend(stability_rows_for_policy(policy_name, policy_kind, policy_rows, total_rows, selected_exit))
        if policy_kind == "candidate_ladder":
            policy_matrix_rows, policy_cost_rows = matrix_and_cost_rows_for_policy(policy_name, policy_kind, policy_rows, args)
            exit_matrix_rows.extend(policy_matrix_rows)
            cost_rows.extend(policy_cost_rows)

    inventory_rows = field_inventory(rows)
    verdict_value, blockers = verdict(summary_rows, stability_rows)

    output_dir = paths["output_dir"]
    output_dir.mkdir(parents=True, exist_ok=True)
    output_files = {
        "summary": output_dir / "organic_candidate_policy_summary.csv",
        "exit_matrix": output_dir / "organic_candidate_policy_exit_matrix.csv",
        "cost_sensitivity": output_dir / "organic_candidate_policy_cost_sensitivity.csv",
        "stability": output_dir / "organic_candidate_policy_stability.csv",
        "inventory": output_dir / "organic_candidate_policy_inventory.csv",
        "thresholds": output_dir / "organic_candidate_policy_thresholds.csv",
        "report": paths["report_path"],
    }

    write_csv(output_files["summary"], summary_rows)
    write_csv(output_files["exit_matrix"], exit_matrix_rows)
    write_csv(output_files["cost_sensitivity"], cost_rows)
    write_csv(output_files["stability"], stability_rows)
    write_csv(output_files["inventory"], inventory_rows)
    write_csv(output_files["thresholds"], threshold_rows)

    controls = {
        "scope": args.scope,
        "decision_lane": args.decision_lane,
        "profile": args.profile,
        "split": "chronological_terciles_single_run_weak_evidence",
        "exit_replay": replay_controls,
        "decision_log": decision_controls,
        "selector_shadow_score": selector_controls,
        "joins": join_controls,
        "shadow_lifecycle": lifecycle_scan(paths["shadow_lifecycle"]),
        "probe_shadow_lifecycle": lifecycle_scan(paths["probe_lifecycle"]),
        "selected_exits": {key: value for key, value in selected_exits.items()},
        "verdict": verdict_value,
        "blockers": blockers,
    }
    write_report(
        paths["report_path"],
        args=args,
        paths=paths,
        controls=controls,
        inventory_rows=inventory_rows,
        threshold_rows=threshold_rows,
        summary_rows=summary_rows,
        stability_rows=stability_rows,
        verdict_value=verdict_value,
        blockers=blockers,
        output_files=output_files,
    )

    print(f"verdict={verdict_value}")
    print(f"report={paths['report_path']}")
    for key, value in output_files.items():
        print(f"{key}={value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
