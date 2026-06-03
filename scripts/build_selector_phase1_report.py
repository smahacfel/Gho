#!/usr/bin/env python3
"""Build Phase 1 selector join coverage and dataset manifest artifacts."""

from __future__ import annotations

import argparse
import hashlib
import json
from collections import Counter
from pathlib import Path
from typing import Any

import selector_pipeline_common as common


def file_provenance(path: Path | None) -> dict[str, Any]:
    if path is None:
        return {"path": None, "exists": False}
    payload: dict[str, Any] = {"path": str(path), "exists": path.exists()}
    if not path.exists() or not path.is_file():
        return payload
    digest = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1024 * 1024), b""):
            digest.update(chunk)
    payload.update(
        {
            "size_bytes": path.stat().st_size,
            "sha256": digest.hexdigest(),
        }
    )
    return payload


def read_json(path: Path | None) -> dict[str, Any]:
    if path is None or not path.exists():
        return {}
    with path.open(encoding="utf-8") as fh:
        payload = json.load(fh)
    if not isinstance(payload, dict):
        raise ValueError(f"expected JSON object in {path}")
    return payload


def build_phase1_join_coverage(
    *,
    candidate_universe: Path,
    accepted_lifecycle: Path,
    candidate_manifest: Path | None,
    accepted_manifest: Path | None,
    window_start_ms: int | None = None,
    window_end_ms: int | None = None,
    allow_r2_universe_only: bool = False,
) -> dict[str, Any]:
    candidates = list(common.iter_json_objects(candidate_universe))
    accepted_rows = list(common.iter_json_objects(accepted_lifecycle))
    candidate_report = read_json(candidate_manifest)
    accepted_report = read_json(accepted_manifest)

    indexed, ambiguous = common.build_identity_join_index(candidates)
    joined = 0
    unmatched = 0
    ambiguous_count = 0
    exact_candidate_id_joined = 0
    joined_key_counts: Counter[str] = Counter()
    unmatched_samples: list[dict[str, Any]] = []
    ambiguous_samples: list[dict[str, Any]] = []
    lifecycle_status_counts: Counter[str] = Counter()

    for row in accepted_rows:
        lifecycle_status_counts[str(row.get("lifecycle_status") or row.get("analysis_status") or "unknown")] += 1
        matched, join_key, saw_ambiguous = common.lookup_identity_join(row, indexed, ambiguous)
        if matched is not None:
            joined += 1
            if join_key:
                joined_key_counts[join_key.split(":", 1)[0]] += 1
                if join_key.startswith("candidate_id:"):
                    exact_candidate_id_joined += 1
            continue
        if saw_ambiguous:
            ambiguous_count += 1
            if len(ambiguous_samples) < 20:
                ambiguous_samples.append(
                    {
                        "candidate_id": row.get("candidate_id"),
                        "base_mint": row.get("base_mint") or row.get("mint_id"),
                        "pool_id": row.get("pool_id"),
                    }
                )
        else:
            unmatched += 1
            if len(unmatched_samples) < 20:
                unmatched_samples.append(
                    {
                        "candidate_id": row.get("candidate_id"),
                        "base_mint": row.get("base_mint") or row.get("mint_id"),
                        "pool_id": row.get("pool_id"),
                    }
                )

    accepted_rows_count = len(accepted_rows)
    completeness = joined / accepted_rows_count if accepted_rows_count else 0.0
    status_counts = Counter(str(row.get("candidate_universe_status") or "unknown") for row in candidates)
    fail_reasons = []
    if not candidates:
        fail_reasons.append("candidate_universe_no_rows")
    if candidate_report.get("status") != "ok":
        fail_reasons.append("candidate_universe_not_ok")
    if candidate_report.get("identity_collisions"):
        fail_reasons.append("identity_collisions")
    if status_counts.get("universe_incomplete", 0) > 0:
        fail_reasons.append("universe_incomplete_rows")
    if candidate_report.get("decision_logs_created_denominator_rows", 0) != 0:
        fail_reasons.append("decision_logs_created_denominator_rows_nonzero")
    if not accepted_rows and not allow_r2_universe_only:
        fail_reasons.append("accepted_lifecycle_no_rows")
    if accepted_rows and completeness < 0.99:
        fail_reasons.append("accepted_lifecycle_join_completeness_below_99pct")

    accepted_resolved = sum(1 for row in accepted_rows if row.get("label_resolved") is True)
    accepted_pending = sum(1 for row in accepted_rows if row.get("lifecycle_status") == "pending_horizon_cutoff")
    accepted_unresolved = accepted_rows_count - accepted_resolved - accepted_pending

    if allow_r2_universe_only and not accepted_rows and not fail_reasons:
        status = "PASS_FOR_R2_UNIVERSE_ONLY"
    elif not fail_reasons:
        status = "PASS"
    elif allow_r2_universe_only and not accepted_rows and "accepted_lifecycle_no_rows" in fail_reasons:
        status = "NO-GO"
    else:
        status = "NO-GO"

    phase3_precision_readiness = (
        "NO-GO_NO_ACCEPTED_LIFECYCLE"
        if status == "PASS_FOR_R2_UNIVERSE_ONLY"
        else ("PENDING_PHASE2_R2_DENOMINATOR" if status == "PASS" else "NO-GO")
    )

    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "phase1_join_coverage_v1",
        "phase": "phase1",
        "scope_kind": "windowed" if window_start_ms is not None or window_end_ms is not None else "full",
        "window_start_ts_ms": window_start_ms,
        "window_end_ts_ms": window_end_ms,
        "status": status,
        "phase1_gate_status": status,
        "fail_reasons": fail_reasons,
        "allow_r2_universe_only": allow_r2_universe_only,
        "phase3_precision_readiness": phase3_precision_readiness,
        "candidate_universe_rows": len(candidates),
        "candidate_universe_ok_rows": status_counts.get("ok", 0),
        "candidate_universe_incomplete_rows": status_counts.get("universe_incomplete", 0),
        "candidate_universe_non_sol_rows": status_counts.get("non_sol_quote", 0),
        "candidate_universe_report_status": candidate_report.get("status"),
        "identity_collision_count": len(candidate_report.get("identity_collisions") or []),
        "accepted_lifecycle_rows": accepted_rows_count,
        "accepted_lifecycle_resolved_rows": accepted_resolved,
        "accepted_lifecycle_pending_rows": accepted_pending,
        "accepted_lifecycle_unresolved_rows": accepted_unresolved,
        "accepted_lifecycle_report_status": accepted_report.get("status"),
        "accepted_rows_joined": joined,
        "accepted_rows_unmatched": unmatched,
        "accepted_rows_ambiguous": ambiguous_count,
        "accepted_lifecycle_join_completeness": completeness,
        "accepted_lifecycle_exact_candidate_id_joined": exact_candidate_id_joined,
        "accepted_lifecycle_join_key_counts": common.counter_dict(joined_key_counts),
        "accepted_unmatched_samples": unmatched_samples,
        "accepted_ambiguous_samples": ambiguous_samples,
        "lifecycle_status_counts": common.counter_dict(lifecycle_status_counts),
        "denominator_source": "event_artifact_only",
        "r2_labels_built": False,
        "decision_logs_created_denominator_rows": candidate_report.get(
            "decision_logs_created_denominator_rows", 0
        ),
    }


