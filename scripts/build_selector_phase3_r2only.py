#!/usr/bin/env python3
"""Build Phase 3 R2-only selector training-view draft without baseline or tuning."""

from __future__ import annotations

import argparse
import hashlib
import json
from collections import Counter
from pathlib import Path
from typing import Any

import build_selector_training_view as training
import selector_pipeline_common as common


def read_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as fh:
        payload = json.load(fh)
    if not isinstance(payload, dict):
        raise ValueError(f"expected JSON object in {path}")
    return payload


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
    payload.update({"size_bytes": path.stat().st_size, "sha256": digest.hexdigest()})
    return payload


def output_path_from_manifest(manifest: dict[str, Any], name: str, fallback: Path) -> Path:
    output = manifest.get("outputs", {}).get(name)
    if isinstance(output, dict):
        raw = common.str_or_none(output.get("path"))
        if raw:
            return Path(raw)
    return fallback


def require_r2_only_phase2(manifest: dict[str, Any]) -> None:
    if manifest.get("denominator_source") != "event_artifact_only":
        raise ValueError("Phase 3 R2-only requires event_artifact_only denominator")
    if manifest.get("phase2_status") not in {
        "P2C_PASS_LABEL_COVERAGE",
        "P2C_PASS_LABEL_COVERAGE_R2_ONLY",
    }:
        raise ValueError("Phase 3 R2-only requires Phase 2 label coverage PASS")
    if manifest.get("r2_resolved_denominator_built") is not True:
        raise ValueError("Phase 3 R2-only requires resolved R2 denominator")
    if manifest.get("selector_training_view_built") is True:
        raise ValueError("Phase 3 R2-only refuses to overwrite an existing training-view claim")
    if manifest.get("baseline_built") is True or manifest.get("gatekeeper_compare_built") is True:
        raise ValueError("Phase 3 R2-only draft must start before baseline/comparison")


def split_label_counts(rows: list[dict[str, Any]]) -> dict[str, dict[str, int]]:
    counts: dict[str, Counter[str]] = {}
    for row in rows:
        if row.get("r2_only_training_denominator") is not True:
            continue
        split = str(row.get("split") or "unknown")
        label = str(row.get("r2_label") or "unresolved")
        counts.setdefault(split, Counter())[label] += 1
    return {split: common.counter_dict(counter) for split, counter in sorted(counts.items())}


def r2_label_counts(rows: list[dict[str, Any]]) -> dict[str, int]:
    return common.counter_dict(
        Counter(
            str(row.get("r2_label"))
            for row in rows
            if row.get("r2_only_training_denominator") is True
            and row.get("r2_label") in {"positive", "negative"}
        )
    )


