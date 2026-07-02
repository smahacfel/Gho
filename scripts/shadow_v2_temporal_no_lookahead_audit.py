#!/usr/bin/env python3
from __future__ import annotations

from collections import Counter, defaultdict

from shadow_v2_offline_audit_common import (
    canonical_payload_schema,
    canonical_rows,
    emit,
    envelope,
    event_order_key,
    lifecycle_rows,
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


def main() -> int:
    args = parser("Offline Shadow V2 temporal/no-lookahead audit").parse_args()
    rows, malformed = canonical_rows(args.scope_root)
    replay, replay_malformed = replay_rows(args.scope_root)
    lifecycle, lifecycle_malformed = lifecycle_rows(args.scope_root)
    temporal_by_schema: dict[str, Counter[str]] = defaultdict(Counter)
    clock_by_schema: dict[str, Counter[str]] = defaultdict(Counter)
    event_order_present = 0
    event_order_missing = 0
    unknown_components: Counter[str] = Counter()
    seq_by_position: dict[str, list[int]] = defaultdict(list)
    non_monotonic = 0
    post_entry_pre_decision_violation = 0
    terminal_pre_entry_violation = 0
    for row in rows:
        schema = canonical_payload_schema(row)
        env = envelope(row)
        temporal_by_schema[schema][str(env.get("temporal_class") or "UNKNOWN")] += 1
        clock_by_schema[schema][str(env.get("clock_domain") or "UNKNOWN")] += 1
        eok = event_order_key(row)
        if eok:
            event_order_present += 1
            for component in CHAIN_COMPONENTS:
                if eok.get(component) == "UNKNOWN" or eok.get(component) is None:
                    unknown_components[component] += 1
            seq = eok.get("event_seq_in_process")
            pos = position_id(row)
            if isinstance(seq, int) and pos:
                seq_by_position[pos].append(seq)
        else:
            event_order_missing += 1
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
    if malformed or replay_malformed or lifecycle_malformed or post_entry_pre_decision_violation or terminal_pre_entry_violation or non_monotonic or derived_as_canonical_input:
        verdict = "FAIL_LOOKAHEAD_OR_ORDERING_VIOLATION"
    elif unknown_components or event_order_missing:
        verdict = "BLOCKED_TEMPORAL_AMBIGUITY_REMAINS"
    else:
        verdict = "PASS_TEMPORAL_NO_LOOKAHEAD_AUDIT"
    result = {
        "audit": "temporal_no_lookahead",
        "scope_root": args.scope_root,
        "malformed_canonical_rows": malformed,
        "malformed_replay_rows": replay_malformed,
        "malformed_lifecycle_rows": lifecycle_malformed,
        "temporal_class_values_per_event_family": {k: dict(v) for k, v in temporal_by_schema.items()},
        "clock_domain_values_per_event_family": {k: dict(v) for k, v in clock_by_schema.items()},
        "event_order_key_present_rows": event_order_present,
        "event_order_key_missing_rows": event_order_missing,
        "explicit_unknown_chain_order_components": dict(unknown_components),
        "non_monotonic_event_seq_in_process": non_monotonic,
        "post_entry_fields_used_in_pre_decision_context": post_entry_pre_decision_violation,
        "terminal_truth_used_as_pre_entry_evidence": terminal_pre_entry_violation,
        "derived_replay_lifecycle_used_as_canonical_input": derived_as_canonical_input,
        "verdict": verdict,
    }
    emit(result, args.pretty)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
