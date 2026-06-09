#!/usr/bin/env python3
"""Join selector universe, features, lifecycle, and R2 path labels."""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any

import selector_pipeline_common as common


def index_feature_rows(rows: list[dict[str, Any]]) -> dict[tuple[str, str], dict[str, Any]]:
    indexed: dict[tuple[str, str], dict[str, Any]] = {}
    for row in rows:
        candidate_id = common.str_or_none(row.get("candidate_id"))
        snapshot_kind = common.str_or_none(row.get("snapshot_kind"))
        if candidate_id and snapshot_kind:
            indexed[(candidate_id, snapshot_kind)] = row
    return indexed


def choose_feature(
    candidate_id: str,
    feature_index: dict[tuple[str, str], dict[str, Any]],
    *,
    snapshot_kind: str,
    fallback_snapshot_kind: str,
) -> dict[str, Any] | None:
    return feature_index.get((candidate_id, snapshot_kind)) or feature_index.get(
        (candidate_id, fallback_snapshot_kind)
    )


def load_gatekeeper_feature_context(
    path: Path | None,
) -> tuple[dict[str, dict[str, Any]], dict[str, Any]]:
    if path is None:
        return {}, {
            "enabled": False,
            "rows": 0,
            "joined_rows": 0,
            "valid_for_model_rows": 0,
            "context_status_counts": {},
            "cutoff_status_counts": {},
            "feature_columns": [],
            "model_feature_columns": [],
        }
    if not path.exists():
        raise FileNotFoundError(path)
    rows = list(common.iter_json_objects(path))
    indexed: dict[str, dict[str, Any]] = {}
    status_counts: Counter[str] = Counter()
    cutoff_counts: Counter[str] = Counter()
    feature_columns = sorted({key for row in rows for key in row if key.startswith("gk_")})
    model_feature_columns = [
        column
        for column in feature_columns
        if column
        not in {
            "gk_log_schema_version",
            "gk_decision_plane",
            "gk_observation_profile",
            "gk_context_status",
            "gk_cutoff_status",
        }
    ]
    valid_for_model_rows = 0
    for row in rows:
        candidate_id = common.str_or_none(row.get("candidate_id"))
        if not candidate_id:
            continue
        indexed[candidate_id] = row
        status_counts[str(row.get("gk_context_status") or "unknown")] += 1
        cutoff_counts[str(row.get("gk_cutoff_status") or "unknown")] += 1
        if row.get("gk_context_status") == "ok" and row.get("gk_cutoff_status") in {
            "ok",
            "same_decision_time",
        }:
            valid_for_model_rows += 1
    return indexed, {
        "enabled": True,
        "path": str(path),
        "rows": len(rows),
        "joined_rows": len(indexed),
        "valid_for_model_rows": valid_for_model_rows,
        "context_status_counts": common.counter_dict(status_counts),
        "cutoff_status_counts": common.counter_dict(cutoff_counts),
        "feature_columns": feature_columns,
        "model_feature_columns": model_feature_columns,
    }


def attach_gatekeeper_context(row: dict[str, Any], context: dict[str, Any] | None) -> None:
    if context is None:
        return
    for key, value in context.items():
        if key.startswith("gk_") or key in {
            "gk_context_status",
            "gk_cutoff_status",
            "gk_observation_profile",
        }:
            row[key] = value
    row["gatekeeper_feature_context_joined"] = True
    row["gatekeeper_feature_context_join_method"] = context.get("join_method")
    row["gatekeeper_feature_context_source"] = context.get("source")


def finite_number(row: dict[str, Any], field: str) -> float | None:
    return common.float_or_none(row.get(field))


def first_finite_source(row: dict[str, Any], sources: list[tuple[str, str]]) -> tuple[float | None, str | None]:
    for field, source in sources:
        value = finite_number(row, field)
        if value is not None:
            return value, source
    return None, None


