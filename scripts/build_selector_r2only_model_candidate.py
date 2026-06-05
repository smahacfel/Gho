#!/usr/bin/env python3
"""Build P3G diagnostic R2-only model/rules baseline candidates without tuning."""

from __future__ import annotations

import argparse
import csv
import json
import math
import random
from collections import Counter
from pathlib import Path
from typing import Any

import build_selector_r2only_baseline_report as baseline
import build_selector_r2only_feature_contribution as contribution
import selector_pipeline_common as common


FLOW_FEATURES = (
    "net_quote_in_15s",
    "net_quote_in_30s",
    "trade_rate",
    "unique_buyers",
    "sell_share",
    "top1_wallet_share",
    "buyer_hhi",
)
FEATURES = FLOW_FEATURES
GK_PROVENANCE_COLUMNS = {
    "gk_log_schema_version",
    "gk_decision_plane",
    "gk_observation_profile",
    "gk_context_status",
    "gk_cutoff_status",
}
GK_MODEL_ALLOWED_CUTOFF_STATUSES = {"ok", "same_decision_time"}
SPLITS = ("train", "validation", "holdout")
TOP_K = (10, 25, 50, 100)


def read_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as fh:
        payload = json.load(fh)
    if not isinstance(payload, dict):
        raise ValueError(f"expected JSON object in {path}")
    return payload


def requested_feature_sets(args: argparse.Namespace) -> list[str]:
    raw = args.feature_set or ["flow"]
    out: list[str] = []
    for item in raw:
        if item not in out:
            out.append(item)
    return out


def gk_row_valid_for_model(row: dict[str, Any]) -> bool:
    return bool(
        row.get("gk_context_status") == "ok"
        and row.get("gk_cutoff_status") in GK_MODEL_ALLOWED_CUTOFF_STATUSES
    )


def feature_value(row: dict[str, Any], feature: str) -> float | None:
    if feature.startswith("gk_") and feature not in GK_PROVENANCE_COLUMNS:
        if not gk_row_valid_for_model(row):
            return None
    value = row.get(feature)
    if isinstance(value, bool):
        return 1.0 if value else 0.0
    return common.float_or_none(value)


def denominator_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [row for row in rows if baseline.r2only_denominator(row)]


def label_positive(row: dict[str, Any]) -> bool:
    return row.get("r2_label") == "positive"


def base_rate(rows: list[dict[str, Any]]) -> float | None:
    if not rows:
        return None
    return sum(1 for row in rows if label_positive(row)) / len(rows)


def split_counts(rows: list[dict[str, Any]]) -> dict[str, dict[str, int]]:
    counts: dict[str, Counter[str]] = {}
    for row in rows:
        counts.setdefault(str(row.get("split") or "unknown"), Counter())[str(row.get("r2_label") or "unknown")] += 1
    return {split: common.counter_dict(counter) for split, counter in sorted(counts.items())}


def read_gatekeeper_manifest(report_dir: Path) -> dict[str, Any]:
    path = report_dir / "gatekeeper_feature_context_manifest_v1.json"
    if not path.exists() or path.stat().st_size == 0:
        return {}
    return read_json(path)


def gk_manifest_feature_columns(report_dir: Path, rows: list[dict[str, Any]]) -> list[str]:
    manifest = read_gatekeeper_manifest(report_dir)
    raw_columns = manifest.get("model_feature_columns")
    if not isinstance(raw_columns, list):
        raw_columns = manifest.get("feature_columns")
    if isinstance(raw_columns, list):
        candidates = [str(feature) for feature in raw_columns if str(feature).startswith("gk_")]
    else:
        candidates = sorted({key for row in rows for key in row if key.startswith("gk_")})
    return [
        feature
        for feature in candidates
        if feature not in GK_PROVENANCE_COLUMNS
    ]


def has_variation(rows: list[dict[str, Any]], feature: str) -> bool:
    values = [feature_value(row, feature) for row in rows]
    present = [value for value in values if value is not None]
    return bool(present and len(set(present)) > 1)


