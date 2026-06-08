#!/usr/bin/env python3
"""Audit TopK drift between runtime partial and offline full selector scores.

This P3L-C2 audit is offline-only.  It does not train a model, does not change
Gatekeeper, does not change runtime scoring, and does not map missing flow
features into runtime.  It compares:

* runtime partial selector_shadow_score_v1 rows emitted by P3L-B
* offline full score recomputed from the same frozen score spec and a
  same-scope training view that contains flow + GK context features

R2 labels are used only when the supplied training view has a sufficient
resolved R2 denominator.
"""

from __future__ import annotations

import argparse
import csv
import json
from collections import Counter
from pathlib import Path
from typing import Any

import audit_selector_shadow_score_parity as parity
import build_selector_r2only_model_candidate as p3g
import selector_pipeline_common as common


ARTIFACT = "selector_shadow_score_topk_drift_v1"
ROWS_ARTIFACT = "selector_shadow_score_topk_drift_rows_v1"
MD_ARTIFACT = "SELECTOR_SHADOW_SCORE_TOPK_DRIFT"
SCORE_FILE = "selector_shadow_score_v1.jsonl"
DECISION_FILE = "gatekeeper_v2_decisions.jsonl"
EXPECTED_SCORE_VERSION = "selector_shadow_score_combined_simple_v1"
EXPECTED_CANDIDATE_ID = "combined:simple_feature_score_v1"
DEFAULT_RUST_SOURCE = "ghost-brain/src/oracle/decision_logger.rs"
DEFAULT_SELECTOR_SCOPE = "selector-phase1-pumpfun-sol-v1-20260608-r20b-shadow-score-parity-topk-drift"


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime-scope", required=True)
    parser.add_argument("--selector-scope", default=DEFAULT_SELECTOR_SCOPE)
    parser.add_argument("--root", default="/root/Gho")
    parser.add_argument("--decision-plane", default="legacy_live")
    parser.add_argument("--training-view", default=None)
    parser.add_argument("--rust-source", default=DEFAULT_RUST_SOURCE)
    parser.add_argument("--min-r2-resolved-rows", type=int, default=100)
    parser.add_argument("--top-k", type=int, action="append", default=[10, 25, 50])
    parser.add_argument("--min-top25-overlap-rate", type=float, default=0.60)
    parser.add_argument("--max-top25-hit-rate-drop", type=float, default=0.05)
    parser.add_argument("--min-top25-resolved", type=int, default=10)
    parser.add_argument("--output", default=None)
    parser.add_argument("--rows-output", default=None)
    parser.add_argument("--csv-output", default=None)
    parser.add_argument("--md-output", default=None)
    parser.add_argument("--json", action="store_true")
    return parser


def default_training_view(root: Path, selector_scope: str) -> Path:
    return root / "datasets" / "selector" / selector_scope / "selector_training_view_v1.jsonl"


def default_report_path(root: Path, runtime_scope: str) -> Path:
    return root / "reports" / "selector" / runtime_scope / f"{ARTIFACT}.json"


def default_rows_path(root: Path, runtime_scope: str) -> Path:
    return root / "reports" / "selector" / runtime_scope / f"{ROWS_ARTIFACT}.jsonl"


def default_csv_path(root: Path, runtime_scope: str) -> Path:
    return root / "reports" / "selector" / runtime_scope / f"{ROWS_ARTIFACT}.csv"


def default_md_path(root: Path, runtime_scope: str) -> Path:
    return root / "reports" / "selector" / runtime_scope / f"{MD_ARTIFACT}.md"


def score_dirs(root: Path, scope: str, decision_plane: str | None) -> list[Path]:
    decisions_root = root / "logs" / "rollout" / scope / "decisions" / scope
    if not decisions_root.exists():
        return []
    paths = sorted(decisions_root.rglob(SCORE_FILE))
    if decision_plane:
        paths = [path for path in paths if f"/{decision_plane}/" in path.as_posix()]
    return [path.parent for path in paths]


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    return list(common.iter_json_objects(path))


