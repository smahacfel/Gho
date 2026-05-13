#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import subprocess
import sys
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Iterable

from shadow_run_report import (
    BUY_LOG_NAME,
    DECISIONS_LOG_NAME,
    DEFAULT_CONFIG,
    detect_latest_run_scope,
    load_toml,
    preferred_gatekeeper_decision_plane,
    resolve_config_path,
    resolve_gatekeeper_log_path,
    resolve_runtime_path,
)

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_EXPECTED_ROLLOUT_PROFILE = "shadow-burnin-v25-repair-r2"


@dataclass
class GateResult:
    passed: bool
    details: str
    observed: Any = None


@dataclass
class Inputs:
    config_path: Path
    ghost_brain_config_path: Path
    decisions_dir: Path
    buys_log: Path
    decisions_log: Path
    coverage_audit_log: Path
    shadow_log: Path
    shadow_entry_log: Path
    shadow_lifecycle_log: Path
    events_dir: Path
    wal_dir: Path
    snapshot_dir: Path
    system_log_path: Path
    oracle_log_path: Path
    session_start_ms: int | None
    expected_rollout_profile: str
    expected_plane: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Fail-closed validation gate for the clean Gatekeeper V2.5 repair shadow burn-in."
    )
    parser.add_argument(
        "--config",
        type=Path,
        default=DEFAULT_CONFIG,
        help=f"Launcher config used for the clean repaired shadow rerun (default: {DEFAULT_CONFIG})",
    )
    parser.add_argument(
        "--expected-rollout-profile",
        default=DEFAULT_EXPECTED_ROLLOUT_PROFILE,
        help=(
            "Fail-closed rollout profile required by P6 clean validation "
            f"(default: {DEFAULT_EXPECTED_ROLLOUT_PROFILE})"
        ),
    )
    parser.add_argument("--json", action="store_true", help="Print JSON report")
    return parser.parse_args()


def iter_json_objects(path: Path) -> Iterable[dict[str, Any]]:
    if not path.exists():
        return
    decoder = json.JSONDecoder()
    with path.open("r", encoding="utf-8", errors="ignore") as fh:
        for raw_line in fh:
            line = raw_line.strip()
            if not line:
                continue
            idx = 0
            while idx < len(line):
                try:
                    value, next_idx = decoder.raw_decode(line, idx)
                except json.JSONDecodeError:
                    break
                if isinstance(value, dict):
                    yield value
                idx = next_idx
                while idx < len(line) and line[idx].isspace():
                    idx += 1


def resolve_inputs(config_arg: Path, expected_rollout_profile: str) -> Inputs:
    config_path = resolve_config_path(config_arg)
    config = load_toml(config_path)
    execution_mode = str(config.get("execution", {}).get("execution_mode", "paper")).lower()
    runtime_lane = "shadow" if execution_mode == "shadow" else "paper"
    expected_plane = preferred_gatekeeper_decision_plane(runtime_lane)
    decisions_dir = resolve_runtime_path(
        config_path,
        config.get("oracle", {}).get("decision_log_path", "logs/decisions"),
    )
    buys_log = resolve_gatekeeper_log_path(
        decisions_dir,
        BUY_LOG_NAME,
        preferred_plane=expected_plane,
    )
    decisions_log = resolve_gatekeeper_log_path(
        decisions_dir,
        DECISIONS_LOG_NAME,
        preferred_plane=expected_plane,
    )
    coverage_audit_log = decisions_dir / "seer_runtime_coverage_audit.jsonl"
    shadow_log = resolve_runtime_path(
        config_path,
        config.get("trigger", {}).get("shadow_run", {}).get("output_path", "logs/shadow_run/buys.jsonl"),
    )
    shadow_entry_log = resolve_runtime_path(
        config_path,
        config.get("execution", {}).get("shadow", {}).get("entry_log_path", "logs/shadow_run/shadow_entries.jsonl"),
    )
    shadow_lifecycle_log = resolve_runtime_path(
        config_path,
        config.get("execution", {}).get("shadow", {}).get("lifecycle_log_path")
        or shadow_entry_log.with_name("shadow_lifecycle.jsonl"),
    )
    events_dir = resolve_runtime_path(
        config_path, config.get("execution", {}).get("events", {}).get("output_dir", "datasets/events")
    )
    wal_dir = resolve_runtime_path(
        config_path, config.get("durability", {}).get("wal_dir", "data/rollout/wal")
    )
    snapshot_dir = resolve_runtime_path(
        config_path, config.get("durability", {}).get("snapshot_dir", "data/rollout/snapshots")
    )
    system_log_path = resolve_runtime_path(
        config_path, config.get("logging", {}).get("file_path", "logs/system.log")
    )
    oracle_log_path = resolve_runtime_path(
        config_path, config.get("logging", {}).get("oracle_log_path", "logs/oracle.log")
    )
    _, session_start_ms = detect_latest_run_scope(events_dir)
    ghost_brain_config_path = resolve_runtime_path(
        config_path,
        config.get("ghost_brain_config_path", "../../ghost-brain/ghost_brain_config.toml"),
    )
    return Inputs(
        config_path=config_path,
        ghost_brain_config_path=ghost_brain_config_path,
        decisions_dir=decisions_dir,
        buys_log=buys_log,
        decisions_log=decisions_log,
        coverage_audit_log=coverage_audit_log,
        shadow_log=shadow_log,
        shadow_entry_log=shadow_entry_log,
        shadow_lifecycle_log=shadow_lifecycle_log,
        events_dir=events_dir,
        wal_dir=wal_dir,
        snapshot_dir=snapshot_dir,
        system_log_path=system_log_path,
        oracle_log_path=oracle_log_path,
        session_start_ms=session_start_ms,
        expected_rollout_profile=expected_rollout_profile,
        expected_plane=expected_plane,
    )


