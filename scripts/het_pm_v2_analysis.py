#!/usr/bin/env python3
"""Deterministic offline summary for HET-PM V2 PR A observations.

The tool consumes comparison JSONL plus an independent writer-health artifact.
It never edits runtime artifacts and never promotes V2. Its position denominator
is the immutable `(position_id, position_epoch)` identity, while writer capture
uses durable producer-attempt and enqueue-attempt counters rather than surviving
rows alone.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Iterable

TOOL_ID = "het_pm_v2_analysis_v1"
SCHEMA_VERSION = 3
POLICY_ID = "hierarchical_executable_trajectory_pm_v2"
POLICY_VERSION = 2
V1_POLICY_ID = "position_manager_lite_exit_policy_v1"
V1_POLICY_VERSION = 1
SAMPLING_MODE = "latest_canonical_state_per_monitor_tick"
TRAJECTORY_GRADE = "online_non_lookahead_sampled_trajectory"
WRITER_HEALTH_SCHEMA_VERSION = 2
WRITER_HEALTH_ARTIFACT_TYPE = "het_pm_v2_writer_health"
COLLAPSED_CANONICAL_UPDATES = 1 << 3
SAME_SLOT_ONLY = 1 << 4
ROUTES = {"pump_curve_supported", "curve_complete_pump_swap_unsupported", "unknown"}
TRAJECTORY_QUALITIES = {
    "usable", "partial_history", "insufficient_samples", "stale", "invalid", "unavailable"
}
VITALITY_STATES = {"alive", "weak", "heartbeat_only", "stale_or_unknown"}
WINNING_GATES = {
    "pending", "integrity", "crash", "hard_loss", "executable_trailing",
    "vitality_decay", "absolute_max_hold", "hold",
}
ENTRY_VALUE_SOURCES = {
    "persisted_entry_amount", "diagnostic_price_times_quantity_fallback", "unavailable"
}
V1_OUTCOMES = {
    "hold", "proposal_started", "exit_applied", "pending_recovery", "blocked", "apply_rejected"
}
V1_FINAL_BY_OUTCOME = {
    "hold": "Hold",
    "proposal_started": "ProposalStarted",
    "exit_applied": "ExitApplied",
    "pending_recovery": "PendingRecovery",
    "blocked": "Blocked",
    "apply_rejected": "ApplyRejected",
}
V1_EXIT_APPLY_STATUSES = {"not_applied", "applied", "rejected"}
V1_TERMINAL_COMMIT_STATUSES = {"not_required", "pending", "committed"}
V1_UNKNOWN_REASONS = {
    "PolicyConfigMismatch", "MarkUnavailable", "MarkStale", "MarkInvalid",
    "InvalidEntryPrice", "InvalidEntryQuantity", "InvalidRemainingQuantity",
    "QuoteUnavailable", "QuoteStale", "QuoteSemanticViolation", "QuoteNoFill",
    "QuoteQuantityMismatch",
}
V1_CANDIDATE_LABELS = {
    "stop_loss", "take_profit", "inactivity", "absolute_max_hold", "crash_guard"
}
CRASH_NOT_TRIGGERED_REASONS = {
    "Disabled", "NotShadowLane", "InvalidPositionContract", "PendingProposal",
    "MissingSample", "StaleSample", "InsufficientDistinctSlots", "InvalidOrdering",
    "NonDescendingPath", "ShortWindowDropTooSmall", "PeakDrawdownTooSmall",
}
V2_EXIT_REASONS = {
    "Crash", "HardLoss", "ExecutableTrailing", "VitalityDecay", "AbsoluteMaxHold"
}
V2_GATE_EVALUATION_ORDER = (
    "crash", "hard_loss", "executable_trailing", "vitality_decay", "absolute_max_hold"
)
V2_GATE_QUOTE_STATUSES = {
    "not_required", "pending", "blocked_pre_quote", "quote_unavailable", "resolved",
    "rejected_by_quote", "blocked_by_data", "blocked_after_quote",
}
V2_UNKNOWN_REASONS = {
    "PolicyDisabled", "InvalidPositionContract", "EntryCapitalUnavailable",
    "MarkUnavailable", "MarkStale", "MarkInvalid", "TrajectoryUnavailable",
    "TrajectoryStale", "TrajectoryInvalid", "RouteUnsupported", "RouteUnknown",
    "VitalityEvidenceStale", "AnchorUnavailable", "AnchorPositionMismatch",
    "AnchorEpochMismatch", "AnchorQuantityMismatch", "AnchorRouteMismatch",
    "AnchorQuoteModelMismatch", "AnchorPolicyConfigMismatch", "AnchorRevisionAhead",
    "QuoteUnavailable", "QuoteQuantityMismatch", "QuoteInvalid",
}
CRASH_QUOTE_REJECTION_REASONS_DEBUG = {
    "QuoteNotExecutable", "QuoteQuantityMismatch", "ExecutableReturnNotSevereEnough"
}
QUOTE_FAILURE_KINDS = {
    "MissingSnapshot", "StaleSnapshot", "InvalidReserves", "InvalidNormalization",
    "QuantityMismatch", "ZeroOutput", "SemanticViolation", "InternalFailure",
}
WRITER_HEALTH_COUNTERS = (
    "comparison_attempts",
    "comparison_ready_for_enqueue",
    "core_validation_skips",
    "final_validation_skips",
    "serialization_skips",
    "payload_oversized_skips",
    "enqueue_attempts",
    "enqueued",
    "queue_full_drops",
    "queue_closed_drops",
    "writes_succeeded",
    "writes_failed",
    "cancelled_before_write",
    "terminal_timeouts",
    "terminal_outcome_unknown",
)


class ContractError(ValueError):
    """Raised when an input cannot satisfy the PR A evidence contract."""


def canonical_json(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False) + "\n").encode()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def require(record: dict[str, Any], name: str, expected_type: type | tuple[type, ...]) -> Any:
    if name not in record:
        raise ContractError(f"missing required field: {name}")
    value = record[name]
    allowed_types = expected_type if isinstance(expected_type, tuple) else (expected_type,)
    if not isinstance(value, allowed_types) or isinstance(value, bool) and bool not in allowed_types:
        raise ContractError(f"invalid type for field: {name}")
    return value


def require_optional(
    record: dict[str, Any], name: str, expected_type: type | tuple[type, ...]
) -> Any:
    value = require(record, name, (expected_type, type(None)) if isinstance(expected_type, type) else (*expected_type, type(None)))
    return value


def reject_non_finite(value: Any, path: str = "record") -> None:
    if isinstance(value, float) and not math.isfinite(value):
        raise ContractError(f"non-finite numeric value at {path}")
    if isinstance(value, dict):
        for key, nested in value.items():
            reject_non_finite(nested, f"{path}.{key}")
    elif isinstance(value, list):
        for index, nested in enumerate(value):
            reject_non_finite(nested, f"{path}[{index}]")


def validate_trajectory(record: dict[str, Any]) -> None:
    trajectory = require(record, "trajectory", dict)
    for name in (
        "return_1500ms_bps", "return_5s_bps", "return_15s_bps",
        "drawdown_from_peak_bps", "peak_giveback_velocity_bps_per_sec",
    ):
        require_optional(trajectory, name, int)
    require_optional(trajectory, "peak_mark_price_sol", (int, float))
    for name in (
        "peak_sample_slot", "peak_sample_timestamp_ms", "time_since_peak_ms",
        "newest_sample_slot", "newest_sample_timestamp_ms", "newest_sample_age_ms",
    ):
        require_optional(trajectory, name, int)
    require(trajectory, "distinct_slots_1500ms", int)
    require(trajectory, "state_update_delta_since_previous_sample", int)
    quality = require(trajectory, "quality", str)
    if quality not in TRAJECTORY_QUALITIES:
        raise ContractError(f"invalid trajectory quality: {quality}")
    require(trajectory, "flags", int)


def validate_vitality(record: dict[str, Any]) -> None:
    vitality = require(record, "vitality", dict)
    state = require(vitality, "current_state", str)
    if state not in VITALITY_STATES:
        raise ContractError(f"invalid vitality state: {state}")
    require(vitality, "consecutive_non_alive_windows", int)
    require_optional(vitality, "last_window_at_ms", int)
    require_optional(vitality, "last_alive_at_ms", int)
    require_optional(vitality, "latest_window_price_delta_bps", int)
    require_optional(vitality, "latest_window_state_update_delta", int)
    require(vitality, "quality_fresh", bool)


def validate_crash_quote_decision(value: Any, name: str) -> None:
    if value is None:
        return
    if not isinstance(value, dict):
        raise ContractError(f"{name} must be an object or null")
    status = require(value, "status", str)
    if status not in {"confirmed", "rejected_by_quote", "blocked_by_data"}:
        raise ContractError(f"invalid {name}.status: {status}")
    if status == "rejected_by_quote":
        reason_payload = require(value, "reason", (str, dict))
        reason = (
            require(reason_payload, "reason", str)
            if isinstance(reason_payload, dict)
            else reason_payload
        )
        if reason not in {
            "quote_quantity_mismatch", "quote_not_executable", "executable_return_not_severe_enough"
        }:
            raise ContractError(f"invalid {name}.reason: {reason}")


def validate_anchor(value: Any) -> None:
    if value is None:
        return
    if not isinstance(value, dict):
        raise ContractError("anchor_before must be an object or null")
    for name in ("position_id", "route_id", "quote_model_id", "policy_config_hash", "source_snapshot_id"):
        require(value, name, str)
    for name in ("position_epoch", "remaining_quantity_raw", "quote_state_revision", "anchor_seq", "created_at_ms"):
        require(value, name, int)
    require_optional(value, "source_sample_slot", int)
    require_optional(value, "source_sample_timestamp_ms", int)
    require(value, "peak_mark_price_sol", (int, float))
    require_optional(value, "executable_value_quote_raw", int)
    require(value, "executable_value_sol", (int, float))
    require_optional(value, "executable_gross_return_bps", int)


def validate_v1_prequote(value: str, source: str) -> None:
    if value == "hold":
        return
    if value.startswith("unknown:") and value.removeprefix("unknown:") in V1_UNKNOWN_REASONS:
        return
    if value.startswith("quote_required:") and value.removeprefix("quote_required:") in V1_CANDIDATE_LABELS:
        return
    raise ContractError(f"{source}: invalid v1_prequote enum label")


def validate_v1_crash_prequote(value: str, source: str) -> None:
    if value == "Disabled":
        return
    not_triggered = re.fullmatch(r"NotTriggered \{ reason: ([A-Za-z]+) \}", value)
    if not_triggered and not_triggered.group(1) in CRASH_NOT_TRIGGERED_REASONS:
        return
    if value == "QuoteRequired { candidate: ExitCandidate { reason: CrashGuard } }":
        return
    raise ContractError(f"{source}: invalid v1_crash_prequote enum label")


def validate_v2_prequote(value: str, source: str) -> None:
    if value in {"Hold", "Pending"}:
        return
    blocked = re.fullmatch(r"Blocked\(([A-Za-z]+)\)", value)
    if blocked and blocked.group(1) in V2_UNKNOWN_REASONS:
        return
    required = re.fullmatch(r"QuoteRequired\(([A-Za-z]+)\)", value)
    if required and required.group(1) in V2_EXIT_REASONS:
        return
    raise ContractError(f"{source}: invalid v2_prequote enum label")


def validate_v2_final(value: str, source: str) -> None:
    if value in {"Hold", "Pending", "CrashBlockedByData"}:
        return
    blocked = re.fullmatch(r"Blocked\(([A-Za-z]+)\)", value)
    if blocked and blocked.group(1) in V2_UNKNOWN_REASONS:
        return
    rejected = re.fullmatch(
        r"CrashRejectedByQuote \{ reason: ([A-Za-z]+) \}", value
    )
    if rejected and rejected.group(1) in CRASH_QUOTE_REJECTION_REASONS_DEBUG:
        return
    exit_all = re.fullmatch(
        r"ExitAll \{ reason: ([A-Za-z]+), quantity_raw: ([0-9]+), "
        r"executable_gross_return_bps: (-?[0-9]+) \}",
        value,
    )
    if exit_all and exit_all.group(1) in V2_EXIT_REASONS and int(exit_all.group(2)) > 0:
        return
    raise ContractError(f"{source}: invalid v2_final enum label")


def validate_quote_status(value: str, source: str) -> None:
    if value == "resolved":
        return
    blocked = re.fullmatch(r"blocked:([A-Za-z]+)", value)
    if blocked and blocked.group(1) in QUOTE_FAILURE_KINDS:
        return
    raise ContractError(f"{source}: invalid quote status enum label")


def validate_v2_gate_evaluations(value: Any, source: str) -> None:
    if not isinstance(value, list) or len(value) != len(V2_GATE_EVALUATION_ORDER):
        raise ContractError(f"{source}: gate-evaluation lattice cardinality mismatch")
    for expected_gate, evaluation in zip(V2_GATE_EVALUATION_ORDER, value):
        if not isinstance(evaluation, dict):
            raise ContractError(f"{source}: invalid gate-evaluation row")
        if require(evaluation, "gate", str) != expected_gate:
            raise ContractError(f"{source}: gate-evaluation order or identity mismatch")
        prequote = require(evaluation, "prequote", dict)
        kind = require(prequote, "kind", str)
        if kind not in {"hold", "pending", "blocked", "quote_required"}:
            raise ContractError(f"{source}: invalid gate-evaluation prequote kind")
        if kind == "quote_required" and require(prequote, "detail", str) != expected_gate:
            raise ContractError(f"{source}: gate-evaluation quote-required reason mismatch")
        final = require(evaluation, "final_decision", dict)
        final_kind = require(final, "kind", str)
        if final_kind not in {
            "hold", "pending", "blocked", "crash_rejected_by_quote", "crash_blocked_by_data", "exit_all"
        }:
            raise ContractError(f"{source}: invalid gate-evaluation final kind")
        if final_kind == "exit_all":
            detail = require(final, "detail", dict)
            if require(detail, "reason", str) != expected_gate:
                raise ContractError(f"{source}: gate-evaluation exit reason mismatch")
            if require(detail, "quantity_raw", int) <= 0:
                raise ContractError(f"{source}: gate-evaluation exit quantity invalid")
            if require(detail, "executable_gross_return_bps", int) != require(
                evaluation, "executable_gross_return_bps", int
            ):
                raise ContractError(f"{source}: gate-evaluation executable return mismatch")
        elif evaluation.get("executable_gross_return_bps") is not None:
            raise ContractError(f"{source}: non-exit gate evaluation carries executable return")
        if require(evaluation, "quote_status", str) not in V2_GATE_QUOTE_STATUSES:
            raise ContractError(f"{source}: invalid gate-evaluation quote status")


def validate_anchor_request(value: str | None, source: str) -> None:
    if value is None or value in {
        "quote_required_on_new_canonical_peak",
        "quote_required_on_historical_peak_backfill",
    }:
        return
    blocked = re.fullmatch(r"blocked:([A-Za-z]+)", value)
    if blocked and blocked.group(1) in V2_UNKNOWN_REASONS:
        return
    raise ContractError(f"{source}: invalid anchor_request enum label")


def validate_record(record: dict[str, Any], source: str) -> None:
    reject_non_finite(record, source)
    if require(record, "schema_version", int) != SCHEMA_VERSION:
        raise ContractError(f"{source}: unsupported schema_version")
    if not require(record, "comparison_id", str):
        raise ContractError(f"{source}: empty comparison_id")
    if require(record, "policy_id", str) != POLICY_ID:
        raise ContractError(f"{source}: unexpected policy_id")
    if require(record, "policy_version", int) != POLICY_VERSION:
        raise ContractError(f"{source}: unsupported policy_version")
    if not require(record, "policy_config_hash", str):
        raise ContractError(f"{source}: empty policy_config_hash")
    if require(record, "v1_policy_id", str) != V1_POLICY_ID:
        raise ContractError(f"{source}: unexpected v1_policy_id")
    if require(record, "v1_policy_version", int) != V1_POLICY_VERSION:
        raise ContractError(f"{source}: unsupported v1_policy_version")
    if not require(record, "v1_policy_config_hash", str):
        raise ContractError(f"{source}: empty v1_policy_config_hash")
    if not require(record, "time_stop_v2_config_hash", str):
        raise ContractError(f"{source}: empty time_stop_v2_config_hash")
    if not require(record, "run_id", str):
        raise ContractError(f"{source}: empty run_id")
    if not require(record, "writer_instance_id", str):
        raise ContractError(f"{source}: empty writer_instance_id")
    if require(record, "lane", str) != "shadow":
        raise ContractError(f"{source}: lane must be shadow")
    if not require(record, "position_id", str):
        raise ContractError(f"{source}: empty position_id")
    require(record, "position_epoch", int)
    state_revision = require(record, "state_revision", int)
    remaining_quantity = require(record, "remaining_quantity_raw", int)
    if remaining_quantity <= 0:
        raise ContractError(f"{source}: remaining_quantity_raw must be positive")
    snapshot_id = require(record, "snapshot_id", str)
    if not snapshot_id:
        raise ContractError(f"{source}: empty snapshot_id")
    require(record, "observation_timestamp_ms", int)
    terminal_tick = require(record, "terminal_tick", bool)
    if require(record, "trajectory_sampling_mode", str) != SAMPLING_MODE:
        raise ContractError(f"{source}: unexpected trajectory_sampling_mode")
    if require(record, "trajectory_measurement_grade", str) != TRAJECTORY_GRADE:
        raise ContractError(f"{source}: unexpected trajectory_measurement_grade")
    if require(record, "monitor_tick_ms", int) <= 0:
        raise ContractError(f"{source}: monitor_tick_ms must be positive")

    validate_v1_prequote(require(record, "v1_prequote", str), source)
    validate_v1_crash_prequote(require(record, "v1_crash_prequote", str), source)
    v1_final = require(record, "v1_final", str)
    receipt = require(record, "v1_authority_receipt", dict)
    if require(receipt, "snapshot_id", str) != snapshot_id:
        raise ContractError(f"{source}: V1 receipt snapshot_id mismatch")
    if require(receipt, "state_revision", int) != state_revision:
        raise ContractError(f"{source}: V1 receipt state_revision mismatch")
    if require(receipt, "remaining_quantity_raw", int) != remaining_quantity:
        raise ContractError(f"{source}: V1 receipt quantity mismatch")
    outcome = require(receipt, "outcome", str)
    if outcome not in V1_OUTCOMES:
        raise ContractError(f"{source}: invalid V1 receipt outcome")
    if v1_final != V1_FINAL_BY_OUTCOME[outcome]:
        raise ContractError(f"{source}: v1_final disagrees with V1 receipt outcome")
    exit_apply_status = require(receipt, "exit_apply_status", str)
    terminal_commit_status = require(receipt, "terminal_commit_status", str)
    if exit_apply_status not in V1_EXIT_APPLY_STATUSES:
        raise ContractError(f"{source}: invalid V1 exit_apply_status")
    if terminal_commit_status not in V1_TERMINAL_COMMIT_STATUSES:
        raise ContractError(f"{source}: invalid V1 terminal_commit_status")
    axes_valid = (
        outcome == "exit_applied"
        and exit_apply_status == "applied"
        and terminal_commit_status in {"pending", "committed"}
    ) or (
        outcome == "apply_rejected"
        and exit_apply_status == "rejected"
        and terminal_commit_status == "not_required"
    ) or (
        outcome == "pending_recovery"
        and exit_apply_status == "not_applied"
        and terminal_commit_status in {"not_required", "pending", "committed"}
    ) or (
        outcome in {"hold", "proposal_started", "blocked"}
        and exit_apply_status == "not_applied"
        and terminal_commit_status == "not_required"
    )
    if not axes_valid:
        raise ContractError(f"{source}: V1 apply/terminal commit axes disagree with outcome")
    require_optional(receipt, "action_id", str)
    require_optional(receipt, "reason", str)
    validate_crash_quote_decision(require_optional(receipt, "crash_quote_decision", dict), "v1 receipt crash_quote_decision")
    expected_terminal_tick = (
        exit_apply_status == "applied" or terminal_commit_status != "not_required"
    )
    if terminal_tick != expected_terminal_tick:
        raise ContractError(f"{source}: terminal_tick disagrees with V1 receipt")

    validate_v2_prequote(require(record, "v2_prequote", str), source)
    validate_v2_final(require(record, "v2_final", str), source)
    validate_crash_quote_decision(require_optional(record, "v2_crash_quote_decision", dict), "v2_crash_quote_decision")
    if require(record, "v2_winning_gate", str) not in WINNING_GATES:
        raise ContractError(f"{source}: invalid v2_winning_gate")
    require(record, "v2_suppressed_gates_mask", int)
    validate_v2_gate_evaluations(require(record, "v2_gate_evaluations", list), source)
    consumed_by_policy = require(record, "consumed_by_policy", bool)
    v1_shadow_authority = require(record, "v1_shadow_authority", bool)
    v2_shadow_authority = require(record, "v2_shadow_authority", bool)
    if require(record, "live_authority", bool):
        raise ContractError(f"{source}: live_authority must be false")
    if consumed_by_policy:
        if not v2_shadow_authority or v1_shadow_authority:
            raise ContractError(f"{source}: active shadow policy must be V2-owned only")
    elif not v1_shadow_authority or v2_shadow_authority:
        raise ContractError(f"{source}: observe-only shadow policy must be V1-owned only")
    for name in (
        "v2_economic_mutation", "v2_proposal_created", "v2_time_stop_mutation",
        "duplicate_action_observed", "route_build_authority_changed",
        "terminal_isolation_violation", "entry_value_authoritative_for_shadow", "anchor_applied",
    ):
        require(record, name, bool)

    validate_trajectory(record)
    validate_vitality(record)
    route = require(record, "route_status", str)
    if route not in ROUTES:
        raise ContractError(f"{source}: invalid route_status")
    require_optional(record, "entry_value_quote_raw", int)
    if require(record, "entry_value_source", str) not in ENTRY_VALUE_SOURCES:
        raise ContractError(f"{source}: invalid entry_value_source")
    validate_anchor(require_optional(record, "anchor_before", dict))
    validate_anchor_request(require_optional(record, "anchor_request", str), source)
    quote_keys = require(record, "quote_keys", list)
    quote_statuses = require(record, "quote_statuses", list)
    if not all(isinstance(item, str) for item in quote_keys + quote_statuses):
        raise ContractError(f"{source}: quote keys and statuses must be strings")
    for status in quote_statuses:
        validate_quote_status(status, source)
    resolution_count = require(record, "quote_resolution_count", int)
    if resolution_count != len(quote_keys) or resolution_count != len(quote_statuses):
        raise ContractError(f"{source}: quote plan cardinalities disagree")
    if resolution_count > 2:
        raise ContractError(f"{source}: quote plan exceeds PR A bound")
    require_optional(record, "current_executable_value_sol", (int, float))
    require_optional(record, "current_executable_gross_return_bps", int)
    require_optional(record, "known_estimated_costs_sol", (int, float))


def comparison_contract(record: dict[str, Any]) -> tuple[Any, ...]:
    return (
        record["schema_version"],
        record["policy_id"],
        record["policy_version"],
        record["policy_config_hash"],
        record["v1_policy_id"],
        record["v1_policy_version"],
        record["v1_policy_config_hash"],
        record["time_stop_v2_config_hash"],
        record["trajectory_sampling_mode"],
        record["trajectory_measurement_grade"],
        record["monitor_tick_ms"],
    )


def evidence_contract_manifest(record: dict[str, Any]) -> dict[str, Any]:
    return {
        "schema_version": record["schema_version"],
        "het_pm_v2": {
            "policy_id": record["policy_id"],
            "policy_version": record["policy_version"],
            "policy_config_hash": record["policy_config_hash"],
        },
        "v1_authority": {
            "policy_id": record["v1_policy_id"],
            "policy_version": record["v1_policy_version"],
            "policy_config_hash": record["v1_policy_config_hash"],
        },
        "time_stop_v2_source": {
            "config_hash": record["time_stop_v2_config_hash"],
        },
        "sampling": {
            "mode": record["trajectory_sampling_mode"],
            "measurement_grade": record["trajectory_measurement_grade"],
            "monitor_tick_ms": record["monitor_tick_ms"],
        },
    }


def load_records(paths: Iterable[Path]) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    records: list[dict[str, Any]] = []
    inputs: list[dict[str, Any]] = []
    for path in sorted(paths, key=lambda item: str(item)):
        if not path.is_file():
            raise ContractError(f"input does not exist: {path}")
        input_start = len(records)
        with path.open("r", encoding="utf-8") as handle:
            for line_number, line in enumerate(handle, 1):
                if not line.strip():
                    continue
                try:
                    record = json.loads(
                        line,
                        parse_constant=lambda value: (_ for _ in ()).throw(
                            ValueError(f"non-finite JSON constant: {value}")
                        ),
                    )
                except (json.JSONDecodeError, ValueError) as error:
                    raise ContractError(f"{path}:{line_number}: invalid JSON: {error}") from error
                if not isinstance(record, dict):
                    raise ContractError(f"{path}:{line_number}: row is not an object")
                validate_record(record, f"{path}:{line_number}")
                record["_source_path"] = str(path)
                records.append(record)
        input_records = records[input_start:]
        if not input_records:
            raise ContractError(f"input contains no HET-PM V2 observations: {path}")
        input_contracts = {comparison_contract(record) for record in input_records}
        if len(input_contracts) != 1:
            raise ContractError(f"{path}: mixed evidence contracts are forbidden")
        manifest = evidence_contract_manifest(input_records[0])
        inputs.append(
            {
                "path": str(path),
                "sha256": sha256(path),
                "record_count": len(input_records),
                "policy_config_hash": manifest["het_pm_v2"]["policy_config_hash"],
                "v1_policy_config_hash": manifest["v1_authority"]["policy_config_hash"],
                "time_stop_v2_config_hash": manifest["time_stop_v2_source"]["config_hash"],
            }
        )
    if not records:
        raise ContractError("no HET-PM V2 observations found")
    comparison_ids = [record["comparison_id"] for record in records]
    if len(comparison_ids) != len(set(comparison_ids)):
        raise ContractError("duplicate comparison_id is forbidden")
    contracts = {comparison_contract(record) for record in records}
    if len(contracts) != 1:
        raise ContractError(
            "mixed schema/HET/V1/TimeStop/sampling contracts are forbidden in one report"
        )
    return records, inputs


def validate_writer_health(record: dict[str, Any], source: str) -> None:
    reject_non_finite(record, source)
    if require(record, "schema_version", int) != WRITER_HEALTH_SCHEMA_VERSION:
        raise ContractError(f"{source}: unsupported writer-health schema_version")
    if require(record, "artifact_type", str) != WRITER_HEALTH_ARTIFACT_TYPE:
        raise ContractError(f"{source}: unexpected writer-health artifact_type")
    if not require(record, "writer_instance_id", str):
        raise ContractError(f"{source}: empty writer_instance_id")
    if not require(record, "run_id", str):
        raise ContractError(f"{source}: empty writer-health run_id")
    if require(record, "mixed_run_ids", bool):
        raise ContractError(f"{source}: writer-health reports mixed run IDs")
    require(record, "shutdown_complete", bool)
    if require(record, "policy_id", str) != POLICY_ID:
        raise ContractError(f"{source}: unexpected writer-health policy_id")
    if require(record, "policy_version", int) != POLICY_VERSION:
        raise ContractError(f"{source}: unsupported writer-health policy_version")
    if not require(record, "policy_config_hash", str):
        raise ContractError(f"{source}: empty writer-health policy_config_hash")
    if not require(record, "sidecar_path", str):
        raise ContractError(f"{source}: empty writer-health sidecar_path")
    require(record, "started_at_ms", int)
    require(record, "snapshot_generated_at_ms", int)
    require(record, "revision", int)
    for name in WRITER_HEALTH_COUNTERS:
        if require(record, name, int) < 0:
            raise ContractError(f"{source}: negative writer-health counter: {name}")
    comparison_attempts = record["comparison_attempts"]
    local_outcomes = (
        record["comparison_ready_for_enqueue"]
        + record["core_validation_skips"]
        + record["final_validation_skips"]
        + record["serialization_skips"]
        + record["payload_oversized_skips"]
    )
    if local_outcomes > comparison_attempts:
        raise ContractError(
            f"{source}: writer-health producer counters exceed comparison attempts"
        )
    if record["enqueue_attempts"] > record["comparison_ready_for_enqueue"]:
        raise ContractError(
            f"{source}: writer-health enqueue attempts exceed ready comparisons"
        )
    attempts = record["enqueue_attempts"]
    enqueue_outcomes = (
        record["enqueued"]
        + record["queue_full_drops"]
        + record["queue_closed_drops"]
    )
    if enqueue_outcomes > attempts:
        raise ContractError(f"{source}: writer-health enqueue counters exceed attempts")
    completed = (
        record["writes_succeeded"]
        + record["writes_failed"]
        + record["cancelled_before_write"]
    )
    if completed > record["enqueued"]:
        raise ContractError(f"{source}: writer-health completion counters exceed enqueued")


def load_writer_health(
    paths: Iterable[Path],
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    records: list[dict[str, Any]] = []
    inputs: list[dict[str, Any]] = []
    for path in sorted(paths, key=lambda item: str(item)):
        if not path.is_file():
            raise ContractError(f"writer-health input does not exist: {path}")
        try:
            record = json.loads(
                path.read_text(encoding="utf-8"),
                parse_constant=lambda value: (_ for _ in ()).throw(
                    ValueError(f"non-finite JSON constant: {value}")
                ),
            )
        except (json.JSONDecodeError, ValueError) as error:
            raise ContractError(f"{path}: invalid writer-health JSON: {error}") from error
        if not isinstance(record, dict):
            raise ContractError(f"{path}: writer-health artifact is not an object")
        validate_writer_health(record, str(path))
        records.append(record)
        inputs.append(
            {
                "path": str(path),
                "sha256": sha256(path),
                "schema_version": record["schema_version"],
                "writer_instance_id": record["writer_instance_id"],
                "run_id": record["run_id"],
                "policy_config_hash": record["policy_config_hash"],
                "sidecar_path": record["sidecar_path"],
                "shutdown_complete": record["shutdown_complete"],
                "snapshot_generated_at_ms": record["snapshot_generated_at_ms"],
                "revision": record["revision"],
                "comparison_attempts": record["comparison_attempts"],
                "comparison_ready_for_enqueue": record["comparison_ready_for_enqueue"],
                "core_validation_skips": record["core_validation_skips"],
                "final_validation_skips": record["final_validation_skips"],
                "serialization_skips": record["serialization_skips"],
                "payload_oversized_skips": record["payload_oversized_skips"],
                "enqueue_attempts": record["enqueue_attempts"],
                "writes_succeeded": record["writes_succeeded"],
                "queue_full_drops": record["queue_full_drops"],
                "queue_closed_drops": record["queue_closed_drops"],
                "writes_failed": record["writes_failed"],
                "terminal_timeouts": record["terminal_timeouts"],
                "terminal_outcome_unknown": record["terminal_outcome_unknown"],
            }
        )
    if not records:
        raise ContractError("no HET-PM V2 writer-health artifacts found")
    writer_ids = [record["writer_instance_id"] for record in records]
    if len(writer_ids) != len(set(writer_ids)):
        raise ContractError("duplicate writer_instance_id is forbidden")
    contracts = {
        (
            record["schema_version"],
            record["policy_id"],
            record["policy_version"],
            record["policy_config_hash"],
        )
        for record in records
    }
    if len(contracts) != 1:
        raise ContractError("mixed writer-health contracts are forbidden in one report")
    return records, inputs


def ratio(numerator: int, denominator: int) -> float:
    return numerator / denominator if denominator else 0.0


def quantile(values: list[int], q: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    index = (len(ordered) - 1) * q
    lower = math.floor(index)
    upper = math.ceil(index)
    if lower == upper:
        return float(ordered[lower])
    weight = index - lower
    return ordered[lower] * (1.0 - weight) + ordered[upper] * weight


def comparable_path(value: str) -> str:
    path = Path(value)
    try:
        if path.exists():
            return str(path.resolve())
    except OSError:
        pass
    return value


def summarize_writer_health(
    records: list[dict[str, Any]],
    inputs: list[dict[str, Any]],
    writer_health_records: list[dict[str, Any]] | None,
) -> dict[str, Any]:
    if not writer_health_records:
        return {
            "writer_health_evidence_status": "missing",
            "coverage_unknown": True,
            "run_identity_match": False,
            "config_identity_match": False,
            "counter_consistency": False,
            "all_writers_shutdown_cleanly": False,
            "sidecar_record_count_match": False,
            "sidecar_file_identity_match": False,
            "per_writer_identity_match": False,
            "producer_denominator_consistency": False,
            "ready_enqueue_consistency": False,
            "comparison_attempts": None,
            "comparison_ready_for_enqueue": None,
            "core_validation_skip_count": None,
            "final_validation_skip_count": None,
            "serialization_skip_count": None,
            "payload_oversized_skip_count": None,
            "enqueue_attempts": None,
            "enqueued": None,
            "successfully_written": None,
            "queue_full_drop_count": None,
            "queue_closed_drop_count": None,
            "io_failure_count": None,
            "cancelled_before_write_count": None,
            "pending_or_inflight_count": None,
            "capture_ratio": None,
            "producer_validity_ratio": None,
            "writer_capture_ratio": None,
            "end_to_end_capture_ratio": None,
            "drop_ratio": None,
            "io_failure_ratio": None,
            "terminal_timeout_count": None,
            "terminal_outcome_unknown_count": None,
            "promotion_evidence_available": False,
        }

    totals = {
        name: sum(record[name] for record in writer_health_records)
        for name in WRITER_HEALTH_COUNTERS
    }
    row_groups: Counter[tuple[str, str, str]] = Counter()
    row_group_sources: dict[tuple[str, str, str], set[str]] = defaultdict(set)
    for record in records:
        key = (
            record["writer_instance_id"],
            record["run_id"],
            record["policy_config_hash"],
        )
        row_groups[key] += 1
        if "_source_path" in record:
            row_group_sources[key].add(comparable_path(record["_source_path"]))

    health_by_writer = {
        record["writer_instance_id"]: record for record in writer_health_records
    }
    run_identity_match = True
    config_identity_match = True
    sidecar_record_count_match = True
    sidecar_file_identity_match = True
    per_writer_identity_match = True
    for key, row_count in row_groups.items():
        writer_instance_id, run_id, policy_config_hash = key
        health = health_by_writer.get(writer_instance_id)
        if health is None:
            per_writer_identity_match = False
            sidecar_record_count_match = False
            continue
        if health["run_id"] != run_id:
            run_identity_match = False
            per_writer_identity_match = False
        if health["policy_config_hash"] != policy_config_hash:
            config_identity_match = False
            per_writer_identity_match = False
        if health["writes_succeeded"] != row_count:
            sidecar_record_count_match = False
        source_paths = row_group_sources.get(key, set())
        if source_paths:
            health_sidecar = comparable_path(health["sidecar_path"])
            if source_paths != {health_sidecar}:
                sidecar_file_identity_match = False
                per_writer_identity_match = False

    row_writer_ids = {key[0] for key in row_groups}
    for health in writer_health_records:
        key = (
            health["writer_instance_id"],
            health["run_id"],
            health["policy_config_hash"],
        )
        if key not in row_groups:
            per_writer_identity_match = False
            sidecar_record_count_match = False
        if health["writer_instance_id"] not in row_writer_ids:
            per_writer_identity_match = False

    enqueue_outcomes = (
        totals["enqueued"]
        + totals["queue_full_drops"]
        + totals["queue_closed_drops"]
    )
    producer_outcomes = (
        totals["comparison_ready_for_enqueue"]
        + totals["core_validation_skips"]
        + totals["final_validation_skips"]
        + totals["serialization_skips"]
        + totals["payload_oversized_skips"]
    )
    completed = (
        totals["writes_succeeded"]
        + totals["writes_failed"]
        + totals["cancelled_before_write"]
    )
    producer_denominator_consistency = producer_outcomes == totals["comparison_attempts"]
    ready_enqueue_consistency = (
        totals["comparison_ready_for_enqueue"] == totals["enqueue_attempts"]
    )
    counter_consistency = (
        producer_denominator_consistency
        and ready_enqueue_consistency
        and enqueue_outcomes == totals["enqueue_attempts"]
        and completed <= totals["enqueued"]
    )
    all_writers_shutdown_cleanly = all(
        record["shutdown_complete"] for record in writer_health_records
    )
    pending_or_inflight = max(0, totals["enqueued"] - completed)
    coverage_unknown = not (
        run_identity_match
        and config_identity_match
        and per_writer_identity_match
        and sidecar_file_identity_match
        and counter_consistency
        and all_writers_shutdown_cleanly
        and pending_or_inflight == 0
        and sidecar_record_count_match
    )
    local_skips = (
        totals["core_validation_skips"]
        + totals["final_validation_skips"]
        + totals["serialization_skips"]
        + totals["payload_oversized_skips"]
    )
    dropped = totals["queue_full_drops"] + totals["queue_closed_drops"]
    observation_loss = (
        local_skips
        + dropped
        + totals["writes_failed"]
        + totals["cancelled_before_write"]
        + totals["terminal_outcome_unknown"]
    )
    if coverage_unknown:
        status = "incomplete_or_inconsistent"
    elif observation_loss:
        status = "complete_with_observation_loss"
    else:
        status = "complete"
    return {
        "writer_health_evidence_status": status,
        "coverage_unknown": coverage_unknown,
        "run_identity_match": run_identity_match,
        "config_identity_match": config_identity_match,
        "counter_consistency": counter_consistency,
        "all_writers_shutdown_cleanly": all_writers_shutdown_cleanly,
        "sidecar_record_count_match": sidecar_record_count_match,
        "sidecar_file_identity_match": sidecar_file_identity_match,
        "per_writer_identity_match": per_writer_identity_match,
        "producer_denominator_consistency": producer_denominator_consistency,
        "ready_enqueue_consistency": ready_enqueue_consistency,
        "comparison_attempts": totals["comparison_attempts"],
        "comparison_ready_for_enqueue": totals["comparison_ready_for_enqueue"],
        "core_validation_skip_count": totals["core_validation_skips"],
        "final_validation_skip_count": totals["final_validation_skips"],
        "serialization_skip_count": totals["serialization_skips"],
        "payload_oversized_skip_count": totals["payload_oversized_skips"],
        "enqueue_attempts": totals["enqueue_attempts"],
        "enqueued": totals["enqueued"],
        "successfully_written": totals["writes_succeeded"],
        "queue_full_drop_count": totals["queue_full_drops"],
        "queue_closed_drop_count": totals["queue_closed_drops"],
        "io_failure_count": totals["writes_failed"],
        "cancelled_before_write_count": totals["cancelled_before_write"],
        "pending_or_inflight_count": pending_or_inflight,
        "capture_ratio": ratio(totals["writes_succeeded"], totals["comparison_attempts"]),
        "producer_validity_ratio": ratio(
            totals["comparison_ready_for_enqueue"], totals["comparison_attempts"]
        ),
        "writer_capture_ratio": ratio(totals["writes_succeeded"], totals["enqueue_attempts"]),
        "end_to_end_capture_ratio": ratio(
            totals["writes_succeeded"], totals["comparison_attempts"]
        ),
        "drop_ratio": ratio(dropped, totals["enqueue_attempts"]),
        "io_failure_ratio": ratio(totals["writes_failed"], totals["enqueue_attempts"]),
        "terminal_timeout_count": totals["terminal_timeouts"],
        "terminal_outcome_unknown_count": totals["terminal_outcome_unknown"],
        "promotion_evidence_available": status == "complete",
    }


def analyze(
    records: list[dict[str, Any]],
    inputs: list[dict[str, Any]],
    fixed_floor_sol: float,
    writer_health_records: list[dict[str, Any]] | None = None,
    writer_health_inputs: list[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    positions: dict[tuple[str, int], list[dict[str, Any]]] = defaultdict(list)
    terminal_seen: Counter[tuple[str, int]] = Counter()
    quote_counts: Counter[tuple[str, int]] = Counter()
    gate_counts: Counter[str] = Counter()
    route_counts: Counter[str] = Counter()
    quote_blockers: Counter[str] = Counter()
    cost_scenarios: dict[str, list[float]] = defaultdict(list)

    usable = collapsed = same_slot = anchored = route_classified = quote_classified = 0
    missing_to_hold = hold_quotes = duplicate_key_resolutions = micropeak_anchor_requests = 0
    historical_peak_anchor_requests = 0
    current_executable_record_count = 0
    quote_planned_record_count = 0
    quote_planned_current_executable_record_count = 0
    quote_planned_stale_snapshot_record_count = 0
    v2_economic_mutations = v2_proposals = v2_time_stop_mutations = 0

    for record in records:
        identity = (record["position_id"], record["position_epoch"])
        positions[identity].append(record)
        if record.get("terminal_tick"):
            terminal_seen[identity] += 1

        trajectory = record["trajectory"]
        quality = trajectory.get("quality")
        flags = trajectory.get("flags")
        if not isinstance(flags, int):
            raise ContractError("trajectory.flags must be an integer bitset")
        usable += quality == "usable"
        collapsed += bool(flags & COLLAPSED_CANONICAL_UPDATES)
        same_slot += bool(flags & SAME_SLOT_ONLY)
        anchored += record.get("anchor_before") is not None or bool(record.get("anchor_applied"))

        route = require(record, "route_status", str)
        route_counts[route] += 1
        route_classified += route in {"pump_curve_supported", "curve_complete_pump_swap_unsupported", "unknown"}
        gate_counts[require(record, "v2_winning_gate", str)] += 1

        quote_keys = record["quote_keys"]
        quote_statuses = record["quote_statuses"]
        resolution_count = require(record, "quote_resolution_count", int)
        if resolution_count != len(quote_statuses) or len(quote_keys) != len(quote_statuses):
            raise ContractError("quote plan cardinalities disagree")
        if resolution_count > 2:
            raise ContractError("quote plan exceeds PR A bound")
        quote_planned = resolution_count > 0
        if quote_planned:
            quote_planned_record_count += 1
        quote_counts[identity] += resolution_count
        duplicate_key_resolutions += len(quote_keys) - len(set(quote_keys))
        row_has_stale_snapshot_quote = False
        for status in quote_statuses:
            if not isinstance(status, str):
                raise ContractError("quote status must be a string")
            quote_classified += 1
            if status != "resolved":
                quote_blockers[status] += 1
            if status == "blocked:StaleSnapshot":
                row_has_stale_snapshot_quote = True
        if quote_planned and row_has_stale_snapshot_quote:
            quote_planned_stale_snapshot_record_count += 1
        if (
            record.get("v2_prequote") == "Hold"
            and record.get("v1_prequote") == "hold"
            and record.get("anchor_request") is None
            and resolution_count
        ):
            hold_quotes += resolution_count
        if record.get("anchor_request") == "quote_required_on_new_canonical_peak":
            micropeak_anchor_requests += 1
        if record.get("anchor_request") == "quote_required_on_historical_peak_backfill":
            historical_peak_anchor_requests += 1
        if record.get("v2_prequote", "").startswith("Blocked") and record.get("v2_final") == "Hold":
            missing_to_hold += 1

        v2_economic_mutations += bool(record.get("v2_economic_mutation"))
        v2_proposals += bool(record.get("v2_proposal_created"))
        v2_time_stop_mutations += bool(record.get("v2_time_stop_mutation"))

        gross_bps = record.get("current_executable_gross_return_bps")
        value_sol = record.get("current_executable_value_sol")
        entry_raw = record.get("entry_value_quote_raw")
        if gross_bps is not None:
            current_executable_record_count += 1
            if quote_planned:
                quote_planned_current_executable_record_count += 1
            if not isinstance(gross_bps, int):
                raise ContractError("current_executable_gross_return_bps must be an integer")
            cost_scenarios["gross_bps"].append(float(gross_bps))
            for cost_bps in (50, 100, 200):
                cost_scenarios[f"gross_minus_{cost_bps}bps"].append(float(gross_bps - cost_bps))
            if isinstance(value_sol, (int, float)) and isinstance(entry_raw, int) and entry_raw > 0:
                entry_sol = entry_raw / 1_000_000_000.0
                fixed_floor_return_bps = 10_000.0 * ((value_sol - fixed_floor_sol) / entry_sol - 1.0)
                if math.isfinite(fixed_floor_return_bps):
                    cost_scenarios["gross_minus_fixed_floor_bps"].append(fixed_floor_return_bps)

    quote_values = list(quote_counts.values())
    position_count = len(positions)
    duplicate_actions = sum(bool(record.get("duplicate_action_observed")) for record in records)
    duplicate_terminals = sum(max(0, count - 1) for count in terminal_seen.values())

    writer_health = summarize_writer_health(records, inputs, writer_health_records)
    evidence_contract = evidence_contract_manifest(records[0])
    evidence_contract["writer_health"] = {
        "schema_version": WRITER_HEALTH_SCHEMA_VERSION,
        "artifact_type": WRITER_HEALTH_ARTIFACT_TYPE,
        "required": True,
        "producer_denominator_boundary": "before_core_final_serialization_validation",
        "join_key": "writer_instance_id+run_id+policy_config_hash",
        "complete_coverage_requires_clean_shutdown": True,
        "complete_coverage_requires_per_writer_row_count_match": True,
        "promotion_requires_zero_observation_loss": True,
    }

    return {
        "tool_id": TOOL_ID,
        "schema_version": SCHEMA_VERSION,
        "evidence_contract": evidence_contract,
        "inputs": inputs,
        "writer_health_inputs": writer_health_inputs or [],
        "denominator_contract": "unique_position_id_position_epoch",
        "record_count": len(records),
        "position_count": position_count,
        "lifecycle_integrity": {
            "evidence_class": "producer_asserted_plus_sidecar_internal_consistency_only",
            "independent_reconciliation_status": "not_evaluated",
            "duplicate_action_count": duplicate_actions,
            "duplicate_terminal_count": duplicate_terminals,
            # In authoritative-shadow mode these are expected observations:
            # V2 selects a shadow proposal and the shared shadow executor
            # records the simulated fill. They are intentionally distinct
            # from a live-authority violation, which this schema rejects.
            "v2_shadow_economic_mutation_count": v2_economic_mutations,
            "v2_shadow_proposal_creation_count": v2_proposals,
            "v2_live_economic_mutation_count": 0,
            "v2_live_proposal_creation_count": 0,
            "live_authority_violation_count": 0,
            # Compatibility names retained for existing offline consumers.
            "v2_economic_mutation_count": v2_economic_mutations,
            "v2_proposal_creation_count": v2_proposals,
            "route_build_authority_change_count": sum(
                bool(record.get("route_build_authority_changed")) for record in records
            ),
            "time_stop_parity_violation_count": v2_time_stop_mutations,
            "terminal_isolation_violation_count": sum(
                bool(record.get("terminal_isolation_violation")) for record in records
            ),
        },
        "producer_asserted_integrity": {
            "evidence_origin": "het_pm_v2_runtime_record_self_report",
            "v2_shadow_economic_mutation_count": v2_economic_mutations,
            "v2_shadow_proposal_creation_count": v2_proposals,
            "v2_live_economic_mutation_count": 0,
            "v2_live_proposal_creation_count": 0,
            "live_authority_violation_count": 0,
            "v2_economic_mutation_count": v2_economic_mutations,
            "v2_proposal_creation_count": v2_proposals,
            "route_build_authority_change_count": sum(
                bool(record["route_build_authority_changed"]) for record in records
            ),
            "time_stop_parity_violation_count": v2_time_stop_mutations,
            "terminal_isolation_violation_count": sum(
                bool(record["terminal_isolation_violation"]) for record in records
            ),
            "promotion_evidence": False,
        },
        "independently_measured_integrity": {
            "status": "not_evaluated_requires_lifecycle_reconciliation_artifact",
            "reconciliation_artifact_present": False,
            "promotion_gate_1_satisfied": False,
            "sidecar_internal_duplicate_terminal_count": duplicate_terminals,
        },
        "coverage": {
            "writer_health": writer_health,
            "trajectory_usable_record_count": usable,
            "trajectory_usable_rate": ratio(usable, len(records)),
            "collapsed_updates_record_count": collapsed,
            "collapsed_updates_rate": ratio(collapsed, len(records)),
            "same_slot_only_record_count": same_slot,
            "anchor_covered_record_count": anchored,
            "anchor_coverage_rate": ratio(anchored, len(records)),
            "route_classification_coverage_rate": ratio(route_classified, len(records)),
            "quote_classification_coverage_rate": ratio(quote_classified, sum(quote_counts.values())),
            "missing_to_hold_violation_count": missing_to_hold,
            "routes": dict(sorted(route_counts.items())),
            "quote_blockers": dict(sorted(quote_blockers.items())),
            "winning_gates": dict(sorted(gate_counts.items())),
        },
        "quote_budget": {
            "quote_count_per_position_p50": quantile(quote_values, 0.50),
            "quote_count_per_position_p95": quantile(quote_values, 0.95),
            "quote_count_per_position_max": max(quote_values, default=0),
            "all_tick_current_executable_record_count": current_executable_record_count,
            "all_tick_current_executable_presence_rate": ratio(
                current_executable_record_count, len(records)
            ),
            "quote_planned_record_count": quote_planned_record_count,
            "quote_not_planned_record_count": len(records) - quote_planned_record_count,
            "quote_required_current_executable_record_count": (
                quote_planned_current_executable_record_count
            ),
            "quote_required_current_executable_resolution_rate": ratio(
                quote_planned_current_executable_record_count, quote_planned_record_count
            ),
            "quote_required_stale_snapshot_record_count": (
                quote_planned_stale_snapshot_record_count
            ),
            "quote_required_stale_snapshot_rate": ratio(
                quote_planned_stale_snapshot_record_count, quote_planned_record_count
            ),
            "hold_quote_count": hold_quotes,
            "duplicate_identical_key_resolution_count": duplicate_key_resolutions,
            "anchor_quote_request_count": micropeak_anchor_requests
            + historical_peak_anchor_requests,
            "new_canonical_peak_anchor_request_count": micropeak_anchor_requests,
            "historical_peak_anchor_request_count": historical_peak_anchor_requests,
            "micropeak_anchor_request_rate": ratio(micropeak_anchor_requests, len(records)),
            "historical_peak_anchor_request_rate": ratio(
                historical_peak_anchor_requests, len(records)
            ),
            "between_tick_cache_reuse_violation_count": None,
            "between_tick_cache_reuse_measurement_status": (
                "not_observable_from_pr_a_sidecar; enforced_by_runtime_local_quote_plan"
            ),
        },
        "cost_scenarios": {
            "fixed_floor_sol": fixed_floor_sol,
            "sample_count": len(cost_scenarios.get("gross_bps", [])),
            "mean_return_bps": {
                name: sum(values) / len(values) for name, values in sorted(cost_scenarios.items()) if values
            },
            "measurement": "gross_executable_value_with_offline_hypothetical_costs",
            "authoritative_net_pnl": False,
        },
        "promotion_gate_evaluated": False,
        "promotion_gate_passed": False,
        "counterfactual_outcome_attribution_status": (
            "not_evaluated; requires explicit lifecycle_and_replay_join"
        ),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", action="append", type=Path, required=True)
    parser.add_argument("--writer-health", action="append", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--fixed-floor-sol", type=float, default=0.0005)
    args = parser.parse_args()
    if not math.isfinite(args.fixed_floor_sol) or args.fixed_floor_sol < 0.0:
        parser.error("--fixed-floor-sol must be finite and non-negative")
    try:
        records, inputs = load_records(args.input)
        writer_health_records, writer_health_inputs = load_writer_health(args.writer_health)
        report = analyze(
            records,
            inputs,
            args.fixed_floor_sol,
            writer_health_records,
            writer_health_inputs,
        )
        encoded = canonical_json(report)
    except (ContractError, ValueError) as error:
        parser.error(str(error))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(encoded)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