def finite_float(value: Any) -> float | None:
    return parity.numeric(value)


def normalized(value: float | None, spec: dict[str, Any]) -> float:
    if value is None:
        return 0.0
    denom = float(spec["max"]) - float(spec["min"])
    if abs(denom) <= 1e-12:
        return 0.0
    out = (value - float(spec["min"])) / denom
    if float(spec["direction"]) < 0.0:
        out = 1.0 - out
    return max(0.0, min(1.0, out))


def score_training_row(row: dict[str, Any], specs: list[dict[str, Any]]) -> tuple[float, dict[str, Any]]:
    total = 0.0
    missing = []
    present = 0
    flow_present = 0
    gk_present = 0
    for spec in specs:
        name = str(spec["name"])
        value = p3g.feature_value(row, name)
        if value is None:
            missing.append(name)
        else:
            present += 1
            if name.startswith("gk_"):
                gk_present += 1
            else:
                flow_present += 1
        total += normalized(value, spec)
    return total / len(specs), {
        "offline_full_present_feature_count": present,
        "offline_full_missing_feature_count": len(missing),
        "offline_full_missing_features": missing,
        "offline_full_flow_present_feature_count": flow_present,
        "offline_full_gk_present_feature_count": gk_present,
    }


def identity_key(row: dict[str, Any]) -> tuple[str, str, int] | None:
    pool_id = common.str_or_none(row.get("pool_id") or row.get("bonding_curve"))
    base_mint = common.str_or_none(row.get("base_mint") or row.get("mint") or row.get("mint_id"))
    decision_ts = common.int_or_none(row.get("decision_ts_ms"))
    if not pool_id or not base_mint or decision_ts is None:
        return None
    return pool_id, base_mint, decision_ts


def label_positive(row: dict[str, Any] | None) -> bool | None:
    if row is None:
        return None
    label = row.get("r2_label")
    if label == "positive":
        return True
    if label == "negative":
        return False
    return None


def topk(rows: list[dict[str, Any]], score_field: str, k: int) -> list[dict[str, Any]]:
    sortable = [
        row
        for row in rows
        if finite_float(row.get(score_field)) is not None
    ]
    return sorted(
        sortable,
        key=lambda row: (
            -(finite_float(row.get(score_field)) or 0.0),
            str(row.get("pool_id") or ""),
            str(row.get("base_mint") or ""),
            common.int_or_none(row.get("decision_ts_ms")) or 0,
        ),
    )[:k]


def set_keys(rows: list[dict[str, Any]]) -> set[tuple[str, str, int]]:
    keys = set()
    for row in rows:
        key = identity_key(row)
        if key is not None:
            keys.add(key)
    return keys


def topk_metrics(
    *,
    k: int,
    runtime_rows: list[dict[str, Any]],
    offline_rows: list[dict[str, Any]],
) -> dict[str, Any]:
    runtime_top = topk(runtime_rows, "runtime_partial_score", k)
    offline_top = topk(offline_rows, "offline_full_score", k)
    runtime_keys = set_keys(runtime_top)
    offline_keys = set_keys(offline_top)
    overlap = runtime_keys & offline_keys
    runtime_resolved = [row for row in runtime_top if label_positive(row.get("training_row")) is not None]
    offline_resolved = [row for row in offline_top if label_positive(row.get("training_row")) is not None]
    runtime_hits = sum(1 for row in runtime_resolved if label_positive(row.get("training_row")) is True)
    offline_hits = sum(1 for row in offline_resolved if label_positive(row.get("training_row")) is True)
    return {
        "k": k,
        "runtime_count": len(runtime_top),
        "offline_full_count": len(offline_top),
        "overlap_count": len(overlap),
        "overlap_rate": len(overlap) / k if k else None,
        "lost_from_runtime_topk_count": len(offline_keys - runtime_keys),
        "gained_by_runtime_topk_count": len(runtime_keys - offline_keys),
        "runtime_resolved_count": len(runtime_resolved),
        "runtime_positive_count": runtime_hits,
        "runtime_hit_rate": runtime_hits / len(runtime_resolved) if runtime_resolved else None,
        "offline_full_resolved_count": len(offline_resolved),
        "offline_full_positive_count": offline_hits,
        "offline_full_hit_rate": offline_hits / len(offline_resolved) if offline_resolved else None,
        "lost_keys": [":".join(map(str, key)) for key in sorted(offline_keys - runtime_keys)[:50]],
        "gained_keys": [":".join(map(str, key)) for key in sorted(runtime_keys - offline_keys)[:50]],
    }


