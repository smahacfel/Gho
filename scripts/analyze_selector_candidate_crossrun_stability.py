#!/usr/bin/env python3
"""Analyze cross-run stability for a selector shadow score candidate.

P3M is offline-only. It does not train a model, change Gatekeeper, change
runtime scoring, or promote a candidate. It compares a train/source selector
scope against an independent validation selector scope and classifies whether
the candidate edge is stable enough to justify more shadow-only study.
"""

from __future__ import annotations

import argparse
import csv
import json
import math
from collections import Counter
from pathlib import Path
from typing import Any

import audit_selector_shadow_score_parity as parity
import audit_selector_shadow_score_topk_drift as topk_drift
import build_selector_r2only_baseline_report as baseline
import build_selector_r2only_model_candidate as p3g
import selector_pipeline_common as common


ARTIFACT = "model_candidate_crossrun_stability_v1"
MD_ARTIFACT = "MODEL_CANDIDATE_CROSSRUN_STABILITY"
FEATURE_DRIFT_ARTIFACT = "model_candidate_feature_drift_v1.csv"
TOPK_ARTIFACT = "model_candidate_topk_comparison_v1.csv"
DEFAULT_RUST_SOURCE = "ghost-brain/src/oracle/decision_logger.rs"
TOP_K = (10, 25, 50)
TARGET_NET_PCT = 40.0
STOP_NET_PCT = 40.0

FLOW_FEATURES = [
    "net_quote_in_15s",
    "net_quote_in_30s",
    "trade_rate",
    "unique_buyers",
    "sell_share",
    "top1_wallet_share",
    "buyer_hhi",
]
GK_CORE_FEATURES = [
    "gk_bonding_progress_pct",
    "gk_current_market_cap_sol",
    "gk_price_change_ratio",
]
CONCENTRATION_FEATURES = [
    "gk_hhi",
    "gk_top3_volume_pct",
]
RISK_DEV_FEATURES = [
    "gk_dev_has_sold",
    "gk_dev_volume_ratio",
    "gk_dev_sold_within_3s",
    "gk_dev_sold_within_5s",
]
FEATURE_GROUPS = {
    "flow": FLOW_FEATURES,
    "gk_core": GK_CORE_FEATURES,
    "concentration": CONCENTRATION_FEATURES,
    "risk_dev": RISK_DEV_FEATURES,
}


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="/root/Gho")
    parser.add_argument("--train-scope", required=True)
    parser.add_argument("--validation-scope", required=True)
    parser.add_argument("--candidate", default="combined:simple_feature_score_v1")
    parser.add_argument("--rust-source", default=DEFAULT_RUST_SOURCE)
    parser.add_argument("--min-top25-hit-rate", type=float, default=0.60)
    parser.add_argument("--min-top50-lift-vs-base", type=float, default=0.05)
    parser.add_argument("--max-high-drift-feature-rate", type=float, default=0.25)
    parser.add_argument("--high-ks-distance", type=float, default=0.35)
    parser.add_argument("--output", default=None)
    parser.add_argument("--md-output", default=None)
    parser.add_argument("--feature-drift-output", default=None)
    parser.add_argument("--topk-output", default=None)
    parser.add_argument("--json", action="store_true")
    return parser


def read_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as fh:
        payload = json.load(fh)
    if not isinstance(payload, dict):
        raise ValueError(f"expected JSON object in {path}")
    return payload


def training_view_path(root: Path, scope: str) -> Path:
    return root / "datasets" / "selector" / scope / "selector_training_view_v1.jsonl"


def report_dir(root: Path, validation_scope: str) -> Path:
    return root / "reports" / "selector" / validation_scope


def denominator_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [row for row in rows if baseline.r2only_denominator(row)]


def label_positive(row: dict[str, Any]) -> bool:
    return row.get("r2_label") == "positive"


def numeric_feature(row: dict[str, Any], feature: str) -> float | None:
    value = p3g.feature_value(row, feature)
    if value is None or isinstance(value, bool):
        return None
    if isinstance(value, (int, float)):
        out = float(value)
        return out if math.isfinite(out) else None
    return None


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


def mean(values: list[float]) -> float | None:
    return sum(values) / len(values) if values else None


