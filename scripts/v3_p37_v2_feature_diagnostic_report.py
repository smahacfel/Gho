#!/usr/bin/env python3
"""Diagnostic V2/V2.5 feature separation on P3.7 shadow lifecycle truth.

This report is intentionally diagnostic. It compares decision-time Gatekeeper
fields against shadow lifecycle labels, but it does not recommend runtime
thresholds and does not claim V3 selector readiness.
"""

from __future__ import annotations

import argparse
import json
import math
import random
import statistics as st
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Iterable

import v3_p37_shadow_lifecycle_feature_availability as availability


SCHEMA_VERSION = 1
DEFAULT_PLAN_REPORT = Path(
    "PLANS/AUDYT/RAPORT_P3_7_V2_V25_FEATURE_DIAGNOSTIC_BUY_HEAVY_RERUN_20260519.md"
)
SUMMARY_JSON_NAME = "feature_diagnostic_summary.json"
SUMMARY_MD_NAME = "feature_diagnostic_summary.md"

LABEL_TIMING_FEATURES = {
    "decision_to_execution_ms",
    "detection_to_execution_ms",
}
FORBIDDEN_PREDICTIVE_FIELDS = {
    "analysis_status",
    "truth_status",
    "truth_source",
    "sample_price_state",
    "market_outcome_class",
    "execution_verification_class",
    "truth_gap_class",
    "buy_quality_class",
    "entry_truth_gap_class",
    "exit_truth_gap_class",
    "curve_finality_entry",
    "curve_finality_exit",
    "entry_truth_gap_ms",
    "exit_truth_gap_ms",
    "final_pnl_sol",
    "final_pnl_pct",
    "gross_pnl_sol",
    "net_pnl_sol",
    "estimated_costs_sol",
    "close_reason",
    "position_duration_ms",
    "duration_ms",
    "total_exits",
}

NUMERIC_DECISION_FEATURES = {
    "ab_fail_count_window",
    "ab_tx_count_window",
    "ab_unique_signers_window",
    "aps_shadow_branch3_hhi",
    "aps_shadow_entry_drift_max",
    "aps_shadow_prosperity_mcap",
    "avg_cpi_depth_50tx",
    "avg_inner_ix_count_50tx",
    "avg_interval_ms",
    "avg_tx_sol",
    "bonding_progress_pct",
    "burst_ratio",
    "buy_count",
    "buy_ratio",
    "buyer_pre_balance_cv",
    "cu_price_p90_10s",
    "cu_price_p90_1s",
    "current_market_cap_sol",
    "curve_wait_elapsed_ms",
    "curve_wait_ms",
    "dev_buy_total_sol",
    "dev_buyer_infrastructure_affinity",
    "dev_paperhand_latency_ms",
    "dev_tx_ratio",
    "dev_volume_ratio",
    "demand_elasticity_score",
    "dust_filtered_count",
    "early_slot_volume_dominance_buy",
    "early_top3_buy_volume_pct_3s",
    "entry_drift_pct",
    "fee_topology_diversity_index",
    "fixed_size_buy_ratio",
    "fixed_size_buy_ratio_1e4",
    "flip_ratio_10s",
    "flipper_presence_ratio",
    "hhi",
    "interval_cv",
    "jito_tip_intensity",
    "max_avg_inner_ix_count_50tx",
    "max_avg_interval_ms",
    "max_avg_tx_sol",
    "max_bonding_progress_pct",
    "max_burst_ratio",
    "max_buy_ratio",
    "max_consecutive_buys_observed",
    "max_dev_buy_sol",
    "max_dev_buyer_infrastructure_affinity",
    "max_dev_tx_ratio",
    "max_dev_volume_ratio",
    "max_early_slot_volume_dominance_buy",
    "max_early_top3_buy_volume_pct_3s",
    "max_funding_source_concentration",
    "max_hhi",
    "max_interval_cv",
    "max_price_change_ratio",
    "max_same_ms_tx_ratio",
    "max_sell_buy_ratio",
    "max_signer_cross_pool_velocity",
    "max_single_tx_price_impact_pct",
    "max_single_tx_price_impact_pct_observed",
    "max_sol_buy_ratio",
    "max_static_fee_profile_ratio",
    "max_top3_volume_pct",
    "max_tx_per_signer",
    "max_tx_per_signer_observed",
    "max_unique_ratio",
    "max_volume_gini",
    "max_wait_time_ms",
    "min_avg_inner_ix_count_50tx",
    "min_avg_interval_ms",
    "min_avg_tx_sol",
    "min_bonding_progress_pct",
    "min_buy_count",
    "min_buy_ratio",
    "min_consecutive_buys",
    "min_dev_buy_sol",
    "min_dev_tx_ratio",
    "min_dev_volume_ratio",
    "min_dust_filtered_count",
    "min_fixed_size_buy_ratio",
    "min_interval_cv",
    "min_market_cap_sol",
    "min_phases_to_pass",
    "min_price_change_ratio",
    "min_sell_buy_ratio",
    "min_sol_buy_ratio",
    "min_static_fee_profile_ratio",
    "min_tx_count",
    "min_unique_ratio",
    "min_unique_signers",
    "min_volume_gini",
    "observation_duration_ms",
    "pdd_entry_drift_pct",
    "pdd_score",
    "pdd_whale_top3_pct",
    "phase2_passed",
    "phase3_passed",
    "phase4_passed",
    "phase5_passed",
    "phase6_passed",
    "phases_passed",
    "price_change_ratio",
    "same_ms_tx_ratio",
    "sell_buy_ratio",
    "shadow_early_elapsed_ms",
    "shadow_early_phases_passed",
    "shadow_normal_phases_passed",
    "signer_cross_pool_velocity",
    "sol_buy_ratio",
    "spend_fraction_divergence",
    "static_fee_profile_ratio",
    "top3_volume_pct",
    "total_tx_evaluated",
    "unique_ratio",
    "unique_signers_evaluated",
    "unique_tx_evaluated",
    "volume_gini",
    "whale_reversal_ratio_top1",
    "whale_reversal_ratio_top3",
}

