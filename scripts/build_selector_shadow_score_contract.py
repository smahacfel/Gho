#!/usr/bin/env python3
"""Build P3K shadow-only selector score contract and offline score rows.

This is contract/report tooling only. It does not change Gatekeeper decisions,
runtime behavior, execution, send path, or production thresholds.
"""

from __future__ import annotations

import argparse
import csv
import json
from collections import Counter
from pathlib import Path
from typing import Any

import build_selector_r2only_baseline_report as baseline
import build_selector_r2only_model_candidate as p3g
import selector_pipeline_common as common


ARTIFACT = "selector_shadow_score_contract_v1"
STATUS_PASS = "P3K_PASS_SHADOW_SCORE_CONTRACT_DRAFT"
SCORE_VERSION = "selector_shadow_score_combined_simple_v1"
DEFAULT_CANDIDATE_ID = "combined:simple_feature_score_v1"
TARGET_NET_PCT = 40.0
STOP_NET_PCT = 40.0
HORIZON_MS = 60_000

FLOW_FEATURES = (
    "net_quote_in_15s",
    "net_quote_in_30s",
    "trade_rate",
    "unique_buyers",
    "sell_share",
    "top1_wallet_share",
    "buyer_hhi",
)
GK_CURVE_MARKET_CORE = (
    "gk_bonding_progress_pct",
    "gk_current_market_cap_sol",
    "gk_price_change_ratio",
    "gk_curve_data_known",
    "gk_observation_duration_ms",
    "gk_curve_wait_elapsed_ms",
)
REQUIRED_CORE_CURVE_MARKET = (
    "gk_bonding_progress_pct",
    "gk_current_market_cap_sol",
    "gk_price_change_ratio",
)
GK_FLOW_CONCENTRATION_SUPPORT = (
    "gk_hhi",
    "gk_top3_volume_pct",
    "gk_buy_ratio",
    "gk_sell_buy_ratio",
    "gk_total_volume_sol",
    "gk_unique_signers_evaluated",
    "gk_buy_count",
    "gk_volume_gini",
)
CONCENTRATION_FEATURES = ("gk_hhi", "gk_top3_volume_pct")
DEV_RISK_FEATURES = (
    "gk_dev_has_sold",
    "gk_dev_volume_ratio",
    "gk_dev_tx_ratio",
    "gk_dev_sold_within_3s",
    "gk_dev_sold_within_5s",
)
INFRA_ALPHA_RAW_METRICS = (
    "gk_fee_topology_diversity_index",
    "gk_spend_fraction_divergence",
    "gk_demand_elasticity_score",
    "gk_signer_cross_pool_velocity",
    "gk_priority_fee_surge_slope",
)
FSC_DIAGNOSTICS_EVIDENCE_ONLY = (
    "gk_fsc_known_source_rate",
    "gk_fsc_unknown_buyer_rate",
    "gk_fsc_buyer_sample_count",
)
THRESHOLD_LABELS = (
    "top_10",
    "top_25",
    "q0.99",
    "q0.98",
    "q0.975",
    "target_precision_0.7",
)


def read_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as fh:
        payload = json.load(fh)
    if not isinstance(payload, dict):
        raise ValueError(f"expected JSON object in {path}")
    return payload


def write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n")


def write_csv(path: Path, rows: list[dict[str, Any]], fieldnames: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames)
        writer.writeheader()
        for row in rows:
            writer.writerow({field: row.get(field) for field in fieldnames})


def denominator_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [row for row in rows if baseline.r2only_denominator(row)]


def row_id(row: dict[str, Any]) -> str:
    return common.str_or_none(row.get("candidate_id")) or ""


def split_rows(rows: list[dict[str, Any]], split: str) -> list[dict[str, Any]]:
    return [row for row in rows if row.get("split") == split]


def label_positive(row: dict[str, Any]) -> bool:
    return row.get("r2_label") == "positive"


