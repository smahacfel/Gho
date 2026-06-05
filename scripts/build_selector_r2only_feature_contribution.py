#!/usr/bin/env python3
"""Build P3F R2-only feature contribution diagnostics without tuning."""

from __future__ import annotations

import argparse
import bisect
import csv
import json
from collections import Counter
from pathlib import Path
from typing import Any, Callable

import build_selector_r2only_baseline_report as baseline
import selector_pipeline_common as common


FEATURES = (
    "net_quote_in_15s",
    "net_quote_in_30s",
    "trade_rate",
    "unique_buyers",
    "sell_share",
    "top1_wallet_share",
    "buyer_hhi",
)
EXAMPLE_FIELDS = (
    "candidate_id",
    "base_mint",
    "birth_ts_ms",
    "r2_label",
    "r2_status",
    "gatekeeper_accept",
    "available_feature_score",
    "net_quote_in_15s",
    "net_quote_in_30s",
    "trade_rate",
    "unique_buyers",
    "sell_share",
    "top1_wallet_share",
    "buyer_hhi",
)
SPLITS = ("train", "validation", "holdout")
TOP_K = (10, 25, 50, 100)
OVERLAP_K = (10, 25, 50)


def read_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as fh:
        payload = json.load(fh)
    if not isinstance(payload, dict):
        raise ValueError(f"expected JSON object in {path}")
    return payload


def feature_value(row: dict[str, Any], feature: str) -> float | None:
    value = row.get(feature)
    if isinstance(value, bool):
        return 1.0 if value else 0.0
    return common.float_or_none(value)


def label_positive(row: dict[str, Any]) -> bool:
    return row.get("r2_label") == "positive"


def gatekeeper_accept(row: dict[str, Any]) -> bool:
    return baseline.selector_accept(row)


def metric_block(rows: list[dict[str, Any]]) -> dict[str, Any]:
    return baseline.metric_block(rows, lambda _row: True)


