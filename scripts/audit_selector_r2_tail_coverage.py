#!/usr/bin/env python3
"""Audit post-decision R2 tail coverage for an offline Gatekeeper edge candidate.

This audit is intentionally read-only. It does not rebuild labels, tune the
candidate, change Gatekeeper, alter execution, or promote any policy fork. Its
job is to explain whether an otherwise promising candidate is blocked by missing
canonical AccountUpdate evidence after the terminal decision timestamp.
"""

from __future__ import annotations

import argparse
import csv
import json
from collections import Counter
from pathlib import Path
from typing import Any, Iterable

import selector_pipeline_common as common


ARTIFACT = "selector_r2_tail_coverage_audit_v1"
DEFAULT_JSON_NAME = f"{ARTIFACT}.json"
DEFAULT_ROWS_NAME = "selector_r2_tail_coverage_audit_rows_v1.csv"
SELECTED_VERDICT = "WOULD_ALLOW_R2_OPPORTUNITY_NOT_EXECUTION_SAFE"
RESOLVED_STATUSES = {"positive", "negative", "resolved"}
NON_CLAIMS = {
    "runtime_changed": False,
    "gatekeeper_changed": False,
    "execution_changed": False,
    "send_path_changed": False,
    "thresholds_tuned": False,
    "candidate_changed": False,
    "labels_rebuilt": False,
    "production_promotion_allowed": False,
}


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="/root/Gho")
    parser.add_argument("--selector-scope", required=True)
    parser.add_argument("--selected-rows-csv", default=None)
    parser.add_argument("--candidate-universe", default=None)
    parser.add_argument("--canonical-r2-source", default=None)
    parser.add_argument("--r2-market-paths", default=None)
    parser.add_argument("--output", default=None)
    parser.add_argument("--rows-output", default=None)
    parser.add_argument("--selected-verdict", default=SELECTED_VERDICT)
    parser.add_argument("--min-resolved-rows", type=int, default=100)
    parser.add_argument("--horizon-ms", type=int, default=60_000)
    parser.add_argument("--short-tail-ms", type=int, default=20_000)
    parser.add_argument("--near-horizon-ms", type=int, default=55_000)
    parser.add_argument("--json", action="store_true")
    return parser


def selector_dataset_dir(root: Path, scope: str) -> Path:
    return root / "datasets" / "selector" / scope


def selector_report_dir(root: Path, scope: str) -> Path:
    return root / "reports" / "selector" / scope


def default_paths(root: Path, scope: str, args: argparse.Namespace) -> dict[str, Path]:
    dataset_dir = selector_dataset_dir(root, scope)
    report_dir = selector_report_dir(root, scope)
    return {
        "candidate_universe": Path(args.candidate_universe)
        if args.candidate_universe
        else dataset_dir / "candidate_universe_v1.jsonl",
        "canonical_r2_source": Path(args.canonical_r2_source)
        if args.canonical_r2_source
        else dataset_dir / "canonical_r2_source_v1.jsonl",
        "r2_market_paths": Path(args.r2_market_paths)
        if args.r2_market_paths
        else dataset_dir / "r2_market_paths_v1.jsonl",
        "selected_rows_csv": Path(args.selected_rows_csv)
        if args.selected_rows_csv
        else discover_selected_rows_csv(report_dir),
        "output": Path(args.output) if args.output else report_dir / DEFAULT_JSON_NAME,
        "rows_output": Path(args.rows_output) if args.rows_output else report_dir / DEFAULT_ROWS_NAME,
    }


def discover_selected_rows_csv(report_dir: Path) -> Path:
    matches = sorted(report_dir.glob("gatekeeper_edge_policy_fork*_rows_v1.csv"))
    if not matches:
        raise SystemExit(
            "--selected-rows-csv is required when no gatekeeper_edge_policy_fork*_rows_v1.csv exists"
        )
    if len(matches) > 1:
        return max(matches, key=lambda path: path.stat().st_mtime)
    return matches[0]


def read_jsonl_by_candidate(path: Path) -> dict[str, dict[str, Any]]:
    rows: dict[str, dict[str, Any]] = {}
    for row in common.iter_json_objects(path):
        candidate_id = common.str_or_none(row.get("candidate_id"))
        if candidate_id:
            rows[candidate_id] = row
    return rows


def read_selected_rows(path: Path, selected_verdict: str) -> list[dict[str, Any]]:
    selected: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8", errors="ignore", newline="") as fh:
        reader = csv.DictReader(fh)
        for row in reader:
            if "policy_fork_verdict" not in row or row.get("policy_fork_verdict") == selected_verdict:
                if row.get("candidate_id"):
                    selected.append(dict(row))
    return selected


def int_value(row: dict[str, Any] | None, *fields: str) -> int | None:
    if not row:
        return None
    for field in fields:
        value = common.int_or_none(row.get(field))
        if value is not None:
            return value
    return None


