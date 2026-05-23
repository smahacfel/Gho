#!/usr/bin/env python3
"""Audit decision-time feature availability for P3.7 shadow lifecycle labels."""

from __future__ import annotations

import argparse
import json
import math
import statistics as st
from collections import Counter, defaultdict
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any, Iterable

from shadow_run_report import load_toml, resolve_config_path, resolve_runtime_path


SCHEMA_VERSION = 1
DECISION_FILE_NAMES = ("gatekeeper_v2_buys.jsonl", "gatekeeper_v2_decisions.jsonl")
FEATURE_GROUPS = (
    "v3_materialized_feature_snapshot",
    "v3_evidence_status",
    "v3_organic_broadening",
    "v3_manipulation_contradictions",
    "v3_component_scores",
    "v3_shadow_reason_code",
    "v3_shadow_verdict",
    "tx_intel_fields",
    "checkpoint_features",
    "account_features",
    "curve_readiness",
    "sybil_resistance",
    "alpha_fingerprint",
    "pdd_fields",
    "tas_fields",
    "dow_stage_fields",
    "gatekeeper_v2_v25_phase_fields",
    "v25_shadow_fields",
    "legacy_decision_fields",
)

TX_INTEL_FIELDS = {
    "block0_sniped_supply_pct",
    "cu_price_p90_1s",
    "cu_price_p90_10s",
    "priority_fee_surge_slope",
    "avg_inner_ix_count_50tx",
    "avg_cpi_depth_50tx",
    "compute_unit_cluster_dominance",
    "static_fee_profile_ratio",
    "fixed_size_buy_ratio",
    "fixed_size_buy_ratio_1e4",
    "flipper_presence_ratio",
    "jito_tip_intensity",
    "early_slot_volume_dominance_buy",
    "early_top3_buy_volume_pct_3s",
    "whale_reversal_ratio_top3",
    "whale_reversal_ratio_top1",
    "dev_paperhand_latency_ms",
    "fee_topology_diversity_index",
    "dev_buyer_infrastructure_affinity",
    "spend_fraction_divergence",
    "demand_elasticity_score",
    "signer_cross_pool_velocity",
    "funding_source_diagnostics",
    "max_funding_source_concentration",
}
CHECKPOINT_FIELDS = {
    "curve_t0_event_ts_ms",
    "curve_t0_clock_source",
    "end_10s_ts_ms",
    "price_change_ratio",
    "entry_drift_pct",
    "entry_drift_anchor_source",
    "entry_drift_anchor_quality",
    "current_market_cap_sol",
    "bonding_progress_pct",
}
ACCOUNT_FIELDS = {
    "curve_data_known",
    "curve_finality",
    "curve_finality_is_finalized",
    "current_market_cap_sol",
    "bonding_progress_pct",
}
CURVE_READINESS_FIELDS = {
    "curve_required_for_buy",
    "curve_wait_ms",
    "curve_wait_elapsed_ms",
    "curve_data_known",
    "curve_finality",
    "curve_finality_is_finalized",
    "bonding_progress_check_skipped",
}
GATEKEEPER_PHASE_FIELDS = {
    "core_pass",
    "core1_passed",
    "core2_passed",
    "core3_passed",
    "phase2_passed",
    "phase3_passed",
    "phase4_passed",
    "phase5_passed",
    "phase6_passed",
    "phases_passed",
    "min_phases_to_pass",
    "decision_reason",
    "decision_verdict_buy",
    "verdict_type",
    "gatekeeper_version",
}
DOW_STAGE_FIELDS = {
    "mode",
    "observation_stage",
    "v25_shadow_observation_stage",
    "shadow_early_verdict",
    "shadow_early_elapsed_ms",
    "shadow_early_phases_passed",
}
LEAKAGE_FIELD_NAMES = {
    "final_pnl",
    "final_pnl_pct",
    "final_pnl_sol",
    "gross_pnl_sol",
    "net_pnl_sol",
    "close_reason",
    "position_closed",
    "exit_filled",
    "exit_value_sol",
    "truth_status",
    "truth_source",
}


@dataclass(slots=True)
class DecisionRow:
    row: dict[str, Any]
    path: Path
    log_kind: str
    timestamp_ms: int | None
    feature_groups: set[str]