def candidate_from_p3j(p3j_report: dict[str, Any], candidate_id: str) -> dict[str, Any]:
    for candidate in p3j_report.get("candidates") or []:
        if isinstance(candidate, dict) and candidate.get("candidate_id") == candidate_id:
            return candidate
    raise ValueError(f"P3J candidate not found: {candidate_id}")


def selected_thresholds(p3j_report: dict[str, Any], candidate_id: str) -> list[dict[str, Any]]:
    raw = (p3j_report.get("threshold_grid") or {}).get(candidate_id)
    if not isinstance(raw, list):
        raise ValueError(f"P3J threshold grid missing candidate: {candidate_id}")
    out = []
    for item in raw:
        if not isinstance(item, dict):
            continue
        if item.get("threshold_label") in THRESHOLD_LABELS:
            out.append(item)
    return out


def threshold_value_for_row(row: dict[str, Any], threshold: dict[str, Any], scores: dict[str, float]) -> bool:
    label = str(threshold.get("threshold_label") or "")
    if label.startswith("top_"):
        rank = common.int_or_none(row.get("score_rank_split"))
        try:
            k = int(label.removeprefix("top_"))
        except ValueError:
            k = None
        return bool(rank is not None and k is not None and rank <= k)
    threshold_value = common.float_or_none(threshold.get("threshold"))
    if threshold_value is None:
        return False
    return scores.get(str(row.get("candidate_id") or ""), float("-inf")) >= threshold_value


def precision_from_threshold(threshold: dict[str, Any], split: str) -> float | None:
    payload = threshold.get(split)
    if isinstance(payload, dict):
        return common.float_or_none(payload.get("precision"))
    return None


def feature_groups(candidate_features: list[str]) -> dict[str, list[str]]:
    feature_set = set(candidate_features)
    groups = {
        "flow": [feature for feature in FLOW_FEATURES if feature in feature_set],
        "gk_curve_market_core": [feature for feature in GK_CURVE_MARKET_CORE if feature in feature_set],
        "gk_flow_concentration_support": [
            feature for feature in GK_FLOW_CONCENTRATION_SUPPORT if feature in feature_set
        ],
        "dev_risk": [feature for feature in DEV_RISK_FEATURES if feature in feature_set],
        "infrastructure_alpha_raw_metrics": [
            feature for feature in INFRA_ALPHA_RAW_METRICS if feature in feature_set
        ],
        "fsc_diagnostics_evidence_only": [
            feature for feature in FSC_DIAGNOSTICS_EVIDENCE_ONLY if feature in feature_set
        ],
    }
    grouped = {feature for values in groups.values() for feature in values}
    groups["other_combined_features"] = [feature for feature in candidate_features if feature not in grouped]
    return groups


def missing_features(row: dict[str, Any], features: list[str]) -> list[str]:
    return [
        feature
        for feature in features
        if p3g.feature_value(row, feature) is None
    ]


def score_validity(row: dict[str, Any], candidate_features: list[str]) -> tuple[str, dict[str, Any]]:
    missing = missing_features(row, candidate_features)
    required_missing = missing_features(row, list(REQUIRED_CORE_CURVE_MARKET))
    concentration_missing = missing_features(row, list(CONCENTRATION_FEATURES))
    gk_context_available = p3g.gk_row_valid_for_model(row)
    cutoff_verified = row.get("gk_cutoff_status") in p3g.GK_MODEL_ALLOWED_CUTOFF_STATUSES
    if not cutoff_verified:
        status = "score_invalid_cutoff_unverified"
    elif required_missing:
        status = "score_invalid_missing_core_curve_market"
    elif not gk_context_available:
        status = "score_degraded_missing_gk_context"
    elif concentration_missing:
        status = "score_degraded_missing_concentration"
    else:
        status = "score_valid"
    return status, {
        "feature_missing_count": len(missing),
        "required_feature_missing_count": len(required_missing),
        "missing_features": missing,
        "required_missing_features": required_missing,
        "concentration_available": not concentration_missing,
        "concentration_missing_features": concentration_missing,
        "gk_context_available": gk_context_available,
        "cutoff_verified": cutoff_verified,
    }


