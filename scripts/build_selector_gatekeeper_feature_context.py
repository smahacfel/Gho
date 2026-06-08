#!/usr/bin/env python3
"""Build selector Gatekeeper decision-time feature context from decision logs.

This script is offline-only.  It attaches Gatekeeper raw decision-time metrics
to an existing selector candidate universe without creating denominator rows.
"""

from __future__ import annotations

import argparse
import glob
import json
import math
import statistics
from collections import Counter, defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import selector_pipeline_common as common


SCHEMA_VERSION = "gatekeeper_feature_context_v1"
SOURCE = "gatekeeper_v2_decision_log"
DECISION_PLANES = ("v25_shadow", "legacy_live", "auto")
OBSERVATION_PROFILES = ("observation_8s_10s", "observation_60s", "all")
MODEL_ALLOWED_CUTOFF_STATUSES = {"ok", "same_decision_time"}
MODEL_ALLOWED_CONTEXT_STATUS = "ok"
PASS_STATUSES = {"PASS", "PASS_CORE_WITH_CONCENTRATION_COVERAGE_WARNING"}
CORE_FEATURE_MIN_PRESENT_RATE = 0.95
CONCENTRATION_FEATURE_MIN_PRESENT_RATE = 0.60
CONCENTRATION_FEATURE_WARNING_PRESENT_RATE = 0.80

PROVENANCE_GK_COLUMNS = {
    "gk_log_schema_version",
    "gk_decision_plane",
    "gk_observation_profile",
    "gk_context_status",
    "gk_cutoff_status",
}

CORE_MARKET_CURVE_FEATURES = (
    "gk_bonding_progress_pct",
    "gk_current_market_cap_sol",
    "gk_price_change_ratio",
    "gk_curve_data_known",
    "gk_observation_start_ts_ms",
    "gk_observation_end_ts_ms",
    "gk_observation_duration_ms",
)

CONCENTRATION_SUPPORT_FEATURES = (
    "gk_hhi",
    "gk_top3_volume_pct",
)

RAW_FEATURES = (
    "max_wait_time_ms",
    "curve_wait_ms",
    "curve_wait_elapsed_ms",
    "bonding_progress_pct",
    "curve_data_known",
    "curve_finality_is_finalized",
    "current_market_cap_sol",
    "price_change_ratio",
    "max_single_tx_price_impact_pct_observed",
    "max_single_sell_impact_pct_observed",
    "total_tx_evaluated",
    "unique_tx_evaluated",
    "unique_signers_evaluated",
    "buy_count",
    "buy_ratio",
    "sell_buy_ratio",
    "sol_buy_ratio",
    "total_volume_sol",
    "avg_tx_sol",
    "volume_cv",
    "volume_gini",
    "hhi",
    "top3_volume_pct",
    "same_ms_tx_ratio",
    "max_consecutive_buys_observed",
    "dev_wallet_known",
    "dev_buy_total_sol",
    "dev_tx_ratio",
    "dev_volume_ratio",
    "dev_has_sold",
    "dev_sold_within_3s",
    "dev_sold_within_5s",
    "block0_sniped_supply_pct",
    "flip_ratio_10s",
    "buyer_pre_balance_cv",
    "cu_price_p90_1s",
    "cu_price_p90_10s",
    "priority_fee_surge_slope",
    "fee_topology_diversity_index",
    "dev_buyer_infrastructure_affinity",
    "spend_fraction_divergence",
    "demand_elasticity_score",
    "signer_cross_pool_velocity",
    "funding_source_concentration",
    "compute_unit_cluster_dominance",
    "static_fee_profile_ratio",
    "fixed_size_buy_ratio",
    "flipper_presence_ratio",
    "jito_tip_intensity",
    "early_slot_volume_dominance_buy",
    "early_top3_buy_volume_pct_3s",
    "whale_reversal_ratio_top3",
    "whale_reversal_ratio_top1",
    "iwim_confidence",
    "iwim_rug_threat_score",
    "iwim_sybil_score",
    "iwim_organic_score",
)