CATEGORICAL_DECISION_FEATURES = {
    "bonding_progress_check_skipped",
    "curve_required_for_buy",
    "decision_reason",
    "decision_verdict_buy",
    "dev_has_sold",
    "dev_sold_within_3s",
    "dev_sold_within_5s",
    "dev_unknown",
    "dev_wallet_known",
    "entry_drift_anchor_quality",
    "entry_drift_anchor_source",
    "fingerprint_reason",
    "legacy_live_reason_chain",
    "legacy_live_verdict_buy",
    "legacy_live_verdict_type",
    "observation_stage",
    "pdd_entry_drift_anchor_quality",
    "pdd_entry_drift_anchor_source",
    "pdd_flash_crash_risk",
    "pdd_hard_fail",
    "pdd_price_anchor_available",
    "pdd_ramping_detected",
    "pdd_sequence_signals_available",
    "pdd_soft_flags",
    "pdd_spike_detected",
    "phase2_passed",
    "phase3_passed",
    "phase4_passed",
    "phase5_passed",
    "phase6_passed",
    "reject_on_dev_sell",
    "shadow_early_verdict",
    "shadow_normal_verdict",
    "shadow_pdd_reject_reason",
    "sybil_metric_degraded_reasons",
    "tas_available",
    "tas_unavailable_reason",
    "v25_confidence_unavailable_reason",
    "v25_shadow_observation_stage",
    "v25_shadow_reason_chain",
    "v25_shadow_verdict_type",
    "verdict_type",
}

FEATURE_FAMILIES: dict[str, set[str]] = {
    "gatekeeper_reason": {
        "decision_reason",
        "verdict_type",
        "decision_verdict_buy",
        "legacy_live_reason_chain",
        "legacy_live_verdict_type",
        "v25_shadow_reason_chain",
        "v25_shadow_verdict_type",
        "shadow_early_verdict",
        "shadow_normal_verdict",
    },
    "phase_fields": {
        "phase2_passed",
        "phase3_passed",
        "phase4_passed",
        "phase5_passed",
        "phase6_passed",
        "phases_passed",
        "min_phases_to_pass",
        "shadow_early_phases_passed",
        "shadow_normal_phases_passed",
    },
    "pdd": {
        "pdd_entry_drift_pct",
        "pdd_flash_crash_risk",
        "pdd_hard_fail",
        "pdd_price_anchor_available",
        "pdd_ramping_detected",
        "pdd_score",
        "pdd_sequence_signals_available",
        "pdd_soft_flags",
        "pdd_spike_detected",
        "pdd_whale_top3_pct",
        "shadow_pdd_reject_reason",
    },
    "tas": {
        "tas_available",
        "tas_unavailable_reason",
    },
    "market_curve": {
        "bonding_progress_pct",
        "current_market_cap_sol",
        "min_market_cap_sol",
        "price_change_ratio",
        "max_price_change_ratio",
        "curve_wait_elapsed_ms",
        "curve_wait_ms",
        "curve_required_for_buy",
    },
    "tx_intel": {
        "total_tx_evaluated",
        "unique_tx_evaluated",
        "unique_signers_evaluated",
        "buy_count",
        "buy_ratio",
        "sol_buy_ratio",
        "sell_buy_ratio",
        "avg_tx_sol",
        "avg_interval_ms",
        "interval_cv",
        "burst_ratio",
        "same_ms_tx_ratio",
    },
    "concentration_sybil": {
        "hhi",
        "top3_volume_pct",
        "volume_gini",
        "unique_ratio",
        "max_tx_per_signer",
        "max_tx_per_signer_observed",
        "signer_cross_pool_velocity",
        "fee_topology_diversity_index",
        "spend_fraction_divergence",
    },
    "alpha_manipulation": {
        "fixed_size_buy_ratio",
        "flipper_presence_ratio",
        "early_slot_volume_dominance_buy",
        "early_top3_buy_volume_pct_3s",
        "whale_reversal_ratio_top1",
        "whale_reversal_ratio_top3",
        "dev_buyer_infrastructure_affinity",
        "static_fee_profile_ratio",
        "jito_tip_intensity",
    },
    "dev": {
        "dev_buy_total_sol",
        "dev_has_sold",
        "dev_paperhand_latency_ms",
        "dev_sold_within_3s",
        "dev_sold_within_5s",
        "dev_tx_ratio",
        "dev_volume_ratio",
        "dev_wallet_known",
        "max_dev_buy_sol",
        "max_dev_tx_ratio",
        "max_dev_volume_ratio",
    },
    "timing": {
        "decision_to_execution_ms",
        "detection_to_execution_ms",
        "observation_duration_ms",
        "shadow_early_elapsed_ms",
    },
}


