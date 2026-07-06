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
import hashlib
import json
import re
import shutil
import subprocess
import sys
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import shadow_v2_gatekeeper_coverage_denominator_audit as gatekeeper_audit
import shadow_v2_path_density_horizon_audit as density_horizon_audit
from shadow_v2_offline_audit_common import (
    canonical_payload_schema,
    envelope,
    iter_canonical_rows,
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


def derive_candidate_universe_from_launcher_log(
    *,
    launcher_log: Path,
    run_id: str,
    candidate_universe: Path,
    candidate_manifest: Path,
) -> bool:
    if not launcher_log.exists():
        return False

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
            birth_slot = int(match.group("slot"))
            birth_ts_ms = parse_log_ts_ms(match.group("ts")) or 0
            key = (base_mint, pool_id)
            if key in rows_by_key:
                continue
            candidate_id = f"{base_mint}:{pool_id}:{birth_ts_ms}"
            identity = hashlib.sha256(candidate_id.encode("utf-8")).hexdigest()
            rows_by_key[key] = {
                "candidate_id": candidate_id,
                "candidate_id_source": "launcher_stdout_new_pool_detected",
                "candidate_identity_hash": identity,
                "candidate_identity_missing_fields": [],
                "candidate_universe_status": "ok",
                "cohort": "pumpfun_bonding_curve_sol_v1",
                "cohort_in_scope": True,
                "stream_completeness_ok": True,
                "run_id": run_id,
                "event_type": "NewPoolDetected",
                "raw_source_kind": "launcher_stdout_new_pool_detected",
                "universe_source_kind": "event_artifact",
                "event_source": str(launcher_log),
                "event_source_index": idx,
                "base_mint": base_mint,
                "mint_id": base_mint,
                "pool_id": pool_id,
                "bonding_curve": pool_id,
                "birth_slot": birth_slot,
                "birth_ts_ms": birth_ts_ms,
                "birth_create_event_verified": True,
                "decision_context_join_key": f"mint_pool:{base_mint}:{pool_id}",
                "quote_mint": "So11111111111111111111111111111111111111112",
                "quote_mint_is_sol": True,
                "selector_schema_version": 1,
            }

    if not rows_by_key:
        return False

    rows = list(rows_by_key.values())
    candidate_universe.parent.mkdir(parents=True, exist_ok=True)
    with candidate_universe.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True, separators=(",", ":")))
            fh.write("\n")

    write_json(
        candidate_manifest,
        {
            "artifact": "candidate_universe_v1",
            "status": "ok",
            "scope_kind": "l2_f_runtime_launcher_log",
            "denominator_source": "launcher_stdout_new_pool_detected",
            "denominator_invariant_status": "PASS",
            "decision_logs_created_denominator_rows": 0,
            "candidate_ids_from_decision_only": 0,
            "decision_only_rows_skipped": 0,
            "event_denominator_rows_after_dedupe": len(rows),
            "rows_written": len(rows),
            "event_load": {
                "rows_read": rows_read,
                "rows_loaded": matched,
                "skipped_counts": {"non_new_pool_detected_log_line": max(rows_read - matched, 0)},
            },
            "decision_context_join_key_counts": {"mint_pool": len(rows)},
            "decision_context_rows_joined": len(rows),
            "decision_context_rows_ambiguous": 0,
            "duplicates": max(matched - len(rows), 0),
            "input_event_paths": [str(launcher_log)],
            "input_decision_paths": [],
            "status_counts": {"ok": len(rows)},
            "universe_contract": {
                "cohort": "SOL-paired pump.fun NewPoolDetected launcher event observations",
                "decision_logs": "context_only_not_denominator_by_default",
            },
        },
    )
    return True


def write_summary_csv(path: Path, report: dict[str, Any]) -> None:
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
        ("density_retention_verdict", report["density_audit"]["verdict"], ""),
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
        ("unknown_untyped_blockers", report["unknown_untyped_blockers"], ""),
        ("manifest_status", report["manifest_audit"]["status"], ""),
        ("replay_lifecycle_verdict", report["replay_lifecycle_audit"]["verdict"], ""),
        ("account_data_hash_coverage_verdict", report["account_data_hash_coverage"]["verdict"], ""),
        ("fake_handoff_signature_count", report["temporal_audit"].get("fake_handoff_signature_count"), ""),
        ("event_seq_chain_order_substitute_count", report["temporal_audit"].get("event_seq_chain_order_substitute_count"), ""),
        ("terminal_truth_not_derived_count", report["temporal_audit"].get("terminal_truth_not_derived_count"), ""),
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
            if execution_grade(row) == "RESEARCH_CANDIDATE":
                entry_research_positions.add(pos)
        elif schema == "shadow_exit_fill_v2" and fill_status(row) == "FILLED":
            exit_filled_positions.add(pos)
            if execution_grade(row) == "RESEARCH_CANDIDATE":
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

    for idx, row, malformed in density_horizon_audit.iter_density_jsonl(
        scope_root / "shadow_path_density_v2.jsonl"
    ) or ():
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
    position_blockers: dict[str, set[str]] = {}
    horizon_blocker_counts: Counter[str] = Counter()
    blocker_counts = Counter()

    for position in sorted(research_roundtrip_positions):
        blockers: set[str] = set()
        for horizon in sorted(declared_horizons):
            item = latest.get((position, horizon))
            if item is None:
                blocker = "MISSING_DECLARED_HORIZON"
            else:
                _idx, row = item
                blocker = classify_density_row(row, required_replay_horizon_ms)
            if blocker is not None:
                blockers.add(blocker)
                horizon_blocker_counts[f"{horizon}:{blocker}"] += 1
        if blockers:
            blocked_positions.add(position)
            position_blockers[position] = blockers
            blocker_counts.update(blockers)
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
    if (
        report["density_audit"]["verdict"] not in L2_DENSITY_PASS_VERDICTS
        and position_density_verdict != POSITION_LEVEL_DENSITY_PASS_VERDICT
    ):
        blockers.append(
            "density/retention audit did not pass: "
            f"{report['density_audit']['verdict']}; "
            f"position-level gate: {position_density_verdict}"
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

    if (
        args.scope_root
        and args.candidate_universe == DEFAULT_CANDIDATE_UNIVERSE
        and args.candidate_manifest == DEFAULT_CANDIDATE_MANIFEST
    ):
        derive_candidate_universe_from_launcher_log(
            launcher_log=args.scope_root / "launcher.stdout.log",
            run_id=args.run_id,
            candidate_universe=output_root / "candidate_universe_v1.jsonl",
            candidate_manifest=output_root / "candidate_universe_manifest_v1.json",
        )
        if (output_root / "candidate_universe_v1.jsonl").exists():
            args.candidate_universe = output_root / "candidate_universe_v1.jsonl"
        if (output_root / "candidate_universe_manifest_v1.json").exists():
            args.candidate_manifest = output_root / "candidate_universe_manifest_v1.json"

    copy_if_exists(args.candidate_universe, output_root / "candidate_universe_v1.jsonl")
    copy_if_exists(args.candidate_manifest, output_root / "candidate_universe_manifest_v1.json")

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
        position_density_gate.pop("evidence_complete_position_ids", None)
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