def materialize_evidence_sufficiency(row: dict[str, Any]) -> None:
    """Attach decision-time evidence sufficiency fields to a training row.

    The source row is already bounded by the feature cutoff and optional
    Gatekeeper decision-time context.  This helper only exposes provenance and
    eligibility semantics; it does not change R2 labels or model denominator.
    """
    tx_count, tx_source = first_finite_source(
        row,
        [
            ("tx_event_count", "flow_tx_event_count"),
            ("gk_total_tx_evaluated", "gk_total_tx_evaluated"),
            ("gk_unique_tx_evaluated", "gk_unique_tx_evaluated"),
        ],
    )
    buy_count, buy_source = first_finite_source(
        row,
        [
            ("buy_count", "flow_buy_count"),
            ("gk_buy_count", "gk_buy_count"),
        ],
    )
    unique_buyers, unique_buyers_source = first_finite_source(
        row,
        [
            ("unique_buyers", "flow_unique_buyers"),
            ("gk_unique_buyers", "gk_unique_buyers"),
        ],
    )
    unique_signers, unique_signers_source = first_finite_source(
        row,
        [
            ("unique_signers", "flow_unique_signers"),
            ("gk_unique_signers_evaluated", "gk_unique_signers_evaluated"),
        ],
    )
    total_volume, total_volume_source = first_finite_source(
        row,
        [
            ("total_volume_sol", "flow_total_volume_sol"),
            ("gk_total_volume_sol", "gk_total_volume_sol"),
        ],
    )
    net_quote_15s, net_quote_15s_source = first_finite_source(
        row,
        [("net_quote_in_15s", "flow_net_quote_in_15s")],
    )
    net_quote_30s, net_quote_30s_source = first_finite_source(
        row,
        [("net_quote_in_30s", "flow_net_quote_in_30s")],
    )
    sell_share, sell_share_source = first_finite_source(
        row,
        [("sell_share", "flow_sell_share")],
    )
    evidence_window, evidence_window_source = first_finite_source(
        row,
        [
            ("gk_observation_duration_ms", "gk_observation_duration_ms"),
            ("observation_window_ms", "training_observation_window_ms"),
        ],
    )

    row.update(
        {
            "evidence_tx_count": tx_count,
            "evidence_tx_count_source": tx_source,
            "evidence_buy_count": buy_count,
            "evidence_buy_count_source": buy_source,
            "evidence_unique_buyers": unique_buyers,
            "evidence_unique_buyers_source": unique_buyers_source,
            "evidence_unique_signers": unique_signers,
            "evidence_unique_signers_source": unique_signers_source,
            "evidence_total_volume_sol": total_volume,
            "evidence_total_volume_sol_source": total_volume_source,
            "evidence_net_quote_in_15s": net_quote_15s,
            "evidence_net_quote_in_15s_source": net_quote_15s_source,
            "evidence_net_quote_in_30s": net_quote_30s,
            "evidence_net_quote_in_30s_source": net_quote_30s_source,
            "evidence_sell_share": sell_share,
            "evidence_sell_share_source": sell_share_source,
            "evidence_window_ms": evidence_window,
            "evidence_window_source": evidence_window_source,
        }
    )

    reasons: list[str] = []
    hard_fail_reasons: list[str] = []
    partial_reasons: list[str] = []
    evidence_sources = {
        source
        for source in (
            tx_source,
            buy_source,
            unique_buyers_source,
            unique_signers_source,
            total_volume_source,
            net_quote_15s_source,
            net_quote_30s_source,
            sell_share_source,
        )
        if source
    }
    if not evidence_sources:
        reasons.append("no_evidence_sources_available")

    if tx_count is None:
        hard_fail_reasons.append("missing_evidence_tx_count")
    elif tx_count < 3:
        hard_fail_reasons.append("evidence_tx_count_below_3")
    if buy_count is None:
        hard_fail_reasons.append("missing_evidence_buy_count")
    elif buy_count < 2:
        hard_fail_reasons.append("evidence_buy_count_below_2")
    actor_count = unique_buyers if unique_buyers is not None else unique_signers
    if actor_count is None:
        hard_fail_reasons.append("missing_evidence_unique_actor_count")
    elif actor_count < 2:
        hard_fail_reasons.append("evidence_unique_actor_count_below_2")
    if unique_buyers is None and unique_signers is not None:
        partial_reasons.append("unique_actor_count_uses_gk_unique_signers_fallback")
    if total_volume is None:
        hard_fail_reasons.append("missing_evidence_total_volume_sol")
    for field in (
        "gk_bonding_progress_pct",
        "gk_current_market_cap_sol",
        "gk_price_change_ratio",
    ):
        if finite_number(row, field) is None:
            hard_fail_reasons.append(f"missing_core_curve_market:{field}")
    if tx_source and tx_source.startswith("gk_"):
        partial_reasons.append("tx_count_uses_gk_fallback")

    reasons.extend(hard_fail_reasons)
    reasons.extend(reason for reason in partial_reasons if reason not in reasons)
    if not evidence_sources:
        sufficiency_status = "unknown"
    elif hard_fail_reasons:
        sufficiency_status = "insufficient"
    elif partial_reasons:
        sufficiency_status = "partial"
    else:
        sufficiency_status = "sufficient"

    if sufficiency_status == "sufficient":
        eligibility_status = "eligible"
    elif sufficiency_status == "partial":
        eligibility_status = "score_degraded_partial_evidence"
    else:
        eligibility_status = "score_invalid_insufficient_market_evidence"
    row["evidence_source_status"] = (
        "missing_sources"
        if not evidence_sources
        else "gk_only"
        if all(source.startswith("gk_") for source in evidence_sources)
        else "flow_only"
        if all(source.startswith("flow_") for source in evidence_sources)
        else "flow_and_gk"
    )
    row["evidence_sufficiency_status"] = sufficiency_status
    row["evidence_sufficiency_reasons"] = reasons
    row["score_eligibility_status"] = eligibility_status
    row["score_eligibility_reasons"] = list(reasons)