@dataclass(slots=True)
class DiagnosticRow:
    label: dict[str, Any]
    decision: availability.DecisionRow
    match_time_delta_ms: int | None
    numeric: dict[str, float]
    categorical: dict[str, str]


@dataclass(slots=True)
class ComparisonSpec:
    name: str
    description: str
    predicate_a: Callable[[DiagnosticRow], bool]
    predicate_b: Callable[[DiagnosticRow], bool]


def read_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as fh:
        data = json.load(fh)
    if not isinstance(data, dict):
        raise ValueError(f"expected JSON object: {path}")
    return data


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_jsonl(path: Path, rows: Iterable[dict[str, Any]]) -> int:
    path.parent.mkdir(parents=True, exist_ok=True)
    count = 0
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n")
            count += 1
    return count


def as_number(value: Any) -> float | None:
    if isinstance(value, bool):
        return 1.0 if value else 0.0
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        return float(value)
    return None


def as_category(value: Any) -> str | None:
    if value is None:
        return None
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        return str(value)
    if isinstance(value, list):
        normalized = [str(item) for item in value if item is not None]
        return ",".join(sorted(normalized)) if normalized else None
    if isinstance(value, str) and value:
        return value
    return None


def median(values: list[float]) -> float | None:
    if not values:
        return None
    return st.median(values)


def mean(values: list[float]) -> float | None:
    if not values:
        return None
    return st.fmean(values)


def stdev(values: list[float]) -> float:
    if len(values) < 2:
        return 0.0
    return st.stdev(values)


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


def auc_rank(values_a: list[float], values_b: list[float]) -> float | None:
    if not values_a or not values_b:
        return None
    sorted_b = sorted(values_b)
    wins = 0.0
    ties = 0.0
    for value in values_a:
        less = lower_bound(sorted_b, value)
        upper = upper_bound(sorted_b, value)
        wins += less
        ties += upper - less
    return (wins + 0.5 * ties) / (len(values_a) * len(values_b))


def lower_bound(values: list[float], target: float) -> int:
    lo = 0
    hi = len(values)
    while lo < hi:
        mid = (lo + hi) // 2
        if values[mid] < target:
            lo = mid + 1
        else:
            hi = mid
    return lo


def upper_bound(values: list[float], target: float) -> int:
    lo = 0
    hi = len(values)
    while lo < hi:
        mid = (lo + hi) // 2
        if values[mid] <= target:
            lo = mid + 1
        else:
            hi = mid
    return lo


def overlap(values_a: list[float], values_b: list[float], bins: int = 40) -> float | None:
    if not values_a or not values_b:
        return None
    lo = min(values_a + values_b)
    hi = max(values_a + values_b)
    if hi == lo:
        return 1.0
    width = (hi - lo) / bins
    hist_a = [0] * bins
    hist_b = [0] * bins
    for value in values_a:
        hist_a[min(int((value - lo) / width), bins - 1)] += 1
    for value in values_b:
        hist_b[min(int((value - lo) / width), bins - 1)] += 1
    return sum(min(a / len(values_a), b / len(values_b)) for a, b in zip(hist_a, hist_b))


def bootstrap_mean_delta(values_a: list[float], values_b: list[float], seed: str) -> dict[str, Any]:
    if len(values_a) < 20 or len(values_b) < 20:
        return {"status": "insufficient_sample"}
    rng = random.Random(seed)
    deltas: list[float] = []
    for _ in range(250):
        sample_a = [rng.choice(values_a) for _ in values_a]
        sample_b = [rng.choice(values_b) for _ in values_b]
        deltas.append((mean(sample_a) or 0.0) - (mean(sample_b) or 0.0))
    deltas.sort()
    lo = deltas[int(len(deltas) * 0.025)]
    hi = deltas[min(len(deltas) - 1, int(len(deltas) * 0.975))]
    status = "directional" if (lo > 0 and hi > 0) or (lo < 0 and hi < 0) else "crosses_zero"
    return {"status": status, "ci95_low": round(lo, 6), "ci95_high": round(hi, 6)}