def claim_boundary_ok(row: dict[str, Any]) -> bool:
    boundaries = row.get("claim_boundaries")
    if not isinstance(boundaries, dict):
        return False
    expected_false = (
        "changes_gatekeeper_decision",
        "changes_execution",
        "send_path_changed",
        "production_promotion_allowed",
    )
    expected_true = ("diagnostic_only", "shadow_only")
    return all(boundaries.get(key) is False for key in expected_false) and all(
        boundaries.get(key) is True for key in expected_true
    )


def build_report(args: argparse.Namespace) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    root = Path(args.root)
    training_view = Path(args.training_view) if args.training_view else default_training_view(root, args.selector_scope)
    specs, thresholds = parity.parse_runtime_spec(root / args.rust_source)
    score_paths = [directory / SCORE_FILE for directory in score_dirs(root, args.runtime_scope, args.decision_plane)]
    if not score_paths:
        raise FileNotFoundError(f"no {SCORE_FILE} found for scope={args.runtime_scope} plane={args.decision_plane}")

    score_rows = [row for path in score_paths for row in read_jsonl(path)]
    training_rows = read_jsonl(training_view)
    training_by_key: dict[tuple[str, str, int], dict[str, Any]] = {}
    ambiguous_training_keys: set[tuple[str, str, int]] = set()
    for row in training_rows:
        key = identity_key(row)
        if key is None:
            continue
        if key in training_by_key:
            ambiguous_training_keys.add(key)
        else:
            training_by_key[key] = row
    for key in ambiguous_training_keys:
        training_by_key.pop(key, None)

    combined_rows: list[dict[str, Any]] = []
    unmatched_score_rows = 0
    claim_boundary_violations = 0
    for score_row in score_rows:
        key = identity_key(score_row)
        training_row = training_by_key.get(key) if key is not None else None
        if training_row is None:
            unmatched_score_rows += 1
            offline_score = None
            availability = {
                "offline_full_present_feature_count": 0,
                "offline_full_missing_feature_count": len(specs),
                "offline_full_missing_features": [str(spec["name"]) for spec in specs],
                "offline_full_flow_present_feature_count": 0,
                "offline_full_gk_present_feature_count": 0,
            }
        else:
            offline_score, availability = score_training_row(training_row, specs)
        if not claim_boundary_ok(score_row):
            claim_boundary_violations += 1
        combined_rows.append(
            {
                "pool_id": score_row.get("pool_id"),
                "base_mint": score_row.get("base_mint"),
                "decision_ts_ms": score_row.get("decision_ts_ms"),
                "runtime_candidate_id": score_row.get("candidate_id"),
                "training_candidate_id": training_row.get("candidate_id") if training_row else None,
                "score_version": score_row.get("score_version"),
                "score_candidate_id": score_row.get("score_candidate_id"),
                "runtime_partial_score": finite_float(score_row.get("selector_shadow_score")),
                "offline_full_score": offline_score,
                "score_delta_runtime_minus_full": (
                    (finite_float(score_row.get("selector_shadow_score")) or 0.0) - offline_score
                    if offline_score is not None and finite_float(score_row.get("selector_shadow_score")) is not None
                    else None
                ),
                "score_validity_status": score_row.get("score_validity_status"),
                "feature_mapping_status": score_row.get("feature_mapping_status"),
                "r2_label": training_row.get("r2_label") if training_row else None,
                "r2_status": training_row.get("r2_status") if training_row else None,
                "r2_resolved": label_positive(training_row) is not None,
                "r2_positive": label_positive(training_row),
                "claim_boundary_ok": claim_boundary_ok(score_row),
                "training_row": training_row,
                **availability,
            }
        )

    r2_resolved_rows = sum(1 for row in training_rows if row.get("r2_only_training_denominator") is True)
    r2_positive_rows = sum(
        1 for row in training_rows if row.get("r2_only_training_denominator") is True and row.get("r2_label") == "positive"
    )
    r2_negative_rows = sum(
        1 for row in training_rows if row.get("r2_only_training_denominator") is True and row.get("r2_label") == "negative"
    )
    topk_reports = [
        topk_metrics(k=k, runtime_rows=combined_rows, offline_rows=combined_rows)
        for k in sorted(set(args.top_k))
    ]
    top25 = next((item for item in topk_reports if item["k"] == 25), None)

    status = "PASS"
    fail_reasons: list[str] = []
    if claim_boundary_violations:
        status = "FAIL"
        fail_reasons.append("claim_boundary_violations")
    if unmatched_score_rows:
        fail_reasons.append("unmatched_score_rows_present")
    if r2_resolved_rows < args.min_r2_resolved_rows:
        verdict = "INSUFFICIENT_R2_LABELS_FOR_DECISION"
    elif top25 is None:
        verdict = "STRUCTURAL_DRIFT_ONLY_NO_LABEL_DECISION"
        fail_reasons.append("missing_top25_report")
    else:
        runtime_rate = top25.get("runtime_hit_rate")
        offline_rate = top25.get("offline_full_hit_rate")
        hit_rate_drop = (
            offline_rate - runtime_rate
            if isinstance(runtime_rate, (int, float)) and isinstance(offline_rate, (int, float))
            else None
        )
        if (
            top25.get("overlap_rate") is not None
            and top25["overlap_rate"] >= args.min_top25_overlap_rate
            and top25.get("runtime_resolved_count", 0) >= args.min_top25_resolved
            and hit_rate_drop is not None
            and hit_rate_drop <= args.max_top25_hit_rate_drop
        ):
            verdict = "FORWARD_SHADOW_READY_WITH_PARTIAL_SCORE"
        else:
            verdict = "FLOW_MAPPING_REQUIRED_BEFORE_FORWARD_SHADOW"

    report = {
        "artifact": ARTIFACT,
        "status": status,
        "fail_reasons": fail_reasons,
        "verdict": verdict,
        "runtime_scope": args.runtime_scope,
        "selector_scope": args.selector_scope,
        "decision_plane": args.decision_plane,
        "score_version": EXPECTED_SCORE_VERSION,
        "score_candidate_id": EXPECTED_CANDIDATE_ID,
        "non_goals": {
            "trained_model": False,
            "changed_gatekeeper": False,
            "changed_runtime_score": False,
            "mapped_flow_features": False,
            "changed_execution": False,
        },
        "inputs": {
            "score_paths": [str(path) for path in score_paths],
            "training_view": str(training_view),
            "rust_source": str(root / args.rust_source),
        },
        "score_rows": len(score_rows),
        "training_rows": len(training_rows),
        "matched_training_rows": len(score_rows) - unmatched_score_rows,
        "unmatched_score_rows": unmatched_score_rows,
        "ambiguous_training_keys": len(ambiguous_training_keys),
        "claim_boundary_violations": claim_boundary_violations,
        "r2_resolved_rows": r2_resolved_rows,
        "r2_positive_rows": r2_positive_rows,
        "r2_negative_rows": r2_negative_rows,
        "min_r2_resolved_rows": args.min_r2_resolved_rows,
        "runtime_missing_flow_features": [
            str(spec["name"]) for spec in specs if spec.get("source") == "MissingRuntimeMapping"
        ],
        "thresholds": thresholds,
        "decision_rule": {
            "min_top25_overlap_rate": args.min_top25_overlap_rate,
            "max_top25_hit_rate_drop": args.max_top25_hit_rate_drop,
            "min_top25_resolved": args.min_top25_resolved,
            "applies_only_when_r2_resolved_rows_at_least": args.min_r2_resolved_rows,
        },
        "topk": topk_reports,
    }
    return report, combined_rows


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            item = {key: value for key, value in row.items() if key != "training_row"}
            fh.write(json.dumps(item, ensure_ascii=False, sort_keys=True) + "\n")