def build_phase3(args: argparse.Namespace) -> dict[str, Any]:
    dataset_dir = args.root / "datasets" / "selector" / args.scope
    report_dir = args.root / "reports" / "selector" / args.scope
    dataset_dir.mkdir(parents=True, exist_ok=True)
    report_dir.mkdir(parents=True, exist_ok=True)

    phase2_manifest_path = args.phase2_manifest or report_dir / "dataset_manifest_v1.json"
    if args.frozen_explicit_inputs:
        missing_explicit = [
            name
            for name, value in (
                ("candidate_universe", args.candidate_universe),
                ("accepted_lifecycle", args.accepted_lifecycle),
                ("feature_snapshots", args.feature_snapshots),
                ("r2_market_paths", args.r2_market_paths),
            )
            if value is None
        ]
        if missing_explicit:
            raise ValueError(
                "--frozen-explicit-inputs requires: " + ", ".join(missing_explicit)
            )
        phase2_manifest = read_json(phase2_manifest_path) if phase2_manifest_path.exists() else {
            "phase2_status": "FROZEN_EXPLICIT_INPUTS_NO_PHASE2_MANIFEST",
            "denominator_source": "explicit_frozen_inputs",
        }
    else:
        phase2_manifest = read_json(phase2_manifest_path)
        require_r2_only_phase2(phase2_manifest)

    candidate_universe = args.candidate_universe or output_path_from_manifest(
        phase2_manifest,
        "candidate_universe_v1",
        dataset_dir / "candidate_universe_v1.jsonl",
    )
    accepted_lifecycle = args.accepted_lifecycle or output_path_from_manifest(
        phase2_manifest,
        "accepted_lifecycle_v1",
        dataset_dir / "accepted_lifecycle_v1.jsonl",
    )
    feature_snapshots = args.feature_snapshots or output_path_from_manifest(
        phase2_manifest,
        "feature_snapshots_v1",
        dataset_dir / "feature_snapshots_v1.jsonl",
    )
    r2_market_paths = args.r2_market_paths or output_path_from_manifest(
        phase2_manifest,
        "r2_market_paths_v1",
        dataset_dir / "r2_market_paths_v1.jsonl",
    )
    for path in (candidate_universe, accepted_lifecycle, feature_snapshots, r2_market_paths):
        if not path.exists():
            raise FileNotFoundError(path)

    training_output = dataset_dir / "selector_training_view_v1.jsonl"
    training_manifest_output = report_dir / "selector_training_view_manifest_v1.json"
    leakage_output = report_dir / "leakage_audit_v1.json"
    phase3_manifest_output = report_dir / "phase3_r2only_manifest_v1.json"

    rows, coverage, leakage_audit = training.build_training_view(
        candidate_universe=candidate_universe,
        accepted_lifecycle=accepted_lifecycle,
        feature_snapshots=feature_snapshots,
        price_paths=r2_market_paths,
        target_net_pct=args.target_net_pct,
        stop_net_pct=args.stop_net_pct,
        horizon_ms=args.horizon_ms,
        snapshot_kind=args.snapshot_kind,
        fallback_snapshot_kind=args.fallback_snapshot_kind,
        split_denominator="resolved_r2",
        gatekeeper_feature_context=args.gatekeeper_feature_context,
        buyer_quality_context=args.buyer_quality_context,
        funding_graph_context=args.funding_graph_context,
    )
    common.write_jsonl(training_output, rows)
    common.write_json(leakage_output, leakage_audit)

    r2_denominator_rows = int(coverage.get("r2_training_denominator_rows") or 0)
    feature_snapshot_incomplete_excluded_rows = int(
        coverage.get("feature_snapshot_incomplete_excluded_rows") or 0
    )
    missing_feature_cutoff_excluded_rows = int(
        coverage.get("missing_feature_cutoff_excluded_rows") or 0
    )
    resolved_label_counts = r2_label_counts(rows)
    r2_positive_rows = int(resolved_label_counts.get("positive") or 0)
    r2_negative_rows = int(resolved_label_counts.get("negative") or 0)
    fail_reasons: list[str] = []
    if coverage.get("status") != "ok":
        fail_reasons.append("training_view_label_coverage_not_ok")
        fail_reasons.extend(str(reason) for reason in coverage.get("fail_reasons", []))
    if leakage_audit.get("status") != "PASS":
        fail_reasons.append("leakage_audit_not_pass")
    if r2_denominator_rows < args.min_resolved_rows:
        fail_reasons.append("insufficient_r2_resolved_rows")

    status = "PASS_R2_ONLY_DRAFT" if not fail_reasons else "NO-GO"
    training_manifest = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "selector_training_view_manifest_v1",
        "phase": "phase3",
        "dataset_kind": "r2_only_frozen_explicit_inputs" if args.frozen_explicit_inputs else "r2_only",
        "universe_source_class": "ghost_observed_birth_universe",
        "universe_completeness_claim": "system_observed_not_archive_complete",
        "precision_claim_scope": "observed_birth_universe_only",
        "market_recall_claim_allowed": False,
        "status": "PASS" if not fail_reasons else "NO-GO",
        "fail_reasons": fail_reasons,
        "scope": args.scope,
        "output": file_provenance(training_output),
        "input_provenance": {
            "phase2_manifest": file_provenance(phase2_manifest_path),
            "candidate_universe_v1": file_provenance(candidate_universe),
            "accepted_lifecycle_v1": file_provenance(accepted_lifecycle),
            "feature_snapshots_v1": file_provenance(feature_snapshots),
            "r2_market_paths_v1": file_provenance(r2_market_paths),
            "gatekeeper_feature_context_v1": file_provenance(args.gatekeeper_feature_context),
            "buyer_quality_context_v1": file_provenance(args.buyer_quality_context),
            "funding_graph_context_v1": file_provenance(args.funding_graph_context),
        },
        "training_rows": len(rows),
        "r2_training_denominator_rows": r2_denominator_rows,
        "effective_r2_training_denominator_rows": r2_denominator_rows,
        "feature_snapshot_incomplete_excluded_rows": feature_snapshot_incomplete_excluded_rows,
        "missing_feature_cutoff_excluded_rows": missing_feature_cutoff_excluded_rows,
        "excluded_feature_snapshot_incomplete_candidate_ids": coverage.get(
            "excluded_feature_snapshot_incomplete_candidate_ids", []
        ),
        "r2_positive_rows": r2_positive_rows,
        "r2_negative_rows": r2_negative_rows,
        "r2_training_denominator_split_counts": coverage.get(
            "r2_training_denominator_split_counts"
        ),
        "r2_training_denominator_split_label_counts": split_label_counts(rows),
        "target_net_pct": args.target_net_pct,
        "stop_net_pct": args.stop_net_pct,
        "horizon_ms": args.horizon_ms,
        "snapshot_kind": args.snapshot_kind,
        "fallback_snapshot_kind": args.fallback_snapshot_kind,
        "split_denominator": "resolved_r2",
        "frozen_explicit_inputs": args.frozen_explicit_inputs,
        "r1_lifecycle_available": False,
        "realized_pnl_available": False,
        "execution_realization_available": False,
        "execution_success_claim_allowed": False,
        "selector_training_view_built": True,
        "gatekeeper_feature_context_enabled": args.gatekeeper_feature_context is not None,
        "buyer_quality_context_enabled": args.buyer_quality_context is not None,
        "funding_graph_context_enabled": args.funding_graph_context is not None,
        "buyer_quality_context": coverage.get("buyer_quality_context"),
        "funding_graph_context": coverage.get("funding_graph_context"),
        "baseline_built": False,
        "gatekeeper_compare_built": False,
        "gatekeeper_tuning_started": False,
        "production_promotion_allowed": False,
    }
    common.write_json(training_manifest_output, training_manifest)

    phase3_manifest = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "phase3_r2only_manifest_v1",
        "phase": "phase3",
        "dataset_kind": "r2_only_frozen_explicit_inputs" if args.frozen_explicit_inputs else "r2_only",
        "universe_source_class": "ghost_observed_birth_universe",
        "universe_completeness_claim": "system_observed_not_archive_complete",
        "precision_claim_scope": "observed_birth_universe_only",
        "market_recall_claim_allowed": False,
        "status": status,
        "fail_reasons": fail_reasons,
        "scope": args.scope,
        "phase2_status": phase2_manifest.get("phase2_status"),
        "frozen_explicit_inputs": args.frozen_explicit_inputs,
        "phase3_precision_readiness": "R2_ONLY_READY" if status == "PASS_R2_ONLY_DRAFT" else "NO-GO",
        "r1_lifecycle_available": False,
        "realized_pnl_available": False,
        "execution_realization_available": False,
        "execution_success_claim_allowed": False,
        "production_promotion_allowed": False,
        "training_rows": len(rows),
        "r2_training_denominator_rows": r2_denominator_rows,
        "effective_r2_training_denominator_rows": r2_denominator_rows,
        "feature_snapshot_incomplete_excluded_rows": feature_snapshot_incomplete_excluded_rows,
        "missing_feature_cutoff_excluded_rows": missing_feature_cutoff_excluded_rows,
        "excluded_feature_snapshot_incomplete_candidate_ids": coverage.get(
            "excluded_feature_snapshot_incomplete_candidate_ids", []
        ),
        "r2_positive_rows": r2_positive_rows,
        "r2_negative_rows": r2_negative_rows,
        "leakage_audit_status": leakage_audit.get("status"),
        "claim_boundaries": {
            "r2_only_baseline_draft_allowed": status == "PASS_R2_ONLY_DRAFT",
            "r1_lifecycle_claim": False,
            "realized_pnl_claim": False,
            "execution_success_claim": False,
            "production_promotion_claim": False,
            "gatekeeper_tuning_started": False,
        },
        "input_provenance": {
            "phase2_manifest": file_provenance(phase2_manifest_path),
            "candidate_universe_v1": file_provenance(candidate_universe),
            "accepted_lifecycle_v1": file_provenance(accepted_lifecycle),
            "feature_snapshots_v1": file_provenance(feature_snapshots),
            "r2_market_paths_v1": file_provenance(r2_market_paths),
            "gatekeeper_feature_context_v1": file_provenance(args.gatekeeper_feature_context),
            "buyer_quality_context_v1": file_provenance(args.buyer_quality_context),
            "funding_graph_context_v1": file_provenance(args.funding_graph_context),
        },
        "outputs": {
            "selector_training_view_v1": file_provenance(training_output),
            "selector_training_view_manifest_v1": file_provenance(training_manifest_output),
            "leakage_audit_v1": file_provenance(leakage_output),
        },
        "label_coverage": coverage,
        "leakage_audit": leakage_audit,
        "selector_training_view_built": True,
        "gatekeeper_feature_context_enabled": args.gatekeeper_feature_context is not None,
        "buyer_quality_context_enabled": args.buyer_quality_context is not None,
        "funding_graph_context_enabled": args.funding_graph_context is not None,
        "buyer_quality_context": coverage.get("buyer_quality_context"),
        "funding_graph_context": coverage.get("funding_graph_context"),
        "baseline_built": False,
        "gatekeeper_compare_built": False,
        "gatekeeper_tuning_started": False,
    }
    common.write_json(phase3_manifest_output, phase3_manifest)
    return phase3_manifest


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument("--phase2-manifest", type=Path)
    parser.add_argument(
        "--frozen-explicit-inputs",
        action="store_true",
        help="Build from explicit frozen inputs without requiring a Phase2 PASS manifest.",
    )
    parser.add_argument("--candidate-universe", type=Path)
    parser.add_argument("--accepted-lifecycle", type=Path)
    parser.add_argument("--feature-snapshots", type=Path)
    parser.add_argument("--r2-market-paths", type=Path)
    parser.add_argument("--gatekeeper-feature-context", type=Path)
    parser.add_argument("--buyer-quality-context", type=Path)
    parser.add_argument("--funding-graph-context", type=Path)
    parser.add_argument("--target-net-pct", type=float, default=40.0)
    parser.add_argument("--stop-net-pct", type=float, default=40.0)
    parser.add_argument("--horizon-ms", type=int, default=60_000)
    parser.add_argument("--snapshot-kind", default="decision")
    parser.add_argument("--fallback-snapshot-kind", default="birth+30s")
    parser.add_argument("--min-resolved-rows", type=int, default=50)
    parser.add_argument("--json", action="store_true")
    return parser


def run(args: argparse.Namespace) -> dict[str, Any]:
    return build_phase3(args)


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    manifest = run(args)
    if args.json:
        print(json.dumps(manifest, ensure_ascii=False, sort_keys=True))
    return 0 if manifest["status"] == "PASS_R2_ONLY_DRAFT" else 2


if __name__ == "__main__":
    raise SystemExit(main())