@dataclass(slots=True)
class MatchResult:
    decision: DecisionRow | None
    join_quality: str
    ambiguous: bool
    match_time_delta_ms: int | None


def iter_jsonl(path: Path) -> Iterable[dict[str, Any]]:
    if not path.exists():
        return
    with path.open("r", encoding="utf-8", errors="ignore") as fh:
        for line in fh:
            raw = line.strip()
            if not raw:
                continue
            try:
                obj = json.loads(raw)
            except json.JSONDecodeError:
                continue
            if isinstance(obj, dict):
                yield obj


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def parse_iso_to_ms(value: Any) -> int | None:
    if not isinstance(value, str) or not value:
        return None
    raw = value.strip()
    if raw.endswith("Z"):
        raw = raw[:-1] + "+00:00"
    try:
        return int(datetime.fromisoformat(raw).timestamp() * 1000)
    except ValueError:
        return None


def finite_int(value: Any) -> int | None:
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        return int(value)
    return None


def finite_float(value: Any) -> float | None:
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        return float(value)
    return None


def counter_dict(counter: Counter[str]) -> dict[str, int]:
    return {key: counter[key] for key in sorted(counter)}


def percentile(sorted_values: list[float], pct: float) -> float | None:
    if not sorted_values:
        return None
    if len(sorted_values) == 1:
        return sorted_values[0]
    rank = (len(sorted_values) - 1) * pct
    lower = math.floor(rank)
    upper = math.ceil(rank)
    if lower == upper:
        return sorted_values[int(rank)]
    weight = rank - lower
    return sorted_values[lower] * (1.0 - weight) + sorted_values[upper] * weight


def distribution(values: Iterable[int | float | None]) -> dict[str, Any]:
    cleaned = [float(value) for value in values if isinstance(value, (int, float)) and math.isfinite(float(value))]
    if not cleaned:
        return {"count": 0, "min": None, "p50": None, "p90": None, "p99": None, "max": None, "mean": None}
    ordered = sorted(cleaned)
    return {
        "count": len(ordered),
        "min": ordered[0],
        "p50": percentile(ordered, 0.50),
        "p90": percentile(ordered, 0.90),
        "p99": percentile(ordered, 0.99),
        "max": ordered[-1],
        "mean": st.fmean(ordered),
    }


def resolve_decision_root(config_path: Path) -> Path:
    resolved = resolve_config_path(config_path)
    config = load_toml(resolved)
    raw = config.get("oracle", {}).get("decision_log_path", "logs/decisions")
    return resolve_runtime_path(resolved, raw)


def discover_decision_logs(config_path: Path, explicit: list[Path]) -> list[Path]:
    if explicit:
        return [path.resolve() for path in explicit]
    root = resolve_decision_root(config_path)
    paths: list[Path] = []
    for name in DECISION_FILE_NAMES:
        direct = root / name
        if direct.exists():
            paths.append(direct)
        if root.exists():
            paths.extend(path for path in root.rglob(name) if path.is_file() and path != direct)
    return sorted(set(paths))


def has_any_key(row: dict[str, Any], keys: set[str]) -> bool:
    return any(key in row and row.get(key) is not None for key in keys)


def has_prefix(row: dict[str, Any], prefix: str) -> bool:
    return any(key.startswith(prefix) and row.get(key) is not None for key in row)


def has_substring(row: dict[str, Any], text: str) -> bool:
    return any(text in key and row.get(key) is not None for key in row)


