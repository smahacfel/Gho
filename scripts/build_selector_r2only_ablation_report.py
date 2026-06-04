#!/usr/bin/env python3
"""Build diagnostic R2-only feature ablation report without model promotion."""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any

import build_selector_r2only_baseline_report as baseline
import build_selector_training_view as training
import selector_pipeline_common as common


DEFAULT_FEATURES = (
    "curve_progress_pct",
    "net_quote_in_15s",
    "net_quote_in_30s",
    "trade_rate",
    "unique_buyers",
    "sell_share",
    "top1_wallet_share",
    "buyer_hhi",
    "creator_sold_early_flag",
    "quote_mint_is_sol",
)


def read_json(path: Path | None) -> dict[str, Any]:
    if path is None or not path.exists():
        return {}
    with path.open(encoding="utf-8") as fh:
        payload = json.load(fh)
    return payload if isinstance(payload, dict) else {}


def denominator_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [row for row in rows if training.r2_training_denominator(row)]


def feature_value(row: dict[str, Any], feature: str) -> float | None:
    value = row.get(feature)
    if isinstance(value, bool):
        return 1.0 if value else 0.0
    return common.float_or_none(value)


def feature_availability(rows: list[dict[str, Any]], feature: str) -> dict[str, Any]:
    values = [feature_value(row, feature) for row in rows]
    present = [value for value in values if value is not None]
    unique_values = sorted(set(present))
    return {
        "present_rows": len(present),
        "present_rate": len(present) / len(rows) if rows else None,
        "unique_value_count": len(unique_values),
        "min": min(unique_values) if unique_values else None,
        "max": max(unique_values) if unique_values else None,
    }


def available_features(rows: list[dict[str, Any]], *, min_present_rate: float) -> list[str]:
    features = []
    for feature in DEFAULT_FEATURES:
        availability = feature_availability(rows, feature)
        if (
            (availability["present_rate"] or 0.0) >= min_present_rate
            and (availability["unique_value_count"] or 0) > 1
        ):
            features.append(feature)
    return features


def direction_from_train(train_rows: list[dict[str, Any]], feature: str) -> int:
    positives = [feature_value(row, feature) for row in train_rows if row.get("r2_label") == "positive"]
    negatives = [feature_value(row, feature) for row in train_rows if row.get("r2_label") == "negative"]
    positives = [value for value in positives if value is not None]
    negatives = [value for value in negatives if value is not None]
    if not positives or not negatives:
        return 1
    return 1 if (sum(positives) / len(positives)) >= (sum(negatives) / len(negatives)) else -1


def feature_ranges(train_rows: list[dict[str, Any]], features: list[str]) -> dict[str, dict[str, float]]:
    ranges = {}
    for feature in features:
        values = [feature_value(row, feature) for row in train_rows]
        values = [value for value in values if value is not None]
        ranges[feature] = {
            "min": min(values) if values else 0.0,
            "max": max(values) if values else 0.0,
        }
    return ranges


def normalized_feature(row: dict[str, Any], feature: str, ranges: dict[str, dict[str, float]], direction: int) -> float:
    value = feature_value(row, feature)
    if value is None:
        return 0.0
    low = ranges[feature]["min"]
    high = ranges[feature]["max"]
    if high == low:
        score = 0.0
    else:
        score = (value - low) / (high - low)
    if direction < 0:
        score = 1.0 - score
    return max(0.0, min(1.0, score))


def score_rows(
    rows: list[dict[str, Any]],
    *,
    features: list[str],
    train_rows: list[dict[str, Any]],
) -> dict[str, dict[str, float]]:
    directions = {feature: direction_from_train(train_rows, feature) for feature in features}
    ranges = feature_ranges(train_rows, features)
    scores: dict[str, dict[str, float]] = {}
    if not features:
        return scores
    for row in rows:
        candidate_id = common.str_or_none(row.get("candidate_id"))
        if not candidate_id:
            continue
        row_scores = {}
        for feature in features:
            row_scores[feature] = normalized_feature(row, feature, ranges, directions[feature])
        row_scores["simple_available_feature_score"] = (
            sum(row_scores.values()) / len(features) if features else 0.0
        )
        scores[candidate_id] = row_scores
    return scores


