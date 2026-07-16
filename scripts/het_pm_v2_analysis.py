#!/usr/bin/env python3
"""Deterministic offline summary for HET-PM V2 PR A observations.

The tool consumes only `het_pm_v2_observations_v1.jsonl`. It never edits
runtime artifacts and never promotes V2. Its denominator is the immutable
`(position_id, position_epoch)` identity, not the number of ticks.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Iterable

TOOL_ID = "het_pm_v2_analysis_v1"
SCHEMA_VERSION = 1
COLLAPSED_CANONICAL_UPDATES = 1 << 3
SAME_SLOT_ONLY = 1 << 4


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
    if not isinstance(value, expected_type) or isinstance(value, bool) and expected_type is not bool:
        raise ContractError(f"invalid type for field: {name}")
    return value


def load_records(paths: Iterable[Path]) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    records: list[dict[str, Any]] = []
    inputs: list[dict[str, Any]] = []
    for path in sorted(paths, key=lambda item: str(item)):
        if not path.is_file():
            raise ContractError(f"input does not exist: {path}")
        inputs.append({"path": str(path), "sha256": sha256(path)})
        with path.open("r", encoding="utf-8") as handle:
            for line_number, line in enumerate(handle, 1):
                if not line.strip():
                    continue
                try:
                    record = json.loads(line)
                except json.JSONDecodeError as error:
                    raise ContractError(f"{path}:{line_number}: invalid JSON: {error}") from error
                if not isinstance(record, dict):
                    raise ContractError(f"{path}:{line_number}: row is not an object")
                if require(record, "schema_version", int) != SCHEMA_VERSION:
                    raise ContractError(f"{path}:{line_number}: unsupported schema_version")
                if require(record, "policy_id", str) != "hierarchical_executable_trajectory_pm_v2":
                    raise ContractError(f"{path}:{line_number}: unexpected policy_id")
                require(record, "position_id", str)
                require(record, "position_epoch", int)
                require(record, "snapshot_id", str)
                require(record, "trajectory", dict)
                require(record, "vitality", dict)
                require(record, "v1_prequote", str)
                require(record, "v2_prequote", str)
                require(record, "quote_keys", list)
                require(record, "quote_statuses", list)
                records.append(record)
    if not records:
        raise ContractError("no HET-PM V2 observations found")
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


def analyze(records: list[dict[str, Any]], inputs: list[dict[str, Any]], fixed_floor_sol: float) -> dict[str, Any]:
    positions: dict[tuple[str, int], list[dict[str, Any]]] = defaultdict(list)
    terminal_seen: Counter[tuple[str, int]] = Counter()
    quote_counts: Counter[tuple[str, int]] = Counter()
    gate_counts: Counter[str] = Counter()
    route_counts: Counter[str] = Counter()
    quote_blockers: Counter[str] = Counter()
    cost_scenarios: dict[str, list[float]] = defaultdict(list)

    usable = collapsed = same_slot = anchored = route_classified = quote_classified = 0
    missing_to_hold = hold_quotes = duplicate_key_resolutions = micropeak_anchor_requests = 0
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
        quote_counts[identity] += resolution_count
        duplicate_key_resolutions += len(quote_keys) - len(set(quote_keys))
        for status in quote_statuses:
            if not isinstance(status, str):
                raise ContractError("quote status must be a string")
            quote_classified += 1
            if status != "resolved":
                quote_blockers[status] += 1
        if (
            record.get("v2_prequote") == "Hold"
            and record.get("v1_prequote") == "hold"
            and record.get("anchor_request") is None
            and resolution_count
        ):
            hold_quotes += resolution_count
        if record.get("anchor_request") == "quote_required_on_new_canonical_peak":
            micropeak_anchor_requests += 1
        if record.get("v2_prequote", "").startswith("Blocked") and record.get("v2_final") == "Hold":
            missing_to_hold += 1

        v2_economic_mutations += bool(record.get("v2_economic_mutation"))
        v2_proposals += bool(record.get("v2_proposal_created"))
        v2_time_stop_mutations += bool(record.get("v2_time_stop_mutation"))

        gross_bps = record.get("current_executable_gross_return_bps")
        value_sol = record.get("current_executable_value_sol")
        entry_raw = record.get("entry_value_quote_raw")
        if gross_bps is not None:
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

    return {
        "tool_id": TOOL_ID,
        "schema_version": SCHEMA_VERSION,
        "inputs": inputs,
        "denominator_contract": "unique_position_id_position_epoch",
        "record_count": len(records),
        "position_count": position_count,
        "lifecycle_integrity": {
            "duplicate_action_count": duplicate_actions,
            "duplicate_terminal_count": duplicate_terminals,
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
        "coverage": {
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
            "hold_quote_count": hold_quotes,
            "duplicate_identical_key_resolution_count": duplicate_key_resolutions,
            "anchor_quote_request_count": micropeak_anchor_requests,
            "micropeak_anchor_request_rate": ratio(micropeak_anchor_requests, len(records)),
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
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--fixed-floor-sol", type=float, default=0.0005)
    args = parser.parse_args()
    if not math.isfinite(args.fixed_floor_sol) or args.fixed_floor_sol < 0.0:
        parser.error("--fixed-floor-sol must be finite and non-negative")
    try:
        records, inputs = load_records(args.input)
        report = analyze(records, inputs, args.fixed_floor_sol)
        encoded = canonical_json(report)
    except (ContractError, ValueError) as error:
        parser.error(str(error))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(encoded)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