def feature_snapshot_model_exclusion_reasons(row: dict[str, Any]) -> list[str]:
    reasons: list[str] = []
    if row.get("feature_snapshot_status") != "ok":
        reasons.append("feature_snapshot_status_not_ok")
    if common.int_or_none(row.get("feature_cutoff_ts_ms")) is None:
        reasons.append("missing_feature_cutoff_ts_ms")
    if row.get("feature_cutoff_slot") is None:
        reasons.append("missing_feature_cutoff_slot")
    if common.int_or_none(row.get("feature_observed_lag_ms")) is None:
        reasons.append("missing_feature_observed_lag_ms")
    return reasons


def feature_snapshot_model_eligible(row: dict[str, Any]) -> bool:
    return not feature_snapshot_model_exclusion_reasons(row)


def r2_training_denominator(row: dict[str, Any]) -> bool:
    return bool(
        row.get("cohort_in_scope") is True
        and row.get("stream_completeness_ok") is True
        and row.get("feature_snapshot_status") == "ok"
        and row.get("r2_label") in {"positive", "negative"}
        and row.get("r2_status") in {"positive", "negative", "resolved"}
        and row.get("r2_path_coverage_ok") is True
        and row.get("r2_horizon_matured") is True
    )


def choose_resolved_r2_temporal_split(rows: list[dict[str, Any]]) -> dict[str, str]:
    denominator_rows = [
        row
        for row in rows
        if row.get("r2_only_training_denominator") is True
        or ("r2_only_training_denominator" not in row and r2_training_denominator(row))
    ]
    ordered = sorted(
        denominator_rows,
        key=lambda row: (
            common.int_or_none(row.get("birth_ts_ms"))
            or common.int_or_none(row.get("decision_ts_ms"))
            or 0,
            str(row.get("candidate_id")),
        ),
    )
    total = len(ordered)
    splits: dict[str, str] = {}
    for idx, row in enumerate(ordered):
        candidate_id = common.str_or_none(row.get("candidate_id"))
        if not candidate_id:
            continue
        frac = idx / total if total else 0.0
        if frac < 0.70:
            split = "train"
        elif frac < 0.85:
            split = "validation"
        else:
            split = "holdout"
        splits[candidate_id] = split
    return splits