def str_value(row: dict[str, Any] | None, *fields: str) -> str | None:
    if not row:
        return None
    for field in fields:
        value = common.str_or_none(row.get(field))
        if value:
            return value
    return None


def r2_class(row: dict[str, Any] | None, selected_row: dict[str, Any]) -> str:
    label = str_value(row, "r2_label") or str_value(selected_row, "r2_label")
    status = str_value(row, "r2_status") or str_value(selected_row, "r2_class")
    if label in {"positive", "negative"}:
        return label
    if status in {"positive", "negative"}:
        return status
    if status:
        return status
    return "unknown"


def max_sample_offset(canonical_row: dict[str, Any] | None) -> int | None:
    samples = canonical_row.get("samples") if isinstance(canonical_row, dict) else None
    offsets: list[int] = []
    if isinstance(samples, list):
        for sample in samples:
            if isinstance(sample, dict):
                offset = common.int_or_none(sample.get("offset_ms"))
                if offset is not None:
                    offsets.append(offset)
    return max(offsets) if offsets else None


def source_sample_count(canonical_row: dict[str, Any] | None) -> int | None:
    value = int_value(canonical_row, "source_record_count")
    if value is not None:
        return value
    samples = canonical_row.get("samples") if isinstance(canonical_row, dict) else None
    if isinstance(samples, list):
        return len(samples)
    return None


def classify_gap(
    *,
    status: str,
    canonical_row: dict[str, Any] | None,
    max_offset_ms: int | None,
    short_tail_ms: int,
    near_horizon_ms: int,
) -> str:
    if status in {"positive", "negative"}:
        return "resolved"
    if canonical_row is None or status == "missing_path":
        return "no_post_decision_canonical_samples"
    path_status = str_value(canonical_row, "path_status") or status
    if path_status == "horizon_unmatured":
        if max_offset_ms is None:
            return "post_decision_tail_unknown_length"
        if max_offset_ms < short_tail_ms:
            return "post_decision_tail_short"
        if max_offset_ms < near_horizon_ms:
            return "post_decision_tail_partial"
        return "post_decision_tail_near_horizon_but_unmatured"
    if path_status == "stream_incomplete":
        return "post_decision_stream_incomplete"
    return f"unresolved_{path_status}"


def quantiles(values: Iterable[int]) -> dict[str, int | None]:
    ordered = sorted(values)
    if not ordered:
        return {"min": None, "p25": None, "p50": None, "p75": None, "p90": None, "p95": None, "max": None}

    def q(frac: float) -> int:
        return ordered[min(len(ordered) - 1, int((len(ordered) - 1) * frac))]

    return {
        "min": ordered[0],
        "p25": q(0.25),
        "p50": q(0.50),
        "p75": q(0.75),
        "p90": q(0.90),
        "p95": q(0.95),
        "max": ordered[-1],
    }