def stats(values: list[float], denominator: int) -> dict[str, Any]:
    return {
        "present_rows": len(values),
        "missing_rows": max(denominator - len(values), 0),
        "missing_rate": (max(denominator - len(values), 0) / denominator if denominator else None),
        "mean": mean(values),
        "median": percentile(values, 0.50),
        "p10": percentile(values, 0.10),
        "p90": percentile(values, 0.90),
    }


def ks_distance(left: list[float], right: list[float]) -> float | None:
    if not left or not right:
        return None
    a = sorted(left)
    b = sorted(right)
    i = 0
    j = 0
    n = len(a)
    m = len(b)
    out = 0.0
    while i < n or j < m:
        if j >= m or (i < n and a[i] <= b[j]):
            value = a[i]
        else:
            value = b[j]
        while i < n and a[i] <= value:
            i += 1
        while j < m and b[j] <= value:
            j += 1
        out = max(out, abs(i / n - j / m))
    return out


def spec_subset(specs: list[dict[str, Any]], mode: str) -> list[dict[str, Any]]:
    if mode == "combined_full":
        return specs
    if mode == "minus_flow":
        excluded = set(FLOW_FEATURES)
        return [spec for spec in specs if str(spec["name"]) not in excluded]
    if mode == "minus_gk_curve_market":
        excluded = set(GK_CORE_FEATURES)
        return [spec for spec in specs if str(spec["name"]) not in excluded]
    if mode == "minus_concentration":
        excluded = set(CONCENTRATION_FEATURES)
        return [spec for spec in specs if str(spec["name"]) not in excluded]
    if mode == "minus_dev_risk":
        excluded = set(RISK_DEV_FEATURES)
        return [spec for spec in specs if str(spec["name"]) not in excluded]
    if mode == "only_gk_core":
        included = set(GK_CORE_FEATURES)
        return [spec for spec in specs if str(spec["name"]) in included]
    if mode == "only_flow":
        included = set(FLOW_FEATURES)
        return [spec for spec in specs if str(spec["name"]) in included]
    raise ValueError(f"unknown score mode {mode}")


def score_row(row: dict[str, Any], specs: list[dict[str, Any]]) -> float | None:
    if not specs:
        return None
    score, _availability = topk_drift.score_training_row(row, specs)
    return score


def score_map(rows: list[dict[str, Any]], specs: list[dict[str, Any]]) -> dict[str, float]:
    out: dict[str, float] = {}
    for row in rows:
        candidate_id = common.str_or_none(row.get("candidate_id"))
        if not candidate_id:
            continue
        score = score_row(row, specs)
        if score is not None:
            out[candidate_id] = score
    return out


def ordered(rows: list[dict[str, Any]], scores: dict[str, float]) -> list[dict[str, Any]]:
    return sorted(
        [row for row in rows if common.str_or_none(row.get("candidate_id")) in scores],
        key=lambda row: (
            -scores[common.str_or_none(row.get("candidate_id")) or ""],
            common.int_or_none(row.get("birth_ts_ms")) or 0,
            common.str_or_none(row.get("candidate_id")) or "",
        ),
    )


def selected_metric(rows: list[dict[str, Any]], selected: list[dict[str, Any]]) -> dict[str, Any]:
    positives = sum(1 for row in rows if label_positive(row))
    selected_positive = sum(1 for row in selected if label_positive(row))
    selected_negative = len(selected) - selected_positive
    precision = selected_positive / len(selected) if selected else None
    base_rate = positives / len(rows) if rows else None
    return {
        "denominator_rows": len(rows),
        "selected_count": len(selected),
        "positive_count": selected_positive,
        "negative_count": selected_negative,
        "precision": precision,
        "resolved_label_coverage": len(selected) / len(rows) if rows else None,
        "base_positive_rate": base_rate,
        "lift_vs_base_rate_pp": (
            precision - base_rate
            if isinstance(precision, float) and isinstance(base_rate, float)
            else None
        ),
        "ev_proxy_pct": (
            precision * TARGET_NET_PCT - (1.0 - precision) * STOP_NET_PCT
            if isinstance(precision, float)
            else None
        ),
        "recall": selected_positive / positives if positives else None,
    }