def available_features(
    rows: list[dict[str, Any]],
    p3f_report: dict[str, Any],
    *,
    feature_set: str = "flow",
    report_dir: Path | None = None,
) -> list[str]:
    by_set = p3f_report.get("available_features_by_set")
    if isinstance(by_set, dict) and isinstance(by_set.get(feature_set), list):
        candidates = [str(feature) for feature in by_set.get(feature_set, [])]
    elif feature_set == "flow":
        p3f_features = p3f_report.get("available_features_used")
        if isinstance(p3f_features, list):
            candidates = [str(feature) for feature in p3f_features if str(feature) in FLOW_FEATURES]
        else:
            candidates = list(FLOW_FEATURES)
    elif feature_set == "gk":
        candidates = gk_manifest_feature_columns(report_dir or Path("."), rows)
    elif feature_set == "combined":
        candidates = available_features(rows, p3f_report, feature_set="flow", report_dir=report_dir)
        for feature in available_features(rows, p3f_report, feature_set="gk", report_dir=report_dir):
            if feature not in candidates:
                candidates.append(feature)
    else:
        raise ValueError(f"unsupported feature_set: {feature_set}")
    out = []
    for feature in candidates:
        values = [feature_value(row, feature) for row in rows]
        present = [value for value in values if value is not None]
        if present and len(set(present)) > 1:
            out.append(feature)
    return out


def train_direction(train_rows: list[dict[str, Any]], feature: str) -> int:
    positives = [feature_value(row, feature) for row in train_rows if label_positive(row)]
    negatives = [feature_value(row, feature) for row in train_rows if not label_positive(row)]
    positives = [value for value in positives if value is not None]
    negatives = [value for value in negatives if value is not None]
    if not positives or not negatives:
        return 1
    pos_mean = sum(positives) / len(positives)
    neg_mean = sum(negatives) / len(negatives)
    return 1 if pos_mean >= neg_mean else -1


def feature_ranges(train_rows: list[dict[str, Any]], features: list[str]) -> dict[str, dict[str, float]]:
    ranges: dict[str, dict[str, float]] = {}
    for feature in features:
        values = [feature_value(row, feature) for row in train_rows]
        values = [value for value in values if value is not None]
        ranges[feature] = {
            "min": min(values) if values else 0.0,
            "max": max(values) if values else 0.0,
            "direction": float(train_direction(train_rows, feature)),
        }
    return ranges


def normalized_feature(row: dict[str, Any], feature: str, ranges: dict[str, dict[str, float]]) -> float:
    value = feature_value(row, feature)
    if value is None:
        return 0.0
    low = ranges[feature]["min"]
    high = ranges[feature]["max"]
    if high == low:
        score = 0.0
    else:
        score = (value - low) / (high - low)
    if ranges[feature]["direction"] < 0:
        score = 1.0 - score
    return max(0.0, min(1.0, score))


def simple_scores(
    rows: list[dict[str, Any]],
    *,
    train_rows: list[dict[str, Any]],
    features: list[str],
) -> dict[str, dict[str, float]]:
    ranges = feature_ranges(train_rows, features)
    scores: dict[str, dict[str, float]] = {}
    for row in rows:
        candidate_id = common.str_or_none(row.get("candidate_id"))
        if not candidate_id:
            continue
        parts = {feature: normalized_feature(row, feature, ranges) for feature in features}
        scores[candidate_id] = {
            "simple_feature_score_v1": sum(parts.values()) / len(parts) if parts else 0.0,
            **{f"single_feature_ranker:{feature}": value for feature, value in parts.items()},
        }
    return scores


def imputation_stats(train_rows: list[dict[str, Any]], features: list[str]) -> dict[str, dict[str, float]]:
    stats: dict[str, dict[str, float]] = {}
    for feature in features:
        values = [feature_value(row, feature) for row in train_rows]
        values = [value for value in values if value is not None]
        mean = sum(values) / len(values) if values else 0.0
        variance = sum((value - mean) ** 2 for value in values) / len(values) if values else 0.0
        std = math.sqrt(variance) if variance > 0 else 1.0
        stats[feature] = {"mean": mean, "std": std}
    return stats


def feature_vector(row: dict[str, Any], features: list[str], stats: dict[str, dict[str, float]]) -> list[float]:
    vector = []
    for feature in features:
        value = feature_value(row, feature)
        if value is None:
            value = stats[feature]["mean"]
        vector.append((value - stats[feature]["mean"]) / stats[feature]["std"])
    return vector


def sigmoid(value: float) -> float:
    if value >= 0:
        z = math.exp(-value)
        return 1.0 / (1.0 + z)
    z = math.exp(value)
    return z / (1.0 + z)