def diagnostic_rows(
    labels: list[dict[str, Any]],
    decisions: list[availability.DecisionRow],
    *,
    max_match_drift_ms: int,
) -> tuple[list[DiagnosticRow], Counter[str]]:
    indexed = availability.index_decisions(decisions)
    rows: list[DiagnosticRow] = []
    join_quality: Counter[str] = Counter()
    for label in labels:
        match = availability.match_label(label, indexed, max_match_drift_ms=max_match_drift_ms)
        join_quality[match.join_quality] += 1
        if match.ambiguous:
            join_quality["ambiguous_matches"] += 1
        if match.decision is None or not match.decision.feature_groups:
            continue
        numeric: dict[str, float] = {}
        categorical: dict[str, str] = {}
        for field in NUMERIC_DECISION_FEATURES:
            if field in FORBIDDEN_PREDICTIVE_FIELDS:
                continue
            value = as_number(match.decision.row.get(field))
            if value is not None:
                numeric[field] = value
        for field in LABEL_TIMING_FEATURES:
            value = as_number(label.get(field))
            if value is not None:
                numeric[field] = value
        for field in CATEGORICAL_DECISION_FEATURES:
            if field in FORBIDDEN_PREDICTIVE_FIELDS:
                continue
            category = as_category(match.decision.row.get(field))
            if category is not None:
                categorical[field] = category
        rows.append(
            DiagnosticRow(
                label=label,
                decision=match.decision,
                match_time_delta_ms=match.match_time_delta_ms,
                numeric=numeric,
                categorical=categorical,
            )
        )
    return rows, join_quality


def flatten_row(row: DiagnosticRow, collection: str) -> dict[str, Any]:
    out: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "collection": collection,
        "candidate_id": row.label.get("candidate_id"),
        "position_id": row.label.get("position_id"),
        "pool_id": row.label.get("pool_id"),
        "base_mint": row.label.get("base_mint"),
        "buy_quality_class": row.label.get("buy_quality_class"),
        "market_outcome_class": row.label.get("market_outcome_class"),
        "gatekeeper_buy_context_found": row.label.get("gatekeeper_buy_context_found"),
        "close_reason_stratifier": row.label.get("close_reason"),
        "truth_gap_class_stratifier": row.label.get("truth_gap_class"),
        "decision_plane": row.decision.row.get("decision_plane"),
        "gatekeeper_version": row.decision.row.get("gatekeeper_version"),
        "config_hash": row.decision.row.get("config_hash"),
        "decision_log_kind": row.decision.log_kind,
        "match_time_delta_ms": row.match_time_delta_ms,
    }
    out.update(row.numeric)
    out.update({field: row.categorical[field] for field in sorted(row.categorical)})
    return out


def values_for(rows: list[DiagnosticRow], feature: str) -> list[float]:
    return [row.numeric[feature] for row in rows if feature in row.numeric]


def categories_for(rows: list[DiagnosticRow], feature: str) -> Counter[str]:
    return Counter(row.categorical[feature] for row in rows if feature in row.categorical)


def coverage_status(count_a: int, count_b: int, n_a: int, n_b: int, min_rows: int) -> str:
    if count_a == 0 and count_b == 0:
        return "feature_missing"
    if count_a < min_rows or count_b < min_rows:
        return "low_coverage"
    return "available"


def numeric_metrics(
    rows_a: list[DiagnosticRow],
    rows_b: list[DiagnosticRow],
    *,
    comparison_name: str,
    min_rows: int,
) -> list[dict[str, Any]]:
    features = sorted({feature for row in rows_a + rows_b for feature in row.numeric})
    metrics: list[dict[str, Any]] = []
    for feature in features:
        values_a = values_for(rows_a, feature)
        values_b = values_for(rows_b, feature)
        status = coverage_status(len(values_a), len(values_b), len(rows_a), len(rows_b), min_rows)
        if status != "available":
            metrics.append(
                {
                    "feature": feature,
                    "feature_type": "numeric",
                    "status": status,
                    "n_good": len(values_a),
                    "n_bad": len(values_b),
                    "coverage_good": round(len(values_a) / len(rows_a), 6) if rows_a else 0.0,
                    "coverage_bad": round(len(values_b) / len(rows_b), 6) if rows_b else 0.0,
                }
            )
            continue
        avg_a = mean(values_a) or 0.0
        avg_b = mean(values_b) or 0.0
        med_a = median(values_a) or 0.0
        med_b = median(values_b) or 0.0
        pooled = math.sqrt((stdev(values_a) ** 2 + stdev(values_b) ** 2) / 2.0)
        mean_delta = avg_a - avg_b
        standardized_delta = mean_delta / pooled if pooled > 1e-12 else 0.0
        auc = auc_rank(values_a, values_b)
        auc_value = auc if auc is not None else 0.5
        ovl = overlap(values_a, values_b)
        metrics.append(
            {
                "feature": feature,
                "feature_type": "numeric",
                "status": status,
                "n_good": len(values_a),
                "n_bad": len(values_b),
                "coverage_good": round(len(values_a) / len(rows_a), 6) if rows_a else 0.0,
                "coverage_bad": round(len(values_b) / len(rows_b), 6) if rows_b else 0.0,
                "mean_good": round(avg_a, 6),
                "mean_bad": round(avg_b, 6),
                "median_good": round(med_a, 6),
                "median_bad": round(med_b, 6),
                "mean_delta_good_minus_bad": round(mean_delta, 6),
                "standardized_delta": round(standardized_delta, 6),
                "effect_direction": "dirty_good_higher" if med_a > med_b else "bad_higher" if med_b > med_a else "flat",
                "auc_good_gt_bad": round(auc_value, 6),
                "auc_or_u_norm": round(auc_value, 6),
                "rank_biserial": round(2.0 * auc_value - 1.0, 6),
                "auc_separation": round(abs(auc_value - 0.5), 6),
                "overlap": round(ovl, 6) if ovl is not None else None,
                "bootstrap_ci": bootstrap_mean_delta(values_a, values_b, f"{comparison_name}:{feature}"),
            }
        )
    return metrics