def topk_metrics(rows: list[dict[str, Any]], scores: dict[str, float]) -> dict[str, Any]:
    ordered_rows = ordered(rows, scores)
    return {
        f"top{k}": selected_metric(rows, ordered_rows[: min(k, len(ordered_rows))])
        for k in TOP_K
    }


def threshold_metrics(rows: list[dict[str, Any]], scores: dict[str, float], thresholds: dict[str, float]) -> dict[str, Any]:
    out: dict[str, Any] = {}
    for name, threshold in sorted(thresholds.items()):
        selected = [row for row in rows if scores.get(common.str_or_none(row.get("candidate_id")) or "", -math.inf) >= threshold]
        out[name] = selected_metric(rows, ordered(selected, scores))
    return out


def feature_drift_rows(train_rows: list[dict[str, Any]], validation_rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    features = FLOW_FEATURES + GK_CORE_FEATURES + CONCENTRATION_FEATURES + RISK_DEV_FEATURES
    group_by_feature = {
        feature: group
        for group, group_features in FEATURE_GROUPS.items()
        for feature in group_features
    }
    for feature in features:
        train_values = [value for row in train_rows if (value := numeric_feature(row, feature)) is not None]
        validation_values = [value for row in validation_rows if (value := numeric_feature(row, feature)) is not None]
        train_positive = [value for row in train_rows if label_positive(row) and (value := numeric_feature(row, feature)) is not None]
        train_negative = [value for row in train_rows if not label_positive(row) and (value := numeric_feature(row, feature)) is not None]
        validation_positive = [value for row in validation_rows if label_positive(row) and (value := numeric_feature(row, feature)) is not None]
        validation_negative = [value for row in validation_rows if not label_positive(row) and (value := numeric_feature(row, feature)) is not None]
        train_stats = stats(train_values, len(train_rows))
        validation_stats = stats(validation_values, len(validation_rows))
        rows.append(
            {
                "feature": feature,
                "feature_group": group_by_feature.get(feature),
                "train_present_rows": train_stats["present_rows"],
                "train_missing_rate": train_stats["missing_rate"],
                "train_mean": train_stats["mean"],
                "train_median": train_stats["median"],
                "train_p10": train_stats["p10"],
                "train_p90": train_stats["p90"],
                "validation_present_rows": validation_stats["present_rows"],
                "validation_missing_rate": validation_stats["missing_rate"],
                "validation_mean": validation_stats["mean"],
                "validation_median": validation_stats["median"],
                "validation_p10": validation_stats["p10"],
                "validation_p90": validation_stats["p90"],
                "missing_rate_delta": (
                    validation_stats["missing_rate"] - train_stats["missing_rate"]
                    if isinstance(validation_stats["missing_rate"], float)
                    and isinstance(train_stats["missing_rate"], float)
                    else None
                ),
                "ks_distance": ks_distance(train_values, validation_values),
                "train_positive_negative_mean_delta": (
                    mean(train_positive) - mean(train_negative)
                    if mean(train_positive) is not None and mean(train_negative) is not None
                    else None
                ),
                "validation_positive_negative_mean_delta": (
                    mean(validation_positive) - mean(validation_negative)
                    if mean(validation_positive) is not None and mean(validation_negative) is not None
                    else None
                ),
            }
        )
    return rows


def top25_composition(rows: list[dict[str, Any]], scores: dict[str, float]) -> dict[str, Any]:
    selected = ordered(rows, scores)[:25]
    medians = {}
    for feature in FLOW_FEATURES + GK_CORE_FEATURES + CONCENTRATION_FEATURES + RISK_DEV_FEATURES:
        values = [value for row in selected if (value := numeric_feature(row, feature)) is not None]
        medians[feature] = percentile(values, 0.50)
    concentration_available = sum(
        1
        for row in selected
        if numeric_feature(row, "gk_hhi") is not None and numeric_feature(row, "gk_top3_volume_pct") is not None
    )
    return {
        "selected_count": len(selected),
        "positive_count": sum(1 for row in selected if label_positive(row)),
        "resolved_count": len(selected),
        "concentration_available_count": concentration_available,
        "concentration_available_rate": concentration_available / len(selected) if selected else None,
        "feature_medians": medians,
        "candidate_sample": [
            {
                "candidate_id": row.get("candidate_id"),
                "base_mint": row.get("base_mint"),
                "score": scores.get(common.str_or_none(row.get("candidate_id")) or ""),
                "r2_label": row.get("r2_label"),
            }
            for row in selected[:10]
        ],
    }


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fieldnames = sorted({key for row in rows for key in row})
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames)
        writer.writeheader()
        for row in rows:
            writer.writerow({field: row.get(field) for field in fieldnames})


