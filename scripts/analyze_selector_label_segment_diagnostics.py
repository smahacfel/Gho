#!/usr/bin/env python3
"""Analyze selector label and segment diagnostics for P4D.

P4D is offline-only. It does not build a new model, tune Gatekeeper, change
runtime scoring, change execution, or change send behavior. It audits whether
the rejected selector score family failed because of label sensitivity, segment
heterogeneity, missing feature families, or absence of detectable edge.
"""

from __future__ import annotations

import argparse
import csv
import json
import math
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

import audit_selector_shadow_score_parity as parity
import audit_selector_shadow_score_topk_drift as topk_drift
import build_selector_r2only_baseline_report as baseline
import build_selector_r2only_model_candidate as p3g
import selector_pipeline_common as common


ARTIFACT = "selector_label_segment_diagnostics_v1"
MD_ARTIFACT = "SELECTOR_LABEL_SEGMENT_DIAGNOSTICS.md"
SEGMENT_MATRIX_ARTIFACT = "selector_segment_lift_matrix_v1.csv"
LABEL_SENSITIVITY_ARTIFACT = "selector_label_sensitivity_v1.csv"
FAILURE_CLUSTERS_ARTIFACT = "selector_failure_case_clusters_v1.csv"
DEFAULT_RUST_SOURCE = "ghost-brain/src/oracle/decision_logger.rs"
TOP_K = (10, 25, 50)
LABEL_VARIANTS = (
    ("label_20_20_30s", 20.0, 20.0, 30_000),
    ("label_30_30_60s", 30.0, 30.0, 60_000),
    ("label_40_40_60s", 40.0, 40.0, 60_000),
    ("label_50_40_60s", 50.0, 40.0, 60_000),
    ("label_40_30_60s", 40.0, 30.0, 60_000),
    ("label_40_40_120s", 40.0, 40.0, 120_000),
)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="/root/Gho")
    parser.add_argument("--train-scope", required=True)
    parser.add_argument("--validation-scope", required=True)
    parser.add_argument("--rust-source", default=DEFAULT_RUST_SOURCE)
    parser.add_argument("--min-segment-rows", type=int, default=40)
    parser.add_argument("--min-stable-segment-lift-pp", type=float, default=0.10)
    parser.add_argument("--min-label-positive-rate-delta-pp", type=float, default=0.05)
    parser.add_argument("--topk", type=int, default=50)
    parser.add_argument("--output", default=None)
    parser.add_argument("--md-output", default=None)
    parser.add_argument("--segment-output", default=None)
    parser.add_argument("--label-output", default=None)
    parser.add_argument("--failure-cluster-output", default=None)
    parser.add_argument("--json", action="store_true")
    return parser


def training_view_path(root: Path, scope: str) -> Path:
    return root / "datasets" / "selector" / scope / "selector_training_view_v1.jsonl"


def r2_paths_path(root: Path, scope: str) -> Path:
    return root / "datasets" / "selector" / scope / "r2_market_paths_v1.jsonl"


def phase3_manifest_path(root: Path, scope: str) -> Path:
    return root / "reports" / "selector" / scope / "phase3_r2only_manifest_v1.json"


def report_dir(root: Path, validation_scope: str) -> Path:
    return root / "reports" / "selector" / validation_scope


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as fh:
        payload = json.load(fh)
    return payload if isinstance(payload, dict) else {}


def denominator_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [row for row in rows if baseline.r2only_denominator(row)]


def row_key(row: dict[str, Any]) -> str:
    return common.str_or_none(row.get("candidate_id")) or ""


def label_positive(row: dict[str, Any]) -> bool:
    return row.get("r2_label") == "positive"


def num(row: dict[str, Any], field: str) -> float | None:
    value = p3g.feature_value(row, field)
    if value is None or isinstance(value, bool):
        value = row.get(field)
    return common.float_or_none(value)


def bool_value(row: dict[str, Any], field: str) -> bool | None:
    value = row.get(field)
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return bool(value)
    return None


def score_map(rows: list[dict[str, Any]], specs: list[dict[str, Any]]) -> dict[str, float]:
    out: dict[str, float] = {}
    for row in rows:
        key = row_key(row)
        if not key:
            continue
        score, _availability = topk_drift.score_training_row(row, specs)
        if score is not None:
            out[key] = score
    return out


