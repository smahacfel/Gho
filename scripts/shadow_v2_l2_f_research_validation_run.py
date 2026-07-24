#!/usr/bin/env python3
"""Shadow V2 L2-F dedicated research validation audit.

This script is intentionally offline-only. It does not start Ghost, tune
Gatekeeper thresholds, modify runtime decisions, touch provider streams, or
promote any approval flag. It evaluates whether a dedicated L2-F validation
scope already contains enough evidence to claim the offline-only L2 research
candidate verdict.
"""

from __future__ import annotations

import argparse
import csv
import json
import re
import shutil
import subprocess
import sys
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import build_selector_candidate_universe as candidate_builder
import shadow_v2_gatekeeper_coverage_denominator_audit as gatekeeper_audit
import shadow_v2_path_density_horizon_audit as density_horizon_audit
from shadow_v2_offline_audit_common import (
    artifact_rotation_report,
    canonical_payload_schema,
    envelope,
    iter_canonical_rows,
    iter_density_rows,
    iter_lifecycle_rows,
    iter_replay_rows,
    nested_record,
    position_id,
)


SCHEMA_VERSION = 1
ARTIFACT = "shadow_v2_l2_f_research_validation_run"
DEFAULT_RUN_ID = "shadow-v2-l2-f-research-validation-20260705-r1"
EXPECTED_MAIN = "5e6d839984cf38ecb65a0540b0adfabfcead9f23"
REQUIRED_ROUNDTRIPS = 500
DECLARED_HORIZONS_MS = [2_000, 3_000, 10_000, 30_000, 120_000]
UNDECLARED_HORIZONS_MS = [300_000, 500_000]
DEFAULT_CANDIDATE_UNIVERSE = Path("datasets/selector/shadow-v2-l2-e2/candidate_universe_v1.jsonl")
DEFAULT_CANDIDATE_MANIFEST = Path("reports/selector/shadow-v2-l2-e2/candidate_universe_manifest_v1.json")
SELECTOR_CANDIDATE_BUILDER_SOURCE = "scripts/build_selector_candidate_universe.py"
L2_F_LAUNCHER_LOG_ADAPTER_EVENT_SOURCE = "l2_f_launcher_new_pool_detected_event_adapter_v1"
L2_DENSITY_PASS_VERDICTS = {
    "L2_D2_DENSITY_RETENTION_READY_FOR_L2_F",
    "L2_F_DENSITY_RETENTION_PASS",
}
POSITION_LEVEL_DENSITY_PASS_VERDICT = "PASS_L2_F_POSITION_LEVEL_DENSITY_RETENTION"
# L2-F scopes can legitimately contain multi-GB JSONL evidence. SHA256 is
# computed in streaming chunks by shadow_v2_manifest_audit.py, so this limit is
# a research-grade completeness guard, not a memory guard.
LARGE_L2_F_MANIFEST_SHA_BYTES = 64 * 1024 * 1024 * 1024
NEW_POOL_RE = re.compile(
    r"^(?P<ts>\S+)\s+.*Emitting NewPoolDetected: "
    r"pool_amm_id=(?P<pool>\S+), base_mint=(?P<mint>\S+), slot=Some\((?P<slot>\d+)\)"
)

VERDICT_PASS = "L2_RESEARCH_GRADE_CANDIDATE_OFFLINE_ONLY"
VERDICT_INSUFFICIENT_SAMPLE = "BLOCKED_L2_F_INSUFFICIENT_SAMPLE_SIZE"
VERDICT_TEMPORAL = "BLOCKED_L2_F_TEMPORAL_AUDIT"
VERDICT_DENSITY = "BLOCKED_L2_F_DENSITY_RETENTION"
VERDICT_GATEKEEPER = "BLOCKED_L2_F_GATEKEEPER_DENOMINATOR"
VERDICT_STARVATION = "BLOCKED_L2_F_THRESHOLD_STARVATION"
VERDICT_UNKNOWN = "BLOCKED_L2_F_UNKNOWN_OR_UNTYPED_BLOCKERS"
VERDICT_MALFORMED = "BLOCKED_L2_F_MALFORMED_ROWS"
VERDICT_MANIFEST = "BLOCKED_L2_F_MANIFEST_OR_REPLAY_LIFECYCLE"

APPROVAL_FLAGS_FALSE = {
    "runtime_approval": False,
    "research_grade": False,
    "live_equivalence": False,
    "strategy_research_unblocked": False,
    "shadow_close_only": False,
    "active_close": False,
}


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", default=DEFAULT_RUN_ID)
    parser.add_argument(
        "--output-root",
        type=Path,
        default=None,
        help="L2-F artifact directory; default reports/selector/<run-id>",
    )
    parser.add_argument(
        "--scope-root",
        type=Path,
        default=None,
        help="Dedicated L2-F runtime evidence scope to audit.",
    )
    parser.add_argument(
        "--candidate-universe",
        type=Path,
        default=DEFAULT_CANDIDATE_UNIVERSE,
    )
    parser.add_argument(
        "--candidate-manifest",
        type=Path,
        default=DEFAULT_CANDIDATE_MANIFEST,
    )
    parser.add_argument(
        "--decision-root",
        type=Path,
        action="append",
        default=[],
        help="Decision root scanned for gatekeeper_v2_decisions.jsonl. Defaults to logs/decisions.",
    )
    parser.add_argument(
        "--summary-csv",
        type=Path,
        action="append",
        default=[],
        help="Dedicated run summary CSV. Used only for explicit L2-F counters.",
    )
    parser.add_argument(
        "--historical-summary-csv",
        type=Path,
        default=Path("reports/selector/shadow_v2_terminal_executable_pnl_smoke_pr41_summary.csv"),
        help="Prior smoke summary used only as a negative upper-bound context, not as L2-F proof.",
    )
    parser.add_argument(
        "--precondition-density-summary",
        type=Path,
        default=Path("reports/selector/shadow_v2_l2_d3b_runtime_harness_density_emission_summary.csv"),
        help="Previous L2-D3B readiness summary; not accepted as L2-F research sample proof.",
    )
    parser.add_argument(
        "--output-csv",
        type=Path,
        default=Path("reports/selector/shadow_v2_l2_f_research_validation_summary.csv"),
    )
    parser.add_argument("--pretty", action="store_true")
    return parser


def read_json(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    with path.open("r", encoding="utf-8") as fh:
        payload = json.load(fh)
    return payload if isinstance(payload, dict) else {}


def read_summary_csv(paths: list[Path]) -> dict[str, str]:
    metrics: dict[str, str] = {}
    for path in paths:
        if not path.exists():
            continue
        with path.open("r", encoding="utf-8", newline="") as fh:
            reader = csv.DictReader(fh)
            if not reader.fieldnames or "metric" not in reader.fieldnames or "value" not in reader.fieldnames:
                continue
            for row in reader:
                metric = str(row.get("metric") or "").strip()
                if metric and metric not in metrics:
                    metrics[metric] = str(row.get("value") or "").strip()
    return metrics


def int_metric(metrics: dict[str, str], name: str) -> int | None:
    raw = metrics.get(name)
    if raw in (None, ""):
        return None
    try:
        return int(float(raw))
    except ValueError:
        return None


def bool_metric(metrics: dict[str, str], name: str) -> bool | None:
    raw = metrics.get(name)
    if raw is None:
        return None
    value = raw.strip().lower()
    if value in {"true", "1", "yes", "pass"}:
        return True
    if value in {"false", "0", "no", "fail"}:
        return False
    return None


def json_value(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, (dict, list)):
        return json.dumps(value, sort_keys=True, separators=(",", ":"))
    return str(value)


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def parse_log_ts_ms(value: str) -> int | None:
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=timezone.utc)
    return int(parsed.timestamp() * 1000)


def launcher_new_pool_event_adapter_row(
    *,
    launcher_log: Path,
    run_id: str,
    line_index: int,
    match: re.Match[str],
) -> dict[str, Any]:
    """Convert launcher NewPoolDetected text into Selector event-level input.

    This is intentionally an adapter, not a denominator builder. The output
    shape is consumed by build_selector_candidate_universe.py, which remains the
    owner of candidate_universe_v1 normalization, dedupe, invariant checks, and
    manifest generation.
    """
    pool_id = match.group("pool")
    base_mint = match.group("mint")
    birth_slot = int(match.group("slot"))
    birth_ts_ms = parse_log_ts_ms(match.group("ts")) or 0
    return {
        "adapter_schema": L2_F_LAUNCHER_LOG_ADAPTER_EVENT_SOURCE,
        "run_id": run_id,
        "event_type": "NewPoolDetected",
        "is_birth_event": True,
        "base_mint": base_mint,
        "mint_id": base_mint,
        "pool_id": pool_id,
        "bonding_curve": pool_id,
        "birth_ts_ms": birth_ts_ms,
        "timestamp_ms": birth_ts_ms,
        "slot": birth_slot,
        "quote_mint": "So11111111111111111111111111111111111111112",
        "raw_source_kind": "launcher_stdout_new_pool_detected",
        "launcher_log_path": str(launcher_log),
        "launcher_log_line_index": line_index,
        "payload": {
            "source": "launcher_stdout_new_pool_detected_adapter",
            "selector_builder_contract": "event_artifact_birth_observation",
        },
    }