def reason_vector(row: dict[str, Any], ranges: dict[str, dict[str, float]], features: list[str], validity: dict[str, Any]) -> dict[str, list[str]]:
    positive: list[str] = []
    negative: list[str] = []
    for feature in features:
        value = p3g.feature_value(row, feature)
        if value is None:
            continue
        normalized = p3g.normalized_feature(row, feature, ranges)
        direction = ranges.get(feature, {}).get("direction", 1.0)
        if normalized >= 0.75:
            positive.append(f"high_{feature}" if direction >= 0 else f"low_{feature}")
        elif normalized <= 0.25:
            negative.append(f"low_{feature}" if direction >= 0 else f"high_{feature}")
    for feature in validity.get("concentration_missing_features") or []:
        negative.append("gk_concentration_missing" if feature == "gk_hhi" else f"{feature}_missing")
    if not validity.get("gk_context_available"):
        negative.append("gk_context_missing_or_unverified")
    return {
        "positive": positive[:12],
        "negative": negative[:12],
        "missing": list(validity.get("missing_features") or [])[:40],
    }


def order_ids(rows: list[dict[str, Any]], score_map: dict[str, float]) -> list[str]:
    sortable = []
    for row in rows:
        candidate_id = row_id(row)
        if not candidate_id or candidate_id not in score_map:
            continue
        sortable.append(
            (
                score_map[candidate_id],
                common.int_or_none(row.get("birth_ts_ms")) or common.int_or_none(row.get("decision_ts_ms")) or 0,
                candidate_id,
            )
        )
    sortable.sort(key=lambda item: (-item[0], item[1], item[2]))
    return [candidate_id for _score, _ts, candidate_id in sortable]


def rank_maps(rows: list[dict[str, Any]], score_map: dict[str, float]) -> tuple[dict[str, int], dict[str, dict[str, int]]]:
    global_rank = {candidate_id: idx for idx, candidate_id in enumerate(order_ids(rows, score_map), 1)}
    split_rank: dict[str, dict[str, int]] = {}
    for split in p3g.SPLITS:
        split_rank[split] = {
            candidate_id: idx
            for idx, candidate_id in enumerate(order_ids(split_rows(rows, split), score_map), 1)
        }
    return global_rank, split_rank


