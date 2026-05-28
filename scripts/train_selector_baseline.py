#!/usr/bin/env python3
"""Train deterministic offline selector baselines on selector_training_view_v1."""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
from typing import Any, Callable

import selector_pipeline_common as common


FEATURES = (
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


def usable_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [
        row
        for row in rows
        if row.get("eligible") is True
        and row.get("stream_completeness_ok") is True
        and row.get("label_resolved") is True
        and row.get("r2_label") in {"positive", "negative"}
    ]


def label(row: dict[str, Any]) -> int:
    return 1 if row.get("r2_label") == "positive" else 0


def feature_value(row: dict[str, Any], feature: str) -> float:
    value = row.get(feature)
    if isinstance(value, bool):
        return 1.0 if value else 0.0
    number = common.float_or_none(value)
    return number if number is not None else 0.0


def vector(row: dict[str, Any]) -> list[float]:
    return [feature_value(row, feature) for feature in FEATURES]


def normalize(train_rows: list[dict[str, Any]]) -> tuple[list[float], list[float]]:
    columns = list(zip(*(vector(row) for row in train_rows))) if train_rows else []
    means: list[float] = []
    scales: list[float] = []
    for col in columns:
        mean = sum(col) / len(col)
        variance = sum((value - mean) ** 2 for value in col) / len(col)
        scale = math.sqrt(variance) or 1.0
        means.append(mean)
        scales.append(scale)
    if not columns:
        means = [0.0 for _ in FEATURES]
        scales = [1.0 for _ in FEATURES]
    return means, scales


def normalized_vector(row: dict[str, Any], means: list[float], scales: list[float]) -> list[float]:
    return [(value - means[idx]) / scales[idx] for idx, value in enumerate(vector(row))]


def sigmoid(value: float) -> float:
    if value >= 0:
        z = math.exp(-value)
        return 1.0 / (1.0 + z)
    z = math.exp(value)
    return z / (1.0 + z)


def rules_score(row: dict[str, Any]) -> float:
    positive = (
        0.20 * min(max(feature_value(row, "curve_progress_pct") / 100.0, 0.0), 1.0)
        + 0.18 * min(max(feature_value(row, "net_quote_in_15s") / 10.0, 0.0), 1.0)
        + 0.14 * min(max(feature_value(row, "net_quote_in_30s") / 20.0, 0.0), 1.0)
        + 0.12 * min(max(feature_value(row, "trade_rate") / 5.0, 0.0), 1.0)
        + 0.12 * min(max(feature_value(row, "unique_buyers") / 20.0, 0.0), 1.0)
        + 0.08 * feature_value(row, "quote_mint_is_sol")
    )
    negative = (
        0.16 * min(max(feature_value(row, "sell_share"), 0.0), 1.0)
        + 0.12 * min(max(feature_value(row, "top1_wallet_share"), 0.0), 1.0)
        + 0.12 * min(max(feature_value(row, "buyer_hhi"), 0.0), 1.0)
        + 0.10 * feature_value(row, "creator_sold_early_flag")
    )
    return max(0.0, min(1.0, 0.5 + positive - negative))


def train_logistic(train_rows: list[dict[str, Any]]) -> tuple[Callable[[dict[str, Any]], float], dict[str, Any]]:
    means, scales = normalize(train_rows)
    weights = [0.0 for _ in FEATURES]
    bias = 0.0
    lr = 0.05
    l2 = 0.01
    for _ in range(250):
        grad_w = [0.0 for _ in FEATURES]
        grad_b = 0.0
        for row in train_rows:
            x = normalized_vector(row, means, scales)
            y = label(row)
            pred = sigmoid(sum(w * v for w, v in zip(weights, x)) + bias)
            err = pred - y
            for idx, value in enumerate(x):
                grad_w[idx] += err * value
            grad_b += err
        denom = max(len(train_rows), 1)
        for idx in range(len(weights)):
            weights[idx] -= lr * ((grad_w[idx] / denom) + l2 * weights[idx])
        bias -= lr * (grad_b / denom)

    def scorer(row: dict[str, Any]) -> float:
        x = normalized_vector(row, means, scales)
        return sigmoid(sum(w * v for w, v in zip(weights, x)) + bias)

    return scorer, {"features": list(FEATURES), "weights": weights, "bias": bias, "means": means, "scales": scales}


def train_stump_boost(train_rows: list[dict[str, Any]]) -> tuple[Callable[[dict[str, Any]], float], dict[str, Any]]:
    stumps: list[dict[str, Any]] = []
    residuals = {id(row): label(row) - 0.5 for row in train_rows}
    for _ in range(8):
        best: dict[str, Any] | None = None
        for feature in FEATURES:
            values = sorted({feature_value(row, feature) for row in train_rows})
            if not values:
                continue
            threshold = values[len(values) // 2]
            left = [residuals[id(row)] for row in train_rows if feature_value(row, feature) < threshold]
            right = [residuals[id(row)] for row in train_rows if feature_value(row, feature) >= threshold]
            if not left or not right:
                continue
            left_score = sum(left) / len(left)
            right_score = sum(right) / len(right)
            error = sum(
                (
                    residuals[id(row)]
                    - (left_score if feature_value(row, feature) < threshold else right_score)
                )
                ** 2
                for row in train_rows
            )
            if best is None or error < best["error"]:
                best = {
                    "feature": feature,
                    "threshold": threshold,
                    "left": left_score,
                    "right": right_score,
                    "error": error,
                }
        if best is None:
            break
        stumps.append(best)
        for row in train_rows:
            pred = best["left"] if feature_value(row, best["feature"]) < best["threshold"] else best["right"]
            residuals[id(row)] -= 0.2 * pred

    def scorer(row: dict[str, Any]) -> float:
        value = 0.5
        for stump in stumps:
            value += 0.2 * (
                stump["left"]
                if feature_value(row, stump["feature"]) < stump["threshold"]
                else stump["right"]
            )
        return max(0.0, min(1.0, value))

    return scorer, {"stumps": stumps, "learning_rate": 0.2}


def metrics(rows: list[dict[str, Any]], scorer: Callable[[dict[str, Any]], float], threshold: float) -> dict[str, Any]:
    selected = [row for row in rows if scorer(row) >= threshold]
    tp = sum(1 for row in selected if label(row) == 1)
    fp = sum(1 for row in selected if label(row) == 0)
    positives = sum(1 for row in rows if label(row) == 1)
    denominator_rows = [
        dict(row, selector_accept=(scorer(row) >= threshold))
        for row in rows
    ]
    literal_holdout_denominator = [
        row for row in denominator_rows if common.precision_r2_denominator(row)
    ]
    return {
        "rows": len(rows),
        "selected_count": len(selected),
        "tp_r2": tp,
        "fp_r2": fp,
        "precision_r2": tp / (tp + fp) if (tp + fp) else None,
        "recall_r2": tp / positives if positives else None,
        "coverage": len(selected) / len(rows) if rows else None,
        "precision_r2_denominator_contract": common.PRECISION_R2_DENOMINATOR_CONTRACT,
        "literal_precision_r2_holdout_denominator_count": len(literal_holdout_denominator),
    }


def select_threshold(
    validation_rows: list[dict[str, Any]],
    scorer: Callable[[dict[str, Any]], float],
    *,
    target_precision: float,
) -> tuple[float, dict[str, Any]]:
    candidates = sorted({scorer(row) for row in validation_rows}, reverse=True)
    if not candidates:
        return 1.0, metrics(validation_rows, scorer, 1.0)
    best_threshold = candidates[0]
    best_metrics = metrics(validation_rows, scorer, best_threshold)
    for threshold in candidates:
        current = metrics(validation_rows, scorer, threshold)
        precision = current["precision_r2"]
        if precision is None:
            continue
        best_precision = best_metrics["precision_r2"] or -1.0
        if (
            precision >= target_precision
            and current["selected_count"] > best_metrics["selected_count"]
        ) or (best_precision < target_precision and precision > best_precision):
            best_threshold = threshold
            best_metrics = current
    return best_threshold, best_metrics


def sample_gate(
    rows: list[dict[str, Any]],
    *,
    min_first_baseline_accepted: int,
    min_comparison_accepted: int,
    min_eligible: int,
) -> dict[str, Any]:
    accepted_resolved = sum(
        1
        for row in rows
        if row.get("decision_verdict_buy") is True and row.get("label_resolved") is True
    )
    eligible = len(rows)
    reasons = []
    if accepted_resolved < min_first_baseline_accepted:
        reasons.append("insufficient_first_baseline_accepted")
    if accepted_resolved < min_comparison_accepted:
        reasons.append("insufficient_gatekeeper_comparison_accepted")
    if eligible < min_eligible:
        reasons.append("insufficient_eligible_candidates")
    return {
        "status": "PASS" if not reasons else "insufficient_data",
        "accepted_resolved_buy_rows": accepted_resolved,
        "eligible_candidates": eligible,
        "reasons": reasons,
    }


def leakage_gate(path: Path | None) -> dict[str, Any]:
    if path is None:
        return {"status": "NO-GO", "reason": "leakage_audit_not_provided"}
    if not path.exists():
        return {"status": "NO-GO", "reason": "leakage_audit_missing", "path": str(path)}
    payload = json.loads(path.read_text(encoding="utf-8"))
    status = payload.get("status")
    return {
        "status": "PASS" if status == "PASS" else "NO-GO",
        "path": str(path),
        "audit_status": status,
        "violation_count": payload.get("violation_count"),
    }


def permutation_importance(
    rows: list[dict[str, Any]],
    scorer: Callable[[dict[str, Any]], float],
    threshold: float,
) -> list[dict[str, Any]]:
    baseline_metrics = metrics(rows, scorer, threshold)
    baseline_precision = baseline_metrics["precision_r2"]
    if len(rows) < 2:
        return []
    results = []
    for feature in FEATURES:
        shifted_values = [row.get(feature) for row in rows[1:]] + [rows[0].get(feature)]
        perturbed = []
        for idx, row in enumerate(rows):
            copy = dict(row)
            copy[feature] = shifted_values[idx]
            perturbed.append(copy)
        perturbed_metrics = metrics(perturbed, scorer, threshold)
        perturbed_precision = perturbed_metrics["precision_r2"]
        precision_drop = (
            baseline_precision - perturbed_precision
            if baseline_precision is not None and perturbed_precision is not None
            else None
        )
        results.append(
            {
                "feature": feature,
                "holdout_precision_after_permutation": perturbed_precision,
                "precision_drop": precision_drop,
                "selected_count_after_permutation": perturbed_metrics["selected_count"],
            }
        )
    results.sort(key=lambda item: (item["precision_drop"] is None, -(item["precision_drop"] or 0.0)))
    return results


def train(training_view: Path, args: argparse.Namespace) -> dict[str, Any]:
    rows = usable_rows(list(common.iter_json_objects(training_view)))
    train_rows = [row for row in rows if row.get("split") == "train"]
    validation_rows = [row for row in rows if row.get("split") == "validation"]
    holdout_rows = [row for row in rows if row.get("split") == "holdout"]
    gate = sample_gate(
        rows,
        min_first_baseline_accepted=args.min_first_baseline_accepted,
        min_comparison_accepted=args.min_comparison_accepted,
        min_eligible=args.min_eligible,
    )
    leak_gate = leakage_gate(args.leakage_audit)
    methods: dict[str, tuple[Callable[[dict[str, Any]], float], dict[str, Any]]] = {
        "rules": (rules_score, {"features": list(FEATURES), "type": "fixed_rules"}),
    }
    if train_rows:
        methods["logistic_regression"] = train_logistic(train_rows)
        methods["shallow_gradient_boosting"] = train_stump_boost(train_rows)

    reports = {}
    for name, (scorer, model) in methods.items():
        threshold, validation_metrics = select_threshold(
            validation_rows,
            scorer,
            target_precision=args.target_precision,
        )
        holdout_metrics = metrics(holdout_rows, scorer, threshold)
        preliminary_status = (
            "PASS"
            if (
                gate["status"] == "PASS"
                and leak_gate["status"] == "PASS"
                and (holdout_metrics["precision_r2"] or 0.0) >= args.target_precision
                and holdout_metrics["selected_count"] >= args.min_holdout_accepted
            )
            else "NO-GO"
        )
        shadow_emit_status = (
            "PASS"
            if preliminary_status == "PASS"
            and holdout_metrics["selected_count"] >= args.min_holdout_accepted_shadow_emit
            else "NO-GO"
        )
        reports[name] = {
            "model": model,
            "threshold_selected_on_validation": threshold,
            "validation": validation_metrics,
            "holdout": holdout_metrics,
            "permutation_importance_holdout": permutation_importance(holdout_rows, scorer, threshold),
            "promotion_gate": {
                "precision_target": args.target_precision,
                "holdout_accepted_count_min_preliminary": args.min_holdout_accepted,
                "holdout_accepted_count_min_shadow_emit": args.min_holdout_accepted_shadow_emit,
                "leakage_audit_required": True,
                "leakage_audit_status": leak_gate["status"],
                "preliminary_status": preliminary_status,
                "shadow_only_emit_status": shadow_emit_status,
                "status": shadow_emit_status,
            },
        }
    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "selector_baseline_v1",
        "training_view": str(training_view),
        "status": (
            "ok"
            if gate["status"] == "PASS" and leak_gate["status"] == "PASS"
            else "NO-GO"
        ),
        "sample_gate": gate,
        "leakage_gate": leak_gate,
        "split_counts": {
            "train": len(train_rows),
            "validation": len(validation_rows),
            "holdout": len(holdout_rows),
        },
        "methods": reports,
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--training-view", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--target-precision", type=float, default=0.70)
    parser.add_argument("--min-first-baseline-accepted", type=int, default=80)
    parser.add_argument("--min-comparison-accepted", type=int, default=150)
    parser.add_argument("--min-eligible", type=int, default=1000)
    parser.add_argument("--min-holdout-accepted", type=int, default=50)
    parser.add_argument("--min-holdout-accepted-shadow-emit", type=int, default=100)
    parser.add_argument("--leakage-audit", type=Path)
    parser.add_argument("--json", action="store_true")
    return parser


def run(args: argparse.Namespace) -> dict[str, Any]:
    report = train(args.training_view, args)
    common.write_json(args.output, report)
    return report


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = run(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0 if report["status"] == "ok" else 2


if __name__ == "__main__":
    raise SystemExit(main())
