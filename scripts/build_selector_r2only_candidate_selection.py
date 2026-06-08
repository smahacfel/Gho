#!/usr/bin/env python3
"""Build P3J offline R2-only selector candidate selection report.

This is diagnostic/offline only. It does not tune Gatekeeper, does not alter
runtime behavior, and does not promote any candidate to production.
"""

from __future__ import annotations

import argparse
import csv
import json
import math
import random
from collections import Counter
from pathlib import Path
from typing import Any, Callable

import build_selector_r2only_baseline_report as baseline
import build_selector_r2only_model_candidate as p3g
import ci_assert_selector_regression_gates as simcov_gate
import selector_pipeline_common as common


SPLITS = ("train", "validation", "holdout")
TOP_K = (10, 25, 50, 100, 200)
SCORE_QUANTILES = (0.99, 0.98, 0.975, 0.95, 0.90, 0.85, 0.80)
TARGET_PRECISION_THRESHOLDS = (0.55, 0.60, 0.65, 0.70)
TARGET_NET_PCT = 40.0
STOP_NET_PCT = 40.0
BOOTSTRAP_SAMPLES = 1000
BOOTSTRAP_SEED = 5150
MIN_DENOMINATOR_ROWS = 1440


def read_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as fh:
        payload = json.load(fh)
    if not isinstance(payload, dict):
        raise ValueError(f"expected JSON object in {path}")
    return payload


def write_csv(path: Path, rows: list[dict[str, Any]], fieldnames: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames)
        writer.writeheader()
        for row in rows:
            writer.writerow({field: row.get(field) for field in fieldnames})


def denominator_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [row for row in rows if baseline.r2only_denominator(row)]


def label_positive(row: dict[str, Any]) -> bool:
    return row.get("r2_label") == "positive"


def base_rate(rows: list[dict[str, Any]]) -> float | None:
    if not rows:
        return None
    return sum(1 for row in rows if label_positive(row)) / len(rows)


def split_rows(rows: list[dict[str, Any]], split: str) -> list[dict[str, Any]]:
    return [row for row in rows if row.get("split") == split]


def row_id(row: dict[str, Any]) -> str:
    return common.str_or_none(row.get("candidate_id")) or ""


def order_rows(rows: list[dict[str, Any]], score_map: dict[str, float]) -> list[dict[str, Any]]:
    sortable: list[tuple[float, int, str, dict[str, Any]]] = []
    for row in rows:
        candidate_id = row_id(row)
        if not candidate_id or candidate_id not in score_map:
            continue
        sortable.append(
            (
                score_map[candidate_id],
                common.int_or_none(row.get("birth_ts_ms")) or 0,
                candidate_id,
                row,
            )
        )
    sortable.sort(key=lambda item: (-item[0], item[1], item[2]))
    return [item[3] for item in sortable]


def selected_metric(rows: list[dict[str, Any]], selected: list[dict[str, Any]]) -> dict[str, Any]:
    positives = sum(1 for row in rows if label_positive(row))
    selected_positive = sum(1 for row in selected if label_positive(row))
    selected_negative = len(selected) - selected_positive
    precision = selected_positive / len(selected) if selected else None
    split_base_rate = base_rate(rows)
    return {
        "rows": len(rows),
        "selected_count": len(selected),
        "positive_count": selected_positive,
        "negative_count": selected_negative,
        "precision": precision,
        "recall": selected_positive / positives if positives else None,
        "accept_rate": len(selected) / len(rows) if rows else None,
        "base_positive_rate": split_base_rate,
        "lift_vs_base_rate": precision / split_base_rate if precision is not None and split_base_rate else None,
        "ev_proxy_pct": (
            (precision * TARGET_NET_PCT) - ((1.0 - precision) * STOP_NET_PCT)
            if precision is not None
            else None
        ),
    }


def bootstrap_precision_ci(selected: list[dict[str, Any]], *, samples: int, seed: int) -> dict[str, Any]:
    if not selected:
        return {
            "samples": samples,
            "seed": seed,
            "selected_count": 0,
            "precision_p025": None,
            "precision_p50": None,
            "precision_p975": None,
        }
    rng = random.Random(seed)
    values: list[float] = []
    for _ in range(samples):
        draw = [selected[rng.randrange(len(selected))] for _idx in range(len(selected))]
        values.append(sum(1 for row in draw if label_positive(row)) / len(draw))
    values.sort()
    return {
        "samples": samples,
        "seed": seed,
        "selected_count": len(selected),
        "precision_p025": values[int(0.025 * (len(values) - 1))],
        "precision_p50": values[int(0.50 * (len(values) - 1))],
        "precision_p975": values[int(0.975 * (len(values) - 1))],
    }


