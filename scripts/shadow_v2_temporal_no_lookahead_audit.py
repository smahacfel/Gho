#!/usr/bin/env python3
from __future__ import annotations

from collections import Counter, defaultdict
from typing import Any

from shadow_v2_offline_audit_common import (
    canonical_payload_schema,
    canonical_rows,
    emit,
    envelope,
    event_order_key,
    limitations,
    lifecycle_rows,
    nested_record,
    parser,
    position_id,
    replay_rows,
)

CHAIN_COMPONENTS = [
    "slot",
    "block_time",
    "signature",
    "transaction_index_or_unknown",
    "instruction_index_or_unknown",
    "inner_instruction_index_or_unknown",
    "log_index_or_unknown",
    "event_seq_in_process",
    "observed_at_wall_ms",
]

CHAIN_ORDER_VALUE_CLASSIFICATIONS = {
    "UNKNOWN",
    "NOT_APPLICABLE",
    "DERIVED",
    "RUNTIME_LOCAL",
}

ORDERING_REQUIRED_SCHEMAS = {
    "pool_state_sample_v2",
    "shadow_entry_attempt_v2",
    "shadow_entry_fill_v2",
    "shadow_path_sample_v2",
    "shadow_exit_attempt_v2",
    "shadow_exit_fill_v2",
    "shadow_terminal_truth_v2",
}

ORDERING_EXEMPTIONS_BY_SCHEMA = {
    "shadow_position_v2": {
        "ORDERING_EXEMPT_POSITION_CREATED",
        "ORDERING_EXEMPT_VALIDATION_SMOKE_MARKER",
    }
}

TRANSACTION_SOURCE_PROOF_SCHEMAS = {
    "shadow_entry_attempt_v2",
    "shadow_entry_fill_v2",
    "shadow_path_sample_v2",
    "shadow_exit_attempt_v2",
    "shadow_exit_fill_v2",
}

ACCOUNT_STATE_SOURCE_PROOF_SCHEMAS = {"pool_state_sample_v2"}

TRANSACTION_SOURCE_COMPONENTS = [
    ("slot", "TRANSACTION_SOURCE_SLOT"),
    ("block_time", "TRANSACTION_SOURCE_BLOCK_TIME"),
    ("signature", "TRANSACTION_SOURCE_SIGNATURE"),
    ("transaction_index_or_unknown", "TRANSACTION_SOURCE_TRANSACTION_INDEX"),
    ("instruction_index_or_unknown", "TRANSACTION_SOURCE_INSTRUCTION_INDEX"),
]

ACCOUNT_STATE_SOURCE_FIELDS = [
    ("account_data_hash", "ACCOUNT_STATE_SOURCE_HASH"),
    ("source_account_pubkey", "ACCOUNT_STATE_SOURCE_PUBKEY"),
    ("source_account_slot", "ACCOUNT_STATE_SOURCE_SLOT"),
    ("source_write_version", "ACCOUNT_STATE_SOURCE_WRITE_VERSION"),
]

TERMINAL_DERIVED_COMPONENTS = [
    "slot",
    "block_time",
    "signature",
    "transaction_index_or_unknown",
    "instruction_index_or_unknown",
    "inner_instruction_index_or_unknown",
    "log_index_or_unknown",
]


def ordering_exemption(row: dict) -> str | None:
    value = row.get("ordering_exemption")
    if isinstance(value, str) and value:
        return value
    return None


def known_source_value(value: Any) -> bool:
    if value is None:
        return False
    if isinstance(value, str):
        return bool(value.strip()) and value not in CHAIN_ORDER_VALUE_CLASSIFICATIONS
    return value not in CHAIN_ORDER_VALUE_CLASSIFICATIONS


def missing_component_label(prefix: str, value: Any) -> str:
    if isinstance(value, str) and value in CHAIN_ORDER_VALUE_CLASSIFICATIONS:
        return f"{prefix}_{value}"
    if isinstance(value, str) and not value.strip():
        return f"{prefix}_EMPTY"
    return f"{prefix}_UNKNOWN"