def lifecycle_ts_ms(row: dict[str, Any]) -> int | None:
    for field in ("decision_ts_ms", "entry_execution_ts_ms", "first_seen_ts_ms", "curve_t0_event_ts_ms"):
        value = common.int_or_none(row.get(field))
        if value is not None:
            return value
    return None


def candidate_time_scope(candidates: list[dict[str, Any]], *, horizon_ms: int) -> dict[str, int | None]:
    timestamps = [
        value
        for candidate in candidates
        for value in (
            common.int_or_none(candidate.get("birth_ts_ms")),
            common.int_or_none(candidate.get("decision_ts_ms")),
        )
        if value is not None
    ]
    if not timestamps:
        return {"start_ts_ms": None, "end_ts_ms": None}
    return {
        "start_ts_ms": min(timestamps) - horizon_ms,
        "end_ts_ms": max(timestamps) + horizon_ms,
    }


def in_candidate_time_scope(row: dict[str, Any], scope: dict[str, int | None]) -> bool:
    ts = lifecycle_ts_ms(row)
    start = scope.get("start_ts_ms")
    end = scope.get("end_ts_ms")
    if ts is None or start is None or end is None:
        return True
    return start <= ts <= end


def leakage_audit(
    feature_rows: list[dict[str, Any]],
    *,
    excluded_candidate_ids: set[str] | None = None,
) -> dict[str, Any]:
    excluded_candidate_ids = excluded_candidate_ids or set()
    checked_rows: list[dict[str, Any]] = []
    excluded_rows: list[dict[str, Any]] = []
    for row in feature_rows:
        candidate_id = common.str_or_none(row.get("candidate_id"))
        if candidate_id and candidate_id in excluded_candidate_ids:
            excluded_rows.append(row)
        else:
            checked_rows.append(row)
    violations = common.feature_temporal_violations(checked_rows)
    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "leakage_audit_v1",
        "status": "PASS" if not violations else "NO-GO",
        "rows_checked": len(checked_rows),
        "rows_excluded_from_model_audit": len(excluded_rows),
        "excluded_candidate_ids": sorted(excluded_candidate_ids),
        "violation_count": len(violations),
        "violations": violations[:50],
    }