def threshold_metric(
    rows: list[dict[str, Any]],
    score_map: dict[str, float],
    *,
    threshold: float | None = None,
    top_k: int | None = None,
    bootstrap_seed: int,
    bootstrap_samples: int = BOOTSTRAP_SAMPLES,
) -> dict[str, Any]:
    if top_k is not None:
        selected = order_rows(rows, score_map)[: min(top_k, len(rows))]
    elif threshold is not None:
        selected = [row for row in rows if score_map.get(row_id(row), -math.inf) >= threshold]
        selected = order_rows(selected, score_map)
    else:
        selected = []
    metric = selected_metric(rows, selected)
    metric["bootstrap_ci_precision"] = bootstrap_precision_ci(
        selected,
        samples=bootstrap_samples,
        seed=bootstrap_seed,
    )
    return metric


def percentile(values: list[float], pct: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    if len(ordered) == 1:
        return ordered[0]
    pos = (len(ordered) - 1) * pct
    low = int(pos)
    high = min(low + 1, len(ordered) - 1)
    frac = pos - low
    return ordered[low] * (1.0 - frac) + ordered[high] * frac


def train_threshold_for_target_precision(
    train_rows: list[dict[str, Any]],
    score_map: dict[str, float],
    target_precision: float,
) -> float | None:
    ordered = order_rows(train_rows, score_map)
    best_threshold: float | None = None
    for idx in range(1, len(ordered) + 1):
        selected = ordered[:idx]
        metric = selected_metric(train_rows, selected)
        precision = metric.get("precision")
        if isinstance(precision, (int, float)) and precision >= target_precision:
            best_threshold = score_map[row_id(selected[-1])]
    return best_threshold


def build_threshold_grid(
    rows: list[dict[str, Any]],
    candidates: list[dict[str, Any]],
    score_maps: dict[str, dict[str, float]],
    *,
    bootstrap_samples: int = BOOTSTRAP_SAMPLES,
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    report: dict[str, Any] = {}
    csv_rows: list[dict[str, Any]] = []
    train = split_rows(rows, "train")
    for candidate in candidates:
        candidate_id = str(candidate["candidate_id"])
        score_map = score_maps[candidate_id]
        candidate_report: list[dict[str, Any]] = []
        train_scores = [score_map[row_id(row)] for row in train if row_id(row) in score_map]
        specs: list[dict[str, Any]] = []
        for k in TOP_K:
            specs.append({"threshold_type": "top_k", "threshold_label": f"top_{k}", "top_k": k, "threshold": None})
        for quantile in SCORE_QUANTILES:
            threshold = percentile(train_scores, quantile)
            specs.append(
                {
                    "threshold_type": "score_quantile",
                    "threshold_label": f"q{quantile:g}",
                    "top_k": None,
                    "threshold": threshold,
                }
            )
        for target_precision in TARGET_PRECISION_THRESHOLDS:
            threshold = train_threshold_for_target_precision(train, score_map, target_precision)
            specs.append(
                {
                    "threshold_type": "target_precision_threshold",
                    "threshold_label": f"target_precision_{target_precision:g}",
                    "target_precision": target_precision,
                    "top_k": None,
                    "threshold": threshold,
                }
            )
        for spec_idx, spec in enumerate(specs):
            split_payload: dict[str, Any] = {
                "threshold_type": spec["threshold_type"],
                "threshold_label": spec["threshold_label"],
                "threshold": spec.get("threshold"),
            }
            for split in SPLITS:
                current_rows = split_rows(rows, split)
                metric = threshold_metric(
                    current_rows,
                    score_map,
                    threshold=spec.get("threshold"),
                    top_k=spec.get("top_k"),
                    bootstrap_seed=BOOTSTRAP_SEED + spec_idx + len(candidate_report),
                    bootstrap_samples=bootstrap_samples,
                )
                split_payload[split] = metric
                csv_rows.append(
                    {
                        "candidate_id": candidate_id,
                        "threshold_type": spec["threshold_type"],
                        "threshold_label": spec["threshold_label"],
                        "threshold": spec.get("threshold"),
                        "split": split,
                        **{key: metric.get(key) for key in (
                            "rows",
                            "selected_count",
                            "positive_count",
                            "negative_count",
                            "precision",
                            "recall",
                            "accept_rate",
                            "base_positive_rate",
                            "lift_vs_base_rate",
                            "ev_proxy_pct",
                        )},
                        "bootstrap_ci_precision_p025": metric["bootstrap_ci_precision"]["precision_p025"],
                        "bootstrap_ci_precision_p50": metric["bootstrap_ci_precision"]["precision_p50"],
                        "bootstrap_ci_precision_p975": metric["bootstrap_ci_precision"]["precision_p975"],
                    }
                )
            candidate_report.append(split_payload)
        report[candidate_id] = candidate_report
    return report, csv_rows


def candidate_top_metric(candidate: dict[str, Any], split: str, k: int) -> dict[str, Any]:
    return p3g.top_metric(candidate, split, k)


def compact_topk(candidate: dict[str, Any]) -> dict[str, Any]:
    payload: dict[str, Any] = {}
    for split in SPLITS:
        payload[split] = {}
        for k in (10, 25, 50, 100):
            metric = candidate_top_metric(candidate, split, k)
            payload[split][f"top{k}"] = {
                "selected_count": metric.get("selected_count"),
                "precision": metric.get("precision_r2"),
                "tp_r2": metric.get("tp_r2"),
                "fp_r2": metric.get("fp_r2"),
                "lift_vs_base_rate": metric.get("lift_vs_base_rate"),
                "bootstrap_ci_precision": metric.get("bootstrap_ci_precision"),
            }
    return payload


def build_candidate_score_maps(
    rows: list[dict[str, Any]],
    report_dir: Path,
    requested_feature_set: str,
    *,
    bootstrap_samples: int = BOOTSTRAP_SAMPLES,
    logistic_epochs: int = 600,
) -> tuple[list[dict[str, Any]], dict[str, dict[str, float]], dict[str, list[str]]]:
    p3f_report = read_json(report_dir / "selector_r2only_feature_contribution_v1.json")
    train_rows = split_rows(rows, "train")
    score_maps: dict[str, dict[str, float]] = {}
    candidates: list[dict[str, Any]] = []
    features_by_candidate: dict[str, list[str]] = {}

    def add_simple(feature_set: str, candidate_id: str) -> None:
        features = p3g.available_features(rows, p3f_report, feature_set=feature_set, report_dir=report_dir)
        simple = p3g.simple_scores(rows, train_rows=train_rows, features=features)
        score_map = {
            candidate_id_value: payload["simple_feature_score_v1"]
            for candidate_id_value, payload in simple.items()
        }
        score_maps[candidate_id] = score_map
        features_by_candidate[candidate_id] = features
        candidates.append(
            {
                "candidate_id": candidate_id,
                "candidate_kind": "simple_feature_score",
                "feature_set": feature_set,
                "features": features,
                "by_split": p3g.candidate_metrics(
                rows,
                score_map,
                top_k_values=TOP_K,
                bootstrap_samples=bootstrap_samples,
                bootstrap_seed=BOOTSTRAP_SEED,
            ),
        }
        )

    add_simple(requested_feature_set, f"{requested_feature_set}:simple_feature_score_v1")
    add_simple("gk", "gk_context_only:simple_feature_score_v1")
    combined_features = p3g.available_features(rows, p3f_report, feature_set="combined", report_dir=report_dir)
    logistic_model = p3g.train_logistic(
        train_rows,
        features=combined_features,
        learning_rate=0.05,
        l2=0.01,
        epochs=logistic_epochs,
    )
    logistic_id = "combined:logistic_sanity_baseline"
    logistic_score_map = p3g.logistic_scores(rows, model=logistic_model)
    score_maps[logistic_id] = logistic_score_map
    features_by_candidate[logistic_id] = combined_features
    candidates.append(
        {
            "candidate_id": logistic_id,
            "candidate_kind": "logistic_or_tree_sanity_baseline",
            "feature_set": "combined",
            "features": combined_features,
            "model": logistic_model,
            "by_split": p3g.candidate_metrics(
                rows,
                logistic_score_map,
                top_k_values=TOP_K,
                bootstrap_samples=bootstrap_samples,
                bootstrap_seed=BOOTSTRAP_SEED,
            ),
        }
    )
    return candidates, score_maps, features_by_candidate


def best_candidate(candidates: list[dict[str, Any]]) -> dict[str, Any]:
    def key(candidate: dict[str, Any]) -> tuple[float, float, float, str]:
        return (
            float(candidate_top_metric(candidate, "holdout", 10).get("precision_r2") or -1.0),
            float(candidate_top_metric(candidate, "validation", 10).get("precision_r2") or -1.0),
            float(candidate_top_metric(candidate, "holdout", 25).get("precision_r2") or -1.0),
            str(candidate.get("candidate_id")),
        )

    return max(candidates, key=key) if candidates else {}


def bucket_value(row: dict[str, Any], bucket: str, *, ordered_index: dict[str, int], trade_edges: tuple[float, float]) -> str:
    if bucket == "time_bucket_hour":
        ts = common.int_or_none(row.get("birth_ts_ms")) or common.int_or_none(row.get("decision_ts_ms"))
        return f"hour_{ts // 3_600_000}" if ts is not None else "unknown"
    if bucket == "market_activity_bucket":
        trade_rate = p3g.feature_value(row, "trade_rate")
        if trade_rate is None:
            return "trade_rate_missing"
        if trade_rate <= trade_edges[0]:
            return "trade_rate_low"
        if trade_rate <= trade_edges[1]:
            return "trade_rate_mid"
        return "trade_rate_high"
    if bucket == "candidate_birth_order_bucket":
        idx = ordered_index.get(row_id(row), 0)
        total = max(len(ordered_index), 1)
        frac = idx / total
        if frac < 1 / 3:
            return "birth_order_early"
        if frac < 2 / 3:
            return "birth_order_mid"
        return "birth_order_late"
    if bucket == "r2_path_available":
        return "r2_path_available" if row.get("r2_path_coverage_ok") is True else "r2_path_missing"
    if bucket == "gk_context_status":
        return f"gk_context_{row.get('gk_context_status') or 'missing'}"
    if bucket == "gk_concentration_available":
        available = p3g.feature_value(row, "gk_hhi") is not None and p3g.feature_value(row, "gk_top3_volume_pct") is not None
        return "gk_concentration_available" if available else "gk_concentration_missing"
    if bucket == "simcov_outcome":
        return str(
            row.get("execution_feasibility_status")
            or row.get("shadow_execution_outcome")
            or row.get("simulation_outcome")
            or "simcov_unknown"
        )
    return "unknown"


def stability_report(
    rows: list[dict[str, Any]],
    score_map: dict[str, float],
    *,
    bootstrap_samples: int = BOOTSTRAP_SAMPLES,
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    ordered = sorted(
        rows,
        key=lambda row: (
            common.int_or_none(row.get("birth_ts_ms")) or common.int_or_none(row.get("decision_ts_ms")) or 0,
            row_id(row),
        ),
    )
    ordered_index = {row_id(row): idx for idx, row in enumerate(ordered) if row_id(row)}
    trade_values = [p3g.feature_value(row, "trade_rate") for row in rows]
    trade_values = [value for value in trade_values if value is not None]
    trade_edges = (
        percentile(trade_values, 1 / 3) or 0.0,
        percentile(trade_values, 2 / 3) or 0.0,
    )
    bucket_names = (
        "time_bucket_hour",
        "market_activity_bucket",
        "candidate_birth_order_bucket",
        "r2_path_available",
        "gk_context_status",
        "gk_concentration_available",
        "simcov_outcome",
    )
    report: dict[str, Any] = {}
    csv_rows: list[dict[str, Any]] = []
    for bucket in bucket_names:
        bucket_report: dict[str, Any] = {}
        groups: dict[tuple[str, str], list[dict[str, Any]]] = {}
        for row in rows:
            split = str(row.get("split") or "unknown")
            value = bucket_value(row, bucket, ordered_index=ordered_index, trade_edges=trade_edges)
            groups.setdefault((split, value), []).append(row)
        for (split, value), group_rows in sorted(groups.items()):
            metric = threshold_metric(
                group_rows,
                score_map,
                top_k=min(25, len(group_rows)),
                bootstrap_seed=BOOTSTRAP_SEED + len(csv_rows),
                bootstrap_samples=bootstrap_samples,
            )
            bucket_report.setdefault(split, {})[value] = metric
            csv_rows.append(
                {
                    "bucket": bucket,
                    "bucket_value": value,
                    "split": split,
                    **{key: metric.get(key) for key in (
                        "rows",
                        "selected_count",
                        "positive_count",
                        "negative_count",
                        "precision",
                        "recall",
                        "accept_rate",
                        "base_positive_rate",
                        "lift_vs_base_rate",
                        "ev_proxy_pct",
                    )},
                    "bootstrap_ci_precision_p025": metric["bootstrap_ci_precision"]["precision_p025"],
                    "bootstrap_ci_precision_p50": metric["bootstrap_ci_precision"]["precision_p50"],
                    "bootstrap_ci_precision_p975": metric["bootstrap_ci_precision"]["precision_p975"],
                }
            )
        report[bucket] = bucket_report
    return report, csv_rows


def ablation_groups(features: list[str]) -> dict[str, list[str]]:
    groups = {
        "combined_full": features,
        "minus_curve_market_features": [
            feature for feature in features if feature not in {
                "curve_progress_pct",
                "gk_bonding_progress_pct",
                "gk_current_market_cap_sol",
                "gk_price_change_ratio",
                "gk_curve_data_known",
            }
        ],
        "minus_concentration_features": [
            feature for feature in features if "hhi" not in feature and "top3" not in feature and "concentration" not in feature
        ],
        "minus_flow_features": [feature for feature in features if feature not in p3g.FLOW_FEATURES],
        "minus_dev_features": [feature for feature in features if "dev" not in feature],
        "minus_vector_summaries": [feature for feature in features if "vector" not in feature and not feature.startswith("vectors_")],
    }
    return {key: value for key, value in groups.items() if value}


def ablation_report(
    rows: list[dict[str, Any]],
    features: list[str],
    *,
    bootstrap_samples: int = BOOTSTRAP_SAMPLES,
) -> dict[str, Any]:
    train_rows = split_rows(rows, "train")
    report: dict[str, Any] = {}
    for name, group_features in ablation_groups(features).items():
        simple = p3g.simple_scores(rows, train_rows=train_rows, features=group_features)
        score_map = {
            candidate_id: payload["simple_feature_score_v1"]
            for candidate_id, payload in simple.items()
        }
        candidate = {
            "candidate_id": f"ablation:{name}",
            "candidate_kind": "simple_feature_score_ablation",
            "features": group_features,
            "by_split": p3g.candidate_metrics(
                rows,
                score_map,
                top_k_values=(10, 25, 50, 100),
                bootstrap_samples=bootstrap_samples,
                bootstrap_seed=BOOTSTRAP_SEED,
            ),
        }
        report[name] = {
            "features": group_features,
            "topk": compact_topk(candidate),
        }
    return report


def simple_candidate_for_feature_set(
    rows: list[dict[str, Any]],
    report_dir: Path,
    feature_set: str,
    candidate_id: str,
    *,
    bootstrap_samples: int = BOOTSTRAP_SAMPLES,
) -> dict[str, Any]:
    p3f_report = read_json(report_dir / "selector_r2only_feature_contribution_v1.json")
    train_rows = split_rows(rows, "train")
    features = p3g.available_features(rows, p3f_report, feature_set=feature_set, report_dir=report_dir)
    simple = p3g.simple_scores(rows, train_rows=train_rows, features=features)
    score_map = {
        candidate_id_value: payload["simple_feature_score_v1"]
        for candidate_id_value, payload in simple.items()
    }
    candidate = {
        "candidate_id": candidate_id,
        "candidate_kind": "simple_feature_score_comparator",
        "feature_set": feature_set,
        "features": features,
        "by_split": p3g.candidate_metrics(
            rows,
            score_map,
            top_k_values=TOP_K,
            bootstrap_samples=bootstrap_samples,
            bootstrap_seed=BOOTSTRAP_SEED,
        ),
    }
    return {
        "candidate": candidate,
        "topk": compact_topk(candidate),
    }


def gatekeeper_accept_comparator(rows: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        split: baseline.metric_block(split_rows(rows, split), baseline.selector_accept)
        for split in SPLITS
    }


def infer_source_scope(dataset_manifest: dict[str, Any]) -> str | None:
    value = dataset_manifest.get("source_scope")
    return str(value) if isinstance(value, str) and value else None


def run_simcov_gate(root: Path, source_scope: str | None) -> dict[str, Any]:
    if not source_scope:
        return {"status": "NOT_RUN", "fail_reasons": ["source_scope_missing"]}
    args = simcov_gate.build_parser().parse_args(
        [
            "--scope",
            source_scope,
            "--root",
            str(root),
            "--gate-profile",
            "operational",
            "--min-attempt-coverage",
            "0.95",
            "--max-not-executable-rate",
            "0.05",
            "--max-unknown-unclassified",
            "1",
        ]
    )
    return simcov_gate.build_report(args)


def acceptance_status(
    *,
    rows: list[dict[str, Any]],
    candidates: list[dict[str, Any]],
    phase3_manifest: dict[str, Any],
    simcov: dict[str, Any],
    comparators: dict[str, Any],
) -> dict[str, Any]:
    best = best_candidate(candidates)
    validation_top10 = candidate_top_metric(best, "validation", 10).get("precision_r2")
    holdout_top10 = candidate_top_metric(best, "holdout", 10).get("precision_r2")
    holdout_top25 = candidate_top_metric(best, "holdout", 25).get("precision_r2")
    combined = next((candidate for candidate in candidates if candidate.get("candidate_id") == "combined:simple_feature_score_v1"), {})
    fail_reasons: list[str] = []
    denominator = len(rows)
    if denominator < MIN_DENOMINATOR_ROWS:
        fail_reasons.append("r2_training_denominator_below_1440")
    if phase3_manifest.get("leakage_audit_status") != "PASS":
        fail_reasons.append("leakage_not_pass")
    if simcov.get("status") != "PASS":
        fail_reasons.append("simcov_operational_not_pass")
    if (validation_top10 or 0.0) < 0.70:
        fail_reasons.append("validation_top10_below_70pct")
    if (holdout_top10 or 0.0) < 0.70:
        fail_reasons.append("holdout_top10_below_70pct")
    if (holdout_top25 or 0.0) < 0.60:
        fail_reasons.append("holdout_top25_below_60pct")
    if (validation_top10 or 0.0) <= 0.0 or (holdout_top10 or 0.0) <= 0.0:
        fail_reasons.append("validation_holdout_direction_not_consistent")
    ci = candidate_top_metric(best, "holdout", 10).get("bootstrap_ci_precision") or {}
    if isinstance(ci, dict) and isinstance(ci.get("precision_p025"), (int, float)) and ci["precision_p025"] < 0.50:
        fail_reasons.append("bootstrap_lower_bound_catastrophically_low")
    combined_holdout = candidate_top_metric(combined, "holdout", 10).get("precision_r2") if combined else None
    flow_holdout = (
        comparators.get("flow_only", {})
        .get("topk", {})
        .get("holdout", {})
        .get("top10", {})
        .get("precision")
    )
    gatekeeper_holdout = (
        comparators.get("gatekeeper_accept", {})
        .get("holdout", {})
        .get("precision_r2")
    )
    if combined_holdout is not None and flow_holdout is not None and combined_holdout <= flow_holdout:
        fail_reasons.append("combined_not_above_flow_only")
    if combined_holdout is not None and gatekeeper_holdout is not None and combined_holdout <= gatekeeper_holdout:
        fail_reasons.append("combined_not_above_gatekeeper_accept")
    status = "P3J_PASS_OFFLINE_CANDIDATE_SELECTION"
    if not fail_reasons and (holdout_top25 or 0.0) < 0.70:
        status = "P3J_PASS_NARROW_TOPK_CANDIDATE"
    if fail_reasons:
        status = "P3J_NO-GO_UNSTABLE_SIGNAL"
    return {
        "status": status,
        "best_candidate_id": best.get("candidate_id"),
        "validation_top10_precision": validation_top10,
        "holdout_top10_precision": holdout_top10,
        "holdout_top25_precision": holdout_top25,
        "production_promotion_allowed": False,
        "gatekeeper_tuning_started": False,
        "fail_reasons": sorted(set(fail_reasons)),
    }


def format_pct(value: Any) -> str:
    if not isinstance(value, (int, float)):
        return "n/a"
    return f"{value * 100:.2f}%"


def markdown_report(report: dict[str, Any]) -> str:
    acceptance = report.get("acceptance", {})
    lines = [
        "# FEATURE_RICH_R2_CANDIDATE_SELECTION",
        "",
        f"Scope: `{report['scope']}`",
        "",
        "## Verdict",
        "",
        f"Status: `{report['status']}`",
        f"Best candidate: `{acceptance.get('best_candidate_id')}`",
        f"Fail reasons: `{json.dumps(acceptance.get('fail_reasons', []), sort_keys=True)}`",
        "",
        "Offline only. This is not Gatekeeper tuning, not production readiness, and not a live execution claim.",
        "",
        "## Denominator And Gates",
        "",
        f"- R2 training denominator rows: `{report['r2_training_denominator_rows']}`",
        f"- Leakage: `{report['phase3_gate'].get('leakage_audit_status')}`",
        f"- Simcov operational: `{report['simcov_operational_gate'].get('status')}`",
        "",
        "## Candidate Top-K",
        "",
        "| candidate | train top10 | validation top10 | holdout top10 | holdout top25 |",
        "| --- | ---: | ---: | ---: | ---: |",
    ]
    for candidate in report.get("candidates", []):
        lines.append(
            "| {candidate} | {train} | {validation} | {holdout10} | {holdout25} |".format(
                candidate=candidate.get("candidate_id"),
                train=format_pct(candidate_top_metric(candidate, "train", 10).get("precision_r2")),
                validation=format_pct(candidate_top_metric(candidate, "validation", 10).get("precision_r2")),
                holdout10=format_pct(candidate_top_metric(candidate, "holdout", 10).get("precision_r2")),
                holdout25=format_pct(candidate_top_metric(candidate, "holdout", 25).get("precision_r2")),
            )
        )
    lines.extend(
        [
            "",
            "## EV Proxy",
            "",
            f"`EV_proxy = precision * {TARGET_NET_PCT:g} - (1 - precision) * {STOP_NET_PCT:g}`.",
            "This is R2 market-opportunity EV, not live PnL.",
            "",
            "## Claim Boundaries",
            "",
            "- production_promotion_allowed: `false`",
            "- gatekeeper_tuning_started: `false`",
            "- active_execution_changed: `false`",
            "- runtime_changed: `false`",
        ]
    )
    return "\n".join(lines) + "\n"


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = args.root.resolve()
    dataset_dir = root / "datasets" / "selector" / args.scope
    report_dir = root / "reports" / "selector" / args.scope
    training_view = dataset_dir / "selector_training_view_v1.jsonl"
    phase3_manifest_path = report_dir / "phase3_r2only_manifest_v1.json"
    dataset_manifest_path = report_dir / "dataset_manifest_v1.json"
    missing = [
        str(path)
        for path in (training_view, phase3_manifest_path, dataset_manifest_path)
        if not path.exists() or path.stat().st_size == 0
    ]
    if missing:
        raise FileNotFoundError(f"missing required P3J inputs: {missing}")
    rows = denominator_rows(list(common.iter_json_objects(training_view)))
    phase3_manifest = read_json(phase3_manifest_path)
    dataset_manifest = read_json(dataset_manifest_path)
    candidates, score_maps, features_by_candidate = build_candidate_score_maps(
        rows,
        report_dir,
        args.feature_set,
        bootstrap_samples=args.bootstrap_samples,
        logistic_epochs=args.logistic_epochs,
    )
    best = best_candidate(candidates)
    best_id = str(best.get("candidate_id") or "")
    threshold_grid, threshold_csv_rows = build_threshold_grid(
        rows,
        candidates,
        score_maps,
        bootstrap_samples=args.bootstrap_samples,
    )
    stability, stability_csv_rows = stability_report(
        rows,
        score_maps.get(best_id, {}),
        bootstrap_samples=args.bootstrap_samples,
    )
    ablations = ablation_report(
        rows,
        features_by_candidate.get("combined:simple_feature_score_v1", []),
        bootstrap_samples=args.bootstrap_samples,
    )
    comparators = {
        "flow_only": simple_candidate_for_feature_set(
            rows,
            report_dir,
            "flow",
            "flow_only:simple_feature_score_v1",
            bootstrap_samples=args.bootstrap_samples,
        ),
        "gatekeeper_accept": gatekeeper_accept_comparator(rows),
    }
    source_scope = args.source_scope or infer_source_scope(dataset_manifest)
    simcov = run_simcov_gate(root, source_scope)
    report = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "selector_r2only_candidate_selection_v1",
        "phase": "phase3",
        "scope": args.scope,
        "source_scope": source_scope,
        "status": "P3J_PENDING",
        "claim_boundaries": {
            "diagnostic_only": True,
            "production_promotion_allowed": False,
            "gatekeeper_tuning_started": False,
            "runtime_changed": False,
            "active_execution_changed": False,
            "send_path_changed": False,
        },
        "input_paths": {
            "selector_training_view_v1": str(training_view),
            "phase3_r2only_manifest_v1": str(phase3_manifest_path),
            "dataset_manifest_v1": str(dataset_manifest_path),
        },
        "feature_set_requested": args.feature_set,
        "r2_training_denominator_rows": len(rows),
        "positive_rows": sum(1 for row in rows if label_positive(row)),
        "negative_rows": sum(1 for row in rows if not label_positive(row)),
        "split_counts": p3g.split_counts(rows),
        "phase3_gate": {
            "status": phase3_manifest.get("status"),
            "phase3_precision_readiness": phase3_manifest.get("phase3_precision_readiness"),
            "leakage_audit_status": phase3_manifest.get("leakage_audit_status"),
            "fail_reasons": phase3_manifest.get("fail_reasons"),
        },
        "simcov_operational_gate": {
            "status": simcov.get("status"),
            "metrics": simcov.get("metrics"),
            "fail_reasons": simcov.get("fail_reasons"),
        },
        "candidates": [
            {
                **candidate,
                "topk_summary": compact_topk(candidate),
            }
            for candidate in candidates
        ],
        "threshold_grid": threshold_grid,
        "stability": stability,
        "feature_ablation": ablations,
        "comparators": comparators,
        "ev_proxy": {
            "formula": "precision * target_net_pct - (1 - precision) * stop_net_pct",
            "target_net_pct": TARGET_NET_PCT,
            "stop_net_pct": STOP_NET_PCT,
            "claim": "R2 market-opportunity EV proxy, not live PnL",
        },
    }
    report["acceptance"] = acceptance_status(
        rows=rows,
        candidates=candidates,
        phase3_manifest=phase3_manifest,
        simcov=simcov,
        comparators=comparators,
    )
    report["status"] = report["acceptance"]["status"]

    report_dir.mkdir(parents=True, exist_ok=True)
    output_json = report_dir / "selector_r2only_candidate_selection_v1.json"
    output_md = report_dir / "FEATURE_RICH_R2_CANDIDATE_SELECTION.md"
    threshold_csv = report_dir / "selector_r2only_threshold_grid_v1.csv"
    stability_csv = report_dir / "selector_r2only_candidate_stability_v1.csv"
    common.write_json(output_json, report)
    output_md.write_text(markdown_report(report), encoding="utf-8")
    write_csv(
        threshold_csv,
        threshold_csv_rows,
        [
            "candidate_id",
            "threshold_type",
            "threshold_label",
            "threshold",
            "split",
            "rows",
            "selected_count",
            "positive_count",
            "negative_count",
            "precision",
            "recall",
            "accept_rate",
            "base_positive_rate",
            "lift_vs_base_rate",
            "ev_proxy_pct",
            "bootstrap_ci_precision_p025",
            "bootstrap_ci_precision_p50",
            "bootstrap_ci_precision_p975",
        ],
    )
    write_csv(
        stability_csv,
        stability_csv_rows,
        [
            "bucket",
            "bucket_value",
            "split",
            "rows",
            "selected_count",
            "positive_count",
            "negative_count",
            "precision",
            "recall",
            "accept_rate",
            "base_positive_rate",
            "lift_vs_base_rate",
            "ev_proxy_pct",
            "bootstrap_ci_precision_p025",
            "bootstrap_ci_precision_p50",
            "bootstrap_ci_precision_p975",
        ],
    )
    report["outputs"] = {
        "selector_r2only_candidate_selection_v1": str(output_json),
        "FEATURE_RICH_R2_CANDIDATE_SELECTION": str(output_md),
        "selector_r2only_threshold_grid_v1": str(threshold_csv),
        "selector_r2only_candidate_stability_v1": str(stability_csv),
    }
    common.write_json(output_json, report)
    return report


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument("--feature-set", choices=["flow", "gk", "combined"], default="combined")
    parser.add_argument("--source-scope")
    parser.add_argument("--bootstrap-samples", type=int, default=BOOTSTRAP_SAMPLES)
    parser.add_argument("--logistic-epochs", type=int, default=600)
    parser.add_argument("--json", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = build_report(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0 if str(report.get("status", "")).startswith("P3J_PASS") else 1


if __name__ == "__main__":
    raise SystemExit(main())