def detect_feature_groups(row: dict[str, Any]) -> set[str]:
    groups: set[str] = set()
    snapshot = row.get("v3_materialized_feature_snapshot")
    if isinstance(snapshot, dict) and snapshot:
        groups.add("v3_materialized_feature_snapshot")
        if isinstance(snapshot.get("checkpoint_features"), dict):
            groups.add("checkpoint_features")
        if isinstance(snapshot.get("account_features"), dict):
            groups.add("account_features")
    if has_prefix(row, "v3_evidence") or has_any_key(row, {"v3_replay_payload_hash", "v3_feature_snapshot_hash"}):
        groups.add("v3_evidence_status")
    if has_substring(row, "organic") or has_prefix(row, "v3_organic"):
        groups.add("v3_organic_broadening")
    if has_substring(row, "manipulation") or has_substring(row, "contradiction"):
        groups.add("v3_manipulation_contradictions")
    if has_prefix(row, "v3_component") or has_any_key(row, {"v3_component_scores", "component_scores"}):
        groups.add("v3_component_scores")
    if has_any_key(row, {"v3_shadow_reason_code"}):
        groups.add("v3_shadow_reason_code")
    if has_any_key(row, {"v3_shadow_verdict", "v3_shadow_verdict_type"}):
        groups.add("v3_shadow_verdict")
    if has_any_key(row, TX_INTEL_FIELDS):
        groups.add("tx_intel_fields")
    if has_any_key(row, CHECKPOINT_FIELDS):
        groups.add("checkpoint_features")
    if has_any_key(row, ACCOUNT_FIELDS):
        groups.add("account_features")
    if has_any_key(row, CURVE_READINESS_FIELDS):
        groups.add("curve_readiness")
    if has_prefix(row, "sybil_") or has_any_key(row, {"unique_ratio", "hhi", "top3_volume_pct", "signer_cross_pool_velocity"}):
        groups.add("sybil_resistance")
    if has_prefix(row, "alpha_") or has_prefix(row, "fingerprint_"):
        groups.add("alpha_fingerprint")
    if has_prefix(row, "pdd_"):
        groups.add("pdd_fields")
    if has_prefix(row, "tas_") or has_any_key(row, {"tas_available", "tas_unavailable_reason"}):
        groups.add("tas_fields")
    if has_any_key(row, DOW_STAGE_FIELDS):
        groups.add("dow_stage_fields")
    if has_any_key(row, GATEKEEPER_PHASE_FIELDS):
        groups.add("gatekeeper_v2_v25_phase_fields")
    if has_prefix(row, "v25_"):
        groups.add("v25_shadow_fields")
    if has_prefix(row, "legacy_"):
        groups.add("legacy_decision_fields")
    return groups


def load_decision_rows(paths: list[Path]) -> list[DecisionRow]:
    rows: list[DecisionRow] = []
    for path in paths:
        log_kind = "buy" if path.name == "gatekeeper_v2_buys.jsonl" else "decision"
        for row in iter_jsonl(path):
            timestamp_ms = parse_iso_to_ms(row.get("timestamp"))
            rows.append(
                DecisionRow(
                    row=row,
                    path=path,
                    log_kind=log_kind,
                    timestamp_ms=timestamp_ms,
                    feature_groups=detect_feature_groups(row),
                )
            )
    return rows


def index_decisions(rows: list[DecisionRow]) -> dict[str, dict[Any, list[DecisionRow]]]:
    by_pool: dict[tuple[str, str], list[DecisionRow]] = defaultdict(list)
    by_ab: dict[str, list[DecisionRow]] = defaultdict(list)
    by_candidate: dict[str, list[DecisionRow]] = defaultdict(list)
    for decision in rows:
        base_mint = decision.row.get("base_mint")
        pool_id = decision.row.get("pool_id")
        if isinstance(base_mint, str) and isinstance(pool_id, str):
            by_pool[(base_mint, pool_id)].append(decision)
        ab_record_id = decision.row.get("ab_record_id")
        if isinstance(ab_record_id, str) and ab_record_id:
            by_ab[ab_record_id].append(decision)
        candidate_id = decision.row.get("candidate_id") or decision.row.get("execution_candidate_id")
        if isinstance(candidate_id, str) and candidate_id:
            by_candidate[candidate_id].append(decision)
    return {"by_pool": by_pool, "by_ab": by_ab, "by_candidate": by_candidate}


def decision_anchors(decision: DecisionRow) -> list[int]:
    anchors: list[int] = []
    for value in (
        decision.timestamp_ms,
        finite_int(decision.row.get("observation_end_ts_ms")),
        finite_int(decision.row.get("first_seen_ts_ms")),
    ):
        if value is not None:
            anchors.append(value)
    return anchors


def plane_priority(decision: DecisionRow) -> int:
    plane = str(decision.row.get("decision_plane") or "")
    if plane == "v25_shadow":
        return 0
    if plane == "legacy_live":
        return 1
    return 2