def build_audit(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.root)
    paths = default_paths(root, args.selector_scope, args)
    selected_rows = read_selected_rows(paths["selected_rows_csv"], args.selected_verdict)
    candidates = read_jsonl_by_candidate(paths["candidate_universe"])
    canonical_rows = read_jsonl_by_candidate(paths["canonical_r2_source"])
    r2_rows = read_jsonl_by_candidate(paths["r2_market_paths"])

    seen = Counter(str(row.get("candidate_id") or "") for row in selected_rows)
    duplicate_selected_extra_rows = sum(max(count - 1, 0) for count in seen.values())
    row_outputs: list[dict[str, Any]] = []
    status_counts: Counter[str] = Counter()
    gap_counts: Counter[str] = Counter()
    max_offsets: list[int] = []
    unresolved_max_offsets: list[int] = []
    sample_counts: list[int] = []
    unresolved_sample_counts: list[int] = []
    positive = 0
    negative = 0

    for selected in selected_rows:
        candidate_id = str(selected.get("candidate_id") or "")
        candidate = candidates.get(candidate_id, {})
        canonical = canonical_rows.get(candidate_id)
        r2 = r2_rows.get(candidate_id)
        status = r2_class(r2, selected)
        offset = max_sample_offset(canonical)
        sample_count = source_sample_count(canonical)
        gap_class = classify_gap(
            status=status,
            canonical_row=canonical,
            max_offset_ms=offset,
            short_tail_ms=args.short_tail_ms,
            near_horizon_ms=args.near_horizon_ms,
        )
        status_counts[status] += 1
        gap_counts[gap_class] += 1
        if offset is not None:
            max_offsets.append(offset)
            if gap_class != "resolved":
                unresolved_max_offsets.append(offset)
        if sample_count is not None:
            sample_counts.append(sample_count)
            if gap_class != "resolved":
                unresolved_sample_counts.append(sample_count)
        if status == "positive":
            positive += 1
        elif status == "negative":
            negative += 1

        decision_ts_ms = int_value(r2, "decision_ts_ms") or int_value(candidate, "decision_ts_ms") or int_value(selected, "decision_ts_ms")
        path_start_ts_ms = int_value(r2, "path_start_ts_ms")
        path_end_ts_ms = int_value(r2, "path_end_ts_ms")
        row_outputs.append(
            {
                "candidate_id": candidate_id,
                "pool_id": str_value(selected, "pool_id") or str_value(candidate, "pool_id"),
                "base_mint": str_value(selected, "base_mint") or str_value(candidate, "base_mint", "mint_id"),
                "decision_ts_ms": decision_ts_ms,
                "r2_status": status,
                "r2_excluded_reason": str_value(r2, "r2_excluded_reason"),
                "gap_class": gap_class,
                "canonical_path_status": str_value(canonical, "path_status"),
                "canonical_source_record_count": sample_count,
                "canonical_total_updates_for_identity": int_value(canonical, "source_update_count_total_for_identity"),
                "max_post_decision_sample_offset_ms": offset,
                "path_start_ts_ms": path_start_ts_ms,
                "path_end_ts_ms": path_end_ts_ms,
                "decision_to_path_start_gap_ms": (
                    path_start_ts_ms - decision_ts_ms
                    if path_start_ts_ms is not None and decision_ts_ms is not None
                    else None
                ),
                "path_tail_ms": (
                    path_end_ts_ms - decision_ts_ms
                    if path_end_ts_ms is not None and decision_ts_ms is not None
                    else offset
                ),
            }
        )

    resolved = positive + negative
    selected_total = len(selected_rows)
    precision = positive / resolved if resolved else None
    missing_for_guard = max(args.min_resolved_rows - resolved, 0)
    unresolved_available = selected_total - resolved
    coverage_verdict = (
        "TAIL_COVERAGE_OK_FOR_EDGE_VALIDATION"
        if resolved >= args.min_resolved_rows
        else "TAIL_COVERAGE_BLOCKED_FOR_EDGE_VALIDATION"
    )
    business_decision = (
        "LABEL_COVERAGE_OK_FOR_EDGE_REVIEW"
        if resolved >= args.min_resolved_rows
        else "DO_NOT_PROMOTE_EDGE_CANDIDATE_UNTIL_LABEL_COVERAGE_REPAIRED"
    )
    report = {
        "artifact": ARTIFACT,
        "status": "PASS",
        "selector_scope": args.selector_scope,
        "coverage_verdict": coverage_verdict,
        "business_decision": business_decision,
        "selected_verdict": args.selected_verdict,
        "horizon_ms": args.horizon_ms,
        "min_resolved_rows": args.min_resolved_rows,
        "inputs": {key: str(value) for key, value in paths.items() if key not in {"output", "rows_output"}},
        "outputs": {"json": str(paths["output"]), "rows_csv": str(paths["rows_output"])},
        "metrics": {
            "selected_rows": selected_total,
            "selected_unique_candidate_ids": len(seen),
            "selected_duplicate_extra_rows": duplicate_selected_extra_rows,
            "selected_resolved_rows": resolved,
            "selected_positive_rows": positive,
            "selected_negative_rows": negative,
            "selected_precision": precision,
            "selected_unresolved_rows": unresolved_available,
            "selected_resolved_rows_needed_for_guard": missing_for_guard,
            "selected_unresolved_rows_available_for_repair": unresolved_available,
        },
        "r2_status_counts": common.counter_dict(status_counts),
        "gap_class_counts": common.counter_dict(gap_counts),
        "post_decision_max_sample_offset_ms": quantiles(max_offsets),
        "post_decision_unresolved_max_sample_offset_ms": quantiles(unresolved_max_offsets),
        "canonical_source_record_count": quantiles(sample_counts),
        "canonical_unresolved_source_record_count": quantiles(unresolved_sample_counts),
        "diagnosis": {
            "primary_blocker": (
                "post_decision_canonical_account_update_tail_coverage"
                if missing_for_guard > 0
                else None
            ),
            "repair_target": (
                "capture_or_reconstruct_60s_canonical_account_state_path_after_terminal_decision_for_selected_unresolved_rows"
                if missing_for_guard > 0
                else None
            ),
            "model_or_threshold_change_required": False,
            "frozen_candidate_preserved": True,
        },
        "non_claims": dict(NON_CLAIMS),
    }
    write_rows(paths["rows_output"], row_outputs)
    common.write_json(paths["output"], report)
    return report


def write_rows(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fieldnames = [
        "candidate_id",
        "pool_id",
        "base_mint",
        "decision_ts_ms",
        "r2_status",
        "r2_excluded_reason",
        "gap_class",
        "canonical_path_status",
        "canonical_source_record_count",
        "canonical_total_updates_for_identity",
        "max_post_decision_sample_offset_ms",
        "path_start_ts_ms",
        "path_end_ts_ms",
        "decision_to_path_start_gap_ms",
        "path_tail_ms",
    ]
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames)
        writer.writeheader()
        for row in rows:
            writer.writerow(row)


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = build_audit(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
