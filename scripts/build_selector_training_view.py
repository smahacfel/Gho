#!/usr/bin/env python3
"""Join selector universe, features, lifecycle, and R2 path labels."""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any

import selector_pipeline_common as common


def index_feature_rows(rows: list[dict[str, Any]]) -> dict[tuple[str, str], dict[str, Any]]:
    indexed: dict[tuple[str, str], dict[str, Any]] = {}
    for row in rows:
        candidate_id = common.str_or_none(row.get("candidate_id"))
        snapshot_kind = common.str_or_none(row.get("snapshot_kind"))
        if candidate_id and snapshot_kind:
            indexed[(candidate_id, snapshot_kind)] = row
    return indexed


def choose_feature(
    candidate_id: str,
    feature_index: dict[tuple[str, str], dict[str, Any]],
    *,
    snapshot_kind: str,
    fallback_snapshot_kind: str,
) -> dict[str, Any] | None:
    return feature_index.get((candidate_id, snapshot_kind)) or feature_index.get(
        (candidate_id, fallback_snapshot_kind)
    )


def index_price_paths(rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    indexed: dict[str, dict[str, Any]] = {}
    for row in rows:
        for field in ("candidate_id", "execution_candidate_id", "ab_record_id"):
            value = common.str_or_none(row.get(field))
            if value:
                indexed.setdefault(value, row)
    return indexed


def leakage_audit(feature_rows: list[dict[str, Any]]) -> dict[str, Any]:
    violations = common.feature_temporal_violations(feature_rows)
    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "leakage_audit_v1",
        "status": "PASS" if not violations else "NO-GO",
        "rows_checked": len(feature_rows),
        "violation_count": len(violations),
        "violations": violations[:50],
    }