def adapt_launcher_log_to_selector_candidate_universe(
    *,
    launcher_log: Path,
    run_id: str,
    candidate_universe: Path,
    candidate_manifest: Path,
) -> dict[str, Any]:
    """Build candidate_universe_v1 through the existing Selector builder.

    L2-F does not introduce an independent denominator model. Launcher stdout
    is only converted into event-level NewPoolDetected observations, then
    build_selector_candidate_universe.py owns candidate_universe_v1 output and
    candidate_universe_manifest_v1 invariants.
    """
    adapter_events = candidate_universe.with_name(
        "l2_f_launcher_new_pool_detected_event_adapter_v1.jsonl"
    )
    report: dict[str, Any] = {
        "status": "NOT_RUN",
        "adapter_only": True,
        "parallel_denominator_model_detected": False,
        "builder_source": SELECTOR_CANDIDATE_BUILDER_SOURCE,
        "adapter_event_path": str(adapter_events),
        "launcher_log_path": str(launcher_log),
        "launcher_log_rows_read": 0,
        "new_pool_detected_rows": 0,
        "deduped_event_rows": 0,
        "candidate_universe_path": str(candidate_universe),
        "candidate_manifest_path": str(candidate_manifest),
    }
    if not launcher_log.exists():
        report["status"] = "BLOCKED_LAUNCHER_LOG_MISSING"
        return report

    rows_by_key: dict[tuple[str, str], dict[str, Any]] = {}
    rows_read = 0
    matched = 0
    with launcher_log.open("r", encoding="utf-8", errors="replace") as fh:
        for idx, line in enumerate(fh):
            rows_read += 1
            match = NEW_POOL_RE.search(line)
            if not match:
                continue
            matched += 1
            pool_id = match.group("pool")
            base_mint = match.group("mint")
            key = (base_mint, pool_id)
            if key in rows_by_key:
                continue
            rows_by_key[key] = launcher_new_pool_event_adapter_row(
                launcher_log=launcher_log,
                run_id=run_id,
                line_index=idx,
                match=match,
            )

    report["launcher_log_rows_read"] = rows_read
    report["new_pool_detected_rows"] = matched
    report["deduped_event_rows"] = len(rows_by_key)
    if not rows_by_key:
        report["status"] = "BLOCKED_NO_EVENT_LEVEL_CANDIDATE_OBSERVATIONS"
        return report

    rows = list(rows_by_key.values())
    adapter_events.parent.mkdir(parents=True, exist_ok=True)
    with adapter_events.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True, separators=(",", ":")))
            fh.write("\n")

    manifest = candidate_builder.run(
        argparse.Namespace(
            events=[adapter_events],
            decisions=[],
            output=candidate_universe,
            manifest_output=candidate_manifest,
            allow_degraded_events=False,
            allow_decision_universe=False,
            allow_incomplete_universe=False,
            window_start_ms=None,
            window_end_ms=None,
            json=False,
        )
    )
    report["status"] = "PASS" if manifest.get("status") == "ok" else "BLOCKED_SELECTOR_BUILDER_MANIFEST"
    report["selector_builder_manifest_status"] = manifest.get("status")
    report["selector_schema_version"] = manifest.get("selector_schema_version")
    report["denominator_invariant_status"] = manifest.get("denominator_invariant_status")
    report["decision_logs_created_denominator_rows"] = manifest.get(
        "decision_logs_created_denominator_rows"
    )
    report["candidate_ids_from_decision_only"] = manifest.get("candidate_ids_from_decision_only")
    report["candidate_universe_status_counts"] = manifest.get("status_counts", {})
    report["selector_builder_input_event_paths"] = manifest.get("input_event_paths", [])
    return report


def collect_selector_event_artifact_paths(run_id: str) -> list[Path]:
    event_root = Path("datasets") / "events" / run_id
    if not event_root.exists():
        return []
    return sorted(path for path in event_root.glob("*.jsonl") if path.is_file())


def build_selector_candidate_universe_from_event_artifacts(
    *,
    event_paths: list[Path],
    candidate_universe: Path,
    candidate_manifest: Path,
) -> dict[str, Any]:
    report: dict[str, Any] = {
        "status": "NOT_RUN",
        "adapter_only": True,
        "parallel_denominator_model_detected": False,
        "builder_source": SELECTOR_CANDIDATE_BUILDER_SOURCE,
        "adapter_event_path": None,
        "selector_event_artifact_paths": [str(path) for path in event_paths],
        "selector_event_artifact_path_count": len(event_paths),
        "candidate_universe_path": str(candidate_universe),
        "candidate_manifest_path": str(candidate_manifest),
    }
    if not event_paths:
        report["status"] = "BLOCKED_SELECTOR_EVENT_ARTIFACTS_MISSING"
        return report
    manifest = candidate_builder.run(
        argparse.Namespace(
            events=event_paths,
            decisions=[],
            output=candidate_universe,
            manifest_output=candidate_manifest,
            allow_degraded_events=False,
            allow_decision_universe=False,
            allow_incomplete_universe=False,
            window_start_ms=None,
            window_end_ms=None,
            json=False,
        )
    )
    report["status"] = "PASS" if manifest.get("status") == "ok" else "BLOCKED_SELECTOR_BUILDER_MANIFEST"
    report["selector_builder_manifest_status"] = manifest.get("status")
    report["selector_schema_version"] = manifest.get("selector_schema_version")
    report["denominator_invariant_status"] = manifest.get("denominator_invariant_status")
    report["decision_logs_created_denominator_rows"] = manifest.get(
        "decision_logs_created_denominator_rows"
    )
    report["candidate_ids_from_decision_only"] = manifest.get("candidate_ids_from_decision_only")
    report["candidate_universe_status_counts"] = manifest.get("status_counts", {})
    report["selector_builder_input_event_paths"] = manifest.get("input_event_paths", [])
    return report


def derive_candidate_universe_from_launcher_log(
    *,
    launcher_log: Path,
    run_id: str,
    candidate_universe: Path,
    candidate_manifest: Path,
) -> dict[str, Any]:
    """Backward-compatible name for the L2-F launcher-log adapter path."""
    return adapt_launcher_log_to_selector_candidate_universe(
        launcher_log=launcher_log,
        run_id=run_id,
        candidate_universe=candidate_universe,
        candidate_manifest=candidate_manifest,
    )


