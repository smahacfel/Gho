#!/usr/bin/env python3
"""Build Phase 2 selector artifacts without training, baselines, or comparison."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any

import build_selector_feature_snapshots as snapshots
import build_selector_r2_market_paths as r2_paths
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


def read_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as fh:
        payload = json.load(fh)
    if not isinstance(payload, dict):
        raise ValueError(f"expected JSON object in {path}")
    return payload


def output_path_from_manifest(
    manifest: dict[str, Any],
    name: str,
    fallback: Path,
) -> Path:
    output = manifest.get("outputs", {}).get(name)
    if isinstance(output, dict):
        raw = common.str_or_none(output.get("path"))
        if raw:
            return Path(raw)
    return fallback


def event_paths_from_manifest(manifest: dict[str, Any]) -> list[Path]:
    paths: list[Path] = []
    stage = manifest.get("stage_reports", {}).get("candidate_universe_v1", {})
    for raw in stage.get("input_event_paths", []) if isinstance(stage, dict) else []:
        if isinstance(raw, str) and raw:
            paths.append(Path(raw))
    if paths:
        return paths
    for item in manifest.get("input_provenance", {}).get("events", []):
        if isinstance(item, dict):
            raw = common.str_or_none(item.get("path"))
            if raw:
                paths.append(Path(raw))
    return paths


def require_phase1_pass(manifest: dict[str, Any]) -> None:
    if manifest.get("phase1_status") != "PASS":
        raise ValueError("Phase 2 requires phase1_status=PASS")
    if manifest.get("denominator_source") != "event_artifact_only":
        raise ValueError("Phase 2 requires event_artifact_only denominator")
    if manifest.get("r2_labels_built") not in {False, None}:
        raise ValueError("Phase 2 requires r2_labels_built=false before P2A")


def shadow_ledger_snapshot_audit(paths: list[Path]) -> dict[str, Any]:
    entries = []
    for path in paths:
        stat = path.stat() if path.exists() and path.is_file() else None
        entries.append(
            {
                "snapshot_path": str(path),
                "exists": path.exists(),
                "mtime": stat.st_mtime if stat else None,
                "size_bytes": stat.st_size if stat else None,
                "schema_version_if_detectable": None,
                "candidate_overlap_count_if_detectable": None,
                "overlap_detection_status": "not_attempted_without_binary_adapter",
                "not_used_for_r2_reason": "binary_snapshot_adapter_not_qualified",
            }
        )
    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "shadow_ledger_snapshot_audit_v1",
        "status": "audit_only",
        "r2_ssot": False,
        "label_source": False,
        "feature_source": False,
        "adapter_implemented": False,
        "overlap_detection_status": "not_attempted_without_binary_adapter",
        "snapshot_count": len(entries),
        "snapshots": entries,
    }


def build_phase2(args: argparse.Namespace) -> dict[str, Any]:
    dataset_dir = args.root / "datasets" / "selector" / args.scope
    report_dir = args.root / "reports" / "selector" / args.scope
    dataset_dir.mkdir(parents=True, exist_ok=True)
    report_dir.mkdir(parents=True, exist_ok=True)

    manifest_path = args.phase1_manifest or report_dir / "dataset_manifest_v1.json"
    manifest = read_json(manifest_path)
    require_phase1_pass(manifest)

    candidate_universe = args.candidate_universe or output_path_from_manifest(
        manifest,
        "candidate_universe_v1",
        dataset_dir / "candidate_universe_v1.jsonl",
    )
    if not candidate_universe.exists():
        raise FileNotFoundError(f"candidate_universe_v1 not found: {candidate_universe}")

    event_paths = args.events or event_paths_from_manifest(manifest)
    if not event_paths:
        raise ValueError("Phase 2A requires event artifacts for feature snapshots")

    feature_output = dataset_dir / "feature_snapshots_v1.jsonl"
    feature_manifest_output = report_dir / "feature_snapshots_manifest_v1.json"
    feature_rows, feature_report = snapshots.build_feature_snapshots(
        candidate_universe=candidate_universe,
        event_paths=event_paths,
        decision_paths=[],
        snapshot_kinds=args.snapshot_kind
        or ["birth+5s", "birth+15s", "birth+30s", "birth+60s", "decision"],
        include_decision_context=False,
    )
    common.write_jsonl(feature_output, feature_rows)
    common.write_json(feature_manifest_output, feature_report)

    shadow_audit_path = report_dir / "shadow_ledger_snapshot_audit_v1.json"
    shadow_audit = shadow_ledger_snapshot_audit(args.shadow_ledger_snapshot_audit)
    common.write_json(shadow_audit_path, shadow_audit)

    r2_output = dataset_dir / "r2_market_paths_v1.jsonl"
    r2_coverage_output = report_dir / "r2_market_path_coverage_v1.json"
    r2_rows, r2_coverage = r2_paths.build_r2_market_paths(
        candidate_universe=candidate_universe,
        account_update_paths=args.account_update,
        diag_account_update_paths=args.diag_account_update,
        canonical_snapshot_paths=args.canonical_snapshot_jsonl,
        target_net_pct=args.target_net_pct,
        stop_net_pct=args.stop_net_pct,
        horizon_ms=args.horizon_ms,
    )
    common.write_jsonl(r2_output, r2_rows)
    common.write_json(r2_coverage_output, r2_coverage)

    feature_ok = (
        feature_report.get("status") == "ok"
        and feature_report.get("feature_snapshot_gate_status") == "PASS"
        and feature_report.get("leakage_precheck") == "PASS"
    )
    r2_resolved_rows = int(r2_coverage.get("r2_resolved_rows") or 0)
    r2_resolved_denominator_built = r2_resolved_rows > 0
    if not feature_ok:
        phase2_stage_status = "NO-GO/FEATURE_SNAPSHOTS"
        phase2_status = "NO-GO/FEATURE_SNAPSHOTS"
    elif r2_resolved_denominator_built:
        phase2_stage_status = "P2B_PASS"
        phase2_status = "P2B_PASS_PENDING_LABEL_COVERAGE"
    else:
        phase2_stage_status = "P2B_PENDING_R2_DENOMINATOR"
        phase2_status = "NO-GO/PENDING_R2_DENOMINATOR"
    phase2_fail_reasons: list[str] = []
    if not feature_ok:
        phase2_fail_reasons.append("feature_snapshots_not_phase2_ready")
        phase2_fail_reasons.extend(str(reason) for reason in feature_report.get("fail_reasons", []))
    if feature_ok and not r2_resolved_denominator_built:
        phase2_fail_reasons.extend(str(reason) for reason in r2_coverage.get("fail_reasons", []))

    r2_config = {
        "profile": "r2_40_40_60s_v1",
        "target_net_pct": args.target_net_pct,
        "stop_net_pct": args.stop_net_pct,
        "horizon_ms": args.horizon_ms,
        "source": "phase2_manifest",
    }

    outputs = dict(manifest.get("outputs") or {})
    outputs.update(
        {
            "feature_snapshots_v1": file_provenance(feature_output),
            "feature_snapshots_manifest_v1": file_provenance(feature_manifest_output),
            "r2_market_paths_v1": file_provenance(r2_output),
            "r2_market_path_coverage_v1": file_provenance(r2_coverage_output),
            "shadow_ledger_snapshot_audit_v1": file_provenance(shadow_audit_path),
        }
    )
    stage_reports = dict(manifest.get("stage_reports") or {})
    stage_reports["feature_snapshots_v1"] = feature_report
    stage_reports["r2_market_paths_v1"] = r2_coverage
    stage_reports["shadow_ledger_snapshot_audit_v1"] = shadow_audit
    stage_reports["phase2_p2b"] = {
        "status": phase2_stage_status,
        "phase2_status": phase2_status,
        "r2_config": r2_config,
        "feature_snapshots_built": True,
        "r2_market_paths_built": True,
        "r2_label_projection_built": True,
        "r2_resolved_denominator_built": r2_resolved_denominator_built,
        "r2_resolved_rows": r2_resolved_rows,
    }

    manifest.update(
        {
            "current_phase": "phase2",
            "phase2_stage_status": phase2_stage_status,
            "phase2_status": phase2_status,
            "phase2_fail_reasons": phase2_fail_reasons,
            "r2_config": r2_config,
            "feature_snapshots_built": True,
            "r2_labels_built": False,
            "r2_market_paths_built": True,
            "r2_label_projection_built": True,
            "r2_resolved_denominator_built": r2_resolved_denominator_built,
            "r2_resolved_rows": r2_resolved_rows,
            "selector_training_view_built": False,
            "baseline_built": False,
            "gatekeeper_compare_built": False,
            "shadow_only_emit": {
                "enabled": False,
                "reason": "phase2_offline_dataset_builder_only",
            },
            "outputs": outputs,
            "stage_reports": stage_reports,
        }
    )
    common.write_json(manifest_path, manifest)
    return manifest


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument("--phase1-manifest", type=Path)
    parser.add_argument("--candidate-universe", type=Path)
    parser.add_argument("--events", type=Path, action="append", default=[])
    parser.add_argument("--account-update", type=Path, action="append", default=[])
    parser.add_argument("--diag-account-update", type=Path, action="append", default=[])
    parser.add_argument("--canonical-snapshot-jsonl", type=Path, action="append", default=[])
    parser.add_argument("--shadow-ledger-snapshot-audit", type=Path, action="append", default=[])
    parser.add_argument("--target-net-pct", required=True, type=float)
    parser.add_argument("--stop-net-pct", required=True, type=float)
    parser.add_argument("--horizon-ms", required=True, type=int)
    parser.add_argument(
        "--snapshot-kind",
        action="append",
        choices=["birth+5s", "birth+15s", "birth+30s", "birth+60s", "decision"],
        default=None,
    )
    parser.add_argument("--json", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    manifest = build_phase2(args)
    if args.json:
        print(json.dumps(manifest, ensure_ascii=False, sort_keys=True))
    return 0 if manifest.get("phase2_stage_status") in {"P2B_PASS", "P2B_PENDING_R2_DENOMINATOR"} else 2


if __name__ == "__main__":
    raise SystemExit(main())