FSC_FEATURES = (
    "gk_fsc_buyer_sample_count",
    "gk_fsc_known_source_count",
    "gk_fsc_unknown_buyer_count",
    "gk_fsc_known_source_rate",
    "gk_fsc_unknown_buyer_rate",
)

VECTOR_FEATURES = (
    "gk_vector_event_count",
    "gk_vector_price_first",
    "gk_vector_price_last",
    "gk_vector_price_return",
    "gk_vector_price_max",
    "gk_vector_price_min",
    "gk_vector_price_drawdown",
    "gk_vector_sol_sum",
    "gk_vector_sol_max",
    "gk_vector_interval_median",
    "gk_vector_interval_min",
    "gk_vector_interval_max",
)

MODEL_FEATURE_COLUMNS = tuple(
    f"gk_{field}" for field in RAW_FEATURES
) + FSC_FEATURES + VECTOR_FEATURES

FORBIDDEN_FIELDS = {
    "decision_verdict_buy",
    "verdict_type",
    "decision_reason",
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
    "soft_score",
    "soft_points",
    "legacy_soft_points",
    "sybil_soft_points",
    "total_soft_points",
    "alpha_pass",
    "prosperity_pass",
    "prosperity_actionable",
    "prosperity_matched_branches",
    "matched",
    "branches",
    "gatekeeper_version",
    "mode",
}


def number_or_bool(value: Any) -> Any:
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        return value
    return None


def numeric_list(value: Any) -> list[float]:
    if not isinstance(value, list):
        return []
    out = []
    for item in value:
        if isinstance(item, bool):
            continue
        if isinstance(item, (int, float)) and math.isfinite(float(item)):
            out.append(float(item))
    return out


def parse_iso_ts_ms(value: Any) -> int | None:
    if not isinstance(value, str) or not value:
        return None
    raw = value.strip()
    if raw.endswith("Z"):
        raw = raw[:-1] + "+00:00"
    if "." in raw:
        prefix, suffix = raw.split(".", 1)
        tz_pos = max(suffix.find("+"), suffix.find("-"))
        if tz_pos >= 0:
            frac = suffix[:tz_pos]
            tz = suffix[tz_pos:]
        else:
            frac = suffix
            tz = ""
        raw = f"{prefix}.{frac[:6].ljust(6, '0')}{tz}"
    try:
        parsed = datetime.fromisoformat(raw)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=timezone.utc)
    return int(parsed.timestamp() * 1000)


def observation_profile(row: dict[str, Any]) -> str:
    duration = common.int_or_none(row.get("observation_duration_ms"))
    if duration is None:
        return "other"
    if 6_000 <= duration <= 12_000:
        return "observation_8s_10s"
    if 45_000 <= duration <= 75_000:
        return "observation_60s"
    return "other"


def profile_matches(profile: str, requested: str) -> bool:
    return requested == "all" or profile == requested


def feature_context_ts_ms(row: dict[str, Any]) -> int | None:
    end = common.int_or_none(row.get("observation_end_ts_ms"))
    if end is not None:
        return end
    duration = common.int_or_none(row.get("observation_duration_ms"))
    start = common.int_or_none(row.get("observation_start_ts_ms"))
    if start is not None and duration is not None:
        return start + duration
    first_seen = common.int_or_none(row.get("first_seen_ts_ms"))
    if first_seen is not None and duration is not None:
        return first_seen + duration
    for field in ("timestamp_ms", "ts_ms", "timestamp"):
        value = row.get(field)
        parsed = common.int_or_none(value)
        if parsed is not None:
            return parsed
        parsed = parse_iso_ts_ms(value)
        if parsed is not None:
            return parsed
    return None


def observation_start_ts_ms(row: dict[str, Any]) -> int | None:
    value = common.int_or_none(row.get("observation_start_ts_ms"))
    if value is not None:
        return value
    return common.int_or_none(row.get("first_seen_ts_ms"))