def transaction_source_proof_missing(eok: dict[str, Any]) -> list[str]:
    missing: list[str] = []
    for component, prefix in TRANSACTION_SOURCE_COMPONENTS:
        value = eok.get(component)
        if not known_source_value(value):
            missing.append(missing_component_label(prefix, value))
    return missing


def account_state_source_proof_missing(row: dict[str, Any]) -> list[str]:
    record = nested_record(row)
    missing: list[str] = []
    for field, prefix in ACCOUNT_STATE_SOURCE_FIELDS:
        value = record.get(field)
        if not known_source_value(value):
            missing.append(missing_component_label(prefix, value))
    return missing


def accepted_not_applicable_count(schema: str, eok: dict[str, Any]) -> int:
    count = 0
    if eok.get("log_index_or_unknown") == "NOT_APPLICABLE":
        count += 1
    if (
        schema not in TRANSACTION_SOURCE_PROOF_SCHEMAS
        and eok.get("inner_instruction_index_or_unknown") == "NOT_APPLICABLE"
    ):
        count += 1
    return count


def terminal_truth_order_is_derived(eok: dict[str, Any]) -> bool:
    return all(eok.get(component) == "DERIVED" for component in TERMINAL_DERIVED_COMPONENTS)


def has_known_signature(eok: dict[str, Any]) -> bool:
    return known_source_value(eok.get("signature"))


def has_fake_handoff_signature(row: dict[str, Any], eok: dict[str, Any]) -> bool:
    return has_known_signature(eok) and any(
        "HANDOFF_SIGNATURE_NOT_CHAIN_SOURCE" in limitation
        for limitation in limitations(row)
    )


def claims_exact_event_order(row: dict[str, Any]) -> bool:
    record = nested_record(row)
    value = record.get("exact_or_approx")
    return isinstance(value, str) and value.upper() == "EXACT_EVENT_ORDER"