def write_markdown(path: Path, report: dict[str, Any]) -> None:
    lines = [
        "# Model Candidate Cross-Run Stability",
        "",
        f"Status: {report['status']}",
        f"Candidate: `{report['candidate']}`",
        f"Train scope: `{report['train_scope']}`",
        f"Validation scope: `{report['validation_scope']}`",
        "",
        "## Decision",
        "",
        f"- business_decision: {report['business_decision']}",
        f"- fail_reasons: {', '.join(report['acceptance']['fail_reasons']) or 'none'}",
        "",
        "## Performance",
        "",
        "| run | score_mode | base | top10 | top25 | top50 |",
        "|---|---|---:|---:|---:|---:|",
    ]
    for run_name in ("train", "validation"):
        for mode, payload in report["score_modes"][run_name].items():
            lines.append(
                "| {run} | {mode} | {base} | {top10} | {top25} | {top50} |".format(
                    run=run_name,
                    mode=mode,
                    base=f"{payload['base_positive_rate']:.4f}" if payload.get("base_positive_rate") is not None else "n/a",
                    top10=f"{payload['topk']['top10']['precision']:.4f}" if payload["topk"]["top10"].get("precision") is not None else "n/a",
                    top25=f"{payload['topk']['top25']['precision']:.4f}" if payload["topk"]["top25"].get("precision") is not None else "n/a",
                    top50=f"{payload['topk']['top50']['precision']:.4f}" if payload["topk"]["top50"].get("precision") is not None else "n/a",
                )
            )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.root)
    specs, thresholds = parity.parse_runtime_spec(root / args.rust_source)
    train_all = list(common.iter_json_objects(training_view_path(root, args.train_scope)))
    validation_all = list(common.iter_json_objects(training_view_path(root, args.validation_scope)))
    train_rows = denominator_rows(train_all)
    validation_rows = denominator_rows(validation_all)

    modes = [
        "combined_full",
        "minus_flow",
        "minus_gk_curve_market",
        "minus_concentration",
        "minus_dev_risk",
        "only_gk_core",
        "only_flow",
    ]
    score_modes: dict[str, dict[str, Any]] = {"train": {}, "validation": {}}
    topk_csv_rows: list[dict[str, Any]] = []
    mode_scores: dict[str, dict[str, dict[str, float]]] = {"train": {}, "validation": {}}
    for mode in modes:
        subset = spec_subset(specs, mode)
        for run_name, rows in (("train", train_rows), ("validation", validation_rows)):
            scores = score_map(rows, subset)
            mode_scores[run_name][mode] = scores
            topk_payload = topk_metrics(rows, scores)
            payload = {
                "feature_count": len(subset),
                "denominator_rows": len(rows),
                "positive_rows": sum(1 for row in rows if label_positive(row)),
                "negative_rows": sum(1 for row in rows if not label_positive(row)),
                "base_positive_rate": sum(1 for row in rows if label_positive(row)) / len(rows) if rows else None,
                "topk": topk_payload,
                "thresholds": threshold_metrics(rows, scores, thresholds) if mode == "combined_full" else {},
            }
            score_modes[run_name][mode] = payload
            for top_label, metric in topk_payload.items():
                topk_csv_rows.append(
                    {
                        "run": run_name,
                        "score_mode": mode,
                        "topk": top_label,
                        **metric,
                    }
                )

    drift_rows = feature_drift_rows(train_rows, validation_rows)
    high_drift_rows = [
        row for row in drift_rows
        if isinstance(row.get("ks_distance"), (int, float)) and row["ks_distance"] >= args.high_ks_distance
    ]
    high_drift_feature_rate = len(high_drift_rows) / len(drift_rows) if drift_rows else None

    validation_combined = score_modes["validation"]["combined_full"]
    validation_base = validation_combined.get("base_positive_rate")
    validation_top25 = validation_combined["topk"]["top25"]
    validation_top50 = validation_combined["topk"]["top50"]
    validation_top25_rate = validation_top25.get("precision")
    validation_top50_rate = validation_top50.get("precision")
    validation_top50_lift = (
        validation_top50_rate - validation_base
        if isinstance(validation_top50_rate, float) and isinstance(validation_base, float)
        else None
    )

    fail_reasons: list[str] = []
    if not isinstance(validation_top25_rate, float) or validation_top25_rate < args.min_top25_hit_rate:
        fail_reasons.append("validation_top25_below_minimum_hit_rate")
    if not isinstance(validation_top50_lift, float) or validation_top50_lift < args.min_top50_lift_vs_base:
        fail_reasons.append("validation_top50_lift_below_minimum")
    if isinstance(high_drift_feature_rate, float) and high_drift_feature_rate > args.max_high_drift_feature_rate:
        fail_reasons.append("feature_distribution_drift_high")

    status = (
        "P3M_PASS_STABLE_CANDIDATE"
        if not fail_reasons
        else "P3M_NO_GO_CANDIDATE_NOT_STABLE"
    )
    report = {
        "artifact": ARTIFACT,
        "status": status,
        "candidate": args.candidate,
        "train_scope": args.train_scope,
        "validation_scope": args.validation_scope,
        "claim_boundaries": {
            "diagnostic_only": True,
            "offline_only": True,
            "trained_model": False,
            "changed_gatekeeper": False,
            "changed_runtime_score": False,
            "changed_execution": False,
            "production_promotion_allowed": False,
        },
        "business_decision": (
            "CANDIDATE_STABLE_ENOUGH_FOR_SHADOW_ONLY_PLANNING"
            if status == "P3M_PASS_STABLE_CANDIDATE"
            else "DO_NOT_FORWARD_SHADOW_BURN_IN"
        ),
        "acceptance": {
            "min_top25_hit_rate": args.min_top25_hit_rate,
            "min_top50_lift_vs_base": args.min_top50_lift_vs_base,
            "max_high_drift_feature_rate": args.max_high_drift_feature_rate,
            "high_ks_distance": args.high_ks_distance,
            "fail_reasons": fail_reasons,
        },
        "inputs": {
            "train_training_view": str(training_view_path(root, args.train_scope)),
            "validation_training_view": str(training_view_path(root, args.validation_scope)),
            "rust_source": str(root / args.rust_source),
        },
        "score_modes": score_modes,
        "feature_drift_summary": {
            "features_checked": len(drift_rows),
            "high_drift_features": len(high_drift_rows),
            "high_drift_feature_rate": high_drift_feature_rate,
            "high_drift_feature_names": [row["feature"] for row in high_drift_rows],
        },
        "top25_composition": {
            "train": top25_composition(train_rows, mode_scores["train"]["combined_full"]),
            "validation": top25_composition(validation_rows, mode_scores["validation"]["combined_full"]),
        },
        "threshold_stability": {
            "train": score_modes["train"]["combined_full"]["thresholds"],
            "validation": score_modes["validation"]["combined_full"]["thresholds"],
        },
        "outputs": {},
    }

    out_dir = report_dir(root, args.validation_scope)
    output = Path(args.output) if args.output else out_dir / f"{ARTIFACT}.json"
    md_output = Path(args.md_output) if args.md_output else out_dir / f"{MD_ARTIFACT}.md"
    drift_output = Path(args.feature_drift_output) if args.feature_drift_output else out_dir / FEATURE_DRIFT_ARTIFACT
    topk_output = Path(args.topk_output) if args.topk_output else out_dir / TOPK_ARTIFACT
    report["outputs"] = {
        ARTIFACT: str(output),
        MD_ARTIFACT: str(md_output),
        FEATURE_DRIFT_ARTIFACT: str(drift_output),
        TOPK_ARTIFACT: str(topk_output),
    }
    write_json(output, report)
    write_markdown(md_output, report)
    write_csv(drift_output, drift_rows)
    write_csv(topk_output, topk_csv_rows)
    return report


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = build_report(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