def cutoff_status(context_ts_ms: int | None, cutoff_ts_ms: int | None) -> str:
    if context_ts_ms is None or cutoff_ts_ms is None:
        return "unverified"
    if context_ts_ms == cutoff_ts_ms:
        return "same_decision_time"
    if context_ts_ms < cutoff_ts_ms:
        return "ok"
    return "future_after_cutoff"


def cutoff_ts_for_candidate(
    candidate_id: str,
    candidate: dict[str, Any],
    training_cutoffs: dict[str, int],
    r2_cutoffs: dict[str, int],
) -> int | None:
    if candidate_id in training_cutoffs:
        return training_cutoffs[candidate_id]
    for field in ("feature_cutoff_ts_ms", "decision_ts_ms"):
        value = common.int_or_none(candidate.get(field))
        if value is not None:
            return value
    return r2_cutoffs.get(candidate_id)


def row_cutoff(row: dict[str, Any], fields: tuple[str, ...]) -> int | None:
    values = [common.int_or_none(row.get(field)) for field in fields]
    values = [value for value in values if value is not None]
    return min(values) if values else None


def load_cutoff_index(path: Path | None, fields: tuple[str, ...]) -> dict[str, int]:
    out: dict[str, int] = {}
    for row in common.iter_json_objects(path):
        candidate_id = common.str_or_none(row.get("candidate_id"))
        if not candidate_id:
            continue
        cutoff = row_cutoff(row, fields)
        if cutoff is not None:
            out[candidate_id] = cutoff
    return out


def candidate_ts(row: dict[str, Any]) -> int | None:
    for field in ("birth_ts_ms", "first_seen_ts_ms", "decision_ts_ms"):
        value = common.int_or_none(row.get(field))
        if value is not None:
            return value
    return None


def decision_ts_for_nearest(row: dict[str, Any]) -> int | None:
    for field in ("first_seen_ts_ms", "birth_ts_ms", "observation_start_ts_ms", "decision_ts_ms"):
        value = common.int_or_none(row.get(field))
        if value is not None:
            return value
    return None


def build_candidate_indexes(
    candidates: list[dict[str, Any]],
) -> dict[str, dict[Any, list[dict[str, Any]]]]:
    indexes: dict[str, dict[Any, list[dict[str, Any]]]] = {
        "join_key": defaultdict(list),
        "candidate_id": defaultdict(list),
        "pool_id_base_mint": defaultdict(list),
        "base_mint": defaultdict(list),
    }
    for candidate in candidates:
        join_key = common.str_or_none(candidate.get("join_key"))
        if join_key:
            indexes["join_key"][join_key].append(candidate)
        candidate_id = common.str_or_none(candidate.get("candidate_id"))
        if candidate_id:
            indexes["candidate_id"][candidate_id].append(candidate)
        base_mint = common.str_or_none(candidate.get("base_mint")) or common.str_or_none(
            candidate.get("mint_id")
        )
        pool_id = common.str_or_none(candidate.get("pool_id"))
        if pool_id and base_mint:
            indexes["pool_id_base_mint"][(pool_id, base_mint)].append(candidate)
        if base_mint:
            indexes["base_mint"][base_mint].append(candidate)
    return indexes


def unique_or_ambiguous(
    method: str,
    rows: list[dict[str, Any]],
) -> tuple[str, list[dict[str, Any]], bool]:
    if len(rows) == 1:
        return method, rows, False
    if len(rows) > 1:
        return method, rows, True
    return "unmatched", [], False


