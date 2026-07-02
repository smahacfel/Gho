# ADR-8D: Shadow V2 PR18F Canonical Event Ordering

Data: 2026-07-02

Status:

```text
ACCEPTED_FOR_IMPLEMENTATION_PR
```

## D1. Problem

PR18E validation burnin zakonczyl runtime/manifest/shutdown PASS, ale temporal/no-lookahead audit zwrocil:

```text
FAIL_LOOKAHEAD_OR_ORDERING_VIOLATION
event_order_key_missing_rows = 379
non_monotonic_event_seq_in_process = 1
```

Analiza raw scope PR18E wykazala, ze missing ordering dotyczylo:

- `shadow_position_v2` / `POSITION_CREATED`: 190 rows;
- `shadow_terminal_truth_v2` / `TERMINAL_TRUTH`: 189 rows.

Non-monotonic sequence wynikalo z timestamp-derived `event_seq_in_process`, ktory mogl regresowac dla tej samej pozycji, gdy runtime append order byl inny niz timestamp family order.

## D2. Decyzja

Wprowadzono canonical ordering contract dla Shadow V2:

1. `shadow_terminal_truth_v2` ma wymagany `event_order_key`.
2. `shadow_position_v2` / `POSITION_CREATED` ma jawna exemption:

```text
ORDERING_EXEMPT_POSITION_CREATED
```

3. Validation smoke marker ma jawna exemption:

```text
ORDERING_EXEMPT_VALIDATION_SMOKE_MARKER
```

4. Canonical writer failuje przy required event family bez `event_order_key`.
5. Canonical writer utrzymuje monotonic `event_seq_in_process` per `(run_id, position_id)`.
6. Temporal audit script rozroznia rows ordered, rows explicitly exempt, rows missing required ordering i rows missing unclassified ordering.

Finalny verdict PR32:

```text
PR32_IMPLEMENTATION_READY_FOR_VALIDATION
```

## D3. Evidence

Zmienione obszary:

- `ghost-brain/src/guardian/post_buy/shadow_v2.rs`
- `ghost-brain/src/guardian/post_buy/engine.rs`
- `scripts/shadow_v2_temporal_no_lookahead_audit.py`
- `tests/test_shadow_v2_temporal_no_lookahead_audit.py`
- `PLANS/AUDYT/RAPORT_SHADOW_V2_PR18F_CANONICAL_EVENT_ORDERING_20260702.md`

Testy kontraktowe:

```text
shadow_v2_terminal_truth_has_event_order_key
shadow_v2_position_created_ordering_exemption_is_explicit
shadow_v2_event_seq_is_monotonic_per_position
shadow_v2_temporal_audit_fails_missing_required_event_order_key
shadow_v2_temporal_audit_allows_explicit_position_created_exemption
```

Python audit tests:

```text
python3 tests/test_shadow_v2_temporal_no_lookahead_audit.py
```

Required validation:

```text
cargo check -p ghost-brain
cargo check -p ghost-launcher
cargo fmt --check
git diff --check
git diff --cached --check
python3 -m py_compile scripts/shadow_v2_temporal_no_lookahead_audit.py
forbidden staged-file guard
```

## D4. Invariants

Ordering-sensitive canonical families:

```text
pool_state_sample_v2
shadow_entry_attempt_v2
shadow_entry_fill_v2
shadow_path_sample_v2
shadow_exit_attempt_v2
shadow_exit_fill_v2
shadow_terminal_truth_v2
```

nie moga przejsc przez canonical writer bez `event_order_key`.

`shadow_position_v2` nie dostaje fake chain ordering. Jest jawnie exempt tylko jako position-created/smoke-marker event.

Writer nie tworzy fake signature, fake tx index, fake instruction index, fake inner instruction index, fake log index ani fake block time.

Per-position monotonicity jest liczona po:

```text
(run_id, position_id)
```

## D5. Runtime boundary

PR32 nie zmienia runtime decision behavior.

Nie zmieniono:

- BUY/REJECT;
- Gatekeeper policy;
- selector runtime;
- TX/Jito/live path;
- `shadow_close_only`;
- active close;
- runtime approval flags;
- R51.

PR32 nie uruchamia burnina.

## D6. Consequences

Po PR32 nastepny validation/fidelity burnin powinien moc wygenerowac canonical stream bez:

```text
event_order_key_missing_required_rows
event_order_key_missing_unclassified_rows
non_monotonic_event_seq_in_process
```

Jesli chain-order components pozostaja explicit `UNKNOWN`, temporal audit moze nadal dac `BLOCKED_TEMPORAL_AMBIGUITY_REMAINS`, ale nie powinien failowac przez brak wymaganego ordering albo non-monotonic process sequence.

## D7. Rejected alternatives

Odrzucono:

- dodawanie fake `event_order_key` do `shadow_position_v2`;
- traktowanie missing `event_order_key` jako PASS;
- ignorowanie `shadow_terminal_truth_v2` ordering mimo uzycia w replay/lifecycle terminal reconciliation;
- zmiane fill provenance, density, strategy, BUY/REJECT lub live path w tym PR.

## D8. Follow-up

Po merge wymagany jest osobny operator prompt na kolejny validation/fidelity burnin.

Ten burnin powinien uruchomic PR29 temporal audit na nowym scope i sprawdzic:

```text
event_order_key_missing_rows = 0
non_monotonic_event_seq_in_process = 0
post_entry_fields_used_in_pre_decision_context = 0
terminal_truth_used_as_pre_entry_evidence = 0
derived_replay_lifecycle_used_as_canonical_input = 0
```

PR32 nie nadaje:

```text
research_grade = true
live_equivalence = true
runtime_approval = true
shadow_close_only_approval = true
active_close_approval = true
strategy_research_unblocked = true
```