def odds_ratio(a_value: int, b_value: int, a_total: int, b_total: int) -> float:
    a_other = a_total - a_value
    b_other = b_total - b_value
    if a_other == 0 and b_other == 0:
        return 1.0
    return ((a_value + 0.5) / (a_other + 0.5)) / ((b_value + 0.5) / (b_other + 0.5))


def categorical_metrics(
    rows_a: list[DiagnosticRow],
    rows_b: list[DiagnosticRow],
    *,
    min_rows: int,
) -> list[dict[str, Any]]:
    features = sorted({feature for row in rows_a + rows_b for feature in row.categorical})
    metrics: list[dict[str, Any]] = []
    for feature in features:
        counts_a = categories_for(rows_a, feature)
        counts_b = categories_for(rows_b, feature)
        available_a = sum(counts_a.values())
        available_b = sum(counts_b.values())
        status = coverage_status(available_a, available_b, len(rows_a), len(rows_b), min_rows)
        categories: list[dict[str, Any]] = []
        for category in sorted(set(counts_a) | set(counts_b)):
            a_value = counts_a.get(category, 0)
            b_value = counts_b.get(category, 0)
            support = a_value + b_value
            if support == 0:
                continue
            rate_good = a_value / support
            rate_bad = b_value / support
            ratio = odds_ratio(a_value, b_value, len(rows_a), len(rows_b))
            categories.append(
                {
                    "category": category,
                    "n_good": a_value,
                    "n_bad": b_value,
                    "support": support,
                    "dirty_good_rate": round(rate_good, 6),
                    "bad_rate": round(rate_bad, 6),
                    "odds_ratio_dirty_good": round(ratio, 6),
                    "log_odds_abs": round(abs(math.log(ratio)), 6),
                }
            )
        categories.sort(key=lambda item: (item["support"], item["log_odds_abs"]), reverse=True)
        metrics.append(
            {
                "feature": feature,
                "feature_type": "categorical",
                "status": status,
                "n_good": available_a,
                "n_bad": available_b,
                "coverage_good": round(available_a / len(rows_a), 6) if rows_a else 0.0,
                "coverage_bad": round(available_b / len(rows_b), 6) if rows_b else 0.0,
                "top_categories": categories[:12],
            }
        )
    return metrics


def feature_family_of(feature: str) -> str:
    for family, fields in FEATURE_FAMILIES.items():
        if feature in fields:
            return family
    return "other"


def family_summary(numeric: list[dict[str, Any]], categorical: list[dict[str, Any]]) -> dict[str, Any]:
    families: dict[str, dict[str, Any]] = {}
    for item in numeric:
        family = feature_family_of(str(item["feature"]))
        slot = families.setdefault(family, {"features_available": 0, "top_auc_separation": 0.0, "top_feature": None})
        if item.get("status") == "available":
            slot["features_available"] += 1
            sep = float(item.get("auc_separation") or 0.0)
            if sep > slot["top_auc_separation"]:
                slot["top_auc_separation"] = sep
                slot["top_feature"] = item["feature"]
    for item in categorical:
        family = feature_family_of(str(item["feature"]))
        slot = families.setdefault(
            family,
            {"features_available": 0, "top_auc_separation": 0.0, "top_feature": None},
        )
        if item.get("status") == "available":
            slot["features_available"] += 1
    return dict(sorted(families.items()))


def select_signal_level(top_auc_sep: float | None) -> str:
    if top_auc_sep is None:
        return "no_numeric_signal"
    if top_auc_sep >= 0.20:
        return "strong_diagnostic_signal"
    if top_auc_sep >= 0.10:
        return "moderate_diagnostic_signal"
    if top_auc_sep >= 0.05:
        return "weak_diagnostic_signal"
    return "no_material_numeric_signal"