def match_decision_to_candidates(
    row: dict[str, Any],
    indexes: dict[str, dict[Any, list[dict[str, Any]]]],
) -> tuple[str, list[dict[str, Any]], bool]:
    join_key = common.str_or_none(row.get("join_key"))
    if join_key:
        method, matches, ambiguous = unique_or_ambiguous(
            "join_key", indexes["join_key"].get(join_key, [])
        )
        if matches or ambiguous:
            return method, matches, ambiguous

    candidate_id = common.str_or_none(row.get("candidate_id"))
    if candidate_id:
        method, matches, ambiguous = unique_or_ambiguous(
            "candidate_id", indexes["candidate_id"].get(candidate_id, [])
        )
        if matches or ambiguous:
            return method, matches, ambiguous

    base_mint = common.str_or_none(row.get("base_mint")) or common.str_or_none(row.get("mint_id"))
    pool_id = common.str_or_none(row.get("pool_id"))
    if pool_id and base_mint:
        method, matches, ambiguous = unique_or_ambiguous(
            "pool_id_base_mint", indexes["pool_id_base_mint"].get((pool_id, base_mint), [])
        )
        if matches or ambiguous:
            return method, matches, ambiguous

    if base_mint:
        ts = decision_ts_for_nearest(row)
        nearest: list[tuple[int, dict[str, Any]]] = []
        if ts is not None:
            for candidate in indexes["base_mint"].get(base_mint, []):
                candidate_time = candidate_ts(candidate)
                if candidate_time is None:
                    continue
                diff = abs(candidate_time - ts)
                if diff <= 2_000:
                    nearest.append((diff, candidate))
        if nearest:
            min_diff = min(item[0] for item in nearest)
            matches = [candidate for diff, candidate in nearest if diff == min_diff]
            return unique_or_ambiguous("base_mint_nearest_ts", matches)

    return "unmatched", [], False


def extract_raw_features(row: dict[str, Any]) -> dict[str, Any]:
    features: dict[str, Any] = {}
    for raw_field in RAW_FEATURES:
        value = number_or_bool(row.get(raw_field))
        if value is not None:
            features[f"gk_{raw_field}"] = value
    return features


def extract_fsc_features(row: dict[str, Any]) -> dict[str, Any]:
    diagnostics = row.get("funding_source_diagnostics")
    if not isinstance(diagnostics, dict):
        return {}
    sample_count = common.float_or_none(diagnostics.get("buyer_sample_count"))
    known_count = common.float_or_none(diagnostics.get("known_source_count"))
    unknown_count = common.float_or_none(diagnostics.get("unknown_buyer_count"))
    if unknown_count is None:
        unknown_count = sum(
            value
            for value in (
                common.float_or_none(diagnostics.get("structural_unknown_buyer_count")),
                common.float_or_none(diagnostics.get("operational_unknown_buyer_count")),
                common.float_or_none(diagnostics.get("indeterminate_unknown_buyer_count")),
            )
            if value is not None
        )
    features: dict[str, Any] = {}
    if sample_count is not None:
        features["gk_fsc_buyer_sample_count"] = sample_count
    if known_count is not None:
        features["gk_fsc_known_source_count"] = known_count
    if unknown_count is not None:
        features["gk_fsc_unknown_buyer_count"] = unknown_count
    if sample_count and sample_count > 0:
        if known_count is not None:
            features["gk_fsc_known_source_rate"] = known_count / sample_count
        if unknown_count is not None:
            features["gk_fsc_unknown_buyer_rate"] = unknown_count / sample_count
    return features


def price_drawdown(prices: list[float]) -> float | None:
    if not prices:
        return None
    peak = prices[0]
    max_drawdown = 0.0
    for price in prices:
        if price > peak:
            peak = price
        if peak > 0:
            max_drawdown = max(max_drawdown, (peak - price) / peak)
    return max_drawdown


def extract_vector_features(row: dict[str, Any]) -> dict[str, Any]:
    prices = numeric_list(row.get("vectors_prices"))
    sol_amounts = numeric_list(row.get("vectors_sol_amounts"))
    offsets = numeric_list(row.get("vectors_ts_offsets_ms"))
    features: dict[str, Any] = {}
    event_count = max(len(prices), len(sol_amounts), len(offsets))
    if event_count:
        features["gk_vector_event_count"] = event_count
    if prices:
        first = prices[0]
        last = prices[-1]
        features.update(
            {
                "gk_vector_price_first": first,
                "gk_vector_price_last": last,
                "gk_vector_price_max": max(prices),
                "gk_vector_price_min": min(prices),
            }
        )
        if first:
            features["gk_vector_price_return"] = (last / first) - 1.0
        drawdown = price_drawdown(prices)
        if drawdown is not None:
            features["gk_vector_price_drawdown"] = drawdown
    if sol_amounts:
        features["gk_vector_sol_sum"] = sum(sol_amounts)
        features["gk_vector_sol_max"] = max(sol_amounts)
    if len(offsets) >= 2:
        intervals = [max(0.0, offsets[idx] - offsets[idx - 1]) for idx in range(1, len(offsets))]
        features["gk_vector_interval_median"] = statistics.median(intervals)
        features["gk_vector_interval_min"] = min(intervals)
        features["gk_vector_interval_max"] = max(intervals)
    return features