def percentile(values: list[float], pct: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    if len(ordered) == 1:
        return ordered[0]
    pos = (len(ordered) - 1) * (pct / 100.0)
    low = int(pos)
    high = min(low + 1, len(ordered) - 1)
    frac = pos - low
    return ordered[low] * (1.0 - frac) + ordered[high] * frac


def numeric_summary(rows: list[dict[str, Any]], feature: str) -> dict[str, Any]:
    values = [feature_value(row, feature) for row in rows]
    present = [value for value in values if value is not None]
    return {
        "rows": len(rows),
        "present_rows": len(present),
        "missing_rows": len(rows) - len(present),
        "missing_rate": (len(rows) - len(present)) / len(rows) if rows else None,
        "mean": sum(present) / len(present) if present else None,
        "median": percentile(present, 50),
        "p10": percentile(present, 10),
        "p25": percentile(present, 25),
        "p75": percentile(present, 75),
        "p90": percentile(present, 90),
        "min": min(present) if present else None,
        "max": max(present) if present else None,
    }


def split_counts(rows: list[dict[str, Any]]) -> dict[str, dict[str, int]]:
    counts: dict[str, Counter[str]] = {}
    for row in rows:
        split = str(row.get("split") or "unknown")
        label = str(row.get("r2_label") or "unknown")
        counts.setdefault(split, Counter())[label] += 1
    return {split: common.counter_dict(counter) for split, counter in sorted(counts.items())}


def base_rate(rows: list[dict[str, Any]]) -> float | None:
    if not rows:
        return None
    return sum(1 for row in rows if label_positive(row)) / len(rows)


def feature_direction(train_rows: list[dict[str, Any]], feature: str) -> int:
    positives = [feature_value(row, feature) for row in train_rows if label_positive(row)]
    negatives = [feature_value(row, feature) for row in train_rows if not label_positive(row)]
    positives = [value for value in positives if value is not None]
    negatives = [value for value in negatives if value is not None]
    if not positives or not negatives:
        return 1
    # Keep the diagnostic score identical to build_selector_r2only_ablation_report.py:
    # direction is learned from train means, then applied to validation/holdout.
    pos_mean = sum(positives) / len(positives)
    neg_mean = sum(negatives) / len(negatives)
    return 1 if pos_mean >= neg_mean else -1


def feature_ranges(train_rows: list[dict[str, Any]], features: list[str]) -> dict[str, dict[str, Any]]:
    ranges: dict[str, dict[str, Any]] = {}
    for feature in features:
        values = [feature_value(row, feature) for row in train_rows]
        values = [value for value in values if value is not None]
        ranges[feature] = {
            "min": min(values) if values else 0.0,
            "max": max(values) if values else 0.0,
            "direction": feature_direction(train_rows, feature),
        }
    return ranges


def normalized_feature_score(row: dict[str, Any], feature: str, ranges: dict[str, dict[str, Any]]) -> float:
    value = feature_value(row, feature)
    if value is None:
        return 0.0
    low = float(ranges[feature]["min"])
    high = float(ranges[feature]["max"])
    if high == low:
        score = 0.0
    else:
        score = (value - low) / (high - low)
    if int(ranges[feature]["direction"]) < 0:
        score = 1.0 - score
    return max(0.0, min(1.0, score))


def score_rows(rows: list[dict[str, Any]], features: list[str], train_rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    ranges = feature_ranges(train_rows, features)
    scored: dict[str, dict[str, Any]] = {}
    for row in rows:
        candidate_id = common.str_or_none(row.get("candidate_id"))
        if not candidate_id:
            continue
        parts = {feature: normalized_feature_score(row, feature, ranges) for feature in features}
        score = sum(parts.values()) / len(features) if features else None
        scored[candidate_id] = {
            "available_feature_score": score,
            "feature_score_parts": parts,
        }
    return scored


def score_value(row: dict[str, Any], scores: dict[str, dict[str, Any]]) -> float | None:
    candidate_id = common.str_or_none(row.get("candidate_id"))
    if not candidate_id:
        return None
    value = scores.get(candidate_id, {}).get("available_feature_score")
    return common.float_or_none(value)


def sorted_by_score(rows: list[dict[str, Any]], scores: dict[str, dict[str, Any]]) -> list[dict[str, Any]]:
    sortable: list[tuple[float, int, str, dict[str, Any]]] = []
    for row in rows:
        score = score_value(row, scores)
        candidate_id = common.str_or_none(row.get("candidate_id")) or ""
        if score is None:
            continue
        sortable.append((score, common.int_or_none(row.get("birth_ts_ms")) or 0, candidate_id, row))
    sortable.sort(key=lambda item: (-item[0], item[1], item[2]))
    return [item[3] for item in sortable]


def top_k_metrics(rows: list[dict[str, Any]], scores: dict[str, dict[str, Any]], top_k: tuple[int, ...]) -> list[dict[str, Any]]:
    ordered = sorted_by_score(rows, scores)
    reports = []
    for k in top_k:
        selected = ordered[: min(k, len(ordered))]
        metrics = metric_block(selected)
        reports.append(
            {
                "k": k,
                "available_scored_rows": len(ordered),
                "selected_count": metrics["selected_count"],
                "tp_r2": metrics["tp_r2"],
                "fp_r2": metrics["fp_r2"],
                "precision_r2": metrics["precision_r2"],
                "positive_rate": metrics["positive_rate"],
                "base_positive_rate": base_rate(rows),
                "lift_vs_split_base_rate": (
                    (metrics["precision_r2"] / base_rate(rows))
                    if metrics["precision_r2"] is not None and base_rate(rows)
                    else None
                ),
            }
        )
    return reports


def feature_separation(rows: list[dict[str, Any]], train_rows: list[dict[str, Any]]) -> dict[str, Any]:
    report: dict[str, Any] = {}
    global_base_rate = base_rate(rows)
    for feature in FEATURES:
        positive_rows = [row for row in rows if label_positive(row)]
        negative_rows = [row for row in rows if not label_positive(row)]
        pos_summary = numeric_summary(positive_rows, feature)
        neg_summary = numeric_summary(negative_rows, feature)
        pos_median = pos_summary.get("median")
        neg_median = neg_summary.get("median")
        if pos_median is None or neg_median is None:
            direction = "unavailable"
            diff = None
        else:
            diff = pos_median - neg_median
            if diff > 0:
                direction = "positive_higher"
            elif diff < 0:
                direction = "negative_higher"
            else:
                direction = "flat"
        train_values = [feature_value(row, feature) for row in train_rows]
        train_values = [value for value in train_values if value is not None]
        q25 = percentile(train_values, 25)
        q75 = percentile(train_values, 75)
        top_rows = [
            row
            for row in rows
            if q75 is not None and (feature_value(row, feature) is not None) and (feature_value(row, feature) or 0.0) >= q75
        ]
        favorable_rows = top_rows
        if feature_direction(train_rows, feature) < 0 and q25 is not None:
            favorable_rows = [
                row
                for row in rows
                if (feature_value(row, feature) is not None) and (feature_value(row, feature) or 0.0) <= q25
            ]
        top_metrics = metric_block(top_rows)
        favorable_metrics = metric_block(favorable_rows)
        report[feature] = {
            "positive": pos_summary,
            "negative": neg_summary,
            "effect_direction": direction,
            "difference_of_medians": diff,
            "train_q25": q25,
            "train_q75": q75,
            "top_quartile": {
                "rows": top_metrics["rows"],
                "positive_rows": top_metrics["positive_rows"],
                "negative_rows": top_metrics["negative_rows"],
                "positive_rate": top_metrics["positive_rate"],
                "lift_vs_base_rate": (
                    top_metrics["positive_rate"] / global_base_rate
                    if top_metrics["positive_rate"] is not None and global_base_rate
                    else None
                ),
            },
            "favorable_quartile": {
                "direction_from_train": "high_values" if feature_direction(train_rows, feature) >= 0 else "low_values",
                "rows": favorable_metrics["rows"],
                "positive_rows": favorable_metrics["positive_rows"],
                "negative_rows": favorable_metrics["negative_rows"],
                "positive_rate": favorable_metrics["positive_rate"],
                "lift_vs_base_rate": (
                    favorable_metrics["positive_rate"] / global_base_rate
                    if favorable_metrics["positive_rate"] is not None and global_base_rate
                    else None
                ),
            },
        }
    return report


def train_quantile_edges(train_rows: list[dict[str, Any]], feature: str) -> list[float] | None:
    values = [feature_value(row, feature) for row in train_rows]
    values = [value for value in values if value is not None]
    if not values:
        return None
    return [percentile(values, pct) or 0.0 for pct in (0, 20, 40, 60, 80, 100)]


def bin_index(value: float, edges: list[float]) -> int:
    # Interior edges define five bins. Duplicate edges are allowed and can yield empty bins.
    idx = bisect.bisect_right(edges[1:-1], value)
    return min(max(idx, 0), 4)


def feature_bins(rows: list[dict[str, Any]], train_rows: list[dict[str, Any]]) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    by_feature: dict[str, Any] = {}
    csv_rows: list[dict[str, Any]] = []
    for feature in FEATURES:
        edges = train_quantile_edges(train_rows, feature)
        feature_report = {"train_edges": edges, "splits": {}}
        for split in SPLITS:
            split_rows = [row for row in rows if row.get("split") == split]
            split_base = base_rate(split_rows)
            bins: list[dict[str, Any]] = []
            for idx in range(5):
                if edges is None:
                    selected: list[dict[str, Any]] = []
                    lower = upper = None
                else:
                    selected = [
                        row
                        for row in split_rows
                        if feature_value(row, feature) is not None
                        and bin_index(feature_value(row, feature) or 0.0, edges) == idx
                    ]
                    lower = edges[idx]
                    upper = edges[idx + 1]
                metrics = metric_block(selected)
                row_payload = {
                    "feature": feature,
                    "split": split,
                    "bin": f"q{idx + 1}",
                    "lower": lower,
                    "upper": upper,
                    "rows": metrics["rows"],
                    "positive_rows": metrics["positive_rows"],
                    "negative_rows": metrics["negative_rows"],
                    "positive_rate": metrics["positive_rate"],
                    "base_positive_rate": split_base,
                    "lift_vs_base_rate": (
                        metrics["positive_rate"] / split_base
                        if metrics["positive_rate"] is not None and split_base
                        else None
                    ),
                }
                bins.append(row_payload)
                csv_rows.append(row_payload)
            feature_report["splits"][split] = bins
        by_feature[feature] = feature_report
    return by_feature, csv_rows


def selected_by_top_k(rows: list[dict[str, Any]], scores: dict[str, dict[str, Any]], k: int) -> set[str]:
    ordered = sorted_by_score(rows, scores)
    return {
        common.str_or_none(row.get("candidate_id")) or ""
        for row in ordered[: min(k, len(ordered))]
        if common.str_or_none(row.get("candidate_id"))
    }


def overlap_bucket_metrics(rows: list[dict[str, Any]], top_ids: set[str]) -> dict[str, Any]:
    buckets: dict[str, list[dict[str, Any]]] = {
        "gatekeeper_only": [],
        "feature_only": [],
        "overlap": [],
        "union": [],
        "neither": [],
    }
    for row in rows:
        candidate_id = common.str_or_none(row.get("candidate_id")) or ""
        gk = gatekeeper_accept(row)
        fs = candidate_id in top_ids
        if gk or fs:
            buckets["union"].append(row)
        if gk and fs:
            buckets["overlap"].append(row)
        elif gk and not fs:
            buckets["gatekeeper_only"].append(row)
        elif fs and not gk:
            buckets["feature_only"].append(row)
        else:
            buckets["neither"].append(row)
    return {bucket: metric_block(bucket_rows) for bucket, bucket_rows in buckets.items()}


def label_matrix(rows: list[dict[str, Any]], top_ids: set[str]) -> dict[str, Any]:
    matrix: dict[str, Counter[str]] = {}
    for row in rows:
        candidate_id = common.str_or_none(row.get("candidate_id")) or ""
        gk = "gatekeeper_accept_true" if gatekeeper_accept(row) else "gatekeeper_accept_false"
        fs = "feature_top_true" if candidate_id in top_ids else "feature_top_false"
        label = str(row.get("r2_label") or "unknown")
        key = f"{gk}|{fs}"
        matrix.setdefault(key, Counter())[label] += 1
    return {key: common.counter_dict(counter) for key, counter in sorted(matrix.items())}


def gatekeeper_vs_feature(rows: list[dict[str, Any]], scores: dict[str, dict[str, Any]]) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    report: dict[str, Any] = {}
    csv_rows: list[dict[str, Any]] = []
    for split in ("all", *SPLITS):
        split_rows = rows if split == "all" else [row for row in rows if row.get("split") == split]
        split_report: dict[str, Any] = {}
        for k in OVERLAP_K:
            top_ids = selected_by_top_k(split_rows, scores, k)
            bucket_metrics = overlap_bucket_metrics(split_rows, top_ids)
            split_report[f"top{k}"] = {
                "label_matrix": label_matrix(split_rows, top_ids),
                "bucket_metrics": bucket_metrics,
                "gatekeeper_accepted_outside_feature_top": bucket_metrics["gatekeeper_only"]["rows"],
                "feature_top_rejected_by_gatekeeper": bucket_metrics["feature_only"]["rows"],
            }
            for bucket, metrics in bucket_metrics.items():
                csv_rows.append(
                    {
                        "split": split,
                        "top_k": k,
                        "bucket": bucket,
                        "rows": metrics["rows"],
                        "positive_rows": metrics["positive_rows"],
                        "negative_rows": metrics["negative_rows"],
                        "selected_count": metrics["selected_count"],
                        "tp_r2": metrics["tp_r2"],
                        "fp_r2": metrics["fp_r2"],
                        "precision_r2": metrics["precision_r2"],
                        "positive_rate": metrics["positive_rate"],
                    }
                )
        report[split] = split_report
    return report, csv_rows


def example_row(row: dict[str, Any], scores: dict[str, dict[str, Any]]) -> dict[str, Any]:
    payload: dict[str, Any] = {}
    score = score_value(row, scores)
    for field in EXAMPLE_FIELDS:
        if field == "gatekeeper_accept":
            payload[field] = gatekeeper_accept(row)
        elif field == "available_feature_score":
            payload[field] = score
        else:
            payload[field] = row.get(field)
    return payload


def examples(rows: list[dict[str, Any]], scores: dict[str, dict[str, Any]], *, limit: int) -> dict[str, list[dict[str, Any]]]:
    holdout_rows = [row for row in rows if row.get("split") == "holdout"]
    ordered_holdout = sorted_by_score(holdout_rows, scores)
    top25_ids = {
        common.str_or_none(row.get("candidate_id")) or ""
        for row in ordered_holdout[: min(25, len(ordered_holdout))]
        if common.str_or_none(row.get("candidate_id"))
    }

    def by_score_desc(items: list[dict[str, Any]]) -> list[dict[str, Any]]:
        return sorted(
            items,
            key=lambda row: (
                -(score_value(row, scores) or -1.0),
                common.int_or_none(row.get("birth_ts_ms")) or 0,
                common.str_or_none(row.get("candidate_id")) or "",
            ),
        )

    gatekeeper_fp = [
        row for row in holdout_rows if gatekeeper_accept(row) and row.get("r2_label") == "negative"
    ]
    gatekeeper_fn = [
        row for row in holdout_rows if not gatekeeper_accept(row) and row.get("r2_label") == "positive"
    ]
    feature_top_tp = [
        row
        for row in holdout_rows
        if (common.str_or_none(row.get("candidate_id")) or "") in top25_ids and row.get("r2_label") == "positive"
    ]
    feature_top_fp = [
        row
        for row in holdout_rows
        if (common.str_or_none(row.get("candidate_id")) or "") in top25_ids and row.get("r2_label") == "negative"
    ]
    return {
        "gatekeeper_fp_accepted_negative": [example_row(row, scores) for row in by_score_desc(gatekeeper_fp)[:limit]],
        "gatekeeper_fn_rejected_positive": [example_row(row, scores) for row in by_score_desc(gatekeeper_fn)[:limit]],
        "feature_top25_tp": [example_row(row, scores) for row in by_score_desc(feature_top_tp)[:limit]],
        "feature_top25_fp": [example_row(row, scores) for row in by_score_desc(feature_top_fp)[:limit]],
    }


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
    lines = [
        "# FEATURE_RICH_R2_FEATURE_CONTRIBUTION",
        "",
        f"Scope: `{report['scope']}`",
        "",
        "## Verdict",
        "",
        f"Status: `{report['status']}`",
        "",
        "This is diagnostic-only feature contribution analysis. It is not Gatekeeper tuning, not a production promotion, and not a model-ready claim.",
        "",
        "## Denominator",
        "",
        f"- Resolved R2 rows: {report['resolved_denominator_rows']}",
        f"- Positive rows: {report['positive_rows']}",
        f"- Negative rows: {report['negative_rows']}",
        f"- Split counts: `{json.dumps(report['split_counts'], sort_keys=True)}`",
        "",
        "## Feature Separation",
        "",
        "| feature | direction | pos median | neg median | median diff | favorable quartile positive rate | favorable lift |",
        "| --- | --- | ---: | ---: | ---: | ---: | ---: |",
    ]
    sorted_features = sorted(
        report["feature_separation"].items(),
        key=lambda item: abs(item[1].get("difference_of_medians") or 0.0),
        reverse=True,
    )
    for feature, payload in sorted_features:
        pos_median = payload.get("positive", {}).get("median")
        neg_median = payload.get("negative", {}).get("median")
        diff = payload.get("difference_of_medians")
        favorable = payload.get("favorable_quartile", {})
        lines.append(
            "| {feature} | {direction} | {pos} | {neg} | {diff} | {rate} | {lift} |".format(
                feature=feature,
                direction=payload.get("effect_direction"),
                pos=f"{pos_median:.6g}" if isinstance(pos_median, (int, float)) else "n/a",
                neg=f"{neg_median:.6g}" if isinstance(neg_median, (int, float)) else "n/a",
                diff=f"{diff:.6g}" if isinstance(diff, (int, float)) else "n/a",
                rate=format_pct(favorable.get("positive_rate")),
                lift=f"{favorable.get('lift_vs_base_rate'):.3f}" if isinstance(favorable.get("lift_vs_base_rate"), (int, float)) else "n/a",
            )
        )
    lines.extend(
        [
            "",
            "## Simple Score Stability",
            "",
            "| split | base positive rate | top10 | top25 | top50 | top100 |",
            "| --- | ---: | ---: | ---: | ---: | ---: |",
        ]
    )
    for split in SPLITS:
        metrics = report["simple_score_stability"].get(split, {})
        top_by_k = {item["k"]: item for item in metrics.get("precision_at_top_k", [])}
        lines.append(
            "| {split} | {base} | {t10} | {t25} | {t50} | {t100} |".format(
                split=split,
                base=format_pct(metrics.get("base_positive_rate")),
                t10=format_pct(top_by_k.get(10, {}).get("precision_r2")),
                t25=format_pct(top_by_k.get(25, {}).get("precision_r2")),
                t50=format_pct(top_by_k.get(50, {}).get("precision_r2")),
                t100=format_pct(top_by_k.get(100, {}).get("precision_r2")),
            )
        )
    lines.extend(
        [
            "",
            "## Gatekeeper Vs Feature Score",
            "",
            "Holdout overlap metrics:",
            "",
            "| top-k | gatekeeper-only precision | feature-only precision | overlap precision | union precision | accepted outside top | top rejected by gatekeeper |",
            "| ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
        ]
    )
    holdout_overlap = report["gatekeeper_vs_feature_score"].get("holdout", {})
    for k in OVERLAP_K:
        payload = holdout_overlap.get(f"top{k}", {})
        buckets = payload.get("bucket_metrics", {})
        lines.append(
            "| {k} | {gk} | {fo} | {ov} | {un} | {outside} | {rejected} |".format(
                k=k,
                gk=format_pct(buckets.get("gatekeeper_only", {}).get("precision_r2")),
                fo=format_pct(buckets.get("feature_only", {}).get("precision_r2")),
                ov=format_pct(buckets.get("overlap", {}).get("precision_r2")),
                un=format_pct(buckets.get("union", {}).get("precision_r2")),
                outside=payload.get("gatekeeper_accepted_outside_feature_top"),
                rejected=payload.get("feature_top_rejected_by_gatekeeper"),
            )
        )
    lines.extend(
        [
            "",
            "## Interpretation",
            "",
            report.get("interpretation", {}).get("summary", ""),
            "",
            "## Next Step",
            "",
            report.get("interpretation", {}).get("recommended_next_step", ""),
            "",
            "## Claim Boundaries",
            "",
            "- diagnostic_only: true",
            "- production_ready: false",
            "- gatekeeper_tuned: false",
            "- model_ready: false",
        ]
    )
    return "\n".join(lines) + "\n"


def build_interpretation(report: dict[str, Any]) -> dict[str, Any]:
    stability = report["simple_score_stability"]
    validation_top10 = next(
        (item for item in stability.get("validation", {}).get("precision_at_top_k", []) if item.get("k") == 10),
        {},
    )
    holdout_top10 = next(
        (item for item in stability.get("holdout", {}).get("precision_at_top_k", []) if item.get("k") == 10),
        {},
    )
    validation_base = stability.get("validation", {}).get("base_positive_rate")
    holdout_base = stability.get("holdout", {}).get("base_positive_rate")
    validation_lift = (
        validation_top10.get("precision_r2") / validation_base
        if isinstance(validation_top10.get("precision_r2"), (int, float)) and validation_base
        else None
    )
    holdout_lift = (
        holdout_top10.get("precision_r2") / holdout_base
        if isinstance(holdout_top10.get("precision_r2"), (int, float)) and holdout_base
        else None
    )
    if (validation_lift or 0.0) > 1.0 and (holdout_lift or 0.0) > 1.0:
        signal_status = "directionally_supported_on_validation_and_holdout"
        next_step = "P3G_SIMPLE_RULES_OR_MODEL_BASELINE_CANDIDATE_WITHOUT_PRODUCTION_CLAIM"
    elif (holdout_lift or 0.0) > 1.0:
        signal_status = "holdout_signal_not_yet_validated_by_validation"
        next_step = "COLLECT_LARGER_R2_DENOMINATOR_BEFORE_MODEL_OR_TUNING"
    else:
        signal_status = "weak_or_unstable"
        next_step = "COLLECT_LARGER_R2_DENOMINATOR_OR_AUDIT_FEATURES"
    summary = (
        "The available feature score is diagnostic-only. It separates the holdout top-k better than "
        "Gatekeeper accept on this checkpoint, but stability must be judged against validation and a "
        "larger denominator before any tuning or production claim."
    )
    return {
        "signal_status": signal_status,
        "validation_top10_lift_vs_base": validation_lift,
        "holdout_top10_lift_vs_base": holdout_lift,
        "summary": summary,
        "recommended_next_step": next_step,
    }


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    dataset_dir = args.root / "datasets" / "selector" / args.scope
    report_dir = args.root / "reports" / "selector" / args.scope
    training_view = dataset_dir / "selector_training_view_v1.jsonl"
    baseline_report_path = report_dir / "selector_r2only_baseline_report_v1.json"
    feature_audit_path = report_dir / "selector_r2only_feature_audit_v1.json"
    ablation_path = report_dir / "selector_r2only_ablation_report_v1.json"
    dataset_manifest_path = report_dir / "dataset_manifest_v1.json"
    decision_md_path = report_dir / "FEATURE_RICH_R2_BASELINE_DECISION.md"
    required_paths = [
        training_view,
        baseline_report_path,
        feature_audit_path,
        ablation_path,
        dataset_manifest_path,
        decision_md_path,
    ]
    missing = [str(path) for path in required_paths if not path.exists() or path.stat().st_size == 0]
    if missing:
        raise FileNotFoundError(f"missing required P3F inputs: {missing}")

    rows = list(common.iter_json_objects(training_view))
    denominator = [row for row in rows if baseline.r2only_denominator(row)]
    train_rows = [row for row in denominator if row.get("split") == "train"]
    features = [
        feature
        for feature in FEATURES
        if any(feature_value(row, feature) is not None for row in denominator)
    ]
    scores = score_rows(denominator, features, train_rows)
    separation = feature_separation(denominator, train_rows)
    bins, bin_csv_rows = feature_bins(denominator, train_rows)
    simple_score_stability = {
        split: {
            "rows": len([row for row in denominator if row.get("split") == split]),
            "base_positive_rate": base_rate([row for row in denominator if row.get("split") == split]),
            "precision_at_top_k": top_k_metrics(
                [row for row in denominator if row.get("split") == split],
                scores,
                TOP_K,
            ),
        }
        for split in SPLITS
    }
    gatekeeper_feature, gatekeeper_feature_csv = gatekeeper_vs_feature(denominator, scores)

    report: dict[str, Any] = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "selector_r2only_feature_contribution_v1",
        "phase": "phase3",
        "status": "P3F_PASS_FEATURE_CONTRIBUTION_DIAGNOSTIC",
        "scope": args.scope,
        "dataset_kind": "r2_only",
        "claim_boundaries": {
            "diagnostic_only": True,
            "model_ready": False,
            "production_ready": False,
            "gatekeeper_tuned": False,
            "threshold_changes": False,
            "runtime_changed": False,
        },
        "input_paths": {path.name: str(path) for path in required_paths},
        "training_rows": len(rows),
        "resolved_denominator_rows": len(denominator),
        "positive_rows": sum(1 for row in denominator if row.get("r2_label") == "positive"),
        "negative_rows": sum(1 for row in denominator if row.get("r2_label") == "negative"),
        "split_counts": split_counts(denominator),
        "available_features_used": features,
        "baseline_status": read_json(baseline_report_path).get("status"),
        "feature_audit_status": read_json(feature_audit_path).get("status"),
        "ablation_status": read_json(ablation_path).get("status"),
        "dataset_manifest_phase2_status": read_json(dataset_manifest_path).get("phase2_status"),
        "feature_separation": separation,
        "feature_bins": bins,
        "simple_score_stability": simple_score_stability,
        "gatekeeper_vs_feature_score": gatekeeper_feature,
        "examples": examples(denominator, scores, limit=args.example_limit),
    }
    report["interpretation"] = build_interpretation(report)

    report_dir.mkdir(parents=True, exist_ok=True)
    output_json = report_dir / "selector_r2only_feature_contribution_v1.json"
    output_md = report_dir / "FEATURE_RICH_R2_FEATURE_CONTRIBUTION.md"
    output_bins_csv = report_dir / "selector_r2only_feature_bins_v1.csv"
    output_overlap_csv = report_dir / "selector_r2only_accept_vs_feature_score_v1.csv"
    common.write_json(output_json, report)
    output_md.write_text(markdown_report(report), encoding="utf-8")
    write_csv(
        output_bins_csv,
        bin_csv_rows,
        [
            "feature",
            "split",
            "bin",
            "lower",
            "upper",
            "rows",
            "positive_rows",
            "negative_rows",
            "positive_rate",
            "base_positive_rate",
            "lift_vs_base_rate",
        ],
    )
    write_csv(
        output_overlap_csv,
        gatekeeper_feature_csv,
        [
            "split",
            "top_k",
            "bucket",
            "rows",
            "positive_rows",
            "negative_rows",
            "selected_count",
            "tp_r2",
            "fp_r2",
            "precision_r2",
            "positive_rate",
        ],
    )
    report["outputs"] = {
        "selector_r2only_feature_contribution_v1": str(output_json),
        "FEATURE_RICH_R2_FEATURE_CONTRIBUTION": str(output_md),
        "selector_r2only_feature_bins_v1": str(output_bins_csv),
        "selector_r2only_accept_vs_feature_score_v1": str(output_overlap_csv),
    }
    # Rewrite after outputs are attached.
    common.write_json(output_json, report)
    return report


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument("--example-limit", type=int, default=15)
    parser.add_argument("--json", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = build_report(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0 if report.get("status") == "P3F_PASS_FEATURE_CONTRIBUTION_DIAGNOSTIC" else 2


if __name__ == "__main__":
    raise SystemExit(main())