def build_comparison(spec: ComparisonSpec, rows: list[DiagnosticRow], output_dir: Path, *, min_rows: int) -> dict[str, Any]:
    rows_a = [row for row in rows if spec.predicate_a(row)]
    rows_b = [row for row in rows if spec.predicate_b(row)]
    comparison_dir = output_dir / spec.name
    comparison_dir.mkdir(parents=True, exist_ok=True)
    a_path = comparison_dir / "A_dirty_good.jsonl"
    b_path = comparison_dir / "B_bad.jsonl"
    write_jsonl(a_path, (flatten_row(row, "A_dirty_good") for row in rows_a))
    write_jsonl(b_path, (flatten_row(row, "B_bad") for row in rows_b))

    numeric = numeric_metrics(rows_a, rows_b, comparison_name=spec.name, min_rows=min_rows)
    categorical = categorical_metrics(rows_a, rows_b, min_rows=min_rows)
    numeric_available = [item for item in numeric if item.get("status") == "available"]
    auc_ranking = sorted(
        numeric_available,
        key=lambda item: float(item.get("auc_separation") or 0.0),
        reverse=True,
    )
    overlap_ranking = sorted(
        [item for item in numeric_available if item.get("overlap") is not None],
        key=lambda item: float(item.get("overlap") or 1.0),
    )
    categorical_available = [item for item in categorical if item.get("status") == "available"]
    categorical_ranking = sorted(
        categorical_available,
        key=lambda item: max((cat.get("log_odds_abs") or 0.0) for cat in item.get("top_categories", [])[:3]) if item.get("top_categories") else 0.0,
        reverse=True,
    )
    top_auc = float(auc_ranking[0]["auc_separation"]) if auc_ranking else None
    signal_level = select_signal_level(top_auc)
    summary = {
        "schema_version": SCHEMA_VERSION,
        "comparison_name": spec.name,
        "description": spec.description,
        "n_good": len(rows_a),
        "n_bad": len(rows_b),
        "min_feature_rows": min_rows,
        "sample_size_status": "ok" if len(rows_a) >= min_rows and len(rows_b) >= min_rows else "low_sample",
        "signal_level": signal_level,
        "top_auc_separation": top_auc,
        "top_numeric_by_auc": auc_ranking[:20],
        "top_numeric_by_overlap": overlap_ranking[:20],
        "top_categorical": categorical_ranking[:20],
        "feature_family_summary": family_summary(numeric, categorical),
        "feature_coverage": {
            "numeric": [
                {
                    "feature": item["feature"],
                    "status": item["status"],
                    "n_good": item["n_good"],
                    "n_bad": item["n_bad"],
                    "coverage_good": item["coverage_good"],
                    "coverage_bad": item["coverage_bad"],
                }
                for item in numeric
            ],
            "categorical": [
                {
                    "feature": item["feature"],
                    "status": item["status"],
                    "n_good": item["n_good"],
                    "n_bad": item["n_bad"],
                    "coverage_good": item["coverage_good"],
                    "coverage_bad": item["coverage_bad"],
                }
                for item in categorical
            ],
        },
        "input_files": {
            "A_dirty_good": str(a_path),
            "B_bad": str(b_path),
        },
        "governance": {
            "diagnostic_only": True,
            "runtime_threshold_recommendation_allowed": False,
            "v3_selector_claim_allowed": False,
            "forbidden_predictive_fields": sorted(FORBIDDEN_PREDICTIVE_FIELDS),
        },
    }
    write_json(comparison_dir / "comparison_summary.json", summary)
    return summary


def comparison_specs() -> list[ComparisonSpec]:
    def quality(row: DiagnosticRow, value: str) -> bool:
        return row.label.get("buy_quality_class") == value

    def context(row: DiagnosticRow) -> bool:
        return row.label.get("gatekeeper_buy_context_found") is True

    def close_reason(row: DiagnosticRow, value: str) -> bool:
        return row.label.get("close_reason") == value

    def gap(row: DiagnosticRow, value: str) -> bool:
        return row.label.get("truth_gap_class") == value

    return [
        ComparisonSpec(
            name="gatekeeper_context_dirty_good_vs_bad",
            description="Primary selector-relevant subset: Gatekeeper-context dirty_good with features vs Gatekeeper-context bad with features.",
            predicate_a=lambda row: context(row) and quality(row, "buy_quality_dirty_good"),
            predicate_b=lambda row: context(row) and quality(row, "buy_quality_bad"),
        ),
        ComparisonSpec(
            name="all_dirty_good_vs_bad",
            description="All joined lifecycle rows with decision-time features.",
            predicate_a=lambda row: quality(row, "buy_quality_dirty_good"),
            predicate_b=lambda row: quality(row, "buy_quality_bad"),
        ),
        ComparisonSpec(
            name="target_dirty_good_vs_stoploss_bad",
            description="Target positive lifecycle rows vs StopLoss negative lifecycle rows.",
            predicate_a=lambda row: quality(row, "buy_quality_dirty_good") and close_reason(row, "Target"),
            predicate_b=lambda row: quality(row, "buy_quality_bad") and close_reason(row, "StopLoss"),
        ),
        ComparisonSpec(
            name="timestop_dirty_good_vs_stoploss_bad",
            description="TimeStop positive lifecycle rows vs StopLoss negative lifecycle rows.",
            predicate_a=lambda row: quality(row, "buy_quality_dirty_good") and close_reason(row, "TimeStop"),
            predicate_b=lambda row: quality(row, "buy_quality_bad") and close_reason(row, "StopLoss"),
        ),
        ComparisonSpec(
            name="truth_gap_clean_dirty_good_vs_bad",
            description="Clean truth-gap subset.",
            predicate_a=lambda row: quality(row, "buy_quality_dirty_good") and gap(row, "truth_gap_clean"),
            predicate_b=lambda row: quality(row, "buy_quality_bad") and gap(row, "truth_gap_clean"),
        ),
        ComparisonSpec(
            name="truth_gap_degraded_dirty_good_vs_bad",
            description="Degraded acceptable truth-gap subset.",
            predicate_a=lambda row: quality(row, "buy_quality_dirty_good") and gap(row, "truth_gap_degraded_acceptable"),
            predicate_b=lambda row: quality(row, "buy_quality_bad") and gap(row, "truth_gap_degraded_acceptable"),
        ),
        ComparisonSpec(
            name="gatekeeper_context_truth_gap_clean_dirty_good_vs_bad",
            description="Gatekeeper-context rows with clean truth-gap.",
            predicate_a=lambda row: context(row) and quality(row, "buy_quality_dirty_good") and gap(row, "truth_gap_clean"),
            predicate_b=lambda row: context(row) and quality(row, "buy_quality_bad") and gap(row, "truth_gap_clean"),
        ),
        ComparisonSpec(
            name="gatekeeper_context_truth_gap_degraded_dirty_good_vs_bad",
            description="Gatekeeper-context rows with degraded acceptable truth-gap.",
            predicate_a=lambda row: context(row) and quality(row, "buy_quality_dirty_good") and gap(row, "truth_gap_degraded_acceptable"),
            predicate_b=lambda row: context(row) and quality(row, "buy_quality_bad") and gap(row, "truth_gap_degraded_acceptable"),
        ),
    ]