def ordered(rows: list[dict[str, Any]], scores: dict[str, float]) -> list[dict[str, Any]]:
    return sorted(
        [row for row in rows if row_key(row) in scores],
        key=lambda row: (
            -scores[row_key(row)],
            common.int_or_none(row.get("birth_ts_ms")) or common.int_or_none(row.get("decision_ts_ms")) or 0,
            row_key(row),
        ),
    )


def selected_metric(rows: list[dict[str, Any]], selected: list[dict[str, Any]]) -> dict[str, Any]:
    positives = sum(1 for row in rows if label_positive(row))
    selected_positive = sum(1 for row in selected if label_positive(row))
    precision = selected_positive / len(selected) if selected else None
    base_rate = positives / len(rows) if rows else None
    return {
        "rows": len(rows),
        "positive_rows": positives,
        "negative_rows": len(rows) - positives,
        "base_positive_rate": base_rate,
        "selected_count": len(selected),
        "selected_positive_rows": selected_positive,
        "selected_negative_rows": len(selected) - selected_positive,
        "precision": precision,
        "lift_vs_base_rate_pp": (
            precision - base_rate if isinstance(precision, float) and isinstance(base_rate, float) else None
        ),
    }


def topk_metric(rows: list[dict[str, Any]], scores: dict[str, float], k: int) -> dict[str, Any]:
    ranked = ordered(rows, scores)
    return selected_metric(rows, ranked[: min(k, len(ranked))])


def run_quality(root: Path, scope: str, rows: list[dict[str, Any]]) -> dict[str, Any]:
    manifest_path = phase3_manifest_path(root, scope)
    manifest = read_json_object(manifest_path) if manifest_path.exists() else {}
    positives = sum(1 for row in rows if label_positive(row))
    return {
        "scope": scope,
        "denominator_rows": len(rows),
        "positive_rows": positives,
        "negative_rows": len(rows) - positives,
        "base_positive_rate": positives / len(rows) if rows else None,
        "leakage_audit_status": manifest.get("leakage_audit_status"),
        "leakage_clean": manifest.get("leakage_audit_status") == "PASS",
        "phase3_manifest": str(manifest_path),
    }


def load_r2_paths(root: Path, scope: str) -> dict[str, dict[str, Any]]:
    path = r2_paths_path(root, scope)
    if not path.exists():
        return {}
    out: dict[str, dict[str, Any]] = {}
    for row in common.iter_json_objects(path):
        key = row_key(row)
        if key:
            out[key] = row
    return out


def label_variant_rows(
    rows: list[dict[str, Any]],
    paths_by_id: dict[str, dict[str, Any]],
    *,
    target_net_pct: float,
    stop_net_pct: float,
    horizon_ms: int,
) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    for row in rows:
        classified = common.classify_r2(
            paths_by_id.get(row_key(row)),
            target_net_pct=target_net_pct,
            stop_net_pct=stop_net_pct,
            horizon_ms=horizon_ms,
        )
        if classified.get("r2_status") != "resolved":
            continue
        item = dict(row)
        item["r2_label"] = classified.get("r2_label")
        item["r2_status"] = classified.get("r2_status")
        item["r2_label_reason"] = classified.get("r2_label_reason")
        out.append(item)
    return out


def label_sensitivity(
    run_name: str,
    rows: list[dict[str, Any]],
    paths_by_id: dict[str, dict[str, Any]],
    scores: dict[str, float],
) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    for label_id, target, stop, horizon in LABEL_VARIANTS:
        variant_rows = label_variant_rows(
            rows,
            paths_by_id,
            target_net_pct=target,
            stop_net_pct=stop,
            horizon_ms=horizon,
        )
        variant_scores = {row_key(row): scores[row_key(row)] for row in variant_rows if row_key(row) in scores}
        positives = sum(1 for row in variant_rows if label_positive(row))
        base_rate = positives / len(variant_rows) if variant_rows else None
        top25 = topk_metric(variant_rows, variant_scores, 25)
        out.append(
            {
                "run": run_name,
                "label_variant": label_id,
                "target_net_pct": target,
                "stop_net_pct": stop,
                "horizon_ms": horizon,
                "resolved_rows": len(variant_rows),
                "positive_rows": positives,
                "negative_rows": len(variant_rows) - positives,
                "positive_rate": base_rate,
                "path_availability_rate": len(variant_rows) / len(rows) if rows else None,
                "top25_precision": top25.get("precision"),
                "top25_lift_vs_base_rate_pp": top25.get("lift_vs_base_rate_pp"),
            }
        )
    return out


