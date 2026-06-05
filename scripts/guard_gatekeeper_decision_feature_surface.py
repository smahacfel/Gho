#!/usr/bin/env python3
"""Guard Gatekeeper decision-log feature surface for selector runs.

This is an offline audit guard.  It verifies that a rollout decision log still
emits the decision-time fields consumed by selector training.  It does not fix
runtime logging and does not touch Rust/runtime code.
"""

from __future__ import annotations

import argparse
import glob
import json
from collections import Counter
from pathlib import Path
from typing import Any

import selector_pipeline_common as common


ARTIFACT = "gatekeeper_decision_feature_surface_guard_v1"
DECISION_PLANES = ("v25_shadow", "legacy_live", "auto")

CRITICAL_CURVE_MARKET_FIELDS = (
    "bonding_progress_pct",
    "curve_data_known",
    "current_market_cap_sol",
    "price_change_ratio",
    "observation_duration_ms",
    "curve_wait_ms",
    "curve_wait_elapsed_ms",
)
CONCENTRATION_FIELDS = (
    "hhi",
    "top3_volume_pct",
)
REPORTED_FIELDS = CRITICAL_CURVE_MARKET_FIELDS + (
    "total_tx_evaluated",
    "unique_signers_evaluated",
    "buy_count",
    "buy_ratio",
    "sell_buy_ratio",
) + CONCENTRATION_FIELDS


def present(value: Any) -> bool:
    return value not in (None, "", [])


def load_decision_rows(
    *,
    root: Path,
    source_scope: str,
    decision_plane: str,
) -> tuple[list[dict[str, Any]], list[str], Counter[str], Counter[str]]:
    pattern = str(
        root
        / "logs"
        / "rollout"
        / source_scope
        / "decisions"
        / "**"
        / "gatekeeper_v2_decisions.jsonl"
    )
    paths = sorted(glob.glob(pattern, recursive=True))
    rows: list[dict[str, Any]] = []
    planes: Counter[str] = Counter()
    schemas: Counter[str] = Counter()
    for raw_path in paths:
        for row in common.iter_json_objects(Path(raw_path)):
            planes[str(row.get("decision_plane", "legacy_or_unknown"))] += 1
            schemas[str(row.get("log_schema_version", "unknown"))] += 1
            if decision_plane != "auto" and row.get("decision_plane") != decision_plane:
                continue
            rows.append(row)
    return rows, paths, planes, schemas


def field_presence(rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    out: dict[str, dict[str, Any]] = {}
    for field in REPORTED_FIELDS:
        count = sum(1 for row in rows if present(row.get(field)))
        out[field] = {
            "present_rows": count,
            "decision_rows": len(rows),
            "present_rate": count / len(rows) if rows else 0.0,
        }
    return out


def fail_status(
    *,
    rows: list[dict[str, Any]],
    min_rows: int,
    presence: dict[str, dict[str, Any]],
    curve_metric_min_present_rate: float,
    concentration_metric_min_present_rate: float,
) -> tuple[str, list[str]]:
    fail_reasons: list[str] = []
    if not rows:
        return "FAIL_NO_DECISION_LOGS", ["no_decision_rows"]
    if len(rows) < min_rows:
        fail_reasons.append("decision_rows_below_min_rows")
    zero_critical = [
        field
        for field in CRITICAL_CURVE_MARKET_FIELDS
        if int(presence.get(field, {}).get("present_rows") or 0) == 0
    ]
    if zero_critical:
        fail_reasons.append("missing_required_curve_metrics:" + ",".join(zero_critical))
        return "FAIL_NO_REQUIRED_CURVE_METRICS", fail_reasons
    low_curve = [
        field
        for field in CRITICAL_CURVE_MARKET_FIELDS
        if float(presence.get(field, {}).get("present_rate") or 0.0)
        < curve_metric_min_present_rate
    ]
    if low_curve:
        fail_reasons.append("low_curve_metric_coverage:" + ",".join(low_curve))
        return "FAIL_LOW_CURVE_METRIC_COVERAGE", fail_reasons
    low_concentration = [
        field
        for field in CONCENTRATION_FIELDS
        if float(presence.get(field, {}).get("present_rate") or 0.0)
        < concentration_metric_min_present_rate
    ]
    if low_concentration:
        fail_reasons.append("low_concentration_coverage:" + ",".join(low_concentration))
        return "FAIL_LOW_CONCENTRATION_COVERAGE", fail_reasons
    if fail_reasons:
        return "FAIL_LOW_CURVE_METRIC_COVERAGE", fail_reasons
    return "PASS", []


def build_guard(args: argparse.Namespace) -> dict[str, Any]:
    rows, paths, planes, schemas = load_decision_rows(
        root=args.root,
        source_scope=args.source_scope,
        decision_plane=args.decision_plane,
    )
    presence = field_presence(rows)
    status, fail_reasons = fail_status(
        rows=rows,
        min_rows=args.min_rows,
        presence=presence,
        curve_metric_min_present_rate=args.curve_metric_min_present_rate,
        concentration_metric_min_present_rate=args.concentration_metric_min_present_rate,
    )
    warning_reasons = []
    for field in ("total_tx_evaluated", "unique_signers_evaluated", "buy_count", "buy_ratio", "sell_buy_ratio"):
        if rows and float(presence.get(field, {}).get("present_rate") or 0.0) == 0.0:
            warning_reasons.append(f"{field}_absent")
    report = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": ARTIFACT,
        "status": status,
        "source_scope": args.source_scope,
        "decision_plane": args.decision_plane,
        "min_rows": args.min_rows,
        "curve_metric_min_present_rate": args.curve_metric_min_present_rate,
        "concentration_metric_min_present_rate": args.concentration_metric_min_present_rate,
        "decision_log_paths": paths,
        "decision_rows": len(rows),
        "schemas": common.counter_dict(schemas),
        "planes": common.counter_dict(planes),
        "field_presence": presence,
        "fail_reasons": fail_reasons,
        "warning_reasons": warning_reasons,
        "claim_boundaries": {
            "offline_guard_only": True,
            "runtime_changed": False,
            "rust_changed": False,
            "logging_repair_claim": False,
            "selector_training_changed": False,
        },
    }
    output = args.root / "reports" / "selector" / args.source_scope / f"{ARTIFACT}.json"
    common.write_json(output, report)
    report["output"] = str(output)
    common.write_json(output, report)
    return report


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-scope", required=True)
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument("--decision-plane", choices=DECISION_PLANES, default="v25_shadow")
    parser.add_argument("--min-rows", type=int, default=100)
    parser.add_argument("--curve-metric-min-present-rate", type=float, default=0.95)
    parser.add_argument("--concentration-metric-min-present-rate", type=float, default=0.60)
    parser.add_argument("--json", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = build_guard(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0 if report.get("status") == "PASS" else 2


if __name__ == "__main__":
    raise SystemExit(main())