def build_score_rows(
    rows: list[dict[str, Any]],
    *,
    candidate_id: str,
    features: list[str],
    thresholds: list[dict[str, Any]],
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    train_rows = split_rows(rows, "train")
    ranges = p3g.feature_ranges(train_rows, features)
    simple = p3g.simple_scores(rows, train_rows=train_rows, features=features)
    score_map = {
        candidate_id_value: payload["simple_feature_score_v1"]
        for candidate_id_value, payload in simple.items()
    }
    global_rank, split_rank = rank_maps(rows, score_map)
    out: list[dict[str, Any]] = []
    status_counts: Counter[str] = Counter()
    for row in rows:
        candidate_row_id = row_id(row)
        if not candidate_row_id:
            continue
        status, validity = score_validity(row, features)
        status_counts[status] += 1
        score = score_map.get(candidate_row_id)
        score_row = {
            "candidate_id": candidate_row_id,
            "base_mint": row.get("base_mint"),
            "split": row.get("split"),
            "r2_label": row.get("r2_label"),
            "selector_shadow_score": score,
            "score_validity_status": status,
            "score_candidate_id": candidate_id,
            "score_version": SCORE_VERSION,
            "score_rank_global": global_rank.get(candidate_row_id),
            "score_rank_split": split_rank.get(str(row.get("split") or ""), {}).get(candidate_row_id),
            "feature_missing_count": validity["feature_missing_count"],
            "required_feature_missing_count": validity["required_feature_missing_count"],
            "concentration_available": validity["concentration_available"],
            "gk_context_available": validity["gk_context_available"],
            "threshold_pass_top10_equiv": False,
            "threshold_pass_top25_equiv": False,
            "threshold_pass_q99": False,
            "threshold_pass_q98": False,
            "threshold_pass_q975": False,
            "threshold_pass_target_precision_0_70": False,
            "reason_vector": reason_vector(row, ranges, features, validity),
            "non_claims": {
                "changes_gatekeeper_decision": False,
                "changes_execution": False,
                "production_signal": False,
            },
        }
        for threshold in thresholds:
            label = str(threshold.get("threshold_label") or "")
            passed = threshold_value_for_row(score_row, threshold, score_map)
            if label == "top_10":
                score_row["threshold_pass_top10_equiv"] = passed
            elif label == "top_25":
                score_row["threshold_pass_top25_equiv"] = passed
            elif label == "q0.99":
                score_row["threshold_pass_q99"] = passed
            elif label == "q0.98":
                score_row["threshold_pass_q98"] = passed
            elif label == "q0.975":
                score_row["threshold_pass_q975"] = passed
            elif label == "target_precision_0.7":
                score_row["threshold_pass_target_precision_0_70"] = passed
        out.append(score_row)
    return out, {
        "score_ranges": ranges,
        "score_validity_status_counts": common.counter_dict(status_counts),
    }


def selected_metric(rows: list[dict[str, Any]], selected_ids: set[str]) -> dict[str, Any]:
    selected = [row for row in rows if row_id(row) in selected_ids]
    positives = sum(1 for row in rows if label_positive(row))
    selected_positive = sum(1 for row in selected if label_positive(row))
    precision = selected_positive / len(selected) if selected else None
    return {
        "rows": len(rows),
        "selected_count": len(selected),
        "positive_count": selected_positive,
        "negative_count": len(selected) - selected_positive,
        "precision": precision,
        "recall": selected_positive / positives if positives else None,
        "accept_rate": len(selected) / len(rows) if rows else None,
        "ev_proxy_pct": (
            precision * TARGET_NET_PCT - (1.0 - precision) * STOP_NET_PCT
            if precision is not None
            else None
        ),
    }


def threshold_summary(score_rows: list[dict[str, Any]], thresholds: list[dict[str, Any]]) -> list[dict[str, Any]]:
    rows_by_id = {row["candidate_id"]: row for row in score_rows}
    out: list[dict[str, Any]] = []
    field_by_label = {
        "top_10": "threshold_pass_top10_equiv",
        "top_25": "threshold_pass_top25_equiv",
        "q0.99": "threshold_pass_q99",
        "q0.98": "threshold_pass_q98",
        "q0.975": "threshold_pass_q975",
        "target_precision_0.7": "threshold_pass_target_precision_0_70",
    }
    for threshold in thresholds:
        label = str(threshold.get("threshold_label") or "")
        field = field_by_label.get(label)
        if not field:
            continue
        selected_ids = {
            candidate_id
            for candidate_id, row in rows_by_id.items()
            if row.get(field) is True
        }
        row = {
            "threshold_label": label,
            "threshold_type": threshold.get("threshold_type"),
            "threshold": threshold.get("threshold"),
            "p3j_train_precision": precision_from_threshold(threshold, "train"),
            "p3j_validation_precision": precision_from_threshold(threshold, "validation"),
            "p3j_holdout_precision": precision_from_threshold(threshold, "holdout"),
        }
        for split in p3g.SPLITS:
            split_score_rows = [score_row for score_row in score_rows if score_row.get("split") == split]
            split_ids = {score_row["candidate_id"] for score_row in split_score_rows}
            row[f"{split}_selected_count"] = len(selected_ids & split_ids)
            metric = selected_metric(
                [
                    {
                        "candidate_id": score_row["candidate_id"],
                        "r2_label": score_row["r2_label"],
                    }
                    for score_row in split_score_rows
                ],
                selected_ids,
            )
            row[f"{split}_precision"] = metric["precision"]
            row[f"{split}_accept_rate"] = metric["accept_rate"]
            row[f"{split}_ev_proxy_pct"] = metric["ev_proxy_pct"]
        out.append(row)
    return out


def topk_reproduction(
    score_rows: list[dict[str, Any]],
    p3j_candidate: dict[str, Any],
) -> dict[str, Any]:
    out: dict[str, Any] = {}
    for split in p3g.SPLITS:
        out[split] = {}
        for k, field in ((10, "threshold_pass_top10_equiv"), (25, "threshold_pass_top25_equiv")):
            split_rows_for_k = [row for row in score_rows if row.get("split") == split and row.get(field) is True]
            selected_positive = sum(1 for row in split_rows_for_k if row.get("r2_label") == "positive")
            precision = selected_positive / len(split_rows_for_k) if split_rows_for_k else None
            p3j_metric = p3g.top_metric(p3j_candidate, split, k)
            p3j_precision = common.float_or_none(p3j_metric.get("precision_r2"))
            out[split][f"top{k}"] = {
                "contract_precision": precision,
                "p3j_precision": p3j_precision,
                "delta": (
                    abs(precision - p3j_precision)
                    if precision is not None and p3j_precision is not None
                    else None
                ),
                "status": (
                    "PASS"
                    if precision is not None
                    and p3j_precision is not None
                    and abs(precision - p3j_precision) <= 1e-9
                    else "FAIL"
                ),
            }
    return out


def markdown_report(report: dict[str, Any]) -> str:
    acceptance = report.get("acceptance", {})
    lines = [
        "# SELECTOR_SHADOW_SCORE_CONTRACT",
        "",
        f"Scope: `{report['scope']}`",
        f"Status: `{report['status']}`",
        f"Candidate: `{report['candidate_contract']['candidate_id']}`",
        "",
        "This is a shadow-only/counterfactual score contract. It does not change Gatekeeper, runtime, execution, or send path.",
        "",
        "## Acceptance",
        "",
        f"- score rows written: `{acceptance.get('score_rows_written')}`",
        f"- top-k reproduction: `{acceptance.get('topk_reproduction_status')}`",
        f"- reason vectors present: `{acceptance.get('reason_vectors_present')}`",
        f"- non-claim boundaries present: `{acceptance.get('non_claim_boundaries_present')}`",
        f"- fail reasons: `{json.dumps(acceptance.get('fail_reasons', []), sort_keys=True)}`",
        "",
        "## Missing Policy",
        "",
        "- missing values are not safe and are not production negatives",
        "- missing core curve/market features invalidate score",
        "- missing concentration features degrade score",
        "- all threshold passes are diagnostic only",
        "",
        "## Threshold Candidates",
        "",
        "| threshold | validation precision | holdout precision |",
        "| --- | ---: | ---: |",
    ]
    for item in report.get("threshold_candidates", []):
        lines.append(
            f"| {item.get('threshold_label')} | {item.get('validation_precision')} | {item.get('holdout_precision')} |"
        )
    return "\n".join(lines) + "\n"


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = args.root.resolve()
    dataset_dir = root / "datasets" / "selector" / args.scope
    report_dir = root / "reports" / "selector" / args.scope
    training_view = dataset_dir / "selector_training_view_v1.jsonl"
    p3j_path = report_dir / "selector_r2only_candidate_selection_v1.json"
    p3g_path = report_dir / "selector_r2only_model_candidate_v1.json"
    gk_manifest_path = report_dir / "gatekeeper_feature_context_manifest_v1.json"
    phase3_manifest_path = report_dir / "phase3_r2only_manifest_v1.json"
    missing = [
        str(path)
        for path in (training_view, p3j_path, p3g_path, gk_manifest_path, phase3_manifest_path)
        if not path.exists() or path.stat().st_size == 0
    ]
    if missing:
        raise FileNotFoundError(f"missing required P3K inputs: {missing}")

    rows = denominator_rows(list(common.iter_json_objects(training_view)))
    p3j_report = read_json(p3j_path)
    p3j_candidate = candidate_from_p3j(p3j_report, args.candidate_id)
    thresholds = selected_thresholds(p3j_report, args.candidate_id)
    features = [str(feature) for feature in p3j_candidate.get("features") or []]
    if not features:
        raise ValueError(f"P3J candidate has no features: {args.candidate_id}")
    score_rows, scoring_details = build_score_rows(
        rows,
        candidate_id=args.candidate_id,
        features=features,
        thresholds=thresholds,
    )
    threshold_rows = threshold_summary(score_rows, thresholds)
    reproduction = topk_reproduction(score_rows, p3j_candidate)
    threshold_candidates = [
        {
            "threshold_label": row["threshold_label"],
            "threshold_type": row["threshold_type"],
            "threshold": row.get("threshold"),
            "train_precision": row.get("train_precision"),
            "validation_precision": row.get("validation_precision"),
            "holdout_precision": row.get("holdout_precision"),
            "train_accept_rate": row.get("train_accept_rate"),
            "validation_accept_rate": row.get("validation_accept_rate"),
            "holdout_accept_rate": row.get("holdout_accept_rate"),
            "train_ev_proxy_pct": row.get("train_ev_proxy_pct"),
            "validation_ev_proxy_pct": row.get("validation_ev_proxy_pct"),
            "holdout_ev_proxy_pct": row.get("holdout_ev_proxy_pct"),
            "p3j_train_precision": row.get("p3j_train_precision"),
            "p3j_validation_precision": row.get("p3j_validation_precision"),
            "p3j_holdout_precision": row.get("p3j_holdout_precision"),
        }
        for row in threshold_rows
    ]
    fail_reasons: list[str] = []
    if not score_rows:
        fail_reasons.append("score_rows_empty")
    if any("score_validity_status" not in row for row in score_rows):
        fail_reasons.append("score_validity_status_missing")
    if any(not isinstance(row.get("reason_vector"), dict) for row in score_rows if row.get("threshold_pass_top25_equiv")):
        fail_reasons.append("reason_vector_missing_for_topk_rows")
    if any((row.get("non_claims") or {}).get("changes_gatekeeper_decision") for row in score_rows):
        fail_reasons.append("non_claim_changes_gatekeeper_decision")
    if any((row.get("non_claims") or {}).get("changes_execution") for row in score_rows):
        fail_reasons.append("non_claim_changes_execution")
    reproduction_failures = [
        payload
        for split_payload in reproduction.values()
        for payload in split_payload.values()
        if payload.get("status") != "PASS"
    ]
    if reproduction_failures:
        fail_reasons.append("topk_metrics_do_not_reproduce_p3j")

    output_json = report_dir / "selector_shadow_score_contract_v1.json"
    output_md = report_dir / "SELECTOR_SHADOW_SCORE_CONTRACT.md"
    output_scores = dataset_dir / "selector_shadow_scores_v1.jsonl"
    output_thresholds = report_dir / "selector_shadow_score_thresholds_v1.csv"
    report = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": ARTIFACT,
        "status": STATUS_PASS if not fail_reasons else "P3K_NO-GO_SHADOW_SCORE_CONTRACT_DRAFT",
        "phase": "phase3",
        "scope": args.scope,
        "input_paths": {
            "selector_training_view_v1": str(training_view),
            "selector_r2only_candidate_selection_v1": str(p3j_path),
            "selector_r2only_model_candidate_v1": str(p3g_path),
            "gatekeeper_feature_context_manifest_v1": str(gk_manifest_path),
            "phase3_r2only_manifest_v1": str(phase3_manifest_path),
        },
        "candidate_contract": {
            "candidate_id": args.candidate_id,
            "candidate_kind": "simple_feature_score",
            "feature_set": "combined",
            "dataset_kind": "r2_only",
            "score_version": SCORE_VERSION,
            "target_net_pct": TARGET_NET_PCT,
            "stop_net_pct": STOP_NET_PCT,
            "horizon_ms": HORIZON_MS,
            "status": "P3K_SHADOW_SCORE_CONTRACT_DRAFT",
            "production_ready": False,
            "gatekeeper_tuning_ready": False,
            "runtime_active": False,
        },
        "claim_boundaries": {
            "diagnostic_only": True,
            "shadow_only": True,
            "production_promotion_allowed": False,
            "gatekeeper_tuning_started": False,
            "runtime_changed": False,
            "active_execution_changed": False,
            "send_path_changed": False,
            "changes_gatekeeper_decision": False,
            "changes_execution": False,
        },
        "feature_groups": feature_groups(features),
        "missing_policy": {
            "missing_not_safe": True,
            "missing_not_negative_automatically": True,
            "core_curve_market_missing_status": "score_invalid_missing_core_curve_market",
            "concentration_missing_status": "score_degraded_missing_concentration",
            "gk_context_missing_status": "score_degraded_missing_gk_context",
            "cutoff_unverified_status": "score_invalid_cutoff_unverified",
            "required_core_curve_market_features": list(REQUIRED_CORE_CURVE_MARKET),
            "concentration_features": list(CONCENTRATION_FEATURES),
            "fsc_diagnostics_are_evidence_not_safety": True,
        },
        "score_validity_status_counts": scoring_details["score_validity_status_counts"],
        "threshold_candidates": threshold_candidates,
        "topk_reproduction": reproduction,
        "acceptance": {
            "score_rows_written": len(score_rows),
            "all_rows_have_score_validity_status": all("score_validity_status" in row for row in score_rows),
            "reason_vectors_present": all(isinstance(row.get("reason_vector"), dict) for row in score_rows if row.get("threshold_pass_top25_equiv")),
            "non_claim_boundaries_present": all(isinstance(row.get("non_claims"), dict) for row in score_rows),
            "topk_reproduction_status": "PASS" if not reproduction_failures else "FAIL",
            "production_promotion_allowed": False,
            "gatekeeper_tuning_ready": False,
            "fail_reasons": fail_reasons,
        },
        "outputs": {
            "selector_shadow_score_contract_v1": str(output_json),
            "SELECTOR_SHADOW_SCORE_CONTRACT": str(output_md),
            "selector_shadow_scores_v1": str(output_scores),
            "selector_shadow_score_thresholds_v1": str(output_thresholds),
        },
    }
    report_dir.mkdir(parents=True, exist_ok=True)
    dataset_dir.mkdir(parents=True, exist_ok=True)
    common.write_json(output_json, report)
    output_md.write_text(markdown_report(report), encoding="utf-8")
    write_jsonl(output_scores, score_rows)
    write_csv(
        output_thresholds,
        threshold_rows,
        [
            "threshold_label",
            "threshold_type",
            "threshold",
            "p3j_train_precision",
            "p3j_validation_precision",
            "p3j_holdout_precision",
            "train_selected_count",
            "train_precision",
            "train_accept_rate",
            "train_ev_proxy_pct",
            "validation_selected_count",
            "validation_precision",
            "validation_accept_rate",
            "validation_ev_proxy_pct",
            "holdout_selected_count",
            "holdout_precision",
            "holdout_accept_rate",
            "holdout_ev_proxy_pct",
        ],
    )
    return report


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument("--candidate-id", default=DEFAULT_CANDIDATE_ID)
    parser.add_argument("--json", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = build_report(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0 if report["status"] == STATUS_PASS else 1


if __name__ == "__main__":
    raise SystemExit(main())