def parse_timestamp_ms(value: Any) -> int | None:
    if isinstance(value, (int, float)):
        return int(value)
    if not isinstance(value, str) or not value:
        return None
    normalized = value.replace("Z", "+00:00")
    try:
        from datetime import datetime

        return int(datetime.fromisoformat(normalized).timestamp() * 1000)
    except ValueError:
        return None


def row_timestamp_ms(row: dict[str, Any]) -> int | None:
    for key in ("recorded_at_ms", "timestamp", "ts"):
        if key in row:
            parsed = parse_timestamp_ms(row.get(key))
            if parsed is not None:
                return parsed
    return None


def filter_rows_for_session(rows: list[dict[str, Any]], session_start_ms: int | None) -> list[dict[str, Any]]:
    if session_start_ms is None:
        return rows
    filtered: list[dict[str, Any]] = []
    for row in rows:
        row_ts = row_timestamp_ms(row)
        if row_ts is not None and row_ts >= session_start_ms:
            filtered.append(row)
    return filtered


def run_shadow_report(config_path: Path) -> tuple[dict[str, Any] | None, str | None]:
    proc = subprocess.run(
        [
            sys.executable,
            str(REPO_ROOT / "scripts" / "shadow_run_report.py"),
            "--config",
            str(config_path),
            "--json",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    stdout = proc.stdout.strip()
    if stdout:
        try:
            return json.loads(stdout), None
        except json.JSONDecodeError as exc:
            return None, f"invalid JSON from shadow_run_report: {exc}"
    if proc.returncode != 0:
        return None, proc.stderr.strip() or "shadow_run_report failed"
    try:
        return json.loads(proc.stdout), None
    except json.JSONDecodeError as exc:
        return None, f"invalid JSON from shadow_run_report: {exc}"


def row_id(row: dict[str, Any]) -> str:
    for key in ("candidate_id", "execution_candidate_id", "ab_record_id", "pool_id"):
        value = row.get(key)
        if isinstance(value, str) and value:
            return value
    return "<unknown>"


def gate_artifacts_present(inputs: Inputs, rows: list[dict[str, Any]]) -> GateResult:
    buy_log_required = any(row.get("decision_verdict_buy") is True for row in rows)
    present = {
        "buys_log": inputs.buys_log.exists() or not buy_log_required,
        "decisions_log": inputs.decisions_log.exists(),
        "coverage_audit_log": inputs.coverage_audit_log.exists(),
    }
    return GateResult(
        passed=all(present.values()),
        details=(
            " ".join(f"{key}={value}" for key, value in present.items())
            + f" buy_log_required={buy_log_required}"
        ),
        observed={**present, "buy_log_required": buy_log_required},
    )


def gate_runtime_report(shadow_report: dict[str, Any] | None, err: str | None) -> GateResult:
    if shadow_report is None:
        return GateResult(False, err or "shadow_run_report unavailable")
    gates = shadow_report.get("gates") or {}
    lifecycle_gate = gates.get("runtime_lifecycle_complete") or {}
    dispatch_gate = gates.get("dispatch_attempt_classification") or {}
    return GateResult(
        passed=shadow_report.get("verdict") == "GO",
        details=(
            f"verdict={shadow_report.get('verdict', 'UNKNOWN')} "
            f"dispatch=({dispatch_gate.get('details', 'unavailable')}) "
            f"lifecycle=({lifecycle_gate.get('details', 'unavailable')})"
        ),
        observed=gates,
    )


def gate_plane_contract(
    rows: list[dict[str, Any]],
    *,
    expected_plane: str,
    expected_rollout_profile: str,
) -> GateResult:
    if not rows:
        return GateResult(False, "no gatekeeper rows found")
    missing_contracts: list[str] = []
    wrong_plane: list[str] = []
    wrong_rollout: list[str] = []
    unknown_hash: list[str] = []
    for row in rows:
        identifier = row_id(row)
        if not row.get("decision_plane") or not row.get("rollout_profile") or not row.get("config_hash"):
            missing_contracts.append(identifier)
        if row.get("decision_plane") != expected_plane:
            wrong_plane.append(identifier)
        if row.get("rollout_profile") != expected_rollout_profile:
            wrong_rollout.append(identifier)
        config_hash = row.get("config_hash")
        if not isinstance(config_hash, str) or not config_hash or config_hash.startswith("unknown"):
            unknown_hash.append(identifier)
    passed = not (missing_contracts or wrong_plane or wrong_rollout or unknown_hash)
    return GateResult(
        passed=passed,
        details=(
            f"missing_contracts={len(missing_contracts)} "
            f"wrong_plane={len(wrong_plane)} "
            f"wrong_rollout={len(wrong_rollout)} "
            f"unknown_hash={len(unknown_hash)}"
        ),
        observed={
            "missing_contracts": missing_contracts[:20],
            "wrong_plane": wrong_plane[:20],
            "wrong_rollout": wrong_rollout[:20],
            "unknown_hash": unknown_hash[:20],
        },
    )


def gate_rollout_scope_contract(inputs: Inputs) -> GateResult:
    observed = {
        "decisions_dir": str(inputs.decisions_dir),
        "buys_log": str(inputs.buys_log),
        "decisions_log": str(inputs.decisions_log),
        "coverage_audit_log": str(inputs.coverage_audit_log),
        "shadow_log": str(inputs.shadow_log),
        "shadow_entry_log": str(inputs.shadow_entry_log),
        "shadow_lifecycle_log": str(inputs.shadow_lifecycle_log),
        "events_dir": str(inputs.events_dir),
        "wal_dir": str(inputs.wal_dir),
        "snapshot_dir": str(inputs.snapshot_dir),
        "system_log_path": str(inputs.system_log_path),
        "oracle_log_path": str(inputs.oracle_log_path),
    }

    def matches_expected_scope(label: str, raw_path: str) -> bool:
        path = Path(raw_path)
        if inputs.expected_rollout_profile in path.parts:
            return True
        if label == "shadow_log":
            return path.name == f"{inputs.expected_rollout_profile}-buys.jsonl"
        return False

    mismatched = {
        name: value
        for name, value in observed.items()
        if not matches_expected_scope(name, value)
    }
    return GateResult(
        passed=not mismatched,
        details=(
            f"expected_rollout_profile={inputs.expected_rollout_profile} "
            f"mismatched_paths={len(mismatched)}"
        ),
        observed={"mismatched_paths": mismatched, "configured_paths": observed},
    )


def gate_shadow_invariants(rows: list[dict[str, Any]]) -> GateResult:
    contradictions: list[dict[str, Any]] = []
    for row in rows:
        if row.get("decision_verdict_buy") is not True:
            continue
        pdd_hard_fail = row.get("pdd_hard_fail")
        shadow_confidence = row.get("v25_shadow_confidence")
        confidence_available = row.get("v25_confidence_available")
        if pdd_hard_fail:
            contradictions.append({"row": row_id(row), "reason": "buy_with_pdd_hard_fail"})
        elif isinstance(shadow_confidence, (int, float)) and float(shadow_confidence) <= 0.0:
            contradictions.append({"row": row_id(row), "reason": "buy_with_zero_shadow_confidence"})
        elif confidence_available is False:
            contradictions.append({"row": row_id(row), "reason": "buy_with_unavailable_confidence"})
    return GateResult(
        passed=not contradictions,
        details=f"contradictory_shadow_buys={len(contradictions)}",
        observed=contradictions[:20],
    )


def gate_availability_discipline(rows: list[dict[str, Any]]) -> GateResult:
    missing_reasons: list[dict[str, Any]] = []
    ablation_missing: list[str] = []
    for row in rows:
        identifier = row_id(row)
        if row.get("v25_confidence_available") is False and not row.get(
            "v25_confidence_unavailable_reason"
        ):
            missing_reasons.append({"row": identifier, "reason": "missing_confidence_unavailable_reason"})
        if row.get("tas_available") is False and not row.get("tas_unavailable_reason"):
            missing_reasons.append({"row": identifier, "reason": "missing_tas_unavailable_reason"})
        if not any(
            key in row
            for key in (
                "entry_drift_pct",
                "pdd_hard_fail",
                "tas_overall_score",
                "aps_regime",
            )
        ):
            ablation_missing.append(identifier)
    return GateResult(
        passed=not missing_reasons and not ablation_missing,
        details=(
            f"missing_unavailable_reasons={len(missing_reasons)} "
            f"missing_ablation_fields={len(ablation_missing)}"
        ),
        observed={
            "missing_reasons": missing_reasons[:20],
            "missing_ablation_fields": ablation_missing[:20],
        },
    )


def gate_coverage_contract(rows: list[dict[str, Any]]) -> GateResult:
    # This gate freezes the validator-required v5 surface for promotion:
    # the four fields below must always be present, even when null / [].
    # It does not require every optional v5 field or every empty map to be
    # serialized unconditionally.
    required_fields = (
        "timeout_primary_cause",
        "timeout_flags",
        "filtered_reason_keys",
        "dominant_runtime_effective_time_source",
    )
    if not rows:
        return GateResult(False, "no coverage audit rows found")
    schema_failures: list[str] = []
    missing_fields: list[dict[str, Any]] = []
    for row in rows:
        identifier = row_id(row)
        schema_version = row.get("schema_version")
        if not isinstance(schema_version, int) or schema_version < 5:
            schema_failures.append(identifier)
        absent = [field for field in required_fields if field not in row]
        if absent:
            missing_fields.append({"row": identifier, "missing": absent})
    return GateResult(
        passed=not schema_failures and not missing_fields,
        details=(
            f"schema_failures={len(schema_failures)} "
            f"missing_required_fields={len(missing_fields)}"
        ),
        observed={
            "schema_failures": schema_failures[:20],
            "missing_fields": missing_fields[:20],
        },
    )


def gate_promotion_lock(ghost_brain_config_path: Path) -> GateResult:
    if not ghost_brain_config_path.exists():
        return GateResult(False, f"missing ghost brain config: {ghost_brain_config_path}")
    config = load_toml(ghost_brain_config_path)
    v25 = config.get("gatekeeper_v2", {}).get("v25", {})
    require_adr = v25.get("require_promotion_adr")
    live_enabled = v25.get("live_execution_enabled")
    return GateResult(
        passed=bool(require_adr) and live_enabled is False,
        details=f"require_promotion_adr={require_adr} live_execution_enabled={live_enabled}",
        observed={
            "require_promotion_adr": require_adr,
            "live_execution_enabled": live_enabled,
        },
    )


# ══════════════════════════════════════════════════════════════════════════════
# P5: Shadow lifecycle + reason code completeness gates
# ══════════════════════════════════════════════════════════════════════════════

def gate_decision_reason_completeness(rows: list[dict[str, Any]]) -> GateResult:
    """P4/P5: decision_reason must never be null."""
    null_rows = [row_id(r) for r in rows if r.get("decision_reason") is None]
    return GateResult(
        passed=len(null_rows) == 0,
        details=f"decision_reason_null_count={len(null_rows)}",
        observed={"null_rows": null_rows[:20]},
    )


def gate_reason_code_completeness(rows: list[dict[str, Any]]) -> GateResult:
    """P4: reason_code must be populated in 100% of rows."""
    missing = [row_id(r) for r in rows if r.get("reason_code") is None]
    return GateResult(
        passed=len(missing) == 0,
        details=f"reason_code_missing_count={len(missing)}",
        observed={"missing_rows": missing[:20]},
    )


def gate_timeout_taxonomy(rows: list[dict[str, Any]]) -> GateResult:
    """P4: All TIMEOUT rows must have a specific subtype in both verdict_type and reason_code."""
    timeout_rows = [r for r in rows if r.get("verdict_type") is not None
                    and "TIMEOUT" in str(r.get("verdict_type", ""))]
    valid_timeout_pairs = {
        "TIMEOUT_PHASE1_NO_DATA": "TIMEOUT_PHASE1_NO_DATA",
        "TIMEOUT_PHASE1_INSUFFICIENT": "TIMEOUT_PHASE1_INSUFFICIENT",
        "TIMEOUT_DEADLINE_LOW_PHASES": "TIMEOUT_DEADLINE_LOW_PHASES",
    }
    unclassified = [
        row_id(r) for r in timeout_rows
        if r.get("verdict_type") not in valid_timeout_pairs
    ]
    reason_code_mismatch = [
        row_id(r) for r in timeout_rows
        if valid_timeout_pairs.get(str(r.get("verdict_type"))) != r.get("reason_code")
    ]
    return GateResult(
        passed=len(unclassified) == 0 and len(reason_code_mismatch) == 0 and len(timeout_rows) > 0,
        details=(
            f"timeout_rows={len(timeout_rows)} "
            f"unclassified={len(unclassified)} "
            f"reason_code_mismatch={len(reason_code_mismatch)}"
        ),
        observed={
            "unclassified_timeouts": unclassified[:20],
            "reason_code_mismatch": reason_code_mismatch[:20],
        },
    )


def gate_reason_code_verdict_consistency(rows: list[dict[str, Any]]) -> GateResult:
    """P4: verdict_type and reason_code must describe the same terminal cause."""
    expected_exact = {
        "TIMEOUT_PHASE1_NO_DATA": {"TIMEOUT_PHASE1_NO_DATA"},
        "TIMEOUT_PHASE1_INSUFFICIENT": {"TIMEOUT_PHASE1_INSUFFICIENT"},
        "TIMEOUT_DEADLINE_LOW_PHASES": {"TIMEOUT_DEADLINE_LOW_PHASES"},
        "REJECT_CORE_FAIL": {"REJECT_CORE_FAIL"},
        "REJECT_SOFT_EXCESS": {"REJECT_LEGACY_SOFT_EXCESS"},
        "REJECT_SYBIL_SOFT_EXCESS": {"REJECT_SYBIL_SOFT_EXCESS"},
        "REJECT_SYBIL_INTERFERENCE": {"REJECT_SYBIL_INTERFERENCE"},
        "REJECT_LOW_ALPHA": {"REJECT_LOW_ALPHA"},
        "REJECT_LOW_PROSPERITY": {"REJECT_LOW_PROSPERITY"},
        "REJECT_IWIM_VETO": {"REJECT_IWIM_VETO"},
        "REJECT_IWIM_LOW_CONF": {"REJECT_IWIM_LOW_CONF"},
        "REJECT_IWIM_UNKNOWN_STRICT": {"REJECT_IWIM_UNKNOWN_STRICT"},
        "REJECT_ENTRY_DRIFT": {"REJECT_PDD_ENTRY_DRIFT"},
        "REJECT_FLASH_CRASH": {"REJECT_PDD_FLASH_CRASH"},
        "REJECT_RAMPING": {"REJECT_PDD_RAMPING"},
        "REJECT_LOW_TRAJECTORY": {"REJECT_LOW_TRAJECTORY"},
        "BUY": {"BUY_NORMAL", "BUY_EARLY", "BUY_EXTENDED"},
        "EARLY_BUY": {"BUY_EARLY"},
    }
    mismatches: list[str] = []
    for row in rows:
        verdict_type = row.get("verdict_type")
        reason_code = row.get("reason_code")
        if verdict_type is None or reason_code is None:
            continue
        verdict_type = str(verdict_type)
        reason_code = str(reason_code)
        if verdict_type == "REJECT_HARD_FAIL":
            if not reason_code.startswith("HARD_FAIL_"):
                mismatches.append(row_id(row))
            continue
        if verdict_type == "REJECT_PUMP_AND_DUMP":
            if not reason_code.startswith("REJECT_PDD_"):
                mismatches.append(row_id(row))
            continue
        expected = expected_exact.get(verdict_type)
        if expected is not None and reason_code not in expected:
            mismatches.append(row_id(row))
    return GateResult(
        passed=len(mismatches) == 0,
        details=f"reason_code_verdict_mismatch={len(mismatches)}",
        observed={"mismatches": mismatches[:20]},
    )


def gate_dispatch_classification(rows: list[dict[str, Any]]) -> GateResult:
    """P5: Distinguish no_dispatch from dispatched in decision rows."""
    buy_rows = [r for r in rows if r.get("decision_verdict_buy") is True]
    reject_rows = [r for r in rows if r.get("decision_verdict_buy") is False]
    timeout_rows = [r for r in rows if r.get("decision_verdict_buy") is None
                    and r.get("decision_reason") is not None]
    return GateResult(
        passed=True,  # always passes — informational gate
        details=(
            f"no_dispatch_rejected={len(reject_rows)} "
            f"no_dispatch_eligible={len(timeout_rows)} "
            f"dispatched_candidates={len(buy_rows)}"
        ),
        observed={
            "buy_candidates": len(buy_rows),
            "rejected": len(reject_rows),
            "timed_out": len(timeout_rows),
        },
    )


def gate_path_b_confidence_availability(rows: list[dict[str, Any]]) -> GateResult:
    """P1/P6: v25_confidence must be available in >=70% of Path B rows."""
    rows_with_confidence = [
        r for r in rows
        if r.get("v25_confidence_available") is True
        and r.get("v25_confidence") is not None
    ]
    total = len(rows)
    if total == 0:
        return GateResult(False, "no rows to evaluate")
    pct = len(rows_with_confidence) / total * 100.0
    passed = pct >= 70.0
    return GateResult(
        passed=passed,
        details=(
            f"v25_confidence_available_pct={pct:.1f}% "
            f"available={len(rows_with_confidence)}/{total} "
            f"threshold=70.0%"
        ),
        observed={"pct": round(pct, 1), "available": len(rows_with_confidence), "total": total},
    )


def decision_bucket(row: dict[str, Any]) -> str | None:
    verdict_type = str(row.get("verdict_type") or "")
    if row.get("decision_verdict_buy") is True or verdict_type in {"BUY", "EARLY_BUY"}:
        return "BUY"
    if "TIMEOUT" in verdict_type or row.get("decision_verdict_buy") is None:
        return "TIMEOUT"
    if row.get("decision_verdict_buy") is False or verdict_type.startswith("REJECT"):
        return "REJECT"
    return None


def canonical_breakdown_key(row: dict[str, Any]) -> str:
    identifier = row_id(row)
    if identifier != "<unknown>":
        return identifier
    return json.dumps(row, sort_keys=True, default=str)


def build_canonical_breakdown_rows(
    decision_rows: list[dict[str, Any]],
    buy_rows: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    canonical: list[dict[str, Any]] = []
    seen: set[str] = set()
    for row in decision_rows:
        key = canonical_breakdown_key(row)
        if key in seen:
            continue
        seen.add(key)
        canonical.append(row)
    for row in buy_rows:
        key = canonical_breakdown_key(row)
        if key in seen:
            continue
        seen.add(key)
        canonical.append(row)
    return canonical


def build_decision_breakdowns(rows: list[dict[str, Any]]) -> dict[str, dict[str, dict[str, int]]]:
    dimensions = {
        "by_decision_plane": "decision_plane",
        "by_aps_regime": "aps_regime",
        "by_observation_stage": "observation_stage",
    }
    breakdowns: dict[str, dict[str, dict[str, int]]] = {
        name: {} for name in dimensions
    }
    for row in rows:
        bucket = decision_bucket(row)
        if bucket is None:
            continue
        for output_name, field_name in dimensions.items():
            raw_key = row.get(field_name)
            key = str(raw_key) if isinstance(raw_key, str) and raw_key else "<missing>"
            field_counts = breakdowns[output_name].setdefault(
                key,
                {"BUY": 0, "REJECT": 0, "TIMEOUT": 0},
            )
            field_counts[bucket] += 1
    return breakdowns


def build_report(inputs: Inputs) -> dict[str, Any]:
    shadow_report, shadow_report_err = run_shadow_report(inputs.config_path)
    buy_rows = filter_rows_for_session(list(iter_json_objects(inputs.buys_log)), inputs.session_start_ms)
    decision_rows = filter_rows_for_session(
        list(iter_json_objects(inputs.decisions_log)), inputs.session_start_ms
    )
    coverage_rows = filter_rows_for_session(
        list(iter_json_objects(inputs.coverage_audit_log)), inputs.session_start_ms
    )
    combined_rows = decision_rows + buy_rows
    canonical_breakdown_rows = build_canonical_breakdown_rows(decision_rows, buy_rows)

    gates = {
        "artifacts_present": asdict(gate_artifacts_present(inputs, combined_rows)),
        "runtime_reconciliation": asdict(gate_runtime_report(shadow_report, shadow_report_err)),
        "rollout_scope_contract": asdict(gate_rollout_scope_contract(inputs)),
        "plane_contract": asdict(
            gate_plane_contract(
                combined_rows,
                expected_plane=inputs.expected_plane,
                expected_rollout_profile=inputs.expected_rollout_profile,
            )
        ),
        "shadow_invariants": asdict(gate_shadow_invariants(combined_rows)),
        "availability_discipline": asdict(gate_availability_discipline(combined_rows)),
        "coverage_contract": asdict(gate_coverage_contract(coverage_rows)),
        "promotion_lock": asdict(gate_promotion_lock(inputs.ghost_brain_config_path)),
        # P5 gates
        "decision_reason_completeness": asdict(gate_decision_reason_completeness(combined_rows)),
        "reason_code_completeness": asdict(gate_reason_code_completeness(combined_rows)),
        "timeout_taxonomy": asdict(gate_timeout_taxonomy(combined_rows)),
        "reason_code_verdict_consistency": asdict(gate_reason_code_verdict_consistency(combined_rows)),
        "dispatch_classification": asdict(gate_dispatch_classification(combined_rows)),
        # P1/P6: Path B confidence availability after segment_sequence enrichment
        "path_b_confidence_availability": asdict(gate_path_b_confidence_availability(combined_rows)),
    }
    verdict = "GO" if all(gate["passed"] for gate in gates.values()) else "NO-GO"
    return {
        "profile": {
            "config_path": str(inputs.config_path),
            "ghost_brain_config_path": str(inputs.ghost_brain_config_path),
            "decisions_dir": str(inputs.decisions_dir),
            "buys_log": str(inputs.buys_log),
            "decisions_log": str(inputs.decisions_log),
            "coverage_audit_log": str(inputs.coverage_audit_log),
            "shadow_log": str(inputs.shadow_log),
            "shadow_entry_log": str(inputs.shadow_entry_log),
            "shadow_lifecycle_log": str(inputs.shadow_lifecycle_log),
            "events_dir": str(inputs.events_dir),
            "wal_dir": str(inputs.wal_dir),
            "snapshot_dir": str(inputs.snapshot_dir),
            "system_log_path": str(inputs.system_log_path),
            "oracle_log_path": str(inputs.oracle_log_path),
            "session_start_ms": inputs.session_start_ms,
            "expected_rollout_profile": inputs.expected_rollout_profile,
            "expected_plane": inputs.expected_plane,
        },
        "summary": {
            "buy_rows": len(buy_rows),
            "decision_rows": len(decision_rows),
            "coverage_rows": len(coverage_rows),
            "decision_breakdowns": build_decision_breakdowns(canonical_breakdown_rows),
        },
        "shadow_run_report_error": shadow_report_err,
        "shadow_run_report_verdict": None if shadow_report is None else shadow_report.get("verdict"),
        "gates": gates,
        "verdict": verdict,
    }


def format_text_report(report: dict[str, Any]) -> str:
    lines = [
        "Gatekeeper V2.5 Repair Validation",
        f"verdict={report['verdict']}",
        (
            "artifacts="
            f"buys:{report['summary']['buy_rows']} "
            f"decisions:{report['summary']['decision_rows']} "
            f"coverage:{report['summary']['coverage_rows']}"
        ),
    ]
    for name, gate in report["gates"].items():
        lines.append(f"- {name}: {'PASS' if gate['passed'] else 'FAIL'} ({gate['details']})")
    return "\n".join(lines)


def main() -> int:
    args = parse_args()
    report = build_report(resolve_inputs(args.config, args.expected_rollout_profile))
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print(format_text_report(report))
    return 0 if report["verdict"] == "GO" else 2


if __name__ == "__main__":
    raise SystemExit(main())