def precision_top_k_by_score(
    rows: list[dict[str, Any]],
    scores: dict[str, dict[str, float]],
    score_name: str,
    top_k_values: list[int],
) -> list[dict[str, Any]]:
    scored = []
    for row in rows:
        candidate_id = common.str_or_none(row.get("candidate_id"))
        if not candidate_id:
            continue
        score = scores.get(candidate_id, {}).get(score_name)
        if score is None:
            continue
        scored.append((score, common.int_or_none(row.get("birth_ts_ms")) or 0, candidate_id, row))
    scored.sort(key=lambda item: (-item[0], item[1], item[2]))
    reports = []
    for top_k in top_k_values:
        selected = [item[3] for item in scored[: min(top_k, len(scored))]]
        metrics = baseline.metric_block(selected, lambda _row: True)
        reports.append(
            {
                "k": top_k,
                "available_scored_rows": len(scored),
                "selected_count": metrics["selected_count"],
                "tp_r2": metrics["tp_r2"],
                "fp_r2": metrics["fp_r2"],
                "precision_r2": metrics["precision_r2"],
                "positive_rate": metrics["positive_rate"],
            }
        )
    return reports


def split_metric_blocks(rows: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        split: baseline.metric_block(
            [row for row in rows if row.get("split") == split],
            baseline.selector_accept,
        )
        for split in ("train", "validation", "holdout")
    }


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    dataset_dir = args.root / "datasets" / "selector" / args.scope
    report_dir = args.root / "reports" / "selector" / args.scope
    training_view = args.training_view or dataset_dir / "selector_training_view_v1.jsonl"
    feature_audit_path = args.feature_audit or report_dir / "selector_r2only_feature_audit_v1.json"
    rows = list(common.iter_json_objects(training_view))
    denominator = denominator_rows(rows)
    train_rows = [row for row in denominator if row.get("split") == "train"]
    holdout_rows = [row for row in denominator if row.get("split") == "holdout"]
    features = available_features(denominator, min_present_rate=args.min_feature_present_rate)
    scores = score_rows(denominator, features=features, train_rows=train_rows)
    full_score_name = "simple_available_feature_score"
    ablations = []
    for dropped in features:
        reduced_features = [feature for feature in features if feature != dropped]
        reduced_scores = score_rows(denominator, features=reduced_features, train_rows=train_rows)
        ablations.append(
            {
                "dropped_feature": dropped,
                "remaining_features": reduced_features,
                "holdout_precision_at_top_k": precision_top_k_by_score(
                    holdout_rows,
                    reduced_scores,
                    full_score_name,
                    args.top_k,
                ),
            }
        )
    report = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "selector_r2only_ablation_report_v1",
        "phase": "phase3",
        "status": "P3C_PASS_DIAGNOSTIC_ONLY",
        "scope": args.scope,
        "dataset_kind": "r2_only",
        "training_rows": len(rows),
        "resolved_denominator_count": len(denominator),
        "positive_rows": sum(1 for row in denominator if row.get("r2_label") == "positive"),
        "negative_rows": sum(1 for row in denominator if row.get("r2_label") == "negative"),
        "split_counts": {
            split: common.counter_dict(
                Counter(
                    str(row.get("r2_label") or "unknown")
                    for row in denominator
                    if row.get("split") == split
                )
            )
            for split in ("train", "validation", "holdout")
        },
        "feature_audit": {
            "path": str(feature_audit_path),
            "status": read_json(feature_audit_path).get("status"),
        },
        "feature_availability": {
            feature: feature_availability(denominator, feature) for feature in DEFAULT_FEATURES
        },
        "available_features_used": features,
        "selector_accept_context": {
            "by_split": split_metric_blocks(denominator),
            "holdout": baseline.metric_block(holdout_rows, baseline.selector_accept),
        },
        "simple_available_feature_score": {
            "status": "available" if features else "NO_VARIABLE_FEATURES",
            "features": features,
            "holdout_precision_at_top_k": (
                precision_top_k_by_score(
                    holdout_rows,
                    scores,
                    full_score_name,
                    args.top_k,
                )
                if features
                else []
            ),
        },
        "single_feature_rankings": {
            feature: {
                "holdout_precision_at_top_k": precision_top_k_by_score(
                    holdout_rows,
                    scores,
                    feature,
                    args.top_k,
                )
            }
            for feature in features
        },
        "leave_one_feature_out": ablations,
        "claim_boundaries": {
            "diagnostic_only": True,
            "model_ready": False,
            "gatekeeper_tuning_started": False,
            "production_promotion_claim": False,
            "market_recall_claim": False,
        },
        "caveats": [
            "Diagnostic ablation only.",
            "No Gatekeeper tuning.",
            "No production promotion.",
            "No market recall claim.",
            "No R1 lifecycle claim.",
        ],
    }
    common.write_json(args.output or report_dir / "selector_r2only_ablation_report_v1.json", report)
    return report


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument("--training-view", type=Path)
    parser.add_argument("--feature-audit", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--top-k", type=int, nargs="+", default=[10, 25, 50, 100])
    parser.add_argument("--min-feature-present-rate", type=float, default=0.5)
    parser.add_argument("--json", action="store_true")
    return parser


def run(args: argparse.Namespace) -> dict[str, Any]:
    return build_report(args)


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = run(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