def match_label(
    label: dict[str, Any],
    indexed: dict[str, dict[Any, list[DecisionRow]]],
    *,
    max_match_drift_ms: int,
) -> MatchResult:
    ab_record_id = label.get("ab_record_id")
    if isinstance(ab_record_id, str) and ab_record_id:
        matches = indexed["by_ab"].get(ab_record_id, [])
        if matches:
            matches = sorted(matches, key=lambda decision: (plane_priority(decision), str(decision.path)))
            return MatchResult(matches[0], "matched_by_ab_record_id", len(matches) > 1, 0)
    candidate_id = label.get("candidate_id")
    if isinstance(candidate_id, str) and candidate_id:
        matches = indexed["by_candidate"].get(candidate_id, [])
        if matches:
            matches = sorted(matches, key=lambda decision: (plane_priority(decision), str(decision.path)))
            return MatchResult(matches[0], "matched_by_candidate_id", len(matches) > 1, 0)
    base_mint = label.get("base_mint")
    pool_id = label.get("pool_id")
    anchor_ts = finite_int(label.get("decision_ts_ms")) or finite_int(label.get("entry_execution_ts_ms"))
    if not isinstance(base_mint, str) or not isinstance(pool_id, str) or anchor_ts is None:
        return MatchResult(None, "unmatched", False, None)
    candidates = indexed["by_pool"].get((base_mint, pool_id), [])
    if not candidates:
        return MatchResult(None, "unmatched", False, None)
    scored: list[tuple[tuple[int, int, int, int, int], DecisionRow]] = []
    for decision in candidates:
        anchors = decision_anchors(decision)
        if not anchors:
            continue
        distance = min(abs(anchor_ts - value) for value in anchors)
        if distance > max_match_drift_ms:
            continue
        score = (
            plane_priority(decision),
            0 if decision.log_kind == "buy" else 1,
            0 if decision.row.get("decision_verdict_buy") is True else 1,
            0 if decision.row.get("shadow_execution_outcome") == "shadow_simulated" else 1,
            distance,
        )
        scored.append((score, decision))
    if not scored:
        return MatchResult(None, "unmatched", False, None)
    scored.sort(key=lambda item: (item[0], str(item[1].path)))
    best_score, best = scored[0]
    ambiguous = sum(1 for score, _ in scored if score == best_score) > 1
    return MatchResult(best, "matched_by_pool_mint_time_window", ambiguous, best_score[-1])


def has_forbidden_lifecycle_leakage(row: dict[str, Any]) -> bool:
    for key in row:
        lowered = key.lower()
        if lowered in LEAKAGE_FIELD_NAMES:
            return True
    return False


def segment_names(label: dict[str, Any]) -> list[str]:
    names = ["all_rows"]
    quality = str(label.get("buy_quality_class") or "unknown")
    names.append(quality)
    if label.get("gatekeeper_buy_context_found") is True:
        names.append("gatekeeper_context_rows")
        names.append(f"gatekeeper_context_{quality}")
    else:
        names.append("no_gatekeeper_context_rows")
    close_reason = str(label.get("close_reason") or "unknown")
    names.append(f"close_reason_{close_reason}")
    gap_class = str(label.get("truth_gap_class") or "unknown")
    names.append(gap_class)
    return names


def is_execution_feasibility_reject(label: dict[str, Any]) -> bool:
    buy_quality = str(label.get("buy_quality_class") or "")
    status = str(label.get("execution_feasibility_status") or "")
    reason = str(label.get("execution_feasibility_reason") or "")
    route_status = str(label.get("route_resolution_status") or "")
    return (
        buy_quality == "buy_quality_not_executable"
        or status.startswith("not_executable")
        or route_status == "no_executable_route_account_set"
        or "no_executable_route_account_set" in reason
    )


def build_temporal_split(
    labels: list[dict[str, Any]],
    matched_feature_flags: dict[str, bool],
    *,
    min_class_rows: int,
) -> dict[str, Any]:
    rows = [
        label
        for label in labels
        if matched_feature_flags.get(str(label.get("candidate_id")), False)
        and isinstance(label.get("decision_ts_ms"), int)
    ]
    rows.sort(key=lambda row: int(row["decision_ts_ms"]))
    if len(rows) < 2:
        return {"possible": False, "reason": "not_enough_feature_rows", "splits": {}}
    midpoint = len(rows) // 2
    splits = {"early": rows[:midpoint], "late": rows[midpoint:]}
    rendered: dict[str, Any] = {}
    possible = True
    for name, part in splits.items():
        counts = Counter(str(row.get("buy_quality_class") or "unknown") for row in part)
        rendered[name] = {
            "rows": len(part),
            "first_decision_ts_ms": part[0]["decision_ts_ms"] if part else None,
            "last_decision_ts_ms": part[-1]["decision_ts_ms"] if part else None,
            "buy_quality_class_counts": counter_dict(counts),
        }
        if (
            counts.get("buy_quality_dirty_good", 0) < min_class_rows
            or counts.get("buy_quality_bad", 0) < min_class_rows
        ):
            possible = False
    return {
        "possible": possible,
        "reason": "ok" if possible else "insufficient_good_or_bad_in_temporal_half",
        "splits": rendered,
    }