def train_logistic(
    train_rows: list[dict[str, Any]],
    *,
    features: list[str],
    learning_rate: float,
    l2: float,
    epochs: int,
) -> dict[str, Any]:
    stats = imputation_stats(train_rows, features)
    weights = [0.0 for _ in features]
    positives = sum(1 for row in train_rows if label_positive(row))
    base = positives / len(train_rows) if train_rows else 0.5
    intercept = math.log(base / (1.0 - base)) if 0.0 < base < 1.0 else 0.0
    for _epoch in range(epochs):
        grad_w = [0.0 for _ in features]
        grad_b = 0.0
        for row in train_rows:
            vector = feature_vector(row, features, stats)
            y = 1.0 if label_positive(row) else 0.0
            pred = sigmoid(intercept + sum(weight * value for weight, value in zip(weights, vector)))
            err = pred - y
            grad_b += err
            for idx, value in enumerate(vector):
                grad_w[idx] += err * value
        denom = float(max(len(train_rows), 1))
        intercept -= learning_rate * (grad_b / denom)
        for idx, weight in enumerate(weights):
            regularized = (grad_w[idx] / denom) + (l2 * weight)
            weights[idx] -= learning_rate * regularized
    return {
        "features": features,
        "feature_stats": stats,
        "intercept": intercept,
        "weights": dict(zip(features, weights)),
        "learning_rate": learning_rate,
        "l2": l2,
        "epochs": epochs,
    }


def logistic_scores(
    rows: list[dict[str, Any]],
    *,
    model: dict[str, Any],
) -> dict[str, float]:
    features = [str(feature) for feature in model.get("features", [])]
    stats = model.get("feature_stats") if isinstance(model.get("feature_stats"), dict) else {}
    weights = model.get("weights") if isinstance(model.get("weights"), dict) else {}
    intercept = common.float_or_none(model.get("intercept")) or 0.0
    scores: dict[str, float] = {}
    for row in rows:
        candidate_id = common.str_or_none(row.get("candidate_id"))
        if not candidate_id:
            continue
        vector = feature_vector(row, features, stats)
        raw = intercept + sum((common.float_or_none(weights.get(feature)) or 0.0) * value for feature, value in zip(features, vector))
        scores[candidate_id] = sigmoid(raw)
    return scores


def order_rows(
    rows: list[dict[str, Any]],
    score_map: dict[str, float],
) -> list[dict[str, Any]]:
    sortable: list[tuple[float, int, str, dict[str, Any]]] = []
    for row in rows:
        candidate_id = common.str_or_none(row.get("candidate_id"))
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


def bootstrap_ci(selected_rows: list[dict[str, Any]], *, samples: int, seed: int) -> dict[str, Any]:
    if not selected_rows:
        return {
            "samples": samples,
            "seed": seed,
            "selected_count": 0,
            "precision_mean": None,
            "precision_p025": None,
            "precision_p975": None,
        }
    rng = random.Random(seed)
    values = []
    for _ in range(samples):
        draw = [selected_rows[rng.randrange(len(selected_rows))] for _idx in range(len(selected_rows))]
        values.append(sum(1 for row in draw if label_positive(row)) / len(draw))
    values.sort()
    p025_idx = int(0.025 * (len(values) - 1))
    p975_idx = int(0.975 * (len(values) - 1))
    return {
        "samples": samples,
        "seed": seed,
        "selected_count": len(selected_rows),
        "precision_mean": sum(values) / len(values),
        "precision_p025": values[p025_idx],
        "precision_p975": values[p975_idx],
    }


def top_k_metrics(
    rows: list[dict[str, Any]],
    score_map: dict[str, float],
    *,
    top_k_values: tuple[int, ...],
    bootstrap_samples: int,
    bootstrap_seed: int,
    include_bootstrap: bool,
) -> list[dict[str, Any]]:
    ordered = order_rows(rows, score_map)
    split_base = base_rate(rows)
    out = []
    for k in top_k_values:
        selected = ordered[: min(k, len(ordered))]
        metrics = baseline.metric_block(selected, lambda _row: True)
        payload = {
            "k": k,
            "available_scored_rows": len(ordered),
            "selected_count": metrics["selected_count"],
            "tp_r2": metrics["tp_r2"],
            "fp_r2": metrics["fp_r2"],
            "precision_r2": metrics["precision_r2"],
            "positive_rate": metrics["positive_rate"],
            "base_positive_rate": split_base,
            "lift_vs_base_rate": (
                metrics["precision_r2"] / split_base
                if metrics["precision_r2"] is not None and split_base
                else None
            ),
        }
        if include_bootstrap:
            payload["bootstrap_ci_precision"] = bootstrap_ci(
                selected,
                samples=bootstrap_samples,
                seed=bootstrap_seed + k,
            )
        out.append(payload)
    return out