def aggregate_decision(comparisons: list[dict[str, Any]]) -> dict[str, Any]:
    primary = next(
        (item for item in comparisons if item["comparison_name"] == "gatekeeper_context_dirty_good_vs_bad"),
        comparisons[0] if comparisons else {},
    )
    top_auc = primary.get("top_auc_separation")
    primary_signal = primary.get("signal_level", "unknown")
    has_primary_sample = primary.get("sample_size_status") == "ok"
    if has_primary_sample and primary_signal in {"moderate_diagnostic_signal", "strong_diagnostic_signal"}:
        recommendation = "design_forward_v3_mfs_lifecycle_collection_run"
        phase_b_diagnostic = True
        reason = "Primary gatekeeper-context V2/V2.5 comparison has enough sample and at least moderate diagnostic separation."
    elif has_primary_sample and primary_signal == "weak_diagnostic_signal":
        recommendation = "use_as_hypothesis_input_before_new_collection"
        phase_b_diagnostic = True
        reason = "Primary gatekeeper-context comparison has weak but nonzero diagnostic signal; do not tune thresholds from this."
    elif has_primary_sample:
        recommendation = "do_not_copy_v2_feature_family_without_redesign"
        phase_b_diagnostic = False
        reason = "Primary gatekeeper-context comparison has enough sample but no material numeric separation."
    else:
        recommendation = "insufficient_primary_sample"
        phase_b_diagnostic = False
        reason = "Primary gatekeeper-context comparison does not meet minimum sample requirements."
    return {
        "phase_b_v2_v25_diagnostic_allowed": phase_b_diagnostic,
        "phase_b_v3_selector_prototype_allowed": False,
        "runtime_threshold_recommendation_allowed": False,
        "new_collection_run_recommendation": recommendation,
        "primary_signal_level": primary_signal,
        "primary_top_auc_separation": top_auc,
        "reason": reason,
    }