def build_report(
    *,
    labels: list[dict[str, Any]],
    raw_rows: list[dict[str, Any]],
    decision_rows: list[DecisionRow],
    decision_logs: list[Path],
    max_match_drift_ms: int,
    min_feature_label_rows: int,
    min_temporal_split_class_rows: int,
    source_labels: Path,
    source_raw: Path,
    config_path: Path,
) -> dict[str, Any]:
    indexed = index_decisions(decision_rows)
    join_quality = Counter()
    feature_matrix: dict[str, Counter[str]] = defaultdict(Counter)
    feature_row_counts = Counter()
    matched_feature_flags: dict[str, bool] = {}
    matched_rows_by_candidate: dict[str, dict[str, Any]] = {}
    match_delta_values: list[int] = []
    matched_config_hash = Counter()
    matched_decision_plane = Counter()
    matched_gatekeeper_version = Counter()
    matched_log_kind = Counter()
    matched_identifier_presence = Counter()
    ambiguous = 0
    leakage_count = 0

    for label in labels:
        candidate_id = str(label.get("candidate_id") or "")
        match = match_label(label, indexed, max_match_drift_ms=max_match_drift_ms)
        join_quality[match.join_quality] += 1
        if match.ambiguous:
            ambiguous += 1
        has_features = False
        if match.match_time_delta_ms is not None:
            match_delta_values.append(match.match_time_delta_ms)
        if match.decision is not None:
            config_hash = str(match.decision.row.get("config_hash") or "unknown")
            decision_plane = str(match.decision.row.get("decision_plane") or "unknown")
            gatekeeper_version = str(match.decision.row.get("gatekeeper_version") or "unknown")
            matched_config_hash[config_hash] += 1
            matched_decision_plane[decision_plane] += 1
            matched_gatekeeper_version[gatekeeper_version] += 1
            matched_log_kind[match.decision.log_kind] += 1
            for identifier in ("candidate_id", "position_id", "join_key", "ab_record_id", "config_hash"):
                if match.decision.row.get(identifier) not in {None, ""}:
                    matched_identifier_presence[identifier] += 1
            matched_rows_by_candidate[candidate_id] = {
                "path": str(match.decision.path),
                "log_kind": match.decision.log_kind,
                "decision_plane": match.decision.row.get("decision_plane"),
                "gatekeeper_version": match.decision.row.get("gatekeeper_version"),
                "config_hash": match.decision.row.get("config_hash"),
                "feature_groups": sorted(match.decision.feature_groups),
                "match_time_delta_ms": match.match_time_delta_ms,
            }
            if has_forbidden_lifecycle_leakage(match.decision.row):
                leakage_count += 1
            has_features = bool(match.decision.feature_groups)
            for group in FEATURE_GROUPS:
                if group in match.decision.feature_groups:
                    for segment in segment_names(label):
                        feature_matrix[group][segment] += 1
            if has_features:
                for segment in segment_names(label):
                    feature_row_counts[segment] += 1
        matched_feature_flags[candidate_id] = has_features

    label_counts = Counter(str(row.get("buy_quality_class") or "unknown") for row in labels)
    market_counts = Counter(str(row.get("market_outcome_class") or "unknown") for row in labels)
    context_counts = Counter(
        "gatekeeper_context_rows" if row.get("gatekeeper_buy_context_found") is True else "no_gatekeeper_context_rows"
        for row in labels
    )
    close_reason_counts = Counter(str(row.get("close_reason") or "unknown") for row in labels)
    feature_groups_all = {group: feature_matrix[group].get("all_rows", 0) for group in FEATURE_GROUPS}
    dirty_good_with_features = feature_row_counts.get("buy_quality_dirty_good", 0)
    bad_with_features = feature_row_counts.get("buy_quality_bad", 0)
    gatekeeper_dirty_good_with_features = feature_row_counts.get("gatekeeper_context_buy_quality_dirty_good", 0)
    gatekeeper_bad_with_features = feature_row_counts.get("gatekeeper_context_buy_quality_bad", 0)
    buy_quality_denominator_labels = [
        label for label in labels if not is_execution_feasibility_reject(label)
    ]
    execution_feasibility_reject_rows = len(labels) - len(buy_quality_denominator_labels)
    execution_feasibility_coverage = (
        len(buy_quality_denominator_labels) / len(labels) if labels else None
    )
    execution_feasibility_status_counts = Counter(
        str(row.get("execution_feasibility_status") or "unknown") for row in labels
    )
    execution_feasibility_reason_counts = Counter(
        str(row.get("execution_feasibility_reason") or "unknown") for row in labels
    )
    temporal_split = build_temporal_split(
        buy_quality_denominator_labels,
        matched_feature_flags,
        min_class_rows=min_temporal_split_class_rows,
    )

    has_v3_mfs = feature_groups_all["v3_materialized_feature_snapshot"] >= min_feature_label_rows
    has_v2_feature_set = (
        feature_groups_all["gatekeeper_v2_v25_phase_fields"] >= min_feature_label_rows
        and feature_groups_all["tx_intel_fields"] >= min_feature_label_rows
        and (feature_groups_all["pdd_fields"] >= min_feature_label_rows or feature_groups_all["checkpoint_features"] >= min_feature_label_rows)
    )
    has_legacy = feature_groups_all["legacy_decision_fields"] >= min_feature_label_rows
    has_min_labels = dirty_good_with_features >= min_feature_label_rows and bad_with_features >= min_feature_label_rows
    has_gatekeeper_min_labels = (
        gatekeeper_dirty_good_with_features >= min_feature_label_rows
        and gatekeeper_bad_with_features >= min_feature_label_rows
    )
    has_temporal_split = bool(temporal_split["possible"])
    no_lifecycle_leakage = leakage_count == 0

    if has_v3_mfs and has_min_labels and has_temporal_split and no_lifecycle_leakage:
        feature_status = "v3_features_available"
        phase_b_possible = True
        phase_b_scope = "v3_selector_diagnostic_candidate"
        reason = "v3_materialized_feature_snapshot coverage meets minimum label-class counts"
    elif has_v2_feature_set and has_gatekeeper_min_labels and has_temporal_split and no_lifecycle_leakage:
        feature_status = "v2_features_available"
        phase_b_possible = True
        phase_b_scope = "diagnostic_v2_v25_feature_prototype_only"
        reason = "V2/V2.5, tx-intel, and PDD/checkpoint features are available for gatekeeper-context dirty_good and bad rows; V3 selector remains blocked without MFS"
    elif has_legacy and has_min_labels:
        feature_status = "legacy_features_only"
        phase_b_possible = False
        phase_b_scope = "legacy_diagnostic_only"
        reason = "legacy decision fields exist but V3/MFS-compatible coverage is absent"
    elif feature_row_counts.get("all_rows", 0) == 0:
        feature_status = "lifecycle_only"
        phase_b_possible = False
        phase_b_scope = "none"
        reason = "no decision-time feature rows joined to lifecycle labels"
    else:
        feature_status = "insufficient_for_selector"
        phase_b_possible = False
        phase_b_scope = "none"
        if (
            execution_feasibility_reject_rows > 0
            and len(buy_quality_denominator_labels) < max(1, min_feature_label_rows * 2)
        ):
            reason = "execution_feasibility_coverage_too_low"
        else:
            reason = "feature coverage or class balance is below configured minimums"

    diagnostic_minimums = {
        "dirty_good_with_features": dirty_good_with_features,
        "bad_with_features": bad_with_features,
        "gatekeeper_context_dirty_good_with_features": gatekeeper_dirty_good_with_features,
        "gatekeeper_context_bad_with_features": gatekeeper_bad_with_features,
        "min_feature_label_rows": min_feature_label_rows,
        "min_temporal_split_class_rows": min_temporal_split_class_rows,
        "temporal_split_possible": temporal_split["possible"],
        "lifecycle_leakage_fields_in_decision_logs": leakage_count,
    }

    return {
        "schema_version": SCHEMA_VERSION,
        "source_labels": str(source_labels),
        "source_shadow_onchain_lifecycle": str(source_raw),
        "config_path": str(config_path),
        "decision_logs": [str(path) for path in decision_logs],
        "decision_rows_total": len(decision_rows),
        "decision_log_row_counts": counter_dict(Counter(str(decision.path) for decision in decision_rows)),
        "matched_decision_identifier_presence": counter_dict(matched_identifier_presence),
        "matched_config_hash_counts": counter_dict(matched_config_hash),
        "matched_decision_plane_counts": counter_dict(matched_decision_plane),
        "matched_gatekeeper_version_counts": counter_dict(matched_gatekeeper_version),
        "matched_log_kind_counts": counter_dict(matched_log_kind),
        "rows_total": len(labels),
        "buy_quality_denominator_rows": len(buy_quality_denominator_labels),
        "execution_feasibility_reject_rows": execution_feasibility_reject_rows,
        "execution_feasibility_coverage": execution_feasibility_coverage,
        "execution_feasibility_status_counts": counter_dict(execution_feasibility_status_counts),
        "execution_feasibility_reason_counts": counter_dict(execution_feasibility_reason_counts),
        "raw_shadow_onchain_rows_total": len(raw_rows),
        "buy_quality_class_counts": counter_dict(label_counts),
        "market_outcome_class_counts": counter_dict(market_counts),
        "gatekeeper_context_split": counter_dict(context_counts),
        "close_reason_counts": counter_dict(close_reason_counts),
        "join_quality_counts": {
            "matched_by_ab_record_id": join_quality.get("matched_by_ab_record_id", 0),
            "matched_by_candidate_id": join_quality.get("matched_by_candidate_id", 0),
            "matched_by_position_id": 0,
            "matched_by_pool_mint": 0,
            "matched_by_time_window": join_quality.get("matched_by_pool_mint_time_window", 0),
            "matched_by_pool_mint_time_window": join_quality.get("matched_by_pool_mint_time_window", 0),
            "unmatched": join_quality.get("unmatched", 0),
            "ambiguous_matches": ambiguous,
        },
        "feature_group_counts_all_rows": feature_groups_all,
        "feature_group_matrix": {
            group: {
                "all_rows": feature_matrix[group].get("all_rows", 0),
                "dirty_good": feature_matrix[group].get("buy_quality_dirty_good", 0),
                "bad": feature_matrix[group].get("buy_quality_bad", 0),
                "gatekeeper_context_dirty_good": feature_matrix[group].get("gatekeeper_context_buy_quality_dirty_good", 0),
                "gatekeeper_context_bad": feature_matrix[group].get("gatekeeper_context_buy_quality_bad", 0),
            }
            for group in FEATURE_GROUPS
        },
        "rows_with_any_decision_time_features": dict(sorted(feature_row_counts.items())),
        "match_time_delta_ms": distribution(match_delta_values),
        "temporal_split": temporal_split,
        "feature_availability_status": feature_status,
        "phase_b_possible": phase_b_possible,
        "phase_b_scope": phase_b_scope,
        "v3_selector_prototype_possible": feature_status == "v3_features_available",
        "reason": reason,
        "diagnostic_minimums": diagnostic_minimums,
        "matched_row_examples": dict(list(matched_rows_by_candidate.items())[:5]),
    }