def write_csv_rows(path: Path, rows: list[dict[str, Any]]) -> None:
    fields = [
        "pool_id",
        "base_mint",
        "decision_ts_ms",
        "runtime_candidate_id",
        "training_candidate_id",
        "runtime_partial_score",
        "offline_full_score",
        "score_delta_runtime_minus_full",
        "score_validity_status",
        "feature_mapping_status",
        "r2_label",
        "r2_status",
        "r2_resolved",
        "r2_positive",
        "claim_boundary_ok",
        "offline_full_flow_present_feature_count",
        "offline_full_gk_present_feature_count",
        "offline_full_missing_feature_count",
    ]
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=fields)
        writer.writeheader()
        for row in rows:
            writer.writerow({field: row.get(field) for field in fields})


def write_markdown(path: Path, report: dict[str, Any]) -> None:
    lines = [
        "# Selector Shadow Score TopK Drift",
        "",
        f"Status: {report['status']}",
        f"Verdict: {report['verdict']}",
        f"Runtime scope: `{report['runtime_scope']}`",
        f"Selector scope: `{report['selector_scope']}`",
        "",
        "## Counts",
        "",
        f"- score_rows: {report['score_rows']}",
        f"- matched_training_rows: {report['matched_training_rows']} / {report['score_rows']}",
        f"- r2_resolved_rows: {report['r2_resolved_rows']}",
        f"- r2_positive_rows: {report['r2_positive_rows']}",
        f"- r2_negative_rows: {report['r2_negative_rows']}",
        f"- claim_boundary_violations: {report['claim_boundary_violations']}",
        "",
        "## TopK",
        "",
        "| k | overlap | runtime hits | runtime hit-rate | full hits | full hit-rate |",
        "|---:|---:|---:|---:|---:|---:|",
    ]
    for item in report["topk"]:
        runtime_rate = item.get("runtime_hit_rate")
        full_rate = item.get("offline_full_hit_rate")
        lines.append(
            "| {k} | {overlap}/{k} | {rh}/{rr} | {rrate} | {fh}/{fr} | {frate} |".format(
                k=item["k"],
                overlap=item["overlap_count"],
                rh=item["runtime_positive_count"],
                rr=item["runtime_resolved_count"],
                rrate=f"{runtime_rate:.4f}" if isinstance(runtime_rate, float) else "n/a",
                fh=item["offline_full_positive_count"],
                fr=item["offline_full_resolved_count"],
                frate=f"{full_rate:.4f}" if isinstance(full_rate, float) else "n/a",
            )
        )
    lines.extend(
        [
            "",
            "## Boundaries",
            "",
            "- trained_model: false",
            "- changed_gatekeeper: false",
            "- changed_runtime_score: false",
            "- mapped_flow_features: false",
            "- changed_execution: false",
        ]
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    root = Path(args.root)
    report, rows = build_report(args)
    output = Path(args.output) if args.output else default_report_path(root, args.runtime_scope)
    rows_output = Path(args.rows_output) if args.rows_output else default_rows_path(root, args.runtime_scope)
    csv_output = Path(args.csv_output) if args.csv_output else default_csv_path(root, args.runtime_scope)
    md_output = Path(args.md_output) if args.md_output else default_md_path(root, args.runtime_scope)
    write_json(output, report)
    write_jsonl(rows_output, rows)
    write_csv_rows(csv_output, rows)
    write_markdown(md_output, report)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0 if report["status"] == "PASS" else 2


if __name__ == "__main__":
    raise SystemExit(main())