def write_summary_csv(path: Path, report: dict[str, Any]) -> None:
    reuse = report.get("selector_gatekeeper_contract_reuse", {})
    rows = [
        ("final_verdict", report["final_verdict"], "maximum positive verdict remains offline-only"),
        ("run_id", report["run_id"], ""),
        ("expected_main", report["expected_main"], ""),
        ("dedicated_l2_f_scope_present", report["dedicated_l2_f_scope_present"], ""),
        ("validation_run_executed", report["validation_run_executed"], ""),
        ("complete_executable_roundtrip_positions", report["sample_gates"]["complete_executable_roundtrip_positions"], ""),
        ("complete_executable_roundtrip_required", REQUIRED_ROUNDTRIPS, ""),
        ("research_candidate_roundtrip_count", report["sample_gates"]["research_candidate_roundtrip_count"], ""),
        ("entry_execution_label_grade_RESEARCH_CANDIDATE_count", report["sample_gates"]["entry_execution_label_grade_RESEARCH_CANDIDATE_count"], ""),
        ("exit_execution_label_grade_RESEARCH_CANDIDATE_count", report["sample_gates"]["exit_execution_label_grade_RESEARCH_CANDIDATE_count"], ""),
        ("sample_size_gate", report["sample_gates"]["status"], ""),
        ("temporal_audit_verdict", report["temporal_audit"]["verdict"], ""),
        ("density_retention_verdict", report["density_audit"]["verdict"], "backward-compatible raw scope metric"),
        ("density_retention_verdict_raw_scope", report["density_audit"]["verdict"], "full raw density stream context"),
        (
            "density_retention_verdict_evidence_complete_scope",
            report["evidence_complete_density_audit"]["verdict"],
            "density audit rerun with position-scope-jsonl over evidence-complete positions",
        ),
        (
            "position_level_density_retention_verdict",
            report["position_level_density_gate"]["verdict"],
            "L2-F positive claims use only evidence-complete roundtrip positions",
        ),
        (
            "l2_research_evidence_complete_roundtrip_positions",
            report["position_level_density_gate"].get(
                "l2_research_evidence_complete_roundtrip_positions"
            ),
            "roundtrips with entry/exit research candidate, terminal executable truth, and all declared density horizons pass",
        ),
        (
            "density_excluded_roundtrip_positions",
            report["position_level_density_gate"].get("density_excluded_roundtrip_positions"),
            "typed sparse/retention/missing density blockers excluded from positive L2 claim",
        ),
        (
            "density_sparse_approx_only_position_count",
            report["position_level_density_gate"].get("sparse_approx_only_position_count"),
            "",
        ),
        (
            "density_retention_gap_position_count",
            report["position_level_density_gate"].get("retention_gap_position_count"),
            "",
        ),
        (
            "density_missing_declared_horizon_position_count",
            report["position_level_density_gate"].get(
                "missing_declared_horizon_position_count"
            ),
            "",
        ),
        (
            "density_evidence_complete_position_scope_path",
            report["position_level_density_gate"].get("evidence_complete_position_scope_path"),
            "",
        ),
        ("gatekeeper_denominator_verdict", report["gatekeeper_denominator"]["verdict"], ""),
        ("threshold_starvation_verdict", report["gatekeeper_denominator"]["threshold_starvation_verdict"], ""),
        ("unknown_reason_count", report["gatekeeper_denominator"]["unknown_reason_count"], ""),
        ("malformed_rows", report["malformed_rows"], ""),
        ("unknown_untyped_blockers", report["unknown_untyped_blockers"], "backward-compatible metric"),
        ("unknown_untyped_blocker_count", report["unknown_untyped_blockers"], ""),
        ("manifest_status", report["manifest_audit"]["status"], ""),
        ("replay_lifecycle_verdict", report["replay_lifecycle_audit"]["verdict"], "backward-compatible metric"),
        ("replay_lifecycle_status", report["replay_lifecycle_audit"]["verdict"], ""),
        ("account_data_hash_coverage_verdict", report["account_data_hash_coverage"]["verdict"], "backward-compatible metric"),
        ("account_data_hash_coverage_status", report["account_data_hash_coverage"]["verdict"], ""),
        ("fake_handoff_signature_count", report["temporal_audit"].get("fake_handoff_signature_count"), ""),
        ("event_seq_chain_order_substitute_count", report["temporal_audit"].get("event_seq_chain_order_substitute_count"), ""),
        ("terminal_truth_not_derived_count", report["temporal_audit"].get("terminal_truth_not_derived_count"), ""),
        ("terminal_truth_derived_count", report["temporal_audit"].get("terminal_truth_derived_count"), ""),
        (
            "density_excluded_positions_path",
            report["position_level_density_gate"].get("density_excluded_positions_path"),
            "typed fail-closed excluded-position evidence",
        ),
        (
            "selector_gatekeeper_contract_reuse_status",
            reuse.get("status"),
            "L2-F must reuse Selector/Gatekeeper candidate-universe and denominator contracts",
        ),
        (
            "candidate_universe_builder_source",
            reuse.get("candidate_universe_builder_source"),
            "existing Selector builder/contract owner",
        ),
        (
            "candidate_universe_adapter_only",
            reuse.get("candidate_universe_adapter_only"),
            "launcher log path is an event-source adapter only",
        ),
        (
            "candidate_universe_parallel_model_detected",
            reuse.get("candidate_universe_parallel_model_detected"),
            "must remain false",
        ),
        (
            "decision_logs_created_denominator_rows",
            reuse.get("decision_logs_created_denominator_rows"),
            "decision logs are context only",
        ),
        (
            "candidate_ids_from_decision_only",
            reuse.get("candidate_ids_from_decision_only"),
            "must remain zero",
        ),
        (
            "denominator_invariant_status",
            reuse.get("denominator_invariant_status"),
            "candidate_universe_manifest_v1 invariant",
        ),
        (
            "selector_contract_equivalence_tests",
            reuse.get("selector_contract_equivalence_tests"),
            "tests proving adapter equivalence and decision-only fail-closed behavior",
        ),
        ("declared_supported_horizons_ms", DECLARED_HORIZONS_MS, ""),
        ("unsupported_horizons_ms", UNDECLARED_HORIZONS_MS, "not accepted for positive L2 baseline claims"),
        ("l2_f_positive_verdict_allowed", report["final_verdict"] == VERDICT_PASS, ""),
        ("runtime_approval", False, "not granted"),
        ("research_grade", False, "not granted"),
        ("live_equivalence", False, "not granted"),
        ("strategy_research_unblocked", False, "not granted"),
        ("shadow_close_only", False, "not enabled"),
        ("active_close", False, "not enabled"),
    ]
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=["metric", "value", "notes"], lineterminator="\n")
        writer.writeheader()
        for metric, value, notes in rows:
            writer.writerow({"metric": metric, "value": json_value(value), "notes": notes})


def copy_if_exists(src: Path, dst: Path) -> bool:
    if not src.exists():
        return False
    if src.resolve() == dst.resolve():
        return True
    dst.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(src, dst)
    return True


def intish(value: Any) -> int | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value
    if isinstance(value, float):
        return int(value)
    if isinstance(value, str) and value.strip():
        try:
            return int(float(value.strip()))
        except ValueError:
            return None
    return None


def collect_decision_paths(decision_roots: list[Path]) -> list[Path]:
    roots = decision_roots or [Path("logs/decisions")]
    paths: list[Path] = []
    for root in roots:
        if root.is_file() and root.name == gatekeeper_audit.DECISION_FILE_NAME:
            paths.append(root)
        elif root.exists():
            paths.extend(sorted(root.rglob(gatekeeper_audit.DECISION_FILE_NAME)))
    seen: set[str] = set()
    out: list[Path] = []
    for path in paths:
        key = str(path)
        if key not in seen:
            seen.add(key)
            out.append(path)
    return out


def run_gatekeeper_audit(args: argparse.Namespace, decision_paths: list[Path]) -> dict[str, Any]:
    summary_csvs = list(args.summary_csv)
    if args.historical_summary_csv.exists():
        summary_csvs.append(args.historical_summary_csv)
    report = gatekeeper_audit.build_report(
        argparse.Namespace(
            candidate_universe=args.candidate_universe,
            candidate_manifest=args.candidate_manifest,
            decision_jsonl=decision_paths,
            decision_root=[],
            summary_csv=summary_csvs,
            output_csv=None,
            top_n=10,
            pretty=False,
        )
    )
    return {
        "verdict": report.get("final_verdict"),
        "candidate_universe_count": report.get("candidate_universe_count"),
        "eligible_denominator_count": report.get("eligible_denominator_count"),
        "candidate_manifest_denominator_invariant_status": report.get(
            "candidate_manifest_denominator_invariant_status"
        ),
        "candidate_manifest_decision_logs_created_denominator_rows": report.get(
            "candidate_manifest_decision_logs_created_denominator_rows"
        ),
        "candidate_manifest_candidate_ids_from_decision_only": report.get(
            "candidate_manifest_candidate_ids_from_decision_only"
        ),
        "denominator_contract_failures": report.get("denominator_contract_failures", []),
        "gatekeeper_decision_count": report.get("gatekeeper_decision_count"),
        "gatekeeper_decision_joined_to_candidate_count": report.get("gatekeeper_decision_joined_to_candidate_count"),
        "gatekeeper_decision_unmatched_count": report.get("gatekeeper_decision_unmatched_count"),
        "checkpoint_reach_count": report.get("checkpoint_reach_count"),
        "gatekeeper_buy_count": report.get("gatekeeper_buy_count"),
        "gatekeeper_reject_count": report.get("gatekeeper_reject_count"),
        "gatekeeper_timeout_count": report.get("gatekeeper_timeout_count"),
        "unknown_reason_count": report.get("unknown_reason_count") or 0,
        "threshold_starvation_verdict": report.get("threshold_starvation_verdict"),
        "decision_paths": [str(path) for path in decision_paths],
    }