def extract_features(row: dict[str, Any]) -> dict[str, Any]:
    features = extract_raw_features(row)
    features.update(extract_fsc_features(row))
    features.update(extract_vector_features(row))
    return features


def forbidden_output_fields(row: dict[str, Any]) -> list[str]:
    detected = []
    for key in row:
        raw = key[3:] if key.startswith("gk_") else key
        if raw in FORBIDDEN_FIELDS:
            detected.append(key)
        if raw.startswith("min_") and "threshold" in raw:
            detected.append(key)
        if raw.startswith("max_") and "threshold" in raw:
            detected.append(key)
    return sorted(set(detected))


def build_output_row(
    *,
    candidate: dict[str, Any],
    decision: dict[str, Any] | None,
    join_method: str,
    context_status: str,
    cutoff: str,
) -> dict[str, Any]:
    candidate_id = common.str_or_none(candidate.get("candidate_id"))
    context_ts = feature_context_ts_ms(decision or {})
    profile = observation_profile(decision or {})
    log_schema = (decision or {}).get("log_schema_version")
    plane = (decision or {}).get("decision_plane")
    row: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "candidate_id": candidate_id,
        "pool_id": candidate.get("pool_id"),
        "base_mint": candidate.get("base_mint") or candidate.get("mint_id"),
        "join_key": (decision or {}).get("join_key") or candidate.get("join_key"),
        "join_method": join_method,
        "source": SOURCE,
        "decision_plane": plane,
        "log_schema_version": log_schema,
        "gk_context_status": context_status,
        "gk_cutoff_status": cutoff,
        "gk_observation_profile": profile,
        "gk_feature_context_ts_ms": context_ts,
        "gk_observation_start_ts_ms": observation_start_ts_ms(decision or {}),
        "gk_observation_end_ts_ms": common.int_or_none((decision or {}).get("observation_end_ts_ms")),
        "gk_observation_duration_ms": common.int_or_none(
            (decision or {}).get("observation_duration_ms")
        ),
    }
    if log_schema not in (None, ""):
        row["gk_log_schema_version"] = log_schema
    if plane not in (None, ""):
        row["gk_decision_plane"] = plane
    if decision is not None and context_status == MODEL_ALLOWED_CONTEXT_STATUS:
        row.update(extract_features(decision))
    return row


def context_sort_key(item: dict[str, Any]) -> tuple[int, int, int]:
    row = item["decision"]
    duration = common.int_or_none(row.get("observation_duration_ms")) or -1
    end = common.int_or_none(row.get("observation_end_ts_ms"))
    return (0, -duration, end if end is not None else 2**63 - 1)


def select_context(
    contexts: list[dict[str, Any]],
    *,
    decision_plane: str,
    requested_profile: str,
) -> tuple[dict[str, Any] | None, str]:
    ok_contexts = [item for item in contexts if item.get("status") == MODEL_ALLOWED_CONTEXT_STATUS]
    if decision_plane != "auto":
        ok_contexts = [
            item for item in ok_contexts if item["decision"].get("decision_plane") == decision_plane
        ]
    if not ok_contexts:
        if any(item.get("status") == "ambiguous_join" for item in contexts):
            return contexts[0], "ambiguous_join"
        return None, "no_plane_match"
    profiled = [
        item
        for item in ok_contexts
        if profile_matches(observation_profile(item["decision"]), requested_profile)
    ]
    if not profiled:
        return ok_contexts[0], "no_profile_match"
    return sorted(profiled, key=context_sort_key)[0], MODEL_ALLOWED_CONTEXT_STATUS