def render_markdown(report: dict[str, Any]) -> str:
    lines: list[str] = []
    lines.append("# P3.7 Shadow Lifecycle Feature Availability")
    lines.append("")
    lines.append(f"Feature availability status: `{report['feature_availability_status']}`")
    lines.append(f"Phase B possible: `{str(report['phase_b_possible']).lower()}`")
    lines.append(f"Phase B scope: `{report['phase_b_scope']}`")
    lines.append(f"V3 selector prototype possible: `{str(report['v3_selector_prototype_possible']).lower()}`")
    lines.append(f"Reason: `{report['reason']}`")
    lines.append("")
    lines.append("## Inputs")
    lines.append("")
    lines.append(f"- labels: `{report['source_labels']}`")
    lines.append(f"- shadow_onchain_lifecycle: `{report['source_shadow_onchain_lifecycle']}`")
    lines.append(f"- config: `{report['config_path']}`")
    lines.append(f"- decision_logs: `{json.dumps(report['decision_logs'], ensure_ascii=False)}`")
    lines.append("")
    lines.append("## Label Counts")
    lines.append("")
    for key in (
        "rows_total",
        "raw_shadow_onchain_rows_total",
        "buy_quality_denominator_rows",
        "execution_feasibility_reject_rows",
        "execution_feasibility_coverage",
        "execution_feasibility_status_counts",
        "execution_feasibility_reason_counts",
        "buy_quality_class_counts",
        "market_outcome_class_counts",
        "gatekeeper_context_split",
        "close_reason_counts",
        "join_quality_counts",
        "rows_with_any_decision_time_features",
        "decision_rows_total",
        "decision_log_row_counts",
        "matched_decision_identifier_presence",
        "matched_config_hash_counts",
        "matched_decision_plane_counts",
        "matched_gatekeeper_version_counts",
        "matched_log_kind_counts",
        "diagnostic_minimums",
    ):
        lines.append(f"- `{key}`: `{json.dumps(report[key], ensure_ascii=False, sort_keys=True)}`")
    lines.append("")
    lines.append("## Feature Matrix")
    lines.append("")
    lines.append("| feature_group | all rows | dirty_good | bad | gatekeeper_context_dirty_good | gatekeeper_context_bad |")
    lines.append("| --- | ---: | ---: | ---: | ---: | ---: |")
    for group, values in report["feature_group_matrix"].items():
        lines.append(
            f"| `{group}` | {values['all_rows']} | {values['dirty_good']} | {values['bad']} | "
            f"{values['gatekeeper_context_dirty_good']} | {values['gatekeeper_context_bad']} |"
        )
    lines.append("")
    lines.append("## Temporal Split")
    lines.append("")
    lines.append(f"- `temporal_split`: `{json.dumps(report['temporal_split'], ensure_ascii=False, sort_keys=True)}`")
    lines.append(f"- `match_time_delta_ms`: `{json.dumps(report['match_time_delta_ms'], ensure_ascii=False, sort_keys=True)}`")
    lines.append("")
    lines.append("## Decision")
    lines.append("")
    if report["feature_availability_status"] == "v2_features_available":
        lines.append("- GO: diagnostic V2/V2.5 feature analysis on gatekeeper-context rows.")
        lines.append("- NO-GO: V3 selector prototype until V3/MFS payload coverage exists.")
    elif report["feature_availability_status"] == "v3_features_available":
        lines.append("- GO: diagnostic V3 selector feature prototype, still no P2/live/tuning.")
    else:
        lines.append("- NO-GO: Phase B feature prototype with the current recovered dataset.")
    lines.append("- Lifecycle labels are target labels only, not decision-time features.")
    lines.append("- No P2/live/threshold/IWIM/live-sender change is authorized by this audit.")
    return "\n".join(lines).rstrip() + "\n"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--shadow-lifecycle-labels", required=True, type=Path)
    parser.add_argument("--shadow-onchain-lifecycle", required=True, type=Path)
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--decision-log", action="append", type=Path, default=[])
    parser.add_argument("--output-json", required=True, type=Path)
    parser.add_argument("--output-md", required=True, type=Path)
    parser.add_argument("--max-match-drift-ms", type=int, default=60_000)
    parser.add_argument("--min-feature-label-rows", type=int, default=100)
    parser.add_argument("--min-temporal-split-class-rows", type=int, default=20)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    labels = list(iter_jsonl(args.shadow_lifecycle_labels))
    raw_rows = list(iter_jsonl(args.shadow_onchain_lifecycle))
    decision_logs = discover_decision_logs(args.config, args.decision_log)
    decision_rows = load_decision_rows(decision_logs)
    report = build_report(
        labels=labels,
        raw_rows=raw_rows,
        decision_rows=decision_rows,
        decision_logs=decision_logs,
        max_match_drift_ms=args.max_match_drift_ms,
        min_feature_label_rows=args.min_feature_label_rows,
        min_temporal_split_class_rows=args.min_temporal_split_class_rows,
        source_labels=args.shadow_lifecycle_labels,
        source_raw=args.shadow_onchain_lifecycle,
        config_path=args.config,
    )
    write_json(args.output_json, report)
    args.output_md.parent.mkdir(parents=True, exist_ok=True)
    args.output_md.write_text(render_markdown(report), encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