def render_markdown(summary: dict[str, Any]) -> str:
    lines: list[str] = []
    lines.append("# P3.7-I Diagnostic V2/V2.5 Feature Prototype - Buy Heavy Rerun")
    lines.append("")
    decision = summary["decision"]
    lines.append(f"Diagnostic V2/V2.5 allowed: `{str(decision['phase_b_v2_v25_diagnostic_allowed']).lower()}`")
    lines.append(f"V3 selector prototype allowed: `{str(decision['phase_b_v3_selector_prototype_allowed']).lower()}`")
    lines.append(f"Runtime threshold recommendation allowed: `{str(decision['runtime_threshold_recommendation_allowed']).lower()}`")
    lines.append(f"Recommendation: `{decision['new_collection_run_recommendation']}`")
    lines.append(f"Reason: `{decision['reason']}`")
    lines.append("")
    lines.append("## Inputs")
    lines.append("")
    for key, value in summary["inputs"].items():
        lines.append(f"- `{key}`: `{value}`")
    lines.append("")
    lines.append("## Joined Rows")
    lines.append("")
    for key, value in summary["row_counts"].items():
        lines.append(f"- `{key}`: `{json.dumps(value, ensure_ascii=False, sort_keys=True)}`")
    lines.append("")
    lines.append("## Primary Comparison")
    lines.append("")
    primary = summary["comparisons"].get("gatekeeper_context_dirty_good_vs_bad", {})
    lines.append(f"- `n_good`: `{primary.get('n_good')}`")
    lines.append(f"- `n_bad`: `{primary.get('n_bad')}`")
    lines.append(f"- `signal_level`: `{primary.get('signal_level')}`")
    lines.append(f"- `top_auc_separation`: `{primary.get('top_auc_separation')}`")
    lines.append("")
    lines.append("### Top Numeric Features")
    for item in primary.get("top_numeric_by_auc", [])[:10]:
        lines.append(
            f"- `{item['feature']}`: auc={item.get('auc_good_gt_bad')}, "
            f"sep={item.get('auc_separation')}, rank_biserial={item.get('rank_biserial')}, "
            f"dir={item.get('effect_direction')}, overlap={item.get('overlap')}, "
            f"n_good={item.get('n_good')}, n_bad={item.get('n_bad')}"
        )
    lines.append("")
    lines.append("### Top Categorical Features")
    for item in primary.get("top_categorical", [])[:8]:
        cats = item.get("top_categories", [])[:3]
        cat_text = "; ".join(
            f"{cat['category']} good={cat['n_good']} bad={cat['n_bad']} or={cat['odds_ratio_dirty_good']}"
            for cat in cats
        )
        lines.append(f"- `{item['feature']}`: {cat_text}")
    lines.append("")
    lines.append("## Comparison Summary")
    lines.append("")
    lines.append("| comparison | n_good | n_bad | signal | top_auc_sep |")
    lines.append("| --- | ---: | ---: | --- | ---: |")
    for item in summary["comparison_order"]:
        comp = summary["comparisons"][item]
        lines.append(
            f"| `{item}` | {comp['n_good']} | {comp['n_bad']} | "
            f"`{comp['signal_level']}` | {comp['top_auc_separation']} |"
        )
    lines.append("")
    lines.append("## Feature Family Summary")
    lines.append("")
    lines.append("| family | available_features | top_auc_sep | top_feature |")
    lines.append("| --- | ---: | ---: | --- |")
    for family, values in primary.get("feature_family_summary", {}).items():
        lines.append(
            f"| `{family}` | {values.get('features_available', 0)} | "
            f"{values.get('top_auc_separation', 0.0)} | `{values.get('top_feature')}` |"
        )
    lines.append("")
    lines.append("## Governance")
    lines.append("")
    lines.append("- This is diagnostic V2/V2.5 feature analysis only.")
    lines.append("- It is not a V3 selector prototype because recovered rows have `0` V3/MFS coverage.")
    lines.append("- `close_reason`, PnL, truth gap, and curve finality are labels/stratifiers, not predictive features.")
    lines.append("- No P2/live/runtime threshold/tuning/MFS extension is authorized by this report.")
    lines.append("")
    return "\n".join(lines)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--shadow-lifecycle-labels", required=True, type=Path)
    parser.add_argument("--feature-availability", required=True, type=Path)
    parser.add_argument("--decision-log", action="append", type=Path, default=[])
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--summary-md-output", type=Path, default=DEFAULT_PLAN_REPORT)
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--markdown", action="store_true")
    parser.add_argument("--max-match-drift-ms", type=int, default=60_000)
    parser.add_argument("--min-feature-rows", type=int, default=30)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    output_dir = args.output_dir
    output_dir.mkdir(parents=True, exist_ok=True)
    want_json = args.json or not args.markdown
    want_markdown = args.markdown or not args.json

    labels = list(availability.iter_jsonl(args.shadow_lifecycle_labels))
    feature_availability = read_json(args.feature_availability)
    decision_logs = [path.resolve() for path in args.decision_log]
    if not decision_logs:
        decision_logs = [Path(path).resolve() for path in feature_availability.get("decision_logs", [])]
    decision_rows = availability.load_decision_rows(decision_logs)
    rows, join_quality = diagnostic_rows(
        labels,
        decision_rows,
        max_match_drift_ms=args.max_match_drift_ms,
    )
    comparisons = [
        build_comparison(spec, rows, output_dir, min_rows=args.min_feature_rows)
        for spec in comparison_specs()
    ]
    comparison_map = {item["comparison_name"]: item for item in comparisons}
    summary = {
        "schema_version": SCHEMA_VERSION,
        "inputs": {
            "shadow_lifecycle_labels": str(args.shadow_lifecycle_labels),
            "feature_availability": str(args.feature_availability),
            "decision_logs": [str(path) for path in decision_logs],
        },
        "row_counts": {
            "labels_total": len(labels),
            "decision_rows_total": len(decision_rows),
            "joined_feature_rows": len(rows),
            "join_quality_counts": dict(sorted(join_quality.items())),
            "buy_quality_class_counts_joined": dict(
                sorted(Counter(str(row.label.get("buy_quality_class") or "unknown") for row in rows).items())
            ),
            "gatekeeper_context_counts_joined": dict(
                sorted(
                    Counter(
                        "gatekeeper_context_rows"
                        if row.label.get("gatekeeper_buy_context_found") is True
                        else "no_gatekeeper_context_rows"
                        for row in rows
                    ).items()
                )
            ),
            "close_reason_counts_joined": dict(
                sorted(Counter(str(row.label.get("close_reason") or "unknown") for row in rows).items())
            ),
            "truth_gap_counts_joined": dict(
                sorted(Counter(str(row.label.get("truth_gap_class") or "unknown") for row in rows).items())
            ),
        },
        "comparison_order": [item["comparison_name"] for item in comparisons],
        "comparisons": comparison_map,
        "decision": aggregate_decision(comparisons),
        "governance": {
            "diagnostic_only": True,
            "v3_selector_prototype_allowed": False,
            "runtime_threshold_recommendation_allowed": False,
            "no_p2_live_tuning_mfs_extension": True,
        },
    }
    if want_json:
        write_json(output_dir / SUMMARY_JSON_NAME, summary)
    if want_markdown:
        markdown = render_markdown(summary)
        (output_dir / SUMMARY_MD_NAME).write_text(markdown, encoding="utf-8")
        args.summary_md_output.parent.mkdir(parents=True, exist_ok=True)
        args.summary_md_output.write_text(markdown, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