def feature_presence(rows: list[dict[str, Any]], columns: list[str]) -> dict[str, dict[str, Any]]:
    denominator = [row for row in rows if row.get("gk_context_status") == MODEL_ALLOWED_CONTEXT_STATUS]
    out: dict[str, dict[str, Any]] = {}
    for column in columns:
        present = sum(1 for row in denominator if row.get(column) not in (None, "", []))
        out[column] = {
            "present_rows": present,
            "denominator_rows": len(denominator),
            "present_rate": present / len(denominator) if denominator else 0.0,
        }
    return out


def build_context(
    *,
    root: Path,
    scope: str,
    source_scope: str,
    decision_plane: str,
    requested_profile: str,
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    dataset_dir = root / "datasets" / "selector" / scope
    report_dir = root / "reports" / "selector" / scope
    candidate_universe_path = dataset_dir / "candidate_universe_v1.jsonl"
    training_view_path = dataset_dir / "selector_training_view_v1.jsonl"
    r2_paths_path = dataset_dir / "r2_market_paths_v1.jsonl"
    if not candidate_universe_path.exists():
        raise FileNotFoundError(candidate_universe_path)

    candidates = list(common.iter_json_objects(candidate_universe_path))
    candidates_by_id = {
        candidate_id: candidate
        for candidate in candidates
        if (candidate_id := common.str_or_none(candidate.get("candidate_id")))
    }
    indexes = build_candidate_indexes(candidates)
    training_cutoffs = load_cutoff_index(
        training_view_path if training_view_path.exists() else None,
        ("feature_cutoff_ts_ms", "decision_ts_ms"),
    )
    r2_cutoffs = load_cutoff_index(
        r2_paths_path if r2_paths_path.exists() else None,
        ("r2_path_start_ts_ms", "feature_cutoff_ts_ms", "decision_ts_ms"),
    )

    decision_paths = sorted(
        glob.glob(
            str(root / "logs" / "rollout" / source_scope / "decisions" / "**" / "gatekeeper_v2_decisions.jsonl"),
            recursive=True,
        )
    )
    decision_rows_read = 0
    decision_rows_loaded = 0
    join_method_counts: Counter[str] = Counter()
    observation_profile_counts: Counter[str] = Counter()
    contexts_by_candidate: dict[str, list[dict[str, Any]]] = defaultdict(list)

    for path in decision_paths:
        for decision in common.iter_json_objects(Path(path)):
            decision_rows_read += 1
            if decision_plane != "auto" and decision.get("decision_plane") != decision_plane:
                continue
            profile = observation_profile(decision)
            observation_profile_counts[profile] += 1
            decision_rows_loaded += 1
            join_method, matches, ambiguous = match_decision_to_candidates(decision, indexes)
            if not matches:
                join_method_counts["unmatched"] += 1
                continue
            join_method_counts[join_method] += 1
            status = "ambiguous_join" if ambiguous else MODEL_ALLOWED_CONTEXT_STATUS
            for candidate in matches:
                candidate_id = common.str_or_none(candidate.get("candidate_id"))
                if not candidate_id:
                    continue
                contexts_by_candidate[candidate_id].append(
                    {
                        "candidate": candidate,
                        "decision": decision,
                        "join_method": join_method,
                        "status": status,
                    }
                )

    rows: list[dict[str, Any]] = []
    for candidate_id, contexts in sorted(contexts_by_candidate.items()):
        candidate = candidates_by_id.get(candidate_id)
        if not candidate:
            continue
        selected, status = select_context(
            contexts,
            decision_plane=decision_plane,
            requested_profile=requested_profile,
        )
        decision = selected["decision"] if selected else None
        join_method = selected["join_method"] if selected else "unmatched"
        context_ts = feature_context_ts_ms(decision or {})
        cutoff_ts = cutoff_ts_for_candidate(candidate_id, candidate, training_cutoffs, r2_cutoffs)
        cutoff = cutoff_status(context_ts, cutoff_ts)
        if status != MODEL_ALLOWED_CONTEXT_STATUS:
            cutoff = "unverified" if cutoff == "ok" else cutoff
        rows.append(
            build_output_row(
                candidate=candidate,
                decision=decision,
                join_method=join_method,
                context_status=status,
                cutoff=cutoff,
            )
        )

    context_status_counts = Counter(str(row.get("gk_context_status") or "unknown") for row in rows)
    cutoff_status_counts = Counter(str(row.get("gk_cutoff_status") or "unknown") for row in rows)
    emitted_gk_columns = sorted({key for row in rows for key in row if key.startswith("gk_")})
    model_feature_columns = [
        column
        for column in MODEL_FEATURE_COLUMNS
        if column in emitted_gk_columns and column not in PROVENANCE_GK_COLUMNS
    ]
    presence = feature_presence(rows, model_feature_columns)
    gate_feature_presence = feature_presence(
        rows,
        sorted(set(CORE_MARKET_CURVE_FEATURES) | set(CONCENTRATION_SUPPORT_FEATURES)),
    )
    forbidden = sorted({field for row in rows for field in forbidden_output_fields(row)})
    fail_reasons: list[str] = []
    warning_reasons: list[str] = []
    if rows and not any(row.get("gk_context_status") == MODEL_ALLOWED_CONTEXT_STATUS for row in rows):
        fail_reasons.append("no_ok_context_rows")
    if not rows:
        fail_reasons.append("no_context_rows_written")
    for required in CORE_MARKET_CURVE_FEATURES:
        if gate_feature_presence.get(required, {}).get("present_rate", 0.0) < CORE_FEATURE_MIN_PRESENT_RATE:
            fail_reasons.append(f"{required}_present_rate_below_95pct")
    concentration_rates = [
        gate_feature_presence.get(required, {}).get("present_rate", 0.0)
        for required in CONCENTRATION_SUPPORT_FEATURES
    ]
    concentration_feature_surface_status = "PASS"
    if any(rate < CONCENTRATION_FEATURE_MIN_PRESENT_RATE for rate in concentration_rates):
        concentration_feature_surface_status = "NO-GO_LOW_CONCENTRATION_COVERAGE"
        for required in CONCENTRATION_SUPPORT_FEATURES:
            if (
                gate_feature_presence.get(required, {}).get("present_rate", 0.0)
                < CONCENTRATION_FEATURE_MIN_PRESENT_RATE
            ):
                fail_reasons.append(f"{required}_present_rate_below_60pct")
    elif any(rate < CONCENTRATION_FEATURE_WARNING_PRESENT_RATE for rate in concentration_rates):
        concentration_feature_surface_status = "DEGRADED"
        for required in CONCENTRATION_SUPPORT_FEATURES:
            if (
                gate_feature_presence.get(required, {}).get("present_rate", 0.0)
                < CONCENTRATION_FEATURE_WARNING_PRESENT_RATE
            ):
                warning_reasons.append(f"{required}_present_rate_below_80pct")
    if forbidden:
        fail_reasons.append("forbidden_fields_detected")
    core_feature_surface_status = "PASS" if not any(
        reason.startswith("gk_") and reason.endswith("_present_rate_below_95pct")
        for reason in fail_reasons
    ) else "NO-GO"
    if fail_reasons:
        status = "NO-GO"
        gatekeeper_feature_context_status = (
            "NO-GO_LOW_CONCENTRATION_COVERAGE"
            if concentration_feature_surface_status == "NO-GO_LOW_CONCENTRATION_COVERAGE"
            and all(
                reason.startswith("gk_") and reason.endswith("_present_rate_below_60pct")
                for reason in fail_reasons
            )
            else "NO-GO"
        )
    elif concentration_feature_surface_status == "DEGRADED":
        status = "PASS_CORE_WITH_CONCENTRATION_COVERAGE_WARNING"
        gatekeeper_feature_context_status = "PASS_CORE_WITH_CONCENTRATION_COVERAGE_WARNING"
    else:
        status = "PASS"
        gatekeeper_feature_context_status = "PASS"

    manifest = {
        "status": status,
        "gatekeeper_feature_context_status": gatekeeper_feature_context_status,
        "core_feature_surface_status": core_feature_surface_status,
        "concentration_feature_surface_status": concentration_feature_surface_status,
        "model_policy": "missing_not_zero",
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "gatekeeper_feature_context_manifest_v1",
        "schema_version": SCHEMA_VERSION,
        "source_scope": source_scope,
        "selector_scope": scope,
        "decision_plane": decision_plane,
        "observation_profile": requested_profile,
        "candidate_universe_rows": len(candidates),
        "decision_rows_read": decision_rows_read,
        "decision_rows_loaded": decision_rows_loaded,
        "context_rows_written": len(rows),
        "join_method_counts": {
            key: int(join_method_counts.get(key, 0))
            for key in (
                "join_key",
                "candidate_id",
                "pool_id_base_mint",
                "base_mint_nearest_ts",
                "unmatched",
            )
        },
        "context_status_counts": common.counter_dict(context_status_counts),
        "cutoff_status_counts": common.counter_dict(cutoff_status_counts),
        "observation_profile_counts": common.counter_dict(observation_profile_counts),
        "feature_presence": presence,
        "gate_feature_presence": gate_feature_presence,
        "feature_present_rates": {
            column: payload.get("present_rate") for column, payload in presence.items()
        },
        "gate_feature_present_rates": {
            column: payload.get("present_rate") for column, payload in gate_feature_presence.items()
        },
        "gate_thresholds": {
            "core_feature_min_present_rate": CORE_FEATURE_MIN_PRESENT_RATE,
            "concentration_feature_min_present_rate": CONCENTRATION_FEATURE_MIN_PRESENT_RATE,
            "concentration_feature_warning_present_rate": CONCENTRATION_FEATURE_WARNING_PRESENT_RATE,
        },
        "feature_columns": emitted_gk_columns,
        "model_feature_columns": model_feature_columns,
        "provenance_columns_not_model_features": sorted(PROVENANCE_GK_COLUMNS),
        "forbidden_fields_detected": forbidden,
        "denominator_created_rows": 0,
        "fail_reasons": fail_reasons,
        "warning_reasons": warning_reasons,
        "input_paths": {
            "candidate_universe_v1": str(candidate_universe_path),
            "selector_training_view_v1": str(training_view_path) if training_view_path.exists() else None,
            "r2_market_paths_v1": str(r2_paths_path) if r2_paths_path.exists() else None,
            "decision_paths": decision_paths,
        },
    }
    report_dir.mkdir(parents=True, exist_ok=True)
    return rows, manifest


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--source-scope", required=True)
    parser.add_argument("--decision-plane", choices=DECISION_PLANES, default="v25_shadow")
    parser.add_argument("--observation-profile", choices=OBSERVATION_PROFILES, default="observation_8s_10s")
    parser.add_argument("--json", action="store_true")
    return parser


def run(args: argparse.Namespace) -> dict[str, Any]:
    rows, manifest = build_context(
        root=args.root,
        scope=args.scope,
        source_scope=args.source_scope,
        decision_plane=args.decision_plane,
        requested_profile=args.observation_profile,
    )
    dataset_dir = args.root / "datasets" / "selector" / args.scope
    report_dir = args.root / "reports" / "selector" / args.scope
    output = dataset_dir / "gatekeeper_feature_context_v1.jsonl"
    manifest_output = report_dir / "gatekeeper_feature_context_manifest_v1.json"
    common.write_jsonl(output, rows)
    common.write_json(manifest_output, manifest)
    return {
        "manifest": manifest,
        "outputs": {
            "gatekeeper_feature_context_v1": str(output),
            "gatekeeper_feature_context_manifest_v1": str(manifest_output),
        },
    }


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    summary = run(args)
    if args.json:
        print(json.dumps(summary, ensure_ascii=False, sort_keys=True))
    return 0 if summary["manifest"].get("status") in PASS_STATUSES else 2


if __name__ == "__main__":
    raise SystemExit(main())
