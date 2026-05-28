#!/usr/bin/env python3
"""Build the public selector dataset/report layout for one scope.

The script is an offline orchestrator only.  It writes JSONL/JSON artifacts
under datasets/selector/<scope>/ and reports/selector/<scope>/, preserving the
plan's evidence-freeze/provenance contract without touching runtime policy.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from argparse import Namespace
from pathlib import Path
from typing import Any

import build_selector_accepted_lifecycle as accepted
import build_selector_candidate_universe as universe
import build_selector_feature_snapshots as snapshots
import build_selector_training_view as training
import compare_selector_gatekeepers as compare
import selector_pipeline_common as common
import train_selector_baseline as baseline


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


def output_provenance(paths: dict[str, Path]) -> dict[str, Any]:
    return {name: file_provenance(path) for name, path in sorted(paths.items())}


def dataset_status(stage_reports: dict[str, dict[str, Any]]) -> tuple[str, list[str]]:
    fail_reasons = []
    for name, report in stage_reports.items():
        status = report.get("status")
        if status not in {"ok", "PASS"}:
            fail_reasons.append(f"{name}:{status}")
    return ("ok" if not fail_reasons else "NO-GO", fail_reasons)


def build_dataset(args: argparse.Namespace) -> dict[str, Any]:
    dataset_dir = args.root / "datasets" / "selector" / args.scope
    report_dir = args.root / "reports" / "selector" / args.scope
    dataset_dir.mkdir(parents=True, exist_ok=True)
    report_dir.mkdir(parents=True, exist_ok=True)

    outputs = {
        "candidate_universe_v1": dataset_dir / "candidate_universe_v1.jsonl",
        "accepted_lifecycle_v1": dataset_dir / "accepted_lifecycle_v1.jsonl",
        "feature_snapshots_v1": dataset_dir / "feature_snapshots_v1.jsonl",
        "selector_training_view_v1": dataset_dir / "selector_training_view_v1.jsonl",
        "dataset_manifest_v1": report_dir / "dataset_manifest_v1.json",
        "label_coverage_v1": report_dir / "label_coverage_v1.json",
        "gatekeeper_compare_v25_v3_v1": report_dir / "gatekeeper_compare_v25_v3_v1.json",
        "selector_baseline_v1": report_dir / "selector_baseline_v1.json",
        "leakage_audit_v1": report_dir / "leakage_audit_v1.json",
    }

    candidate_rows, candidate_report = universe.build_universe(
        event_paths=args.events,
        decision_paths=args.decisions,
        allow_degraded_events=args.allow_degraded_events,
        allow_decision_universe=args.allow_decision_universe,
        allow_incomplete_universe=args.allow_incomplete_universe,
    )
    common.write_jsonl(outputs["candidate_universe_v1"], candidate_rows)

    accepted_rows, accepted_report = accepted.build_accepted_lifecycle(
        lifecycle_report=args.lifecycle_report,
        pnl_target_net_pct=args.pnl_target_net_pct,
    )
    common.write_jsonl(outputs["accepted_lifecycle_v1"], accepted_rows)

    feature_rows, feature_report = snapshots.build_feature_snapshots(
        candidate_universe=outputs["candidate_universe_v1"],
        event_paths=args.events,
        decision_paths=args.decisions,
        snapshot_kinds=args.snapshot_kind
        or ["birth+5s", "birth+15s", "birth+30s", "birth+60s", "decision"],
        include_decision_context=args.include_decision_context_for_features,
    )
    common.write_jsonl(outputs["feature_snapshots_v1"], feature_rows)

    training_rows, label_coverage, leakage_audit = training.build_training_view(
        candidate_universe=outputs["candidate_universe_v1"],
        accepted_lifecycle=outputs["accepted_lifecycle_v1"],
        feature_snapshots=outputs["feature_snapshots_v1"],
        price_paths=args.price_paths,
        target_net_pct=args.target_net_pct,
        stop_net_pct=args.stop_net_pct,
        horizon_ms=args.horizon_ms,
        snapshot_kind=args.training_snapshot_kind,
        fallback_snapshot_kind=args.fallback_snapshot_kind,
    )
    common.write_jsonl(outputs["selector_training_view_v1"], training_rows)
    common.write_json(outputs["label_coverage_v1"], label_coverage)
    common.write_json(outputs["leakage_audit_v1"], leakage_audit)

    compare_report = compare.compare(
        outputs["selector_training_view_v1"],
        args.accept_rate_bucket or [0.01, 0.025, 0.05, 0.10],
        split=args.compare_split,
    )
    common.write_json(outputs["gatekeeper_compare_v25_v3_v1"], compare_report)

    baseline_report = baseline.train(
        outputs["selector_training_view_v1"],
        Namespace(
            target_precision=args.target_precision,
            min_first_baseline_accepted=args.min_first_baseline_accepted,
            min_comparison_accepted=args.min_comparison_accepted,
            min_eligible=args.min_eligible,
            min_holdout_accepted=args.min_holdout_accepted,
            min_holdout_accepted_shadow_emit=args.min_holdout_accepted_shadow_emit,
            leakage_audit=outputs["leakage_audit_v1"],
        ),
    )
    common.write_json(outputs["selector_baseline_v1"], baseline_report)

    stage_reports = {
        "candidate_universe_v1": candidate_report,
        "accepted_lifecycle_v1": accepted_report,
        "feature_snapshots_v1": feature_report,
        "label_coverage_v1": label_coverage,
        "leakage_audit_v1": leakage_audit,
        "gatekeeper_compare_v25_v3_v1": compare_report,
        "selector_baseline_v1": baseline_report,
    }
    status, fail_reasons = dataset_status(stage_reports)
    manifest = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "dataset_manifest_v1",
        "scope": args.scope,
        "status": status,
        "fail_reasons": fail_reasons,
        "dataset_dir": str(dataset_dir),
        "report_dir": str(report_dir),
        "r2_ssot_contract": {
            "canonical_sources": [
                "Yellowstone/Geyser AccountUpdates",
                "DIAG_ACCOUNT_UPDATE_RELAY",
                "canonical account-state snapshots",
            ],
            "rpc_policy": "RPC may be flagged backfill/enrichment only and is never canonical R2 SSOT.",
        },
        "config": {
            "pnl_target_net_pct": args.pnl_target_net_pct,
            "target_net_pct": args.target_net_pct,
            "stop_net_pct": args.stop_net_pct,
            "horizon_ms": args.horizon_ms,
            "replay_artifact_version": args.replay_artifact_version,
            "replay_artifact_version_cli_policy": (
                "manifest_only; comparison gate requires per-row "
                "gatekeeper_v25/gatekeeper_v3 replay_artifact_version fields"
            ),
        },
        "input_provenance": {
            "events": [file_provenance(path) for path in args.events],
            "decisions": [file_provenance(path) for path in args.decisions],
            "lifecycle_report": file_provenance(args.lifecycle_report),
            "price_paths": file_provenance(args.price_paths),
            "config_snapshots": [file_provenance(path) for path in args.config_snapshot],
        },
        "outputs": output_provenance(outputs),
        "stage_reports": stage_reports,
        "precision_r2_denominator_contract": common.PRECISION_R2_DENOMINATOR_CONTRACT,
        "shadow_only_emit": {
            "enabled": False,
            "reason": "offline_dataset_builder_only",
            "required_gate": "selector_baseline_v1.methods[*].promotion_gate.shadow_only_emit_status == PASS",
        },
    }
    common.write_json(outputs["dataset_manifest_v1"], manifest)
    return manifest


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument("--events", type=Path, action="append", default=[])
    parser.add_argument("--decisions", type=Path, action="append", default=[])
    parser.add_argument("--lifecycle-report", required=True, type=Path)
    parser.add_argument("--price-paths", type=Path)
    parser.add_argument("--config-snapshot", type=Path, action="append", default=[])
    parser.add_argument("--replay-artifact-version")
    parser.add_argument("--pnl-target-net-pct", required=True, type=float)
    parser.add_argument("--target-net-pct", required=True, type=float)
    parser.add_argument("--stop-net-pct", required=True, type=float)
    parser.add_argument("--horizon-ms", required=True, type=int)
    parser.add_argument(
        "--snapshot-kind",
        action="append",
        choices=["birth+5s", "birth+15s", "birth+30s", "birth+60s", "decision"],
        default=None,
    )
    parser.add_argument("--training-snapshot-kind", default="decision")
    parser.add_argument("--fallback-snapshot-kind", default="birth+30s")
    parser.add_argument("--compare-split", default="holdout")
    parser.add_argument("--accept-rate-bucket", type=float, action="append", default=None)
    parser.add_argument("--target-precision", type=float, default=0.70)
    parser.add_argument("--min-first-baseline-accepted", type=int, default=80)
    parser.add_argument("--min-comparison-accepted", type=int, default=150)
    parser.add_argument("--min-eligible", type=int, default=1000)
    parser.add_argument("--min-holdout-accepted", type=int, default=50)
    parser.add_argument("--min-holdout-accepted-shadow-emit", type=int, default=100)
    parser.add_argument("--allow-degraded-events", action="store_true")
    parser.add_argument("--allow-decision-universe", action="store_true")
    parser.add_argument("--allow-incomplete-universe", action="store_true")
    parser.add_argument(
        "--include-decision-context-for-features",
        action="store_true",
        help="NO-GO mode: allow decision logs to contribute to feature rollup diagnostics.",
    )
    parser.add_argument("--json", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    manifest = build_dataset(args)
    if args.json:
        print(json.dumps(manifest, ensure_ascii=False, sort_keys=True))
    return 0 if manifest["status"] == "ok" else 2


if __name__ == "__main__":
    raise SystemExit(main())