def candidate_metrics(
    rows: list[dict[str, Any]],
    score_map: dict[str, float],
    *,
    top_k_values: tuple[int, ...],
    bootstrap_samples: int,
    bootstrap_seed: int,
) -> dict[str, Any]:
    by_split = {}
    for split in SPLITS:
        split_rows = [row for row in rows if row.get("split") == split]
        by_split[split] = {
            "rows": len(split_rows),
            "base_positive_rate": base_rate(split_rows),
            "precision_at_top_k": top_k_metrics(
                split_rows,
                score_map,
                top_k_values=top_k_values,
                bootstrap_samples=bootstrap_samples,
                bootstrap_seed=bootstrap_seed,
                include_bootstrap=split == "holdout",
            ),
        }
    return by_split


def top_metric(candidate: dict[str, Any], split: str, k: int) -> dict[str, Any]:
    metrics = candidate.get("by_split", {}).get(split, {}).get("precision_at_top_k", [])
    if isinstance(metrics, list):
        return next((item for item in metrics if item.get("k") == k), {})
    return {}


def leakage_status(dataset_manifest: dict[str, Any]) -> str:
    leakage = dataset_manifest.get("leakage_precheck") or dataset_manifest.get("leakage_audit_status")
    if isinstance(leakage, dict):
        leakage = leakage.get("status")
    if isinstance(leakage, str):
        return leakage
    stage_reports = dataset_manifest.get("stage_reports")
    if isinstance(stage_reports, dict):
        for stage_name in ("feature_snapshots_v1", "label_coverage_v1"):
            stage = stage_reports.get(stage_name)
            if isinstance(stage, dict) and isinstance(stage.get("leakage_precheck"), str):
                return str(stage["leakage_precheck"])
    return "UNKNOWN"


def acceptance_status(candidates: list[dict[str, Any]], dataset_manifest: dict[str, Any]) -> dict[str, Any]:
    best = best_candidate(candidates)
    leakage = leakage_status(dataset_manifest)
    train_top10 = top_metric(best, "train", 10).get("precision_r2")
    validation_top10 = top_metric(best, "validation", 10).get("precision_r2")
    holdout_top10 = top_metric(best, "holdout", 10).get("precision_r2")
    split_lifts = [
        top_metric(best, split, 10).get("lift_vs_base_rate")
        for split in SPLITS
    ]
    pass_gate = bool(
        (holdout_top10 or 0.0) >= 0.55
        and (validation_top10 or 0.0) >= 0.50
        and all((lift or 0.0) > 1.0 for lift in split_lifts)
        and leakage == "PASS"
    )
    fail_reasons = []
    if (holdout_top10 or 0.0) < 0.55:
        fail_reasons.append("holdout_top10_below_55pct")
    if (validation_top10 or 0.0) < 0.50:
        fail_reasons.append("validation_top10_below_50pct")
    if not all((lift or 0.0) > 1.0 for lift in split_lifts):
        fail_reasons.append("top10_lift_not_positive_across_splits")
    if leakage != "PASS":
        fail_reasons.append("leakage_not_pass")
    return {
        "status": "P3G_PASS_DIAGNOSTIC_MODEL_CANDIDATE" if pass_gate else "P3G_DIAGNOSTIC_NO_GO_OR_NEEDS_MORE_DATA",
        "best_candidate_id": best.get("candidate_id"),
        "train_top10_precision": train_top10,
        "validation_top10_precision": validation_top10,
        "holdout_top10_precision": holdout_top10,
        "top10_lift_vs_base_by_split": dict(zip(SPLITS, split_lifts)),
        "leakage_status": leakage,
        "fail_reasons": fail_reasons,
    }