def selector_gatekeeper_contract_reuse_report(
    *,
    candidate_manifest: dict[str, Any],
    adapter_report: dict[str, Any],
    gatekeeper_report: dict[str, Any],
) -> dict[str, Any]:
    decision_created = intish(candidate_manifest.get("decision_logs_created_denominator_rows"))
    decision_only = intish(candidate_manifest.get("candidate_ids_from_decision_only"))
    invariant = str(candidate_manifest.get("denominator_invariant_status") or "")
    status = str(candidate_manifest.get("status") or "")
    builder_source = adapter_report.get("builder_source") or SELECTOR_CANDIDATE_BUILDER_SOURCE
    adapter_only = bool(adapter_report.get("adapter_only"))
    parallel_model = bool(adapter_report.get("parallel_denominator_model_detected"))
    adapter_status = str(adapter_report.get("status") or "")
    status_counts = candidate_manifest.get("status_counts") if isinstance(candidate_manifest, dict) else {}
    ok_rows = intish(status_counts.get("ok")) if isinstance(status_counts, dict) else None
    failures: list[str] = []
    if adapter_status not in {"PASS", "NOT_USED_EXPLICIT_CANDIDATE_UNIVERSE"}:
        failures.append(f"candidate_universe_adapter_status_{adapter_status or 'missing'}")
    if status != "ok":
        failures.append(f"candidate_universe_manifest_status_{status or 'missing'}")
    if invariant != "PASS":
        failures.append(f"denominator_invariant_status_{invariant or 'missing'}")
    if decision_created != 0:
        failures.append("decision_logs_created_denominator_rows_nonzero")
    if decision_only != 0:
        failures.append("candidate_ids_from_decision_only_nonzero")
    if not adapter_only:
        failures.append("candidate_universe_adapter_only_false")
    if parallel_model:
        failures.append("candidate_universe_parallel_model_detected")
    if ok_rows is None or ok_rows <= 0:
        failures.append("candidate_universe_status_ok_missing")
    if gatekeeper_report.get("verdict") == gatekeeper_audit.VERDICT_DENOMINATOR_UNKNOWN:
        failures.append("gatekeeper_denominator_unknown")
    return {
        "status": "PASS" if not failures else "FAIL",
        "failures": failures,
        "candidate_universe_builder_source": builder_source,
        "candidate_universe_adapter_only": adapter_only,
        "candidate_universe_parallel_model_detected": parallel_model,
        "candidate_universe_adapter_status": adapter_status,
        "candidate_universe_adapter_event_path": adapter_report.get("adapter_event_path"),
        "candidate_universe_manifest_status": status,
        "candidate_universe_status_ok_count": ok_rows,
        "decision_logs_created_denominator_rows": decision_created,
        "candidate_ids_from_decision_only": decision_only,
        "denominator_invariant_status": invariant,
        "decision_context_join_key_semantics": "Selector identity_join_keys mint_pool/base_mint+pool_id; Gatekeeper decisions are context only",
        "gatekeeper_decision_join": gatekeeper_report.get("verdict"),
        "threshold_starvation_verdict": gatekeeper_report.get("threshold_starvation_verdict"),
        "selector_schema_version": candidate_manifest.get("selector_schema_version"),
        "manifest_status_summary": candidate_manifest.get("status"),
        "selector_contract_equivalence_tests": [
            "test_launcher_log_adapter_uses_existing_selector_candidate_universe_contract",
            "test_decision_only_rows_do_not_create_l2_f_candidate_universe_denominator",
            "test_summary_csv_exposes_required_l2_f_metric_names",
        ],
    }


def run_json_audit(script_name: str, args: list[str]) -> dict[str, Any]:
    script = Path("scripts") / script_name
    result = subprocess.run(
        [sys.executable, str(script), *args],
        check=False,
        text=True,
        capture_output=True,
    )
    if result.returncode != 0:
        return {
            "verdict": "AUDIT_COMMAND_FAILED",
            "returncode": result.returncode,
            "stderr": result.stderr.strip(),
        }
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        return {
            "verdict": "AUDIT_OUTPUT_MALFORMED",
            "error": str(exc),
            "stdout": result.stdout[:500],
        }
    if isinstance(payload, dict):
        return payload
    return {"verdict": "AUDIT_OUTPUT_NOT_OBJECT"}


def dedicated_scope_required_files(scope_root: Path | None) -> dict[str, bool]:
    required = [
        "shadow_position_event_v2.jsonl",
        "shadow_replay_v2.jsonl",
        "shadow_lifecycle_v2.jsonl",
        "shadow_path_density_v2.jsonl",
    ]
    return {name: bool(scope_root and (scope_root / name).exists()) for name in required}


def status_from_required_files(required_files: dict[str, bool]) -> tuple[bool, list[str]]:
    missing = [name for name, present in required_files.items() if not present]
    return not missing, missing


def record_value(row: dict[str, Any], field: str) -> Any:
    record = nested_record(row)
    if field in record:
        return record.get(field)
    env = envelope(row)
    return env.get(field)


def fill_status(row: dict[str, Any]) -> str:
    value = record_value(row, "fill_status")
    return str(value or "").upper()


def execution_grade(row: dict[str, Any]) -> str:
    value = record_value(row, "execution_label_grade")
    return str(value or "").upper()


def is_research_candidate_grade(value: str) -> bool:
    return str(value or "").upper() in {
        "RESEARCH_CANDIDATE",
        "RESEARCH_GRADE_CANDIDATE",
    }


def collect_roundtrip_position_sets(scope_root: Path) -> tuple[dict[str, set[str]], int]:
    malformed = 0
    entry_research_positions: set[str] = set()
    exit_research_positions: set[str] = set()
    entry_filled_positions: set[str] = set()
    exit_filled_positions: set[str] = set()
    terminal_executable_positions: set[str] = set()

    for row, row_malformed in iter_canonical_rows(scope_root) or ():
        if row_malformed or row is None:
            malformed += 1
            continue
        schema = canonical_payload_schema(row)
        pos = position_id(row)
        if not pos:
            continue
        if schema == "shadow_entry_fill_v2" and fill_status(row) == "FILLED":
            entry_filled_positions.add(pos)
            if is_research_candidate_grade(execution_grade(row)):
                entry_research_positions.add(pos)
        elif schema == "shadow_exit_fill_v2" and fill_status(row) == "FILLED":
            exit_filled_positions.add(pos)
            if is_research_candidate_grade(execution_grade(row)):
                exit_research_positions.add(pos)
        elif (
            schema == "shadow_terminal_truth_v2"
            and record_value(row, "final_pnl_executable_bps") is not None
        ):
            terminal_executable_positions.add(pos)

    complete = entry_filled_positions & exit_filled_positions & terminal_executable_positions
    research_roundtrip = (
        entry_research_positions
        & exit_research_positions
        & terminal_executable_positions
    )
    return (
        {
            "entry_research_positions": entry_research_positions,
            "exit_research_positions": exit_research_positions,
            "entry_filled_positions": entry_filled_positions,
            "exit_filled_positions": exit_filled_positions,
            "terminal_executable_positions": terminal_executable_positions,
            "complete_positions": complete,
            "research_roundtrip_positions": research_roundtrip,
        },
        malformed,
    )


def scope_sample_metrics(scope_root: Path | None, summary_metrics: dict[str, str]) -> dict[str, Any]:
    if scope_root is None or not scope_root.exists():
        return {
            "source": "historical_summary_context_only",
            "complete_executable_roundtrip_positions": int_metric(summary_metrics, "complete_executable_roundtrip_positions") or 0,
            "research_candidate_roundtrip_count": int_metric(summary_metrics, "research_candidate_roundtrip_count") or 0,
            "entry_execution_label_grade_RESEARCH_CANDIDATE_count": int_metric(
                summary_metrics,
                "entry_execution_label_grade_RESEARCH_CANDIDATE_count",
            )
            or 0,
            "exit_execution_label_grade_RESEARCH_CANDIDATE_count": int_metric(
                summary_metrics,
                "exit_execution_label_grade_RESEARCH_CANDIDATE_count",
            )
            or 0,
            "status": "BLOCKED_NO_DEDICATED_L2_F_SCOPE",
        }

    position_sets, malformed = collect_roundtrip_position_sets(scope_root)
    complete = position_sets["complete_positions"]
    research_roundtrip = position_sets["research_roundtrip_positions"]
    entry_research_positions = position_sets["entry_research_positions"]
    exit_research_positions = position_sets["exit_research_positions"]
    return {
        "source": "dedicated_scope_canonical_stream",
        "complete_executable_roundtrip_positions": len(complete),
        "research_candidate_roundtrip_count": len(research_roundtrip),
        "entry_execution_label_grade_RESEARCH_CANDIDATE_count": len(entry_research_positions),
        "exit_execution_label_grade_RESEARCH_CANDIDATE_count": len(exit_research_positions),
        "malformed_canonical_rows": malformed,
        "status": "PASS" if malformed == 0 else "BLOCKED_MALFORMED_ROWS",
    }