def build_dataset_manifest(
    *,
    args: argparse.Namespace,
    join_coverage: dict[str, Any],
) -> dict[str, Any]:
    dataset_dir = args.root / "datasets" / "selector" / args.scope
    report_dir = args.root / "reports" / "selector" / args.scope
    outputs = {
        "candidate_universe_v1": dataset_dir / "candidate_universe_v1.jsonl",
        "accepted_lifecycle_v1": dataset_dir / "accepted_lifecycle_v1.jsonl",
        "candidate_universe_manifest_v1": report_dir / "candidate_universe_manifest_v1.json",
        "accepted_lifecycle_manifest_v1": report_dir / "accepted_lifecycle_manifest_v1.json",
        "phase1_join_coverage_v1": report_dir / "phase1_join_coverage_v1.json",
        "label_coverage_v1": report_dir / "label_coverage_v1.json",
        "dataset_manifest_v1": report_dir / "dataset_manifest_v1.json",
    }
    phase1_status = (
        str(join_coverage.get("status"))
        if join_coverage.get("status") in {"PASS", "PASS_FOR_R2_UNIVERSE_ONLY"}
        else "NO-GO"
    )
    scope_kind = (
        "windowed"
        if args.window_start_ms is not None or args.window_end_ms is not None
        else "full"
    )
    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "dataset_manifest_v1",
        "scope": args.scope,
        "source_scope": args.source_scope,
        "scope_kind": scope_kind,
        "window_start_ts_ms": args.window_start_ms,
        "window_end_ts_ms": args.window_end_ms,
        "window_reason": args.window_reason,
        "excluded_window_reason": args.excluded_window_reason,
        "phase": "phase1",
        "current_phase": "phase1",
        "status": phase1_status,
        "phase1_status": phase1_status,
        "phase1_fail_reasons": join_coverage.get("fail_reasons", []),
        "phase3_precision_readiness": join_coverage.get("phase3_precision_readiness"),
        "dataset_dir": str(dataset_dir),
        "report_dir": str(report_dir),
        "denominator_source": "event_artifact_only",
        "birth_source_contract": "Ghost canonical birth lane / NewPoolDetected",
        "nln_create_used_for_birth": False,
        "r2_labels_built": False,
        "r2_market_paths_built": False,
        "r2_label_projection_built": False,
        "r2_resolved_denominator_built": False,
        "feature_snapshots_built": False,
        "selector_training_view_built": False,
        "baseline_built": False,
        "gatekeeper_compare_built": False,
        "shadow_only_emit": {
            "enabled": False,
            "reason": "phase1_offline_dataset_builder_only",
        },
        "r2_ssot_contract": {
            "canonical_sources": [
                "Yellowstone/Geyser AccountUpdates",
                "DIAG_ACCOUNT_UPDATE_RELAY",
                "canonical account-state snapshots",
            ],
            "rpc_policy": "RPC may be flagged backfill/enrichment only and is never canonical R2 SSOT.",
        },
        "input_provenance": {
            "events": [file_provenance(path) for path in args.events],
            "decisions": [file_provenance(path) for path in args.decisions],
            "lifecycle_report": file_provenance(args.lifecycle_report),
            "accepted_buy_log": file_provenance(args.accepted_buy_log),
            "lifecycle_report_manifest": file_provenance(args.lifecycle_report_manifest),
            "config_snapshot": file_provenance(args.config_snapshot),
        },
        "window_contract": {
            "scope_kind": scope_kind,
            "candidate_universe_filter": "birth_ts_ms",
            "accepted_lifecycle_filter": "decision_ts_ms_or_entry_execution_ts_ms",
            "decision_logs": "context_only_not_denominator",
        },
        "outputs": {name: file_provenance(path) for name, path in sorted(outputs.items())},
        "stage_reports": {
            "candidate_universe_v1": read_json(outputs["candidate_universe_manifest_v1"]),
            "accepted_lifecycle_v1": read_json(outputs["accepted_lifecycle_manifest_v1"]),
            "phase1_join_coverage_v1": join_coverage,
            "label_coverage_v1": {
                **join_coverage,
                "artifact": "label_coverage_v1",
                "purpose": "accepted_lifecycle_to_candidate_universe_join_coverage",
                "r2_status": "not_built_in_phase1",
            },
        },
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--source-scope", required=True)
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument("--candidate-universe", required=True, type=Path)
    parser.add_argument("--accepted-lifecycle", required=True, type=Path)
    parser.add_argument("--candidate-manifest", required=True, type=Path)
    parser.add_argument("--accepted-manifest", required=True, type=Path)
    parser.add_argument("--events", type=Path, action="append", default=[])
    parser.add_argument("--decisions", type=Path, action="append", default=[])
    parser.add_argument("--lifecycle-report", required=True, type=Path)
    parser.add_argument("--accepted-buy-log", type=Path)
    parser.add_argument("--lifecycle-report-manifest", type=Path)
    parser.add_argument("--config-snapshot", type=Path)
    parser.add_argument("--window-start-ms", type=int)
    parser.add_argument("--window-end-ms", type=int)
    parser.add_argument("--window-reason")
    parser.add_argument("--excluded-window-reason")
    parser.add_argument(
        "--allow-r2-universe-only",
        action="store_true",
        help=(
            "Allow Phase 1 to pass as PASS_FOR_R2_UNIVERSE_ONLY when accepted "
            "lifecycle rows are absent. This does not make the scope Phase 3 "
            "precision-ready."
        ),
    )
    parser.add_argument("--phase1-join-output", required=True, type=Path)
    parser.add_argument("--label-coverage-output", required=True, type=Path)
    parser.add_argument("--dataset-manifest-output", required=True, type=Path)
    parser.add_argument("--json", action="store_true")
    return parser