def main() -> int:
    args = parser("Offline Shadow V2 temporal/no-lookahead audit").parse_args()
    rows, malformed = canonical_rows(args.scope_root)
    replay, replay_malformed = replay_rows(args.scope_root)
    lifecycle, lifecycle_malformed = lifecycle_rows(args.scope_root)
    temporal_by_schema: dict[str, Counter[str]] = defaultdict(Counter)
    clock_by_schema: dict[str, Counter[str]] = defaultdict(Counter)
    event_order_present = 0
    event_order_exempt = 0
    event_order_missing_required = 0
    event_order_missing_unclassified = 0
    unknown_components: Counter[str] = Counter()
    not_applicable_components: Counter[str] = Counter()
    derived_components: Counter[str] = Counter()
    runtime_local_components: Counter[str] = Counter()
    ordering_exemption_counts: Counter[str] = Counter()
    missing_required_examples: list[dict[str, str | None]] = []
    missing_source_examples: list[dict[str, str | None]] = []
    unknown_required_source_components: Counter[str] = Counter()
    seq_by_position: dict[str, list[int]] = defaultdict(list)
    non_monotonic = 0
    post_entry_pre_decision_violation = 0
    terminal_pre_entry_violation = 0
    transaction_source_proof_complete_count = 0
    account_state_source_proof_complete_count = 0
    unknown_required_source_rows = 0
    transaction_source_proof_missing_rows = 0
    account_state_source_proof_missing_rows = 0
    not_applicable_accepted_count = 0
    fake_handoff_signature_count = 0
    event_seq_chain_order_substitute_count = 0
    terminal_truth_derived_count = 0
    terminal_truth_not_derived_count = 0
    for row in rows:
        schema = canonical_payload_schema(row)
        env = envelope(row)
        temporal_by_schema[schema][str(env.get("temporal_class") or "UNKNOWN")] += 1
        clock_by_schema[schema][str(env.get("clock_domain") or "UNKNOWN")] += 1
        eok = event_order_key(row)
        if eok:
            event_order_present += 1
            for component in CHAIN_COMPONENTS:
                value = eok.get(component)
                if value == "UNKNOWN" or value is None:
                    unknown_components[component] += 1
                elif value == "NOT_APPLICABLE":
                    not_applicable_components[component] += 1
                elif value == "DERIVED":
                    derived_components[component] += 1
                elif value == "RUNTIME_LOCAL":
                    runtime_local_components[component] += 1
            seq = eok.get("event_seq_in_process")
            pos = position_id(row)
            if isinstance(seq, int) and pos:
                seq_by_position[pos].append(seq)
            not_applicable_accepted_count += accepted_not_applicable_count(schema, eok)
            if has_fake_handoff_signature(row, eok):
                fake_handoff_signature_count += 1
            missing_source: list[str] = []
            if schema in TRANSACTION_SOURCE_PROOF_SCHEMAS:
                missing_source = transaction_source_proof_missing(eok)
                if missing_source:
                    transaction_source_proof_missing_rows += 1
                else:
                    transaction_source_proof_complete_count += 1
                if missing_source and claims_exact_event_order(row) and isinstance(seq, int):
                    event_seq_chain_order_substitute_count += 1
            elif schema in ACCOUNT_STATE_SOURCE_PROOF_SCHEMAS:
                missing_source = account_state_source_proof_missing(row)
                if missing_source:
                    account_state_source_proof_missing_rows += 1
                else:
                    account_state_source_proof_complete_count += 1
            elif schema == "shadow_terminal_truth_v2":
                if terminal_truth_order_is_derived(eok):
                    terminal_truth_derived_count += 1
                else:
                    terminal_truth_not_derived_count += 1
            if missing_source:
                unknown_required_source_rows += 1
                unknown_required_source_components.update(missing_source)
                if len(missing_source_examples) < 10:
                    missing_source_examples.append(
                        {
                            "schema": schema,
                            "event_id": str(env.get("event_id") or ""),
                            "position_id": position_id(row),
                            "missing": "|".join(missing_source),
                        }
                    )
        else:
            exemption = ordering_exemption(row)
            allowed_exemptions = ORDERING_EXEMPTIONS_BY_SCHEMA.get(schema, set())
            if exemption in allowed_exemptions:
                event_order_exempt += 1
                ordering_exemption_counts[exemption] += 1
            elif schema in ORDERING_REQUIRED_SCHEMAS:
                event_order_missing_required += 1
                if len(missing_required_examples) < 10:
                    missing_required_examples.append(
                        {
                            "schema": schema,
                            "event_id": str(env.get("event_id") or ""),
                            "position_id": position_id(row),
                        }
                    )
            else:
                event_order_missing_unclassified += 1
                if len(missing_required_examples) < 10:
                    missing_required_examples.append(
                        {
                            "schema": schema,
                            "event_id": str(env.get("event_id") or ""),
                            "position_id": position_id(row),
                        }
                    )
        if schema in {"shadow_entry_attempt_v2", "shadow_entry_fill_v2"}:
            if env.get("temporal_class") in {"PRE_DETECTION", "PRE_DECISION", "AT_DECISION"}:
                post_entry_pre_decision_violation += 1
        if schema == "shadow_terminal_truth_v2":
            if env.get("temporal_class") in {"PRE_DETECTION", "PRE_DECISION", "AT_DECISION", "POST_ENTRY"}:
                terminal_pre_entry_violation += 1
    for seqs in seq_by_position.values():
        for prev, cur in zip(seqs, seqs[1:]):
            if cur < prev:
                non_monotonic += 1
    derived_as_canonical_input = 0
    for row in replay + lifecycle:
        refs = envelope(row).get("source_refs") or []
        if any(str(ref).startswith("shadow_replay_v2:") or str(ref).startswith("shadow_lifecycle_v2:") for ref in refs):
            derived_as_canonical_input += 1
    unknown_required_source_count = sum(unknown_required_source_components.values())
    if (
        malformed
        or replay_malformed
        or lifecycle_malformed
        or event_order_missing_required
        or event_order_missing_unclassified
        or post_entry_pre_decision_violation
        or terminal_pre_entry_violation
        or non_monotonic
        or derived_as_canonical_input
        or event_seq_chain_order_substitute_count
        or terminal_truth_not_derived_count
    ):
        verdict = "FAIL_LOOKAHEAD_OR_ORDERING_VIOLATION"
    elif fake_handoff_signature_count:
        verdict = "BLOCKED_FAKE_SOURCE_JOIN_DETECTED"
    elif transaction_source_proof_missing_rows:
        verdict = "BLOCKED_TEMPORAL_TRANSACTION_SOURCE_JOIN"
    elif account_state_source_proof_missing_rows:
        verdict = "BLOCKED_TEMPORAL_ACCOUNT_STATE_SOURCE_PROOF"
    elif unknown_required_source_count:
        verdict = "BLOCKED_TEMPORAL_AMBIGUITY_REMAINS"
    else:
        verdict = "PASS_TEMPORAL_NO_LOOKAHEAD_AUDIT"
    not_applicable_or_derived_count = (
        sum(not_applicable_components.values())
        + sum(derived_components.values())
        + sum(runtime_local_components.values())
    )
    result = {
        "audit": "temporal_no_lookahead",
        "scope_root": args.scope_root,
        "malformed_canonical_rows": malformed,
        "malformed_replay_rows": replay_malformed,
        "malformed_lifecycle_rows": lifecycle_malformed,
        "temporal_class_values_per_event_family": {k: dict(v) for k, v in temporal_by_schema.items()},
        "clock_domain_values_per_event_family": {k: dict(v) for k, v in clock_by_schema.items()},
        "event_order_key_present_rows": event_order_present,
        "event_order_key_exempt_rows": event_order_exempt,
        "event_order_key_missing_required_rows": event_order_missing_required,
        "event_order_key_missing_unclassified_rows": event_order_missing_unclassified,
        "event_order_key_missing_rows": event_order_missing_required
        + event_order_missing_unclassified,
        "ordering_exemption_counts": dict(ordering_exemption_counts),
        "missing_required_event_order_examples": missing_required_examples,
        "explicit_unknown_chain_order_components": dict(unknown_components),
        "raw_unknown_chain_order_component_count": sum(unknown_components.values()),
        "unknown_but_required_for_research_count": unknown_required_source_count,
        "unknown_required_source_count": unknown_required_source_count,
        "unknown_required_source_rows": unknown_required_source_rows,
        "unknown_required_source_components": dict(unknown_required_source_components),
        "unknown_required_source_examples": missing_source_examples,
        "not_applicable_chain_order_components": dict(not_applicable_components),
        "derived_chain_order_components": dict(derived_components),
        "runtime_local_chain_order_components": dict(runtime_local_components),
        "not_applicable_or_derived_chain_components_count": not_applicable_or_derived_count,
        "transaction_source_proof_complete_count": transaction_source_proof_complete_count,
        "transaction_source_proof_missing_rows": transaction_source_proof_missing_rows,
        "account_state_source_proof_complete_count": account_state_source_proof_complete_count,
        "account_state_source_proof_missing_rows": account_state_source_proof_missing_rows,
        "not_applicable_accepted_count": not_applicable_accepted_count,
        "fake_handoff_signature_count": fake_handoff_signature_count,
        "event_seq_chain_order_substitute_count": event_seq_chain_order_substitute_count,
        "terminal_truth_derived_count": terminal_truth_derived_count,
        "terminal_truth_not_derived_count": terminal_truth_not_derived_count,
        "non_monotonic_event_seq_in_process": non_monotonic,
        "post_entry_fields_used_in_pre_decision_context": post_entry_pre_decision_violation,
        "terminal_truth_used_as_pre_entry_evidence": terminal_pre_entry_violation,
        "derived_replay_lifecycle_used_as_canonical_input": derived_as_canonical_input,
        "temporal_audit_verdict": verdict,
        "verdict": verdict,
    }
    emit(result, args.pretty)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