def build_training_view(
    *,
    candidate_universe: Path,
    accepted_lifecycle: Path,
    feature_snapshots: Path,
    price_paths: Path | None,
    target_net_pct: float,
    stop_net_pct: float,
    horizon_ms: int,
    snapshot_kind: str,
    fallback_snapshot_kind: str,
) -> tuple[list[dict[str, Any]], dict[str, Any], dict[str, Any]]:
    candidates = list(common.iter_json_objects(candidate_universe))
    accepted_rows = list(common.iter_json_objects(accepted_lifecycle))
    lifecycle_by_candidate = common.index_rows_by_candidate(accepted_rows)
    feature_rows = list(common.iter_json_objects(feature_snapshots))
    feature_index = index_feature_rows(feature_rows)
    price_by_key = index_price_paths(list(common.iter_json_objects(price_paths)))
    splits = common.choose_temporal_split(candidates)

    rows: list[dict[str, Any]] = []
    universe_candidate_ids = {
        common.str_or_none(candidate.get("candidate_id"))
        for candidate in candidates
        if common.str_or_none(candidate.get("candidate_id"))
    }
    accepted_total = len(accepted_rows)
    accepted_joined = sum(
        1
        for row in accepted_rows
        if common.str_or_none(row.get("candidate_id")) in universe_candidate_ids
    )
    accepted_missing_candidate_id = sum(
        1 for row in accepted_rows if not common.str_or_none(row.get("candidate_id"))
    )
    for candidate in candidates:
        candidate_id = common.str_or_none(candidate.get("candidate_id"))
        if not candidate_id:
            continue
        feature = choose_feature(
            candidate_id,
            feature_index,
            snapshot_kind=snapshot_kind,
            fallback_snapshot_kind=fallback_snapshot_kind,
        )
        lifecycle = lifecycle_by_candidate.get(candidate_id)
        price_path = price_by_key.get(candidate_id)
        feature_complete = bool(feature and feature.get("feature_snapshot_status") == "ok")
        r2 = common.classify_r2(
            price_path,
            target_net_pct=target_net_pct,
            stop_net_pct=stop_net_pct,
            horizon_ms=horizon_ms,
        )
        row: dict[str, Any] = {
            "selector_schema_version": common.SCHEMA_VERSION,
            "training_view_schema_version": common.SCHEMA_VERSION,
            "candidate_id": candidate_id,
            "base_mint": candidate.get("base_mint") or candidate.get("mint_id"),
            "pool_id": candidate.get("pool_id"),
            "bonding_curve": candidate.get("bonding_curve"),
            "quote_mint": candidate.get("quote_mint"),
            "birth_ts_ms": candidate.get("birth_ts_ms"),
            "decision_ts_ms": candidate.get("decision_ts_ms"),
            "target_net_pct": target_net_pct,
            "stop_net_pct": stop_net_pct,
            "horizon_ms": horizon_ms,
            "observation_window_ms": horizon_ms,
            "split": splits.get(candidate_id, "holdout"),
            "cohort_in_scope": candidate.get("cohort_in_scope") is True,
            "stream_completeness_ok": (
                candidate.get("stream_completeness_ok") is True
                and r2.get("r2_status") not in {"stream_incomplete", "missing_path"}
            ),
            "eligible": (
                candidate.get("candidate_universe_status") == "ok"
                and candidate.get("cohort_in_scope") is True
                and feature_complete
            ),
            "feature_snapshot_complete": feature_complete,
            "candidate_universe_status": candidate.get("candidate_universe_status"),
            "gatekeeper_verdict": candidate.get("gatekeeper_verdict"),
            "decision_verdict_buy": candidate.get("decision_verdict_buy"),
            "decision_reason": candidate.get("decision_reason"),
            "accepted_lifecycle_joined": lifecycle is not None,
            "execution_only_failure": False,
            "label_resolved": r2.get("r2_label") in {"positive", "negative"},
        }
        if feature:
            for key, value in feature.items():
                if key in {"selector_schema_version", "feature_snapshot_schema_version"}:
                    continue
                row[key] = value
        else:
            row.update(
                {
                    "snapshot_kind": None,
                    "feature_cutoff_ts_ms": None,
                    "feature_cutoff_slot": None,
                    "feature_source": None,
                    "feature_observed_lag_ms": None,
                    "feature_snapshot_status": "missing_feature_snapshot",
                    "feature_snapshot_incomplete_reason": "missing_feature_snapshot",
                    "feature_time_provenance_ok": False,
                }
            )
        if lifecycle:
            for key in (
                "r1_label",
                "r1_label_reason",
                "r1_excluded_reason",
                "r1_gray_reason",
                "execution_realized",
                "close_reason",
                "truth_status",
                "truth_source",
                "final_pnl_pct",
            ):
                row[key] = lifecycle.get(key)
        row.update(r2)
        row["label_excluded_reason"] = row.get("r2_excluded_reason") or row.get("r1_excluded_reason")
        rows.append(row)

    label_counts = Counter(str(row.get("r2_label") or row.get("r2_status") or "unknown") for row in rows)
    denominator_rows = [
        row
        for row in rows
        if row.get("cohort_in_scope")
        and row.get("stream_completeness_ok")
        and row.get("label_resolved")
        and row.get("r2_label") in {"positive", "negative"}
    ]
    accepted_join_completeness = accepted_joined / accepted_total if accepted_total else 1.0
    coverage_fail_reasons = []
    if not denominator_rows:
        coverage_fail_reasons.append("no_resolved_r2_denominator")
    if accepted_join_completeness < 0.99:
        coverage_fail_reasons.append("accepted_lifecycle_join_completeness_below_99pct")
    coverage = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "label_coverage_v1",
        "status": "ok" if not coverage_fail_reasons else "NO-GO",
        "fail_reasons": coverage_fail_reasons,
        "candidate_rows": len(candidates),
        "training_rows": len(rows),
        "accepted_lifecycle_rows": accepted_total,
        "accepted_lifecycle_joined": accepted_joined,
        "accepted_lifecycle_missing_candidate_id": accepted_missing_candidate_id,
        "accepted_lifecycle_join_completeness": accepted_join_completeness,
        "accepted_lifecycle_join_gate": {
            "required_min": 0.99,
            "status": "PASS" if accepted_join_completeness >= 0.99 else "NO-GO",
        },
        "resolved_r2_rows": len(denominator_rows),
        "r2_label_counts": common.counter_dict(label_counts),
        "matured_r2_resolved_rate": (
            len(denominator_rows) / len(rows) if rows else None
        ),
        "r2_ssot_contract": "Yellowstone/Geyser AccountUpdates, DIAG_ACCOUNT_UPDATE_RELAY, canonical account-state snapshots; RPC only flagged backfill/enrichment.",
        "precision_r2_denominator_contract": common.PRECISION_R2_DENOMINATOR_CONTRACT,
        "precision_r2_holdout_denominator": common.r2_counts(
            dict(row, selector_accept=row.get("decision_verdict_buy") is True) for row in rows
        ),
    }
    return rows, coverage, leakage_audit(feature_rows)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate-universe", required=True, type=Path)
    parser.add_argument("--accepted-lifecycle", required=True, type=Path)
    parser.add_argument("--feature-snapshots", required=True, type=Path)
    parser.add_argument("--price-paths", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--label-coverage-output", type=Path)
    parser.add_argument("--leakage-audit-output", type=Path)
    parser.add_argument("--target-net-pct", required=True, type=float)
    parser.add_argument("--stop-net-pct", required=True, type=float)
    parser.add_argument("--horizon-ms", required=True, type=int)
    parser.add_argument("--snapshot-kind", default="decision")
    parser.add_argument("--fallback-snapshot-kind", default="birth+30s")
    parser.add_argument("--json", action="store_true")
    return parser


def run(args: argparse.Namespace) -> dict[str, Any]:
    rows, coverage, audit = build_training_view(
        candidate_universe=args.candidate_universe,
        accepted_lifecycle=args.accepted_lifecycle,
        feature_snapshots=args.feature_snapshots,
        price_paths=args.price_paths,
        target_net_pct=args.target_net_pct,
        stop_net_pct=args.stop_net_pct,
        horizon_ms=args.horizon_ms,
        snapshot_kind=args.snapshot_kind,
        fallback_snapshot_kind=args.fallback_snapshot_kind,
    )
    common.write_jsonl(args.output, rows)
    if args.label_coverage_output:
        common.write_json(args.label_coverage_output, coverage)
    if args.leakage_audit_output:
        common.write_json(args.leakage_audit_output, audit)
    return {"label_coverage": coverage, "leakage_audit": audit}


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    summary = run(args)
    if args.json:
        print(json.dumps(summary, ensure_ascii=False, sort_keys=True))
    return (
        0
        if summary["leakage_audit"]["status"] == "PASS"
        and summary["label_coverage"]["status"] == "ok"
        else 2
    )


if __name__ == "__main__":
    raise SystemExit(main())