def best_candidate(candidates: list[dict[str, Any]]) -> dict[str, Any]:
    def score(candidate: dict[str, Any]) -> tuple[float, float, float, str]:
        holdout = top_metric(candidate, "holdout", 10).get("precision_r2")
        validation = top_metric(candidate, "validation", 10).get("precision_r2")
        train = top_metric(candidate, "train", 10).get("precision_r2")
        return (
            float(holdout or -1.0),
            float(validation or -1.0),
            float(train or -1.0),
            str(candidate.get("candidate_id")),
        )

    return max(candidates, key=score) if candidates else {}


def gatekeeper_metrics(rows: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        split: baseline.metric_block(
            [row for row in rows if row.get("split") == split],
            baseline.selector_accept,
        )
        for split in SPLITS
    }


def rank_csv_rows(
    rows: list[dict[str, Any]],
    candidates: list[dict[str, Any]],
    score_maps: dict[str, dict[str, float]],
) -> list[dict[str, Any]]:
    out = []
    for candidate in candidates:
        candidate_id = str(candidate.get("candidate_id"))
        score_map = score_maps.get(candidate_id, {})
        for split in SPLITS:
            split_rows = [row for row in rows if row.get("split") == split]
            for rank, row in enumerate(order_rows(split_rows, score_map), start=1):
                if rank > 100:
                    break
                row_id = common.str_or_none(row.get("candidate_id")) or ""
                out.append(
                    {
                        "candidate": candidate_id,
                        "split": split,
                        "rank": rank,
                        "row_candidate_id": row_id,
                        "base_mint": row.get("base_mint"),
                        "r2_label": row.get("r2_label"),
                        "gatekeeper_accept": baseline.selector_accept(row),
                        "score": score_map.get(row_id),
                    }
                )
    return out


def write_csv(path: Path, rows: list[dict[str, Any]], fieldnames: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames)
        writer.writeheader()
        for row in rows:
            writer.writerow({field: row.get(field) for field in fieldnames})


def format_pct(value: Any) -> str:
    if not isinstance(value, (int, float)):
        return "n/a"
    return f"{value * 100:.2f}%"


