#!/usr/bin/env python3
"""Build decision-time-safe selector feature snapshots from offline events."""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any

import selector_pipeline_common as common


FEATURE_COVERAGE_VIOLATIONS = {
    "feature_snapshot_incomplete",
    "missing_feature_cutoff_ts_ms",
    "missing_feature_cutoff_slot",
    "missing_feature_observed_lag_ms",
    "missing_feature_source",
}


def feature_integrity_violations(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Return leakage/source-after-cutoff violations, excluding explicit coverage gaps."""
    return [
        violation
        for violation in common.feature_temporal_violations(rows)
        if violation.get("violation") not in FEATURE_COVERAGE_VIOLATIONS
    ]


def snapshot_cutoff(candidate: dict[str, Any], snapshot_kind: str) -> int | None:
    if snapshot_kind == "decision":
        return common.int_or_none(candidate.get("decision_ts_ms"))
    birth_ts = common.int_or_none(candidate.get("birth_ts_ms"))
    offset = common.SNAPSHOT_OFFSETS_MS.get(snapshot_kind)
    if birth_ts is None or offset is None:
        return None
    return birth_ts + offset


def build_feature_snapshots(
    *,
    candidate_universe: Path,
    event_paths: list[Path],
    decision_paths: list[Path],
    snapshot_kinds: list[str],
    include_decision_context: bool = False,
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    candidates = list(common.iter_json_objects(candidate_universe))
    events: list[dict[str, Any]] = []
    for path in event_paths:
        events.extend(common.iter_json_objects(path))
    decision_context_rows = 0
    if include_decision_context:
        for path in decision_paths:
            decision_rows = list(common.iter_json_objects(path))
            decision_context_rows += len(decision_rows)
            events.extend(decision_rows)
    events_by_candidate = common.index_events_by_candidate(events, candidates)
    rows: list[dict[str, Any]] = []
    skipped = Counter()
    for candidate in candidates:
        candidate_id = common.str_or_none(candidate.get("candidate_id"))
        if not candidate_id:
            skipped["missing_candidate_id"] += 1
            continue
        for snapshot_kind in snapshot_kinds:
            cutoff = snapshot_cutoff(candidate, snapshot_kind)
            if cutoff is None:
                skipped[f"missing_cutoff:{snapshot_kind}"] += 1
                reason = "missing_decision_cutoff" if snapshot_kind == "decision" else "missing_snapshot_cutoff"
                rows.append(
                    common.build_incomplete_feature_snapshot(
                        candidate,
                        snapshot_kind=snapshot_kind,
                        reason=reason,
                    )
                )
                continue
            rows.append(
                common.build_feature_snapshot(
                    candidate,
                    events_by_candidate.get(candidate_id, []),
                    snapshot_kind=snapshot_kind,
                    cutoff_ts_ms=cutoff,
                )
            )
    kind_counts = Counter(str(row.get("snapshot_kind")) for row in rows)
    status_counts = Counter(str(row.get("feature_snapshot_status") or "missing") for row in rows)
    excluded_counts = Counter(
        str(row.get("feature_snapshot_excluded_reason") or "none") for row in rows
    )
    temporal_violations = common.feature_temporal_violations(rows)
    integrity_violations = feature_integrity_violations(rows)
    usable_feature_rows = sum(1 for row in rows if row.get("feature_snapshot_status") == "ok")
    fail_reasons = []
    if not rows:
        fail_reasons.append("no_feature_snapshot_rows")
    if integrity_violations:
        fail_reasons.append("feature_snapshot_integrity_or_leakage_violation")
    if include_decision_context and decision_paths:
        fail_reasons.append("decision_logs_used_for_feature_rollup")
    manifest = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "feature_snapshots_v1",
        "status": "ok" if rows and not fail_reasons else "NO-GO",
        "fail_reasons": fail_reasons,
        "candidate_universe": str(candidate_universe),
        "input_event_paths": [str(path) for path in event_paths],
        "input_decision_paths": [str(path) for path in decision_paths],
        "decision_context_for_features": include_decision_context,
        "decision_context_rows_loaded": decision_context_rows,
        "feature_source_contract": "event_artifacts_only_by_default; decision_logs_require_explicit_NO_GO_context_mode",
        "snapshot_kinds": snapshot_kinds,
        "candidate_rows": len(candidates),
        "source_event_rows": len(events),
        "rows_written": len(rows),
        "usable_feature_snapshot_rows": usable_feature_rows,
        "feature_snapshot_gate_status": "PASS" if usable_feature_rows > 0 else "NO-GO",
        "snapshot_kind_counts": common.counter_dict(kind_counts),
        "feature_snapshot_status_counts": common.counter_dict(status_counts),
        "feature_snapshot_excluded_reason_counts": common.counter_dict(excluded_counts),
        "skipped_counts": common.counter_dict(skipped),
        "leakage_precheck": "PASS" if not integrity_violations else "NO-GO",
        "leakage_guard": (
            "feature_rows_checked_against_outcome_execution_fields_and_source-after-cutoff; "
            "coverage gaps remain explicit feature_snapshot_incomplete rows"
        ),
        "temporal_violation_count": len(temporal_violations),
        "temporal_violations_sample": temporal_violations[:50],
        "integrity_violation_count": len(integrity_violations),
        "integrity_violations_sample": integrity_violations[:50],
    }
    return rows, manifest


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate-universe", required=True, type=Path)
    parser.add_argument("--events", type=Path, action="append", default=[])
    parser.add_argument("--decisions", type=Path, action="append", default=[])
    parser.add_argument(
        "--include-decision-context",
        action="store_true",
        help="NO-GO mode: mix decision rows into feature rollup for diagnostics only.",
    )
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--manifest-output", type=Path)
    parser.add_argument(
        "--snapshot-kind",
        action="append",
        choices=["birth+5s", "birth+15s", "birth+30s", "birth+60s", "decision"],
        default=None,
    )
    parser.add_argument("--json", action="store_true")
    return parser


def run(args: argparse.Namespace) -> dict[str, Any]:
    snapshot_kinds = args.snapshot_kind or ["birth+5s", "birth+15s", "birth+30s", "birth+60s", "decision"]
    rows, manifest = build_feature_snapshots(
        candidate_universe=args.candidate_universe,
        event_paths=args.events,
        decision_paths=args.decisions,
        snapshot_kinds=snapshot_kinds,
        include_decision_context=args.include_decision_context,
    )
    common.write_jsonl(args.output, rows)
    if args.manifest_output:
        common.write_json(args.manifest_output, manifest)
    return manifest


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    manifest = run(args)
    if args.json:
        print(json.dumps(manifest, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