def build_training_view(
    *,
    candidate_universe: Path,
    accepted_lifecycle: Path,
    feature_snapshots: Path,
    price_paths: Path | None,
    target_net_pct: float,
    stop_net_pct: float,
    horizon_ms: int,
    snapshot_kind: str,
    fallback_snapshot_kind: str,
    split_denominator: str = "candidate_universe",
    gatekeeper_feature_context: Path | None = None,
) -> tuple[list[dict[str, Any]], dict[str, Any], dict[str, Any]]:
    candidates = list(common.iter_json_objects(candidate_universe))
    accepted_rows = list(common.iter_json_objects(accepted_lifecycle))
    lifecycle_by_identity, lifecycle_ambiguous = common.build_identity_join_index(accepted_rows)
    feature_rows = list(common.iter_json_objects(feature_snapshots))
    feature_index = index_feature_rows(feature_rows)
    price_by_identity, price_ambiguous = common.build_identity_join_index(
        list(common.iter_json_objects(price_paths))
    )
    splits = common.choose_temporal_split(candidates)
    candidate_identity_index, candidate_identity_ambiguous = common.build_identity_join_index(candidates)
    lifecycle_scope = candidate_time_scope(candidates, horizon_ms=horizon_ms)
    gatekeeper_context_by_candidate, gatekeeper_context_summary = load_gatekeeper_feature_context(
        gatekeeper_feature_context
    )

    rows: list[dict[str, Any]] = []
    universe_candidate_ids = {
        common.str_or_none(candidate.get("candidate_id"))
        for candidate in candidates
        if common.str_or_none(candidate.get("candidate_id"))
    }
    accepted_total = len(accepted_rows)
    accepted_in_scope_rows = [
        row for row in accepted_rows if in_candidate_time_scope(row, lifecycle_scope)
    ]
    accepted_joined = sum(
        1
        for row in accepted_in_scope_rows
        if common.lookup_identity_join(
            row, candidate_identity_index, candidate_identity_ambiguous
        )[0]
        is not None
    )
    accepted_exact_candidate_id_joined = sum(
        1
        for row in accepted_in_scope_rows
        if common.str_or_none(row.get("candidate_id")) in universe_candidate_ids
    )
    accepted_ambiguous = sum(
        1
        for row in accepted_in_scope_rows
        if common.lookup_identity_join(
            row, candidate_identity_index, candidate_identity_ambiguous
        )[2]
    )
    accepted_missing_candidate_id = sum(
        1 for row in accepted_rows if not common.str_or_none(row.get("candidate_id"))
    )
    for candidate in candidates:
        candidate_id = common.str_or_none(candidate.get("candidate_id"))
        if not candidate_id:
            continue
        feature = choose_feature(
            candidate_id,
            feature_index,
            snapshot_kind=snapshot_kind,
            fallback_snapshot_kind=fallback_snapshot_kind,
        )
        lifecycle, lifecycle_join_key, lifecycle_ambiguous_match = common.lookup_identity_join(
            candidate, lifecycle_by_identity, lifecycle_ambiguous
        )
        price_path, price_join_key, price_ambiguous_match = common.lookup_identity_join(
            candidate, price_by_identity, price_ambiguous
        )
        feature_complete = bool(feature and feature.get("feature_snapshot_status") == "ok")
        r2 = common.classify_r2(
            price_path,
            target_net_pct=target_net_pct,
            stop_net_pct=stop_net_pct,
            horizon_ms=horizon_ms,
        )
        row: dict[str, Any] = {
            "selector_schema_version": common.SCHEMA_VERSION,
            "training_view_schema_version": common.SCHEMA_VERSION,
            "candidate_id": candidate_id,
            "base_mint": candidate.get("base_mint") or candidate.get("mint_id"),
            "pool_id": candidate.get("pool_id"),
            "bonding_curve": candidate.get("bonding_curve"),
            "quote_mint": candidate.get("quote_mint"),
            "birth_ts_ms": candidate.get("birth_ts_ms"),
            "decision_ts_ms": candidate.get("decision_ts_ms"),
            "target_net_pct": target_net_pct,
            "stop_net_pct": stop_net_pct,
            "horizon_ms": horizon_ms,
            "observation_window_ms": horizon_ms,
            "split": splits.get(candidate_id, "holdout"),
            "cohort_in_scope": candidate.get("cohort_in_scope") is True,
            "stream_completeness_ok": (
                candidate.get("stream_completeness_ok") is True
                and r2.get("r2_status") not in {"stream_incomplete", "missing_path"}
            ),
            "eligible": (
                candidate.get("candidate_universe_status") == "ok"
                and candidate.get("cohort_in_scope") is True
                and feature_complete
            ),
            "feature_snapshot_complete": feature_complete,
            "candidate_universe_status": candidate.get("candidate_universe_status"),
            "gatekeeper_verdict": candidate.get("gatekeeper_verdict"),
            "decision_verdict_buy": candidate.get("decision_verdict_buy"),
            "decision_reason": candidate.get("decision_reason"),
            "gatekeeper_v25_score": candidate.get("gatekeeper_v25_score"),
            "gatekeeper_v25_accept": candidate.get("gatekeeper_v25_accept"),
            "gatekeeper_v25_replay_artifact_version": candidate.get(
                "gatekeeper_v25_replay_artifact_version"
            ),
            "gatekeeper_v3_score": candidate.get("gatekeeper_v3_score"),
            "gatekeeper_v3_accept": candidate.get("gatekeeper_v3_accept"),
            "gatekeeper_v3_verdict": candidate.get("gatekeeper_v3_verdict"),
            "gatekeeper_v3_replay_artifact_version": candidate.get(
                "gatekeeper_v3_replay_artifact_version"
            ),
            "selector_accept_context": {
                "decision_verdict_buy": candidate.get("decision_verdict_buy"),
                "decision_reason": candidate.get("decision_reason"),
                "gatekeeper_verdict": candidate.get("gatekeeper_verdict"),
            },
            "gatekeeper_legacy_verdict_context": {
                "decision_verdict_buy": candidate.get("decision_verdict_buy"),
                "decision_reason": candidate.get("decision_reason"),
                "gatekeeper_verdict": candidate.get("gatekeeper_verdict"),
            },
            "gatekeeper_v25_verdict_context": {
                "accept": candidate.get("gatekeeper_v25_accept"),
                "score": candidate.get("gatekeeper_v25_score"),
                "confidence": candidate.get("v25_shadow_confidence"),
                "replay_artifact_version": candidate.get("gatekeeper_v25_replay_artifact_version"),
            },
            "v3_shadow_verdict": candidate.get("v3_shadow_verdict"),
            "v3_shadow_confidence": candidate.get("v3_shadow_confidence"),
            "v25_shadow_confidence": candidate.get("v25_shadow_confidence"),
            "accepted_lifecycle_joined": lifecycle is not None,
            "accepted_lifecycle_join_key": lifecycle_join_key,
            "accepted_lifecycle_candidate_id": lifecycle.get("candidate_id") if lifecycle else None,
            "accepted_lifecycle_join_status": (
                "joined"
                if lifecycle is not None
                else "ambiguous"
                if lifecycle_ambiguous_match
                else "not_found"
            ),
            "r2_price_path_join_key": price_join_key,
            "r2_price_path_join_status": (
                "joined"
                if price_path is not None
                else "ambiguous"
                if price_ambiguous_match
                else "not_found"
            ),
            "execution_feasibility_status": (
                lifecycle.get("execution_feasibility_status") if lifecycle else "not_available_r2_only"
            ),
            "execution_only_failure": (
                bool(lifecycle.get("execution_only_failure")) if lifecycle else False
            ),
            "execution_realization_available": lifecycle is not None,
            "phase3_dataset_kind": "r2_only",
            "label_resolved": r2.get("r2_label") in {"positive", "negative"},
            "gatekeeper_feature_context_joined": False,
        }
        if feature:
            for key, value in feature.items():
                if key in {"selector_schema_version", "feature_snapshot_schema_version"}:
                    continue
                row[key] = value
        else:
            row.update(
                {
                    "snapshot_kind": None,
                    "feature_cutoff_ts_ms": None,
                    "feature_cutoff_slot": None,
                    "feature_source": None,
                    "feature_observed_lag_ms": None,
                    "feature_snapshot_status": "missing_feature_snapshot",
                    "feature_snapshot_incomplete_reason": "missing_feature_snapshot",
                    "feature_time_provenance_ok": False,
                }
            )
        if lifecycle:
            for key in (
                "r1_label",
                "r1_label_reason",
                "r1_excluded_reason",
                "r1_gray_reason",
                "execution_realized",
                "close_reason",
                "truth_status",
                "truth_source",
                "final_pnl_pct",
            ):
                row[key] = lifecycle.get(key)
        attach_gatekeeper_context(row, gatekeeper_context_by_candidate.get(candidate_id))
        materialize_evidence_sufficiency(row)
        row.update(r2)
        row["r2_label_resolved"] = row.get("r2_label") in {"positive", "negative"}
        row["label_resolved"] = row["r2_label_resolved"]
        row["label_excluded_reason"] = row.get("r2_excluded_reason") or row.get("r1_excluded_reason")
        feature_exclusion_reasons = feature_snapshot_model_exclusion_reasons(row)
        row["feature_snapshot_model_exclusion_reasons"] = feature_exclusion_reasons
        row["model_eligible"] = False
        row["training_row_status"] = "not_r2_training_denominator"
        if feature_exclusion_reasons:
            row["training_row_status"] = "excluded_feature_snapshot_incomplete"
            row["r2_only_training_denominator"] = False
            row["model_eligible"] = False
        else:
            row["r2_only_training_denominator"] = r2_training_denominator(row)
            row["model_eligible"] = row["r2_only_training_denominator"]
            if row["r2_only_training_denominator"]:
                row["training_row_status"] = "model_eligible"
        rows.append(row)

    if split_denominator == "resolved_r2":
        resolved_splits = choose_resolved_r2_temporal_split(rows)
        for row in rows:
            candidate_id = common.str_or_none(row.get("candidate_id"))
            if candidate_id in resolved_splits:
                row["split"] = resolved_splits[candidate_id]

    label_counts = Counter(str(row.get("r2_label") or row.get("r2_status") or "unknown") for row in rows)
    denominator_rows = [row for row in rows if row.get("r2_only_training_denominator") is True]
    feature_snapshot_incomplete_excluded_rows = [
        row for row in rows if row.get("training_row_status") == "excluded_feature_snapshot_incomplete"
    ]
    missing_feature_cutoff_excluded_rows = [
        row
        for row in feature_snapshot_incomplete_excluded_rows
        if "missing_feature_cutoff_ts_ms" in row.get("feature_snapshot_model_exclusion_reasons", [])
        or "missing_feature_cutoff_slot" in row.get("feature_snapshot_model_exclusion_reasons", [])
    ]
    split_counts = Counter(str(row.get("split") or "unknown") for row in denominator_rows)
    accepted_join_scope_rows = len(accepted_in_scope_rows)
    accepted_join_completeness = (
        accepted_joined / accepted_join_scope_rows if accepted_join_scope_rows else 1.0
    )
    coverage_fail_reasons = []
    if not denominator_rows:
        coverage_fail_reasons.append("no_resolved_r2_denominator")
    if accepted_join_completeness < 0.99:
        coverage_fail_reasons.append("accepted_lifecycle_join_completeness_below_99pct")
    coverage = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "label_coverage_v1",
        "status": "ok" if not coverage_fail_reasons else "NO-GO",
        "fail_reasons": coverage_fail_reasons,
        "phase3_dataset_kind": "r2_only",
        "split_denominator": split_denominator,
        "candidate_rows": len(candidates),
        "training_rows": len(rows),
        "accepted_lifecycle_rows": accepted_total,
        "accepted_lifecycle_join_scope": "candidate_event_time_window",
        "accepted_lifecycle_join_scope_start_ts_ms": lifecycle_scope.get("start_ts_ms"),
        "accepted_lifecycle_join_scope_end_ts_ms": lifecycle_scope.get("end_ts_ms"),
        "accepted_lifecycle_join_scope_rows": accepted_join_scope_rows,
        "accepted_lifecycle_out_of_scope_rows": accepted_total - accepted_join_scope_rows,
        "accepted_lifecycle_joined": accepted_joined,
        "accepted_lifecycle_exact_candidate_id_joined": accepted_exact_candidate_id_joined,
        "accepted_lifecycle_identity_ambiguous": accepted_ambiguous,
        "accepted_lifecycle_missing_candidate_id": accepted_missing_candidate_id,
        "accepted_lifecycle_join_completeness": accepted_join_completeness,
        "accepted_lifecycle_join_gate": {
            "required_min": 0.99,
            "status": "PASS" if accepted_join_completeness >= 0.99 else "NO-GO",
        },
        "resolved_r2_rows": len(denominator_rows),
        "r2_training_denominator_rows": len(denominator_rows),
        "effective_r2_training_denominator_rows": len(denominator_rows),
        "feature_snapshot_incomplete_excluded_rows": len(feature_snapshot_incomplete_excluded_rows),
        "missing_feature_cutoff_excluded_rows": len(missing_feature_cutoff_excluded_rows),
        "excluded_feature_snapshot_incomplete_candidate_ids": sorted(
            common.str_or_none(row.get("candidate_id")) or ""
            for row in feature_snapshot_incomplete_excluded_rows
        ),
        "r2_training_denominator_split_counts": common.counter_dict(split_counts),
        "r2_label_counts": common.counter_dict(label_counts),
        "matured_r2_resolved_rate": (
            len(denominator_rows) / len(rows) if rows else None
        ),
        "r2_ssot_contract": "Yellowstone/Geyser AccountUpdates, DIAG_ACCOUNT_UPDATE_RELAY, canonical account-state snapshots; RPC only flagged backfill/enrichment.",
        "precision_r2_denominator_contract": common.PRECISION_R2_DENOMINATOR_CONTRACT,
        "precision_r2_holdout_denominator": common.r2_counts(
            dict(row, selector_accept=row.get("decision_verdict_buy") is True) for row in rows
        ),
        "gatekeeper_feature_context": {
            **gatekeeper_context_summary,
            "training_rows_joined": sum(
                1 for row in rows if row.get("gatekeeper_feature_context_joined") is True
            ),
            "training_valid_for_model_rows": sum(
                1
                for row in rows
                if row.get("gatekeeper_feature_context_joined") is True
                and row.get("gk_context_status") == "ok"
                and row.get("gk_cutoff_status") in {"ok", "same_decision_time"}
            ),
        },
    }
    excluded_candidate_ids = {
        common.str_or_none(row.get("candidate_id"))
        for row in feature_snapshot_incomplete_excluded_rows
        if common.str_or_none(row.get("candidate_id"))
    }
    return rows, coverage, leakage_audit(
        feature_rows,
        excluded_candidate_ids={candidate_id for candidate_id in excluded_candidate_ids if candidate_id},
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate-universe", required=True, type=Path)
    parser.add_argument("--accepted-lifecycle", required=True, type=Path)
    parser.add_argument("--feature-snapshots", required=True, type=Path)
    parser.add_argument("--price-paths", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--label-coverage-output", type=Path)
    parser.add_argument("--leakage-audit-output", type=Path)
    parser.add_argument("--target-net-pct", required=True, type=float)
    parser.add_argument("--stop-net-pct", required=True, type=float)
    parser.add_argument("--horizon-ms", required=True, type=int)
    parser.add_argument("--snapshot-kind", default="decision")
    parser.add_argument("--fallback-snapshot-kind", default="birth+30s")
    parser.add_argument(
        "--split-denominator",
        choices=["candidate_universe", "resolved_r2"],
        default="candidate_universe",
    )
    parser.add_argument("--gatekeeper-feature-context", type=Path)
    parser.add_argument("--json", action="store_true")
    return parser


def run(args: argparse.Namespace) -> dict[str, Any]:
    rows, coverage, audit = build_training_view(
        candidate_universe=args.candidate_universe,
        accepted_lifecycle=args.accepted_lifecycle,
        feature_snapshots=args.feature_snapshots,
        price_paths=args.price_paths,
        target_net_pct=args.target_net_pct,
        stop_net_pct=args.stop_net_pct,
        horizon_ms=args.horizon_ms,
        snapshot_kind=args.snapshot_kind,
        fallback_snapshot_kind=args.fallback_snapshot_kind,
        split_denominator=args.split_denominator,
        gatekeeper_feature_context=args.gatekeeper_feature_context,
    )
    common.write_jsonl(args.output, rows)
    if args.label_coverage_output:
        common.write_json(args.label_coverage_output, coverage)
    if args.leakage_audit_output:
        common.write_json(args.leakage_audit_output, audit)
    return {"label_coverage": coverage, "leakage_audit": audit}


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    summary = run(args)
    if args.json:
        print(json.dumps(summary, ensure_ascii=False, sort_keys=True))
    return (
        0
        if summary["leakage_audit"]["status"] == "PASS"
        and summary["label_coverage"]["status"] == "ok"
        else 2
    )


if __name__ == "__main__":
    raise SystemExit(main())