def run(args: argparse.Namespace) -> dict[str, Any]:
    join_coverage = build_phase1_join_coverage(
        candidate_universe=args.candidate_universe,
        accepted_lifecycle=args.accepted_lifecycle,
        candidate_manifest=args.candidate_manifest,
        accepted_manifest=args.accepted_manifest,
        window_start_ms=args.window_start_ms,
        window_end_ms=args.window_end_ms,
        allow_r2_universe_only=args.allow_r2_universe_only,
    )
    common.write_json(args.phase1_join_output, join_coverage)
    label_coverage = {
        **join_coverage,
        "artifact": "label_coverage_v1",
        "purpose": "accepted_lifecycle_to_candidate_universe_join_coverage",
        "r2_status": "not_built_in_phase1",
    }
    common.write_json(args.label_coverage_output, label_coverage)
    manifest = build_dataset_manifest(args=args, join_coverage=join_coverage)
    common.write_json(args.dataset_manifest_output, manifest)
    return manifest


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    manifest = run(args)
    if args.json:
        print(json.dumps(manifest, ensure_ascii=False, sort_keys=True))
    return 0 if manifest.get("phase1_status") in {"PASS", "PASS_FOR_R2_UNIVERSE_ONLY"} else 2


if __name__ == "__main__":
    raise SystemExit(main())