def bucket_num(value: float | None, *, low: float, high: float) -> str:
    if value is None:
        return "missing"
    if value < low:
        return "low"
    if value < high:
        return "mid"
    return "high"


def quantile_bucket(value: float | None, values: list[float]) -> str:
    if value is None:
        return "missing"
    if len(values) < 4:
        return "known"
    ordered_values = sorted(values)
    q1 = ordered_values[int((len(ordered_values) - 1) * 0.25)]
    q2 = ordered_values[int((len(ordered_values) - 1) * 0.50)]
    q3 = ordered_values[int((len(ordered_values) - 1) * 0.75)]
    if value <= q1:
        return "q1"
    if value <= q2:
        return "q2"
    if value <= q3:
        return "q3"
    return "q4"


def segment_value(row: dict[str, Any], segment: str, quantiles: dict[str, list[float]]) -> str:
    if segment == "market_cap":
        return bucket_num(num(row, "gk_current_market_cap_sol"), low=30.0, high=100.0)
    if segment == "bonding_progress":
        return bucket_num(num(row, "gk_bonding_progress_pct"), low=20.0, high=70.0)
    if segment == "evidence_sufficiency":
        return common.str_or_none(row.get("evidence_sufficiency_status")) or "missing"
    if segment == "tx_count":
        return quantile_bucket(num(row, "evidence_tx_count"), quantiles.get("evidence_tx_count", []))
    if segment == "buy_count":
        return quantile_bucket(num(row, "evidence_buy_count"), quantiles.get("evidence_buy_count", []))
    if segment == "unique_actors":
        value = num(row, "evidence_unique_buyers")
        if value is None:
            value = num(row, "evidence_unique_signers")
        return quantile_bucket(value, quantiles.get("unique_actors", []))
    if segment == "total_volume":
        return quantile_bucket(num(row, "evidence_total_volume_sol"), quantiles.get("evidence_total_volume_sol", []))
    if segment == "hhi_available":
        return "present" if num(row, "gk_hhi") is not None else "missing"
    if segment == "top3_available":
        return "present" if num(row, "gk_top3_volume_pct") is not None else "missing"
    if segment == "concentration":
        hhi = num(row, "gk_hhi")
        top3 = num(row, "gk_top3_volume_pct")
        if hhi is None and top3 is None:
            return "missing"
        if (hhi is not None and hhi >= 0.80) or (top3 is not None and top3 >= 0.90):
            return "high"
        return "low"
    if segment == "dev_has_sold":
        value = bool_value(row, "gk_dev_has_sold")
        return "true" if value is True else "false" if value is False else "missing"
    if segment == "dev_volume_ratio":
        return bucket_num(num(row, "gk_dev_volume_ratio"), low=0.05, high=0.50)
    if segment == "sell_share":
        return bucket_num(num(row, "sell_share") if num(row, "sell_share") is not None else num(row, "evidence_sell_share"), low=0.10, high=0.40)
    if segment == "sell_buy_ratio":
        return bucket_num(num(row, "gk_sell_buy_ratio"), low=0.50, high=1.50)
    if segment == "time_hour":
        ts = common.int_or_none(row.get("birth_ts_ms") or row.get("decision_ts_ms"))
        if ts is None:
            return "missing"
        return str((ts // 3_600_000) % 24)
    if segment == "birth_order_quartile":
        return quantile_bucket(common.float_or_none(row.get("birth_ts_ms")), quantiles.get("birth_ts_ms", []))
    raise ValueError(f"unknown segment {segment}")


SEGMENTS = (
    "market_cap",
    "bonding_progress",
    "evidence_sufficiency",
    "tx_count",
    "buy_count",
    "unique_actors",
    "total_volume",
    "hhi_available",
    "top3_available",
    "concentration",
    "dev_has_sold",
    "dev_volume_ratio",
    "sell_share",
    "sell_buy_ratio",
    "time_hour",
    "birth_order_quartile",
)


def quantile_sources(rows: list[dict[str, Any]]) -> dict[str, list[float]]:
    fields = {
        "evidence_tx_count": "evidence_tx_count",
        "evidence_buy_count": "evidence_buy_count",
        "evidence_total_volume_sol": "evidence_total_volume_sol",
        "birth_ts_ms": "birth_ts_ms",
    }
    out: dict[str, list[float]] = {}
    for key, field in fields.items():
        out[key] = [value for row in rows if (value := num(row, field)) is not None]
    out["unique_actors"] = [
        value
        for row in rows
        if (value := (num(row, "evidence_unique_buyers") if num(row, "evidence_unique_buyers") is not None else num(row, "evidence_unique_signers"))) is not None
    ]
    return out


def segment_lift_matrix(
    run_name: str,
    rows: list[dict[str, Any]],
    scores: dict[str, float],
    *,
    min_segment_rows: int,
) -> list[dict[str, Any]]:
    quantiles = quantile_sources(rows)
    out: list[dict[str, Any]] = []
    for segment in SEGMENTS:
        grouped: dict[str, list[dict[str, Any]]] = defaultdict(list)
        for row in rows:
            grouped[segment_value(row, segment, quantiles)].append(row)
        for value, segment_rows in sorted(grouped.items()):
            if len(segment_rows) < min_segment_rows:
                continue
            segment_scores = {row_key(row): scores[row_key(row)] for row in segment_rows if row_key(row) in scores}
            metric = topk_metric(segment_rows, segment_scores, 25)
            out.append(
                {
                    "run": run_name,
                    "segment": segment,
                    "segment_value": value,
                    "rows": len(segment_rows),
                    "positive_rows": sum(1 for row in segment_rows if label_positive(row)),
                    "base_positive_rate": metric.get("base_positive_rate"),
                    "top25_precision": metric.get("precision"),
                    "top25_lift_vs_segment_base_pp": metric.get("lift_vs_base_rate_pp"),
                    "selected_count": metric.get("selected_count"),
                }
            )
    return out


def cluster_false_positive(row: dict[str, Any]) -> str:
    if row.get("score_eligibility_status") == "score_invalid_insufficient_market_evidence":
        return "insufficient_evidence"
    dev_tx = num(row, "gk_dev_tx_ratio")
    dev_vol = num(row, "gk_dev_volume_ratio")
    if (dev_tx is not None and dev_tx >= 0.80) or (dev_vol is not None and dev_vol >= 0.80):
        return "dev_dominated"
    hhi = num(row, "gk_hhi")
    top3 = num(row, "gk_top3_volume_pct")
    if hhi is None and top3 is None:
        return "missing_concentration"
    if (hhi is not None and hhi >= 0.80) or (top3 is not None and top3 >= 0.90):
        return "high_concentration"
    tx_count = num(row, "evidence_tx_count")
    buyers = num(row, "evidence_unique_buyers")
    if (tx_count is not None and tx_count < 5) or (buyers is not None and buyers < 3):
        return "low_flow"
    progress = num(row, "gk_bonding_progress_pct")
    if progress is not None and progress >= 70.0 and (buyers is None or buyers < 5):
        return "high_curve_low_buyers"
    sell_share = num(row, "sell_share")
    if sell_share is None:
        sell_share = num(row, "evidence_sell_share")
    if sell_share is not None and sell_share >= 0.40:
        return "sell_pressure"
    market_cap = num(row, "gk_current_market_cap_sol")
    if market_cap is not None and market_cap < 30.0:
        return "low_market_cap"
    if market_cap is not None and market_cap >= 100.0 and (tx_count is None or tx_count < 8):
        return "high_market_cap_dead_flow"
    return "other_false_positive"


def missed_positive_cluster(row: dict[str, Any]) -> str:
    if row.get("score_eligibility_status") == "score_invalid_insufficient_market_evidence":
        return "positive_rejected_insufficient_evidence"
    if num(row, "gk_hhi") is None and num(row, "gk_top3_volume_pct") is None:
        return "positive_missing_concentration"
    if row.get("evidence_sufficiency_status") == "partial":
        return "positive_partial_evidence"
    if (num(row, "net_quote_in_15s") is None and num(row, "net_quote_in_30s") is None):
        return "positive_missing_flow"
    return "positive_ranked_below_topk"


def cluster_rows(
    run_name: str,
    rows: list[dict[str, Any]],
    scores: dict[str, float],
    *,
    topk: int,
) -> list[dict[str, Any]]:
    ranked = ordered(rows, scores)
    selected_ids = {row_key(row) for row in ranked[: min(topk, len(ranked))]}
    clusters: Counter[str] = Counter()
    examples: dict[str, list[str]] = defaultdict(list)
    for row in ranked[: min(topk, len(ranked))]:
        if label_positive(row):
            continue
        cluster = cluster_false_positive(row)
        clusters[cluster] += 1
        if len(examples[cluster]) < 5:
            examples[cluster].append(row_key(row))
    for row in rows:
        if not label_positive(row) or row_key(row) in selected_ids:
            continue
        cluster = missed_positive_cluster(row)
        clusters[cluster] += 1
        if len(examples[cluster]) < 5:
            examples[cluster].append(row_key(row))
    return [
        {
            "run": run_name,
            "cluster": cluster,
            "count": count,
            "example_candidate_ids": ";".join(examples.get(cluster, [])),
        }
        for cluster, count in sorted(clusters.items())
    ]


def stable_segment_edges(
    train_matrix: list[dict[str, Any]],
    validation_matrix: list[dict[str, Any]],
    *,
    min_lift_pp: float,
) -> list[dict[str, Any]]:
    train_by_key = {(row["segment"], row["segment_value"]): row for row in train_matrix}
    out = []
    for validation in validation_matrix:
        key = (validation["segment"], validation["segment_value"])
        train = train_by_key.get(key)
        if not train:
            continue
        train_lift = train.get("top25_lift_vs_segment_base_pp")
        validation_lift = validation.get("top25_lift_vs_segment_base_pp")
        if (
            isinstance(train_lift, float)
            and isinstance(validation_lift, float)
            and train_lift >= min_lift_pp
            and validation_lift >= min_lift_pp
        ):
            out.append(
                {
                    "segment": key[0],
                    "segment_value": key[1],
                    "train_lift_pp": train_lift,
                    "validation_lift_pp": validation_lift,
                    "train_rows": train["rows"],
                    "validation_rows": validation["rows"],
                }
            )
    return out


def label_review_required(label_rows: list[dict[str, Any]], *, min_delta_pp: float) -> bool:
    by_label: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in label_rows:
        by_label[str(row["label_variant"])].append(row)
    canonical = by_label.get("label_40_40_60s", [])
    if not canonical:
        return False
    canonical_lifts = [
        item.get("top25_lift_vs_base_rate_pp")
        for item in canonical
        if isinstance(item.get("top25_lift_vs_base_rate_pp"), float)
    ]
    canonical_best = max(canonical_lifts) if canonical_lifts else 0.0
    for label_id, rows in by_label.items():
        if label_id == "label_40_40_60s":
            continue
        lifts = [
            item.get("top25_lift_vs_base_rate_pp")
            for item in rows
            if isinstance(item.get("top25_lift_vs_base_rate_pp"), float)
        ]
        if lifts and min(lifts) >= canonical_best + min_delta_pp:
            return True
    return False


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
        "# Selector Label Segment Diagnostics",
        "",
        f"Status: {report['status']}",
        f"Business decision: {report['business_decision']}",
        f"Train scope: `{report['train_scope']}`",
        f"Validation scope: `{report['validation_scope']}`",
        "",
        "## Stable Segment Edges",
        "",
        "| segment | value | train lift | validation lift | train rows | validation rows |",
        "|---|---|---:|---:|---:|---:|",
    ]
    for row in report["stable_segment_edges"][:20]:
        lines.append(
            "| {segment} | {value} | {train:.4f} | {validation:.4f} | {train_rows} | {validation_rows} |".format(
                segment=row["segment"],
                value=row["segment_value"],
                train=row["train_lift_pp"],
                validation=row["validation_lift_pp"],
                train_rows=row["train_rows"],
                validation_rows=row["validation_rows"],
            )
        )
    if not report["stable_segment_edges"]:
        lines.append("| none | none | n/a | n/a | 0 | 0 |")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.root)
    specs, _thresholds = parity.parse_runtime_spec(root / args.rust_source)
    train_rows = denominator_rows(list(common.iter_json_objects(training_view_path(root, args.train_scope))))
    validation_rows = denominator_rows(list(common.iter_json_objects(training_view_path(root, args.validation_scope))))
    train_scores = score_map(train_rows, specs)
    validation_scores = score_map(validation_rows, specs)
    train_paths = load_r2_paths(root, args.train_scope)
    validation_paths = load_r2_paths(root, args.validation_scope)

    train_labels = label_sensitivity("train", train_rows, train_paths, train_scores)
    validation_labels = label_sensitivity("validation", validation_rows, validation_paths, validation_scores)
    train_segments = segment_lift_matrix("train", train_rows, train_scores, min_segment_rows=args.min_segment_rows)
    validation_segments = segment_lift_matrix(
        "validation",
        validation_rows,
        validation_scores,
        min_segment_rows=args.min_segment_rows,
    )
    stable_edges = stable_segment_edges(
        train_segments,
        validation_segments,
        min_lift_pp=args.min_stable_segment_lift_pp,
    )
    clusters = (
        cluster_rows("train", train_rows, train_scores, topk=args.topk)
        + cluster_rows("validation", validation_rows, validation_scores, topk=args.topk)
    )

    train_quality = run_quality(root, args.train_scope, train_rows)
    validation_quality = run_quality(root, args.validation_scope, validation_rows)
    labels_need_review = label_review_required(
        train_labels + validation_labels,
        min_delta_pp=args.min_label_positive_rate_delta_pp,
    )
    if stable_edges:
        status = "P4D_SEGMENT_EDGE_FOUND"
        business_decision = "DESIGN_SEGMENT_SPECIFIC_CANDIDATE_OFFLINE"
        fail_reasons: list[str] = []
    elif labels_need_review:
        status = "P4D_LABEL_REVIEW_REQUIRED"
        business_decision = "REVIEW_R2_LABEL_CONTRACT"
        fail_reasons = ["alternate_label_variant_has_more_stable_top25_lift"]
    else:
        status = "P4D_NEW_FEATURE_FAMILY_REQUIRED"
        business_decision = "DO_NOT_RUN_RUNTIME"
        fail_reasons = ["no_stable_segment_edge_found_with_current_score_features"]
    if not train_quality["leakage_clean"] or not validation_quality["leakage_clean"]:
        status = "P4D_LABEL_REVIEW_REQUIRED"
        business_decision = "DO_NOT_RUN_RUNTIME"
        fail_reasons.append("leakage_audit_not_clean")

    out_dir = report_dir(root, args.validation_scope)
    output = Path(args.output) if args.output else out_dir / f"{ARTIFACT}.json"
    md_output = Path(args.md_output) if args.md_output else out_dir / MD_ARTIFACT
    segment_output = Path(args.segment_output) if args.segment_output else out_dir / SEGMENT_MATRIX_ARTIFACT
    label_output = Path(args.label_output) if args.label_output else out_dir / LABEL_SENSITIVITY_ARTIFACT
    cluster_output = (
        Path(args.failure_cluster_output)
        if args.failure_cluster_output
        else out_dir / FAILURE_CLUSTERS_ARTIFACT
    )
    report = {
        "artifact": ARTIFACT,
        "status": status,
        "business_decision": business_decision,
        "train_scope": args.train_scope,
        "validation_scope": args.validation_scope,
        "run_quality": {"train": train_quality, "validation": validation_quality},
        "stable_segment_edges": stable_edges,
        "label_review_required": labels_need_review,
        "claim_boundaries": {
            "offline_only": True,
            "diagnostic_only": True,
            "builds_new_model": False,
            "changes_runtime": False,
            "changes_gatekeeper": False,
            "changes_execution": False,
            "changes_send_path": False,
            "production_promotion_allowed": False,
        },
        "acceptance": {
            "fail_reasons": fail_reasons,
            "min_segment_rows": args.min_segment_rows,
            "min_stable_segment_lift_pp": args.min_stable_segment_lift_pp,
        },
        "summary": {
            "label_variant_rows": len(train_labels) + len(validation_labels),
            "segment_rows": len(train_segments) + len(validation_segments),
            "failure_cluster_rows": len(clusters),
        },
        "outputs": {
            ARTIFACT: str(output),
            MD_ARTIFACT: str(md_output),
            SEGMENT_MATRIX_ARTIFACT: str(segment_output),
            LABEL_SENSITIVITY_ARTIFACT: str(label_output),
            FAILURE_CLUSTERS_ARTIFACT: str(cluster_output),
        },
    }
    write_json(output, report)
    write_markdown(md_output, report)
    write_csv(segment_output, train_segments + validation_segments)
    write_csv(label_output, train_labels + validation_labels)
    write_csv(cluster_output, clusters)
    return report


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = build_report(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