def sample_gate_status(sample: dict[str, Any]) -> str:
    if sample.get("status") == "BLOCKED_NO_DEDICATED_L2_F_SCOPE":
        return "BLOCKED_NO_DEDICATED_L2_F_SCOPE"
    if int(sample.get("complete_executable_roundtrip_positions") or 0) < REQUIRED_ROUNDTRIPS:
        return "BLOCKED_INSUFFICIENT_COMPLETE_EXECUTABLE_ROUNDTRIPS"
    if int(sample.get("research_candidate_roundtrip_count") or 0) <= 0:
        return "BLOCKED_NO_RESEARCH_CANDIDATE_ROUNDTRIPS"
    if int(sample.get("entry_execution_label_grade_RESEARCH_CANDIDATE_count") or 0) <= 0:
        return "BLOCKED_NO_ENTRY_RESEARCH_CANDIDATE"
    if int(sample.get("exit_execution_label_grade_RESEARCH_CANDIDATE_count") or 0) <= 0:
        return "BLOCKED_NO_EXIT_RESEARCH_CANDIDATE"
    return "PASS"


def classify_density_row(row: dict[str, Any], required_replay_horizon_ms: int) -> str | None:
    verdict = str(row.get("verdict") or "UNKNOWN")
    replay_horizon_ms = density_horizon_audit.finite_int(row.get("replay_horizon_ms"))
    if replay_horizon_ms is None or replay_horizon_ms < required_replay_horizon_ms:
        return "RETENTION_GAP"
    if verdict in density_horizon_audit.EVALUABLE_VERDICTS:
        return None
    if verdict in {
        "NOT_EVALUABLE_NO_COVERAGE",
        "NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY",
    }:
        return "PATH_SAMPLE_COVERAGE_GAP"
    if verdict == "SPARSE_APPROX_ONLY":
        return "SPARSE_APPROX_ONLY"
    return "UNKNOWN_OR_UNTYPED_DENSITY_VERDICT"


def density_exclusion_row(
    position: str,
    blockers: dict[int, str],
) -> dict[str, Any]:
    return {
        "schema": "shadow_v2_l2_f_density_excluded_position_v1",
        "position_id": position,
        "scope": "L2_F_OFFLINE_RESEARCH_CANDIDATE",
        "exclusion_policy": "fail_closed_declared_density_retention_gate",
        "exclusion_reason_kind": "TYPED_DENSITY_RETENTION_BLOCKER",
        "typed_exclusion_reasons": sorted(set(blockers.values())),
        "horizon_blockers": [
            {"horizon_ms": horizon, "reason": reason}
            for horizon, reason in sorted(blockers.items())
        ],
        "selection_inputs": [
            "research_candidate_roundtrip_membership",
            "declared_horizon_density_verdict",
            "declared_horizon_replay_retention",
        ],
        "selection_inputs_exclude_pnl": True,
        "selection_inputs_exclude_terminal_outcome_quality": True,
        "positive_claim_supported": False,
    }


def position_level_density_retention_gate(
    scope_root: Path | None,
    research_roundtrip_positions: set[str],
    *,
    required_roundtrips: int = REQUIRED_ROUNDTRIPS,
) -> dict[str, Any]:
    if scope_root is None or not scope_root.exists():
        return {
            "verdict": "BLOCKED_NO_DEDICATED_L2_F_SCOPE",
            "required_roundtrips": required_roundtrips,
            "research_candidate_roundtrip_count": 0,
            "l2_research_evidence_complete_roundtrip_positions": 0,
        }

    declared_horizons = set(DECLARED_HORIZONS_MS)
    required_replay_horizon_ms = max(declared_horizons) + 1_000
    latest: dict[tuple[str, int], tuple[int, dict[str, Any]]] = {}
    counters = Counter()

    for idx, (row, malformed) in enumerate(iter_density_rows(scope_root) or ()):
        if malformed or row is None:
            counters["malformed_density_rows"] += 1
            continue
        counters["density_rows_input"] += 1
        if density_horizon_audit.is_validation_smoke_density_row(row):
            counters["excluded_validation_smoke_density_rows"] += 1
            continue
        position = density_horizon_audit.row_identity(row, idx)
        if position not in research_roundtrip_positions:
            counters["density_rows_excluded_outside_research_roundtrip_scope"] += 1
            continue
        counters["density_rows_research_roundtrip_scope"] += 1
        horizon = density_horizon_audit.finite_int(row.get("horizon_ms"))
        if horizon is None:
            counters["unknown_horizon_rows"] += 1
            continue
        if horizon not in declared_horizons:
            counters["undeclared_horizon_rows_in_research_scope"] += 1
            continue
        key = (position, horizon)
        current = latest.get(key)
        if current is None or density_horizon_audit.density_snapshot_sort_key(
            row
        ) > density_horizon_audit.density_snapshot_sort_key(current[1]):
            latest[key] = (idx, row)

    complete_positions: set[str] = set()
    blocked_positions: set[str] = set()
    position_blockers: dict[str, dict[int, str]] = {}
    horizon_blocker_counts: Counter[str] = Counter()
    blocker_counts = Counter()

    for position in sorted(research_roundtrip_positions):
        blockers: dict[int, str] = {}
        for horizon in sorted(declared_horizons):
            item = latest.get((position, horizon))
            if item is None:
                blocker = "MISSING_DECLARED_HORIZON"
            else:
                _idx, row = item
                blocker = classify_density_row(row, required_replay_horizon_ms)
            if blocker is not None:
                blockers[horizon] = blocker
                horizon_blocker_counts[f"{horizon}:{blocker}"] += 1
        if blockers:
            blocked_positions.add(position)
            position_blockers[position] = blockers
            blocker_counts.update(set(blockers.values()))
        else:
            complete_positions.add(position)

    malformed_or_unknown_rows = counters["malformed_density_rows"] + counters["unknown_horizon_rows"]
    if malformed_or_unknown_rows:
        verdict = "BLOCKED_L2_F_DENSITY_MALFORMED_OR_UNKNOWN_HORIZON_ROWS"
    elif len(complete_positions) >= required_roundtrips:
        verdict = POSITION_LEVEL_DENSITY_PASS_VERDICT
    else:
        verdict = "BLOCKED_L2_F_POSITION_LEVEL_DENSITY_RETENTION"

    return {
        "verdict": verdict,
        "required_roundtrips": required_roundtrips,
        "declared_supported_horizons_ms": DECLARED_HORIZONS_MS,
        "required_replay_horizon_ms": required_replay_horizon_ms,
        "research_candidate_roundtrip_count": len(research_roundtrip_positions),
        "l2_research_evidence_complete_roundtrip_positions": len(complete_positions),
        "density_excluded_roundtrip_positions": len(blocked_positions),
        "missing_declared_horizon_position_count": blocker_counts["MISSING_DECLARED_HORIZON"],
        "path_sample_coverage_gap_position_count": blocker_counts["PATH_SAMPLE_COVERAGE_GAP"],
        "sparse_approx_only_position_count": blocker_counts["SPARSE_APPROX_ONLY"],
        "retention_gap_position_count": blocker_counts["RETENTION_GAP"],
        "unknown_or_untyped_density_verdict_position_count": blocker_counts[
            "UNKNOWN_OR_UNTYPED_DENSITY_VERDICT"
        ],
        "density_rows_input": counters["density_rows_input"],
        "density_rows_research_roundtrip_scope": counters["density_rows_research_roundtrip_scope"],
        "density_rows_excluded_outside_research_roundtrip_scope": counters[
            "density_rows_excluded_outside_research_roundtrip_scope"
        ],
        "excluded_validation_smoke_density_rows": counters[
            "excluded_validation_smoke_density_rows"
        ],
        "malformed_density_rows": counters["malformed_density_rows"],
        "unknown_horizon_rows": counters["unknown_horizon_rows"],
        "undeclared_horizon_rows_in_research_scope": counters[
            "undeclared_horizon_rows_in_research_scope"
        ],
        "latest_declared_density_snapshots_in_scope": len(latest),
        "horizon_blocker_counts": dict(sorted(horizon_blocker_counts.items())),
        "positive_l2_claim_position_scope": (
            "research_candidate_roundtrips_with_all_declared_density_retention_gates"
        ),
        "non_evaluable_positions_excluded_from_positive_claim": True,
        "density_excluded_positions": [
            density_exclusion_row(position, blockers)
            for position, blockers in sorted(position_blockers.items())
        ],
        "evidence_complete_position_ids": sorted(complete_positions),
    }