def markdown_report(report: dict[str, Any]) -> str:
    acceptance = report.get("acceptance", {})
    lines = [
        "# FEATURE_RICH_R2_MODEL_CANDIDATE",
        "",
        f"Scope: `{report['scope']}`",
        "",
        "## Verdict",
        "",
        f"Status: `{report['status']}`",
        f"Best candidate: `{acceptance.get('best_candidate_id')}`",
        "",
        "Diagnostic-only. This is not Gatekeeper tuning, not a production claim, and not a policy change.",
        "",
        "## Denominator",
        "",
        f"- Resolved R2 rows: {report['resolved_denominator_rows']}",
        f"- Positive rows: {report['positive_rows']}",
        f"- Negative rows: {report['negative_rows']}",
        f"- Split counts: `{json.dumps(report['split_counts'], sort_keys=True)}`",
        "",
        "## Candidate Top10",
        "",
        "| candidate | train | validation | holdout | holdout CI |",
        "| --- | ---: | ---: | ---: | --- |",
    ]
    for candidate in report.get("candidates", []):
        train = top_metric(candidate, "train", 10).get("precision_r2")
        validation = top_metric(candidate, "validation", 10).get("precision_r2")
        holdout = top_metric(candidate, "holdout", 10).get("precision_r2")
        ci = top_metric(candidate, "holdout", 10).get("bootstrap_ci_precision", {})
        ci_text = "n/a"
        if isinstance(ci, dict) and isinstance(ci.get("precision_p025"), (int, float)):
            ci_text = f"{format_pct(ci.get('precision_p025'))}..{format_pct(ci.get('precision_p975'))}"
        lines.append(
            f"| {candidate.get('candidate_id')} | {format_pct(train)} | {format_pct(validation)} | {format_pct(holdout)} | {ci_text} |"
        )
    if report.get("feature_set_reports"):
        lines.extend(
            [
                "",
                "## Feature Set Comparison",
                "",
                "| feature set | features | best candidate | holdout top10 |",
                "| --- | ---: | --- | ---: |",
            ]
        )
        for feature_set, payload in report.get("feature_set_reports", {}).items():
            best = payload.get("best_candidate", {}) if isinstance(payload, dict) else {}
            holdout = top_metric(best, "holdout", 10).get("precision_r2")
            lines.append(
                f"| {feature_set} | {len(payload.get('features_used', [])) if isinstance(payload, dict) else 0} | {best.get('candidate_id')} | {format_pct(holdout)} |"
            )
    lines.extend(
        [
            "",
            "## Gatekeeper Comparator",
            "",
            "| split | selected | precision | accept rate |",
            "| --- | ---: | ---: | ---: |",
        ]
    )
    for split, metrics in report.get("gatekeeper_accept_context", {}).items():
        lines.append(
            f"| {split} | {metrics.get('selected_count')} | {format_pct(metrics.get('precision_r2'))} | {format_pct(metrics.get('accept_rate'))} |"
        )
    lines.extend(
        [
            "",
            "## Acceptance",
            "",
            f"- status: `{acceptance.get('status')}`",
            f"- fail_reasons: `{json.dumps(acceptance.get('fail_reasons', []), sort_keys=True)}`",
            "",
            "## Claim Boundaries",
            "",
            "- diagnostic_only: true",
            "- production_ready: false",
            "- model_ready: false",
            "- gatekeeper_tuned: false",
            "- threshold_changes: false",
            "- runtime_changed: false",
        ]
    )
    return "\n".join(lines) + "\n"


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    dataset_dir = args.root / "datasets" / "selector" / args.scope
    report_dir = args.root / "reports" / "selector" / args.scope
    training_view = dataset_dir / "selector_training_view_v1.jsonl"
    p3f_path = report_dir / "selector_r2only_feature_contribution_v1.json"
    dataset_manifest_path = report_dir / "dataset_manifest_v1.json"
    missing = [str(path) for path in (training_view, p3f_path, dataset_manifest_path) if not path.exists() or path.stat().st_size == 0]
    if missing:
        raise FileNotFoundError(f"missing required P3G inputs: {missing}")
    rows = list(common.iter_json_objects(training_view))
    denominator = denominator_rows(rows)
    train_rows = [row for row in denominator if row.get("split") == "train"]
    p3f_report = read_json(p3f_path)
    dataset_manifest = read_json(dataset_manifest_path)
    feature_sets = requested_feature_sets(args)
    primary_feature_set = feature_sets[0]
    score_maps: dict[str, dict[str, float]] = {}
    candidates: list[dict[str, Any]] = []
    feature_set_reports: dict[str, dict[str, Any]] = {}
    logistic_models: dict[str, dict[str, Any]] = {}

    def candidate_id_for(feature_set: str, base_id: str) -> str:
        if len(feature_sets) == 1 and feature_set == "flow":
            return base_id
        return f"{feature_set}:{base_id}"

    for feature_set in feature_sets:
        features = available_features(
            denominator,
            p3f_report,
            feature_set=feature_set,
            report_dir=report_dir,
        )
        simple = simple_scores(denominator, train_rows=train_rows, features=features)
        set_score_maps: dict[str, dict[str, float]] = {
            candidate_id_for(feature_set, "simple_feature_score_v1"): {
                candidate_id: payload["simple_feature_score_v1"]
                for candidate_id, payload in simple.items()
            }
        }
        for feature in features:
            base_id = f"single_feature_ranker:{feature}"
            set_score_maps[candidate_id_for(feature_set, base_id)] = {
                candidate_id: payload[base_id]
                for candidate_id, payload in simple.items()
            }
        logistic_model = train_logistic(
            train_rows,
            features=features,
            learning_rate=args.logistic_learning_rate,
            l2=args.logistic_l2,
            epochs=args.logistic_epochs,
        )
        logistic_models[feature_set] = logistic_model
        set_score_maps[candidate_id_for(feature_set, "logistic_sanity_baseline")] = logistic_scores(
            denominator, model=logistic_model
        )
        set_candidates: list[dict[str, Any]] = []
        for candidate_id, score_map in set_score_maps.items():
            base_candidate_id = candidate_id.split(":", 1)[1] if candidate_id.startswith(f"{feature_set}:") else candidate_id
            candidate = {
                "candidate_id": candidate_id,
                "feature_set": feature_set,
                "candidate_kind": (
                    "simple_feature_score"
                    if base_candidate_id == "simple_feature_score_v1"
                    else "single_feature_ranker"
                    if base_candidate_id.startswith("single_feature_ranker:")
                    else "logistic_or_tree_sanity_baseline"
                ),
                "features": (
                    features
                    if not base_candidate_id.startswith("single_feature_ranker:")
                    else [base_candidate_id.split(":", 1)[1]]
                ),
                "by_split": candidate_metrics(
                    denominator,
                    score_map,
                    top_k_values=TOP_K,
                    bootstrap_samples=args.bootstrap_samples,
                    bootstrap_seed=args.bootstrap_seed,
                ),
            }
            candidates.append(candidate)
            set_candidates.append(candidate)
        score_maps.update(set_score_maps)
        feature_set_reports[feature_set] = {
            "feature_set": feature_set,
            "features_used": features,
            "logistic_sanity_baseline": logistic_model,
            "candidates": set_candidates,
            "best_candidate": best_candidate(set_candidates),
        }

    features = feature_set_reports[primary_feature_set]["features_used"]
    logistic_model = logistic_models[primary_feature_set]

    report: dict[str, Any] = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "selector_r2only_model_candidate_v1",
        "phase": "phase3",
        "status": "P3G_PENDING",
        "scope": args.scope,
        "dataset_kind": "r2_only",
        "feature_sets_requested": feature_sets,
        "primary_feature_set": primary_feature_set,
        "claim_boundaries": {
            "diagnostic_only": True,
            "model_ready": False,
            "production_ready": False,
            "gatekeeper_tuned": False,
            "threshold_changes": False,
            "runtime_changed": False,
        },
        "input_paths": {
            "selector_training_view_v1": str(training_view),
            "selector_r2only_feature_contribution_v1": str(p3f_path),
            "dataset_manifest_v1": str(dataset_manifest_path),
        },
        "training_rows": len(rows),
        "resolved_denominator_rows": len(denominator),
        "positive_rows": sum(1 for row in denominator if row.get("r2_label") == "positive"),
        "negative_rows": sum(1 for row in denominator if row.get("r2_label") == "negative"),
        "split_counts": split_counts(denominator),
        "features_used": features,
        "features_used_by_set": {
            feature_set: payload["features_used"]
            for feature_set, payload in feature_set_reports.items()
        },
        "gatekeeper_accept_context": gatekeeper_metrics(denominator),
        "p3f_simple_score_reference": p3f_report.get("simple_score_stability"),
        "logistic_sanity_baseline": logistic_model,
        "logistic_sanity_baseline_by_set": logistic_models,
        "feature_set_reports": feature_set_reports,
        "candidates": candidates,
    }
    report["acceptance"] = acceptance_status(candidates, dataset_manifest)
    report["status"] = report["acceptance"]["status"]

    report_dir.mkdir(parents=True, exist_ok=True)
    output_json = report_dir / "selector_r2only_model_candidate_v1.json"
    output_md = report_dir / "FEATURE_RICH_R2_MODEL_CANDIDATE.md"
    output_csv = report_dir / "selector_r2only_model_candidate_rankings_v1.csv"
    common.write_json(output_json, report)
    output_md.write_text(markdown_report(report), encoding="utf-8")
    write_csv(
        output_csv,
        rank_csv_rows(denominator, candidates, score_maps),
        ["candidate", "split", "rank", "row_candidate_id", "base_mint", "r2_label", "gatekeeper_accept", "score"],
    )
    report["outputs"] = {
        "selector_r2only_model_candidate_v1": str(output_json),
        "FEATURE_RICH_R2_MODEL_CANDIDATE": str(output_md),
        "selector_r2only_model_candidate_rankings_v1": str(output_csv),
    }
    common.write_json(output_json, report)
    return report


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument(
        "--feature-set",
        action="append",
        choices=["flow", "gk", "combined"],
        help="Feature set to evaluate. Repeat to compare multiple sets. Defaults to flow.",
    )
    parser.add_argument("--bootstrap-samples", type=int, default=1000)
    parser.add_argument("--bootstrap-seed", type=int, default=4242)
    parser.add_argument("--logistic-learning-rate", type=float, default=0.05)
    parser.add_argument("--logistic-l2", type=float, default=0.01)
    parser.add_argument("--logistic-epochs", type=int, default=600)
    parser.add_argument("--json", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = build_report(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0 if report.get("status") in {"P3G_PASS_DIAGNOSTIC_MODEL_CANDIDATE", "P3G_DIAGNOSTIC_NO_GO_OR_NEEDS_MORE_DATA"} else 2


if __name__ == "__main__":
    raise SystemExit(main())
