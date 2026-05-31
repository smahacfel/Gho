#!/usr/bin/env python3
"""Compare V2.5/V3 selector evidence on the same training-view denominator."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any, Callable
from collections import Counter

import selector_pipeline_common as common


def score(row: dict[str, Any], fields: tuple[str, ...]) -> float | None:
    for field in fields:
        value = common.float_or_none(row.get(field))
        if value is not None:
            return value
    return None


def native_accept(
    row: dict[str, Any],
    fields: tuple[str, ...],
    verdict_fields: tuple[str, ...],
) -> bool | None:
    for field in fields:
        value = common.bool_or_none(row.get(field))
        if value is not None:
            return value
    for field in verdict_fields:
        verdict = row.get(field)
        if isinstance(verdict, str) and verdict:
            return verdict.upper() in {"BUY", "EARLY_BUY"}
    return None


def comparison_eligible(row: dict[str, Any], *, split: str) -> bool:
    return bool(
        row.get("split") == split
        and row.get("cohort_in_scope") is True
        and row.get("stream_completeness_ok") is True
        and row.get("label_resolved") is True
        and row.get("r2_label") in {"positive", "negative"}
        and row.get("eligible") is not False
    )


def metric_rows(rows: list[dict[str, Any]], selector: Callable[[dict[str, Any]], bool]) -> dict[str, Any]:
    selected: list[dict[str, Any]] = []
    eligible = rows
    for row in eligible:
        if selector(row):
            copy = dict(row)
            copy["selector_accept"] = True
            selected.append(copy)
    tp = sum(1 for row in selected if row.get("r2_label") == "positive")
    fp = sum(1 for row in selected if row.get("r2_label") == "negative")
    positives = sum(1 for row in eligible if row.get("r2_label") == "positive")
    return {
        "eligible_count": len(eligible),
        "selected_count": len(selected),
        "tp_r2": tp,
        "fp_r2": fp,
        "precision_r2": tp / (tp + fp) if (tp + fp) else None,
        "recall_r2": tp / positives if positives else None,
        "coverage": len(selected) / len(eligible) if eligible else None,
    }


def top_rate_selector(
    rows: list[dict[str, Any]],
    score_fn: Callable[[dict[str, Any]], float | None],
    rate: float,
) -> Callable[[dict[str, Any]], bool]:
    eligible_scores = [
        value
        for row in rows
        if row.get("label_resolved") is True and (value := score_fn(row)) is not None
    ]
    if not eligible_scores:
        return lambda _row: False
    ordered = sorted(eligible_scores, reverse=True)
    index = max(0, min(len(ordered) - 1, int(len(ordered) * rate) - 1))
    threshold = ordered[index]
    return lambda row: (score_fn(row) is not None and score_fn(row) >= threshold)


def model_report(
    rows: list[dict[str, Any]],
    *,
    model_name: str,
    score_fields: tuple[str, ...],
    accept_fields: tuple[str, ...],
    verdict_fields: tuple[str, ...],
    replay_version_fields: tuple[str, ...],
    buckets: list[float],
) -> dict[str, Any]:
    if not rows:
        return {
            "model": model_name,
            "status": "no_comparison_rows",
            "eligible_rows": 0,
            "ok_replay_rows": 0,
            "row_status_counts": {},
            "missing_score_rows": 0,
            "missing_replay_artifact_version_rows": 0,
            "native": metric_rows([], lambda _row: False),
            "accept_rate_buckets": {
                f"top_{rate:g}": metric_rows([], lambda _row: False) for rate in buckets
            },
            "row_results": [],
        }
    row_results = []
    ok_rows: list[dict[str, Any]] = []
    status_counts: dict[str, int] = {}
    for row in rows:
        raw_score = score(row, score_fields)
        native = native_accept(row, accept_fields, verdict_fields)
        replay_version = first_string(row, replay_version_fields)
        missing = []
        if raw_score is None:
            missing.append("raw_score")
        if replay_version is None:
            missing.append("replay_artifact_version")
        status = "ok" if not missing else "replay_input_missing"
        status_counts[status] = status_counts.get(status, 0) + 1
        row_results.append(
            {
                "candidate_id": row.get("candidate_id"),
                "split": row.get("split"),
                "snapshot_kind": row.get("snapshot_kind"),
                "feature_cutoff_ts_ms": row.get("feature_cutoff_ts_ms"),
                "observation_window_ms": row.get("observation_window_ms") or row.get("horizon_ms"),
                "model": model_name,
                "row_status": status,
                "missing_replay_inputs": missing,
                "raw_score_present": raw_score is not None,
                "native_accept_present": native is not None,
                "replay_artifact_version": replay_version,
            }
        )
        if status == "ok":
            ok_rows.append(row)
    score_fn = lambda row: score(row, score_fields)
    native_metrics = metric_rows(
        ok_rows,
        lambda row: native_accept(row, accept_fields, verdict_fields) is True,
    )
    bucket_metrics = {
        f"top_{rate:g}": metric_rows(ok_rows, top_rate_selector(ok_rows, score_fn, rate))
        for rate in buckets
    }
    return {
        "model": model_name,
        "status": "ok" if rows and len(ok_rows) == len(rows) else "replay_input_missing",
        "eligible_rows": len(rows),
        "ok_replay_rows": len(ok_rows),
        "row_status_counts": status_counts,
        "missing_score_rows": sum(1 for item in row_results if "raw_score" in item["missing_replay_inputs"]),
        "missing_replay_artifact_version_rows": sum(
            1 for item in row_results if "replay_artifact_version" in item["missing_replay_inputs"]
        ),
        "native": native_metrics,
        "accept_rate_buckets": bucket_metrics,
        "row_results": row_results[:500],
    }


def first_string(row: dict[str, Any], fields: tuple[str, ...]) -> str | None:
    for field in fields:
        value = common.str_or_none(row.get(field))
        if value:
            return value
    return None


def contract_checks(rows: list[dict[str, Any]], *, split: str) -> dict[str, Any]:
    eligible = [row for row in rows if comparison_eligible(row, split=split)]
    split_rows = [row for row in rows if row.get("split") == split]
    cohort_rows = [row for row in split_rows if row.get("cohort_in_scope") is True]
    stream_rows = [row for row in cohort_rows if row.get("stream_completeness_ok") is True]
    label_rows = [row for row in stream_rows if row.get("label_resolved") is True]
    r2_rows = [row for row in label_rows if row.get("r2_label") in {"positive", "negative"}]
    eligibility_breakdown = {
        "rows_total": len(rows),
        "split_rows": len(split_rows),
        "cohort_in_scope_rows": len(cohort_rows),
        "stream_completeness_ok_rows": len(stream_rows),
        "label_resolved_rows": len(label_rows),
        "r2_positive_negative_rows": len(r2_rows),
        "comparison_eligible_rows": len(eligible),
        "r2_label_or_status_counts": common.counter_dict(
            Counter(str(row.get("r2_label") or row.get("r2_status") or "unknown") for row in rows)
        ),
    }
    candidate_ids = [common.str_or_none(row.get("candidate_id")) for row in eligible]
    missing_candidate_id = sum(1 for candidate_id in candidate_ids if not candidate_id)
    candidate_id_counts = Counter(candidate_id for candidate_id in candidate_ids if candidate_id)
    duplicate_candidate_ids = [
        candidate_id for candidate_id, count in candidate_id_counts.items() if count > 1
    ]
    split_values = sorted({str(row.get("split") or "missing") for row in eligible})
    eligibility_missing = sum(
        1
        for row in eligible
        if any(
            row.get(field) is None
            for field in ("cohort_in_scope", "stream_completeness_ok", "label_resolved", "eligible")
        )
    )
    cutoff_missing = sum(1 for row in eligible if row.get("feature_cutoff_ts_ms") is None)
    snapshot_missing = sum(1 for row in eligible if row.get("snapshot_kind") in (None, ""))
    observation_window_missing = sum(
        1 for row in eligible if row.get("observation_window_ms") is None and row.get("horizon_ms") is None
    )
    mismatched_replay_versions = []
    replay_versions = set()
    for row in eligible:
        common_version = first_string(row, ("replay_artifact_version", "selector_replay_artifact_version"))
        v25_version = first_string(
            row,
            ("gatekeeper_v25_replay_artifact_version", "v25_replay_artifact_version", "replay_artifact_version"),
        )
        v3_version = first_string(
            row,
            ("gatekeeper_v3_replay_artifact_version", "v3_replay_artifact_version", "replay_artifact_version"),
        )
        for version in (common_version, v25_version, v3_version):
            if version:
                replay_versions.add(version)
        if not v25_version or not v3_version or v25_version != v3_version:
            mismatched_replay_versions.append(
                {
                    "candidate_id": row.get("candidate_id"),
                    "v25_replay_artifact_version": v25_version,
                    "v3_replay_artifact_version": v3_version,
                }
            )
    failures = []
    if not eligible:
        failures.append("no_comparison_eligible_rows")
    if missing_candidate_id:
        failures.append("missing_candidate_id")
    if duplicate_candidate_ids:
        failures.append("duplicate_candidate_ids")
    if split_values != [split] and eligible:
        failures.append("mixed_time_split")
    if eligibility_missing:
        failures.append("missing_eligibility_flags")
    if cutoff_missing:
        failures.append("missing_feature_cutoff")
    if snapshot_missing:
        failures.append("missing_snapshot_kind")
    if observation_window_missing:
        failures.append("missing_observation_window")
    if mismatched_replay_versions:
        failures.append("replay_artifact_version_missing_or_mismatch")
    return {
        "status": "PASS" if not failures else "NO-GO",
        "fail_reasons": failures,
        "split": split,
        "eligible_rows": len(eligible),
        "same_candidate_set": bool(eligible) and not missing_candidate_id and not duplicate_candidate_ids,
        "candidate_ids_seen": len(candidate_id_counts),
        "missing_candidate_id_rows": missing_candidate_id,
        "duplicate_candidate_ids_sample": duplicate_candidate_ids[:50],
        "same_label": "r2_label",
        "same_time_split": split_values == [split] if eligible else False,
        "split_values_seen": split_values,
        "same_eligibility_flags": bool(eligible) and eligibility_missing == 0,
        "missing_eligibility_flag_rows": eligibility_missing,
        "feature_cutoff_present": cutoff_missing == 0,
        "snapshot_kind_present": snapshot_missing == 0,
        "observation_window_present": observation_window_missing == 0,
        "same_replay_artifact_version": not mismatched_replay_versions,
        "replay_artifact_versions_seen": sorted(replay_versions),
        "mismatched_replay_versions_sample": mismatched_replay_versions[:50],
        "eligibility_breakdown": eligibility_breakdown,
    }


def compare(training_view: Path, buckets: list[float], *, split: str) -> dict[str, Any]:
    rows = list(common.iter_json_objects(training_view))
    eligible_rows = [row for row in rows if comparison_eligible(row, split=split)]
    snapshot_versions = sorted({str(row.get("snapshot_kind") or "missing") for row in rows})
    feature_sources = sorted({str(row.get("feature_source") or "missing") for row in rows})
    checks = contract_checks(rows, split=split)
    models = [
        model_report(
            eligible_rows,
            model_name="gatekeeper_v25",
            score_fields=("gatekeeper_v25_score", "v25_score", "gatekeeper_score", "score"),
            accept_fields=("gatekeeper_v25_accept", "decision_verdict_buy"),
            verdict_fields=("gatekeeper_verdict", "verdict_type"),
            replay_version_fields=(
                "gatekeeper_v25_replay_artifact_version",
                "v25_replay_artifact_version",
                "replay_artifact_version",
            ),
            buckets=buckets,
        ),
        model_report(
            eligible_rows,
            model_name="gatekeeper_v3",
            score_fields=("gatekeeper_v3_score", "v3_score", "v3_shadow_score"),
            accept_fields=("gatekeeper_v3_accept", "v3_decision_verdict_buy", "v3_shadow_accept"),
            verdict_fields=("gatekeeper_v3_verdict", "v3_verdict", "v3_shadow_verdict"),
            replay_version_fields=(
                "gatekeeper_v3_replay_artifact_version",
                "v3_replay_artifact_version",
                "replay_artifact_version",
            ),
            buckets=buckets,
        ),
    ]
    ok_sets = {
        model["model"]: {
            common.str_or_none(item.get("candidate_id"))
            for item in model["row_results"]
            if item.get("row_status") == "ok" and common.str_or_none(item.get("candidate_id"))
        }
        for model in models
    }
    eligible_ids = {
        common.str_or_none(row.get("candidate_id"))
        for row in eligible_rows
        if common.str_or_none(row.get("candidate_id"))
    }
    model_sets = list(ok_sets.values())
    same_model_candidate_set = bool(eligible_ids) and bool(model_sets) and all(
        item == eligible_ids for item in model_sets
    )
    if (
        eligible_ids
        and not same_model_candidate_set
        and "model_candidate_set_mismatch" not in checks["fail_reasons"]
    ):
        checks["status"] = "NO-GO"
        checks["fail_reasons"] = [*checks["fail_reasons"], "model_candidate_set_mismatch"]
    report = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "gatekeeper_compare_v25_v3_v1",
        "training_view": str(training_view),
        "rows": len(rows),
        "comparison_split": split,
        "comparison_eligible_rows": len(eligible_rows),
        "comparison_contract": {
            "same_candidate_set": checks["same_candidate_set"] and same_model_candidate_set,
            "same_label": "r2_label",
            "same_time_split": checks["same_time_split"],
            "same_eligibility_flags": checks["same_eligibility_flags"],
            "same_feature_cutoff_required": checks["feature_cutoff_present"],
            "same_observation_window_required": checks["observation_window_present"],
            "same_replay_artifact_version_required": checks["same_replay_artifact_version"],
            "snapshot_kinds_seen": snapshot_versions,
            "feature_sources_seen": feature_sources,
        },
        "contract_checks": checks,
        "model_candidate_sets": {
            "eligible_candidate_count": len(eligible_ids),
            "same_model_candidate_set": same_model_candidate_set,
            "ok_candidate_counts": {name: len(ids) for name, ids in sorted(ok_sets.items())},
        },
        "models": models,
    }
    report["status"] = (
        "ok"
        if checks["status"] == "PASS" and all(model["status"] == "ok" for model in report["models"])
        else "NO-GO"
    )
    return report


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--training-view", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--split", default="holdout", help="Temporal split to compare. Default: holdout.")
    parser.add_argument(
        "--accept-rate-bucket",
        type=float,
        action="append",
        default=None,
        help="Top score fraction, e.g. 0.01. Defaults to 1%, 2.5%, 5%, 10%.",
    )
    parser.add_argument("--json", action="store_true")
    return parser


def run(args: argparse.Namespace) -> dict[str, Any]:
    buckets = args.accept_rate_bucket or [0.01, 0.025, 0.05, 0.10]
    report = compare(args.training_view, buckets, split=args.split)
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