def write_evidence_complete_position_scope(path: Path, gate: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    position_ids = gate.get("evidence_complete_position_ids")
    if not isinstance(position_ids, list):
        position_ids = []
    with path.open("w", encoding="utf-8") as fh:
        for position in position_ids:
            fh.write(
                json.dumps(
                    {
                        "schema": "shadow_v2_l2_f_evidence_complete_position_scope_v1",
                        "position_id": position,
                        "scope": "L2_F_OFFLINE_RESEARCH_CANDIDATE",
                    },
                    sort_keys=True,
                    separators=(",", ":"),
                )
            )
            fh.write("\n")


def write_density_excluded_positions(path: Path, gate: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    rows = gate.get("density_excluded_positions")
    if not isinstance(rows, list):
        rows = []
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            if not isinstance(row, dict):
                continue
            fh.write(json.dumps(row, sort_keys=True, separators=(",", ":")))
            fh.write("\n")


def raw_artifact_audit_consumption(scope_root: Path | None) -> list[dict[str, Any]]:
    root = str(scope_root) if scope_root else None
    return [
        {
            "path": f"{root}/shadow_position_event_v2.jsonl" if root else "shadow_position_event_v2.jsonl",
            "tracked_in_git": False,
            "consumed_by": [
                "sample_gate_canonical_roundtrip_counter",
                "shadow_v2_temporal_no_lookahead_audit.py",
                "shadow_v2_replay_lifecycle_terminal_reconciliation_audit.py",
                "account_data_hash_coverage",
                "shadow_v2_manifest_audit.py",
            ],
        },
        {
            "path": f"{root}/shadow_replay_v2.jsonl" if root else "shadow_replay_v2.jsonl",
            "tracked_in_git": False,
            "consumed_by": [
                "shadow_v2_temporal_no_lookahead_audit.py",
                "shadow_v2_replay_lifecycle_terminal_reconciliation_audit.py",
                "shadow_v2_manifest_audit.py",
            ],
        },
        {
            "path": f"{root}/shadow_lifecycle_v2.jsonl" if root else "shadow_lifecycle_v2.jsonl",
            "tracked_in_git": False,
            "consumed_by": [
                "shadow_v2_temporal_no_lookahead_audit.py",
                "shadow_v2_replay_lifecycle_terminal_reconciliation_audit.py",
                "shadow_v2_manifest_audit.py",
            ],
        },
        {
            "path": f"{root}/shadow_path_density_v2.jsonl" if root else "shadow_path_density_v2.jsonl",
            "tracked_in_git": False,
            "consumed_by": [
                "shadow_v2_path_density_horizon_audit.py raw scope",
                "shadow_v2_path_density_horizon_audit.py evidence-complete position scope",
                "position_level_density_retention_gate",
                "shadow_v2_manifest_audit.py",
            ],
        },
        {
            "path": f"{root}/launcher.stdout.log" if root else "launcher.stdout.log",
            "tracked_in_git": False,
            "consumed_by": [
                "L2-F NewPoolDetected event adapter to build_selector_candidate_universe.py",
                "shadow_v2_manifest_audit.py",
            ],
        },
    ]


def account_data_hash_coverage(scope_root: Path | None) -> dict[str, Any]:
    if scope_root is None or not scope_root.exists():
        return {
            "verdict": "BLOCKED_NO_DEDICATED_L2_F_SCOPE",
            "observed_account_state_boundary_samples": 0,
            "samples_with_account_data_hash": 0,
        }
    malformed = 0
    samples = 0
    with_hash = 0
    for row, row_malformed in iter_canonical_rows(scope_root) or ():
        if row_malformed or row is None:
            malformed += 1
            continue
        if (
            canonical_payload_schema(row) == "pool_state_sample_v2"
            and record_value(row, "source")
            in {"ACCOUNT_STATE_CORE", "ACCOUNT_STATE_UPDATE", "GEYSER_ACCOUNT_UPDATE"}
        ):
            samples += 1
            if record_value(row, "account_data_hash"):
                with_hash += 1
    if malformed:
        verdict = "BLOCKED_MALFORMED_ROWS"
    elif samples and samples == with_hash:
        verdict = "PASS_ACCOUNT_DATA_HASH_COVERAGE"
    elif samples:
        verdict = "BLOCKED_ACCOUNT_DATA_HASH_COVERAGE"
    else:
        verdict = "NOT_EVALUABLE_NO_OBSERVED_ACCOUNT_STATE_BOUNDARY_SAMPLES"
    return {
        "verdict": verdict,
        "observed_account_state_boundary_samples": samples,
        "samples_with_account_data_hash": with_hash,
        "missing_account_data_hash_count": samples - with_hash,
    }


def malformed_row_count(scope_root: Path | None) -> int:
    if scope_root is None or not scope_root.exists():
        return 0
    malformed = 0
    for iterator in (
        iter_canonical_rows(scope_root),
        iter_replay_rows(scope_root),
        iter_lifecycle_rows(scope_root),
    ):
        for _row, row_malformed in iterator or ():
            if row_malformed:
                malformed += 1
    return malformed


def rotated_artifact_reports(scope_root: Path | None) -> list[dict[str, Any]]:
    if scope_root is None or not scope_root.exists():
        return []
    return [
        artifact_rotation_report(scope_root, "shadow_position_event_v2.jsonl"),
        artifact_rotation_report(scope_root, "shadow_replay_v2.jsonl"),
        artifact_rotation_report(scope_root, "shadow_lifecycle_v2.jsonl"),
        artifact_rotation_report(scope_root, "shadow_path_density_v2.jsonl"),
    ]


def build_manifest(
    *,
    args: argparse.Namespace,
    output_root: Path,
    required_files: dict[str, bool],
    final_verdict: str,
    blockers: list[str],
) -> dict[str, Any]:
    complete, missing = status_from_required_files(required_files)
    return {
        "schema": "shadow_v2_l2_f_runtime_post_run_manifest_v1",
        "schema_version": SCHEMA_VERSION,
        "artifact": ARTIFACT,
        "run_id": args.run_id,
        "stage": "L2-F_RESEARCH_VALIDATION_RUN",
        "expected_main": EXPECTED_MAIN,
        "scope_root": str(args.scope_root) if args.scope_root else None,
        "output_root": str(output_root),
        "validation_run_executed": bool(args.scope_root and complete),
        "dedicated_l2_f_scope_present": complete,
        "required_scope_files": required_files,
        "missing_scope_files": missing,
        "final_verdict": final_verdict,
        "blockers": blockers,
        "declared_supported_horizons_ms": DECLARED_HORIZONS_MS,
        "unsupported_horizons_ms": UNDECLARED_HORIZONS_MS,
        "positive_claims_from_undeclared_horizons_allowed": False,
        "raw_artifact_audit_consumption": raw_artifact_audit_consumption(args.scope_root),
        "rotated_artifacts": rotated_artifact_reports(args.scope_root),
        "approval_flags": APPROVAL_FLAGS_FALSE,
        "runtime_decision_behavior_changes": False,
        "gatekeeper_policy_changes": False,
        "buy_reject_logic_changes": False,
        "selector_runtime_changes": False,
        "tx_jito_live_path_changes": False,
        "provider_stream_changes": False,
        "threshold_changes": False,
    }


def choose_final_verdict(report: dict[str, Any]) -> tuple[str, list[str]]:
    blockers: list[str] = []
    reuse = report.get("selector_gatekeeper_contract_reuse", {})
    if reuse.get("status") != "PASS":
        blockers.append(
            "Selector/Gatekeeper candidate universe contract reuse not proven: "
            + ",".join(str(item) for item in reuse.get("failures", []))
        )
        return VERDICT_GATEKEEPER, blockers
    gatekeeper = report["gatekeeper_denominator"]
    if gatekeeper["verdict"] == gatekeeper_audit.VERDICT_DENOMINATOR_UNKNOWN:
        blockers.append("gatekeeper candidate denominator is unknown")
        return VERDICT_GATEKEEPER, blockers
    if gatekeeper["verdict"] == gatekeeper_audit.VERDICT_UNKNOWN_REASONS:
        blockers.append("Gatekeeper reject reasons include unknown/generic buckets")
        return VERDICT_UNKNOWN, blockers
    if gatekeeper["verdict"] == gatekeeper_audit.VERDICT_THRESHOLD_STARVATION:
        blockers.append("Gatekeeper threshold starvation detected")
        return VERDICT_STARVATION, blockers
    if gatekeeper["verdict"] != gatekeeper_audit.VERDICT_COVERAGE_KNOWN:
        blockers.append(f"Gatekeeper denominator audit did not pass: {gatekeeper['verdict']}")
        return VERDICT_GATEKEEPER, blockers

    if not report["dedicated_l2_f_scope_present"]:
        blockers.append("dedicated L2-F runtime evidence scope is missing required canonical/replay/lifecycle/density files")
        return VERDICT_MANIFEST, blockers

    if int(report.get("malformed_rows") or 0) > 0:
        blockers.append("malformed JSONL rows observed in dedicated scope")
        return VERDICT_MALFORMED, blockers

    if report["sample_gates"]["status"] != "PASS":
        blockers.append(report["sample_gates"]["status"])
        return VERDICT_INSUFFICIENT_SAMPLE, blockers

    if report["temporal_audit"]["verdict"] != "PASS_TEMPORAL_NO_LOOKAHEAD_AUDIT":
        blockers.append(f"temporal audit did not pass: {report['temporal_audit']['verdict']}")
        return VERDICT_TEMPORAL, blockers

    position_density_gate = report.get("position_level_density_gate") or {}
    position_density_verdict = position_density_gate.get("verdict")
    evidence_complete_density = report.get("evidence_complete_density_audit") or {}
    evidence_complete_density_verdict = evidence_complete_density.get("verdict")
    if (
        report["density_audit"]["verdict"] not in L2_DENSITY_PASS_VERDICTS
        and (
            position_density_verdict != POSITION_LEVEL_DENSITY_PASS_VERDICT
            or evidence_complete_density_verdict != "L2_F_DENSITY_RETENTION_PASS"
        )
    ):
        blockers.append(
            "density/retention audit did not pass: "
            f"{report['density_audit']['verdict']}; "
            f"position-level gate: {position_density_verdict}; "
            f"evidence-complete density audit: {evidence_complete_density_verdict}"
        )
        return VERDICT_DENSITY, blockers
    if position_density_verdict == POSITION_LEVEL_DENSITY_PASS_VERDICT:
        complete = int(
            position_density_gate.get("l2_research_evidence_complete_roundtrip_positions") or 0
        )
        if complete < REQUIRED_ROUNDTRIPS:
            blockers.append(
                "position-level density evidence-complete roundtrips below required sample size"
            )
            return VERDICT_DENSITY, blockers

    if report["manifest_audit"]["status"] != "PASS" or report["replay_lifecycle_audit"]["verdict"] != "PASS_REPLAY_LIFECYCLE_RECONCILED":
        blockers.append("manifest or replay/lifecycle audit did not pass")
        return VERDICT_MANIFEST, blockers

    if report["account_data_hash_coverage"]["verdict"] != "PASS_ACCOUNT_DATA_HASH_COVERAGE":
        blockers.append(f"account_data_hash coverage did not pass: {report['account_data_hash_coverage']['verdict']}")
        return VERDICT_MANIFEST, blockers

    if int(report.get("unknown_untyped_blockers") or 0) > 0:
        blockers.append("unknown or untyped blockers remain")
        return VERDICT_UNKNOWN, blockers

    return VERDICT_PASS, blockers


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    output_root = args.output_root or Path("reports") / "selector" / args.run_id
    output_root.mkdir(parents=True, exist_ok=True)

    candidate_adapter_report: dict[str, Any] = {
        "status": "NOT_USED_EXPLICIT_CANDIDATE_UNIVERSE",
        "adapter_only": True,
        "parallel_denominator_model_detected": False,
        "builder_source": SELECTOR_CANDIDATE_BUILDER_SOURCE,
    }
    if (
        args.scope_root
        and args.candidate_universe == DEFAULT_CANDIDATE_UNIVERSE
        and args.candidate_manifest == DEFAULT_CANDIDATE_MANIFEST
    ):
        selector_event_paths = collect_selector_event_artifact_paths(args.run_id)
        if selector_event_paths:
            candidate_adapter_report = build_selector_candidate_universe_from_event_artifacts(
                event_paths=selector_event_paths,
                candidate_universe=output_root / "candidate_universe_v1.jsonl",
                candidate_manifest=output_root / "candidate_universe_manifest_v1.json",
            )
        else:
            candidate_adapter_report = derive_candidate_universe_from_launcher_log(
                launcher_log=args.scope_root / "launcher.stdout.log",
                run_id=args.run_id,
                candidate_universe=output_root / "candidate_universe_v1.jsonl",
                candidate_manifest=output_root / "candidate_universe_manifest_v1.json",
            )
        if (output_root / "candidate_universe_v1.jsonl").exists():
            args.candidate_universe = output_root / "candidate_universe_v1.jsonl"
        if (output_root / "candidate_universe_manifest_v1.json").exists():
            args.candidate_manifest = output_root / "candidate_universe_manifest_v1.json"
        if not (output_root / "candidate_universe_v1.jsonl").exists():
            args.candidate_universe = output_root / "candidate_universe_v1.jsonl"
        if not (output_root / "candidate_universe_manifest_v1.json").exists():
            args.candidate_manifest = output_root / "candidate_universe_manifest_v1.json"

    copy_if_exists(args.candidate_universe, output_root / "candidate_universe_v1.jsonl")
    copy_if_exists(args.candidate_manifest, output_root / "candidate_universe_manifest_v1.json")
    candidate_manifest_payload = read_json(output_root / "candidate_universe_manifest_v1.json")

    decision_paths = collect_decision_paths(args.decision_root)
    decision_evidence = {
        "schema": "shadow_v2_l2_f_gatekeeper_decision_root_evidence_v1",
        "run_id": args.run_id,
        "decision_root_inputs": [str(path) for path in (args.decision_root or [Path("logs/decisions")])],
        "gatekeeper_decision_paths": [str(path) for path in decision_paths],
        "gatekeeper_decision_path_count": len(decision_paths),
        "raw_decision_jsonl_copied_into_l2_f_scope": False,
        "decision_logs_created_denominator_rows": 0,
    }
    write_json(output_root / "gatekeeper_decision_root_evidence.json", decision_evidence)

    historical_metrics = read_summary_csv([args.historical_summary_csv])
    dedicated_metrics = read_summary_csv(args.summary_csv)
    summary_metrics = dedicated_metrics or historical_metrics
    sample = scope_sample_metrics(args.scope_root, summary_metrics)
    sample["status"] = sample_gate_status(sample)

    required_files = dedicated_scope_required_files(args.scope_root)
    dedicated_scope_present, missing_scope_files = status_from_required_files(required_files)

    gatekeeper_report = run_gatekeeper_audit(args, decision_paths)
    selector_contract_reuse = selector_gatekeeper_contract_reuse_report(
        candidate_manifest=candidate_manifest_payload,
        adapter_report=candidate_adapter_report,
        gatekeeper_report=gatekeeper_report,
    )

    if dedicated_scope_present and args.scope_root:
        position_sets, _position_set_malformed = collect_roundtrip_position_sets(args.scope_root)
        position_density_gate = position_level_density_retention_gate(
            args.scope_root,
            position_sets["research_roundtrip_positions"],
        )
        evidence_complete_scope_path = output_root / "l2_f_evidence_complete_position_scope_v1.jsonl"
        write_evidence_complete_position_scope(evidence_complete_scope_path, position_density_gate)
        position_density_gate["evidence_complete_position_scope_path"] = str(
            evidence_complete_scope_path
        )
        density_excluded_positions_path = output_root / "l2_f_density_excluded_positions_v1.jsonl"
        write_density_excluded_positions(density_excluded_positions_path, position_density_gate)
        position_density_gate["density_excluded_positions_path"] = str(
            density_excluded_positions_path
        )
        position_density_gate["density_exclusion_policy"] = (
            "fail_closed_declared_density_retention_gate"
        )
        position_density_gate["selection_inputs_exclude_pnl"] = True
        position_density_gate["selection_inputs_exclude_terminal_outcome_quality"] = True
        position_density_gate.pop("evidence_complete_position_ids", None)
        position_density_gate.pop("density_excluded_positions", None)
        temporal_report = run_json_audit(
            "shadow_v2_temporal_no_lookahead_audit.py",
            ["--scope-root", str(args.scope_root)],
        )
        density_report = run_json_audit(
            "shadow_v2_path_density_horizon_audit.py",
            [
                "--scope-root",
                str(args.scope_root),
                "--latest-density-per-position-horizon",
                "--pass-verdict",
                "L2_F_DENSITY_RETENTION_PASS",
            ],
        )
        evidence_complete_density_report = run_json_audit(
            "shadow_v2_path_density_horizon_audit.py",
            [
                "--scope-root",
                str(args.scope_root),
                "--latest-density-per-position-horizon",
                "--position-scope-jsonl",
                str(evidence_complete_scope_path),
                "--pass-verdict",
                "L2_F_DENSITY_RETENTION_PASS",
                "--output-csv",
                str(output_root / "l2_f_evidence_complete_density_audit_summary.csv"),
            ],
        )
        replay_report = run_json_audit(
            "shadow_v2_replay_lifecycle_terminal_reconciliation_audit.py",
            ["--scope-root", str(args.scope_root)],
        )
        manifest_report = run_json_audit(
            "shadow_v2_manifest_audit.py",
            [
                "--scope-root",
                str(args.scope_root),
                "--run-id",
                args.run_id,
                "--manifest-phase",
                "post_run",
                "--write-manifest",
                str(output_root / "post_run_manifest.json"),
                "--write-report-csv",
                str(output_root / "shadow_v2_manifest_report.csv"),
                "--max-sha-bytes",
                str(LARGE_L2_F_MANIFEST_SHA_BYTES),
            ],
        )
    else:
        position_density_gate = {
            "verdict": "SKIPPED_DEDICATED_L2_F_SCOPE_MISSING",
            "l2_research_evidence_complete_roundtrip_positions": 0,
            "density_excluded_roundtrip_positions": None,
        }
        temporal_report = {
            "verdict": "SKIPPED_DEDICATED_L2_F_SCOPE_MISSING",
            "fake_handoff_signature_count": None,
            "event_seq_chain_order_substitute_count": None,
            "terminal_truth_not_derived_count": None,
        }
        density_report = {"verdict": "SKIPPED_DEDICATED_L2_F_SCOPE_MISSING"}
        evidence_complete_density_report = {
            "verdict": "SKIPPED_DEDICATED_L2_F_SCOPE_MISSING"
        }
        replay_report = {"verdict": "SKIPPED_DEDICATED_L2_F_SCOPE_MISSING"}
        manifest_report = {
            "status": "BLOCKED",
            "blockers": [f"missing required scope file: {name}" for name in missing_scope_files],
        }

    precondition_density = read_summary_csv([args.precondition_density_summary])
    report = {
        "schema": "shadow_v2_l2_f_research_validation_summary_v1",
        "schema_version": SCHEMA_VERSION,
        "artifact": ARTIFACT,
        "run_id": args.run_id,
        "expected_main": EXPECTED_MAIN,
        "output_root": str(output_root),
        "scope_root": str(args.scope_root) if args.scope_root else None,
        "validation_run_executed": dedicated_scope_present,
        "dedicated_l2_f_scope_present": dedicated_scope_present,
        "missing_scope_files": missing_scope_files,
        "sample_gates": sample,
        "temporal_audit": {
            "verdict": temporal_report.get("temporal_audit_verdict") or temporal_report.get("verdict"),
            "fake_handoff_signature_count": temporal_report.get("fake_handoff_signature_count"),
            "event_seq_chain_order_substitute_count": temporal_report.get("event_seq_chain_order_substitute_count"),
            "terminal_truth_derived_count": temporal_report.get("terminal_truth_derived_count"),
            "terminal_truth_not_derived_count": temporal_report.get("terminal_truth_not_derived_count"),
        },
        "density_audit": {
            "verdict": density_report.get("verdict"),
            "declared_horizon_present_count": density_report.get("declared_horizon_present_count"),
            "declared_horizon_missing_count": density_report.get("declared_horizon_missing_count"),
            "declared_horizon_path_coverage_blocker_count": density_report.get("declared_horizon_path_coverage_blocker_count"),
            "declared_horizon_retention_blocker_count": density_report.get("declared_horizon_retention_blocker_count"),
            "l2_f_allowed_next": density_report.get("l2_f_allowed_next"),
        },
        "evidence_complete_density_audit": {
            "verdict": evidence_complete_density_report.get("verdict"),
            "declared_horizon_present_count": evidence_complete_density_report.get("declared_horizon_present_count"),
            "declared_horizon_missing_count": evidence_complete_density_report.get("declared_horizon_missing_count"),
            "declared_horizon_path_coverage_blocker_count": evidence_complete_density_report.get("declared_horizon_path_coverage_blocker_count"),
            "declared_horizon_retention_blocker_count": evidence_complete_density_report.get("declared_horizon_retention_blocker_count"),
            "position_scope_jsonl": evidence_complete_density_report.get("position_scope_jsonl"),
            "position_scope_position_count": evidence_complete_density_report.get("position_scope_position_count"),
            "density_rows_excluded_outside_position_scope": evidence_complete_density_report.get("density_rows_excluded_outside_position_scope"),
        },
        "position_level_density_gate": position_density_gate,
        "precondition_density_stage": {
            "summary_csv": str(args.precondition_density_summary),
            "density_audit_verdict": precondition_density.get("density_audit_verdict"),
            "l2_f_allowed_next": bool_metric(precondition_density, "l2_f_allowed_next"),
            "runtime_harness_density_emission_proof": bool_metric(
                precondition_density,
                "runtime_harness_density_emission_proof",
            ),
            "accepted_as_l2_f_research_sample": False,
        },
        "gatekeeper_denominator": gatekeeper_report,
        "selector_gatekeeper_contract_reuse": selector_contract_reuse,
        "selector_gatekeeper_contract_reuse_status": selector_contract_reuse.get("status"),
        "candidate_universe_builder_source": selector_contract_reuse.get(
            "candidate_universe_builder_source"
        ),
        "candidate_universe_adapter_only": selector_contract_reuse.get(
            "candidate_universe_adapter_only"
        ),
        "candidate_universe_parallel_model_detected": selector_contract_reuse.get(
            "candidate_universe_parallel_model_detected"
        ),
        "decision_logs_created_denominator_rows": selector_contract_reuse.get(
            "decision_logs_created_denominator_rows"
        ),
        "candidate_ids_from_decision_only": selector_contract_reuse.get(
            "candidate_ids_from_decision_only"
        ),
        "denominator_invariant_status": selector_contract_reuse.get(
            "denominator_invariant_status"
        ),
        "selector_contract_equivalence_tests": selector_contract_reuse.get(
            "selector_contract_equivalence_tests"
        ),
        "manifest_audit": {
            "status": manifest_report.get("status") or manifest_report.get("verdict"),
            "blockers": manifest_report.get("blockers", []),
        },
        "replay_lifecycle_audit": {"verdict": replay_report.get("verdict")},
        "account_data_hash_coverage": account_data_hash_coverage(args.scope_root),
        "malformed_rows": malformed_row_count(args.scope_root),
        "unknown_untyped_blockers": int(gatekeeper_report.get("unknown_reason_count") or 0),
        "declared_supported_horizons_ms": DECLARED_HORIZONS_MS,
        "unsupported_horizons_ms": UNDECLARED_HORIZONS_MS,
        "positive_claims_from_undeclared_horizons_allowed": False,
        "raw_artifact_audit_consumption": raw_artifact_audit_consumption(args.scope_root),
        "rotated_artifacts": rotated_artifact_reports(args.scope_root),
        "approval_flags": APPROVAL_FLAGS_FALSE,
        "runtime_decision_behavior_changes": False,
        "gatekeeper_policy_changes": False,
        "buy_reject_logic_changes": False,
        "selector_runtime_changes": False,
        "tx_jito_live_path_changes": False,
        "provider_stream_changes": False,
        "threshold_changes": False,
    }
    final_verdict, blockers = choose_final_verdict(report)
    report["final_verdict"] = final_verdict
    report["blockers"] = blockers

    runtime_manifest = build_manifest(
        args=args,
        output_root=output_root,
        required_files=required_files,
        final_verdict=final_verdict,
        blockers=blockers,
    )
    write_json(output_root / "runtime_post_run_manifest.json", runtime_manifest)
    write_json(output_root / "strict_audit_summary.json", report)
    write_summary_csv(args.output_csv, report)
    return report


def main() -> int:
    args = build_parser().parse_args()
    report = build_report(args)
    print(json.dumps(report, indent=2 if args.pretty else None, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
