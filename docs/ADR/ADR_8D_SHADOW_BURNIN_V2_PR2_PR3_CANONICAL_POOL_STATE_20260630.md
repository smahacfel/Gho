# ADR-8D: Shadow Burnin V2 Canonical Event Stream i Pool-State Provenance

Data: 2026-06-30

Status:

```text
IMPLEMENTED_LOCAL_REVIEW_REMEDIATED_PENDING_CI
```

## D1. Problem

P0 Shadow Burnin Fidelity Audit wykazał:

```text
SHADOW_REPLAY_LIFECYCLE_MISMATCH
```

Shadow V1 nie ma jednego canonical position truth. `shadow_lifecycle` i `shadow_exit_replay_v1` mogą dawać konkurencyjne terminal facts, a pool-state provenance nie jest wystarczająco jawny, żeby traktować ceny jako causal research evidence albo live-equivalent fill evidence.

## D2. Decision

Wprowadzono lokalny, inercyjny zakres PR2 i PR3 dla Shadow Burnin Simulation V2 w `ghost-brain/src/guardian/post_buy/shadow_v2.rs`.

PR2 dodaje:

- `ShadowPositionV2`,
- `ShadowPositionEventV2`,
- `ShadowV2Record`,
- `ShadowV2CanonicalEventStream`,
- `JsonlShadowV2CanonicalWriter`,
- `ShadowV2ExactJoinKey`,
- `ShadowV2ExactJoinIndex`,
- typed `ShadowV2Error`.

Po review PR2 zostało doprecyzowane:

- `JsonlShadowV2CanonicalWriter::new()` indeksuje istniejący `shadow_position_event_v2.jsonl` przed przyjęciem nowych eventów,
- duplicate `event_id` i drugi terminal truth są odrzucane również po restarcie procesu,
- writer wykonuje `prepare -> durable JSONL write -> in-memory commit`, więc błąd I/O nie przesuwa canonical stream w pamięci,
- `ShadowV2ExactJoinKey::new()` odrzuca puste `run_id`, `session_id`, `position_id`, `pool_id` i `base_mint`,
- `ShadowV2ExactJoinIndex` jest jawnie terminal-position-level guardem, a nie event-level join indexem.

PR3 dodaje:

- konstruktor `PoolStateSampleV2::from_account_state_core`,
- walidację `PoolStateSampleV2::research_blockers`,
- `PoolStateProvenanceRecorder`,
- hash raw account data przez `account_data_hash_blake3`,
- blokady research dla missing slot/time/source oraz `ShadowLedgerDiagnostic`.

Po review PR3 zostało doprecyzowane:

- brak `account_data_hash` blokuje research-ready dla account-backed sample,
- brak `staleness_ms` albo `staleness_slots` blokuje research-ready,
- odwrócony lub nieznany update->observe staleness dostaje jawny blocker/quality label,
- `from_account_state_core()` wymaga jawnego `TemporalClass`, `ClockDomain` i `token_decimals`, bez ukrytego ustawiania `PRE_DECISION`,
- `event_order_key` ma explicit UNKNOWN sentinel dla chain-order oraz ambiguity labels, więc brak intra-slot order nie przechodzi jako cicha kompletność.

Zakres jest dodatni i side-by-side. Nie podłączono tych typów do aktywnej ścieżki runtime.

## D3. Evidence

Pliki implementacyjne:

- `ghost-brain/src/guardian/post_buy/shadow_v2.rs`
- `reports/selector/shadow_v2_remediation_workbreakdown.csv`
- `reports/selector/shadow_v2_acceptance_gates.csv`

Testy dodane w module `shadow_v2`:

- `shadow_v2_terminal_stream_rejects_duplicate_terminal_truth`
- `shadow_v2_terminal_stream_rejects_duplicate_event_id`
- `shadow_v2_terminal_exact_join_index_rejects_ambiguous_key`
- `shadow_v2_terminal_jsonl_writer_emits_canonical_event_stream`
- `shadow_v2_terminal_jsonl_writer_indexes_existing_file_on_restart`
- `shadow_v2_terminal_jsonl_writer_keeps_memory_clean_after_io_failure`
- `shadow_v2_exact_join_key_rejects_empty_identity_fields`
- `shadow_v2_pool_state_from_account_state_core_carries_provenance`
- `shadow_v2_pool_state_research_sample_blocks_missing_slot_or_unknown_source`
- `shadow_v2_pool_state_shadowledger_source_is_diagnostic_only`
- `shadow_v2_pool_state_recorder_enforces_monotonic_event_sequence`
- `shadow_v2_pool_state_constructor_requires_explicit_temporal_context`
- `shadow_v2_pool_state_research_sample_blocks_missing_hash_staleness_and_chain_order`
- `shadow_v2_pool_state_explicit_unknown_chain_order_is_labeled_not_silent`

## D4. Root Cause

Shadow V1 pozwalał traktować replay i lifecycle jak równoległe prawdy, a nie pochodne widoki z jednego strumienia zdarzeń. Brakowało też egzekwowalnego kontraktu, że pool-state sample posiada slot, wall time, source, event order, reserve provenance i jawny status diagnostic-only dla `ShadowLedger`.

Review PR2/PR3 ujawniło dodatkowe root cause na granicy durable evidence:

- canonical writer egzekwował duplicate/terminal invariants tylko w pamięci bieżącego procesu,
- kolejność `memory commit -> file append` mogła rozjechać in-memory truth i durable artifact przy błędzie I/O,
- pool-state sample logował hash/staleness/order fields, ale nie wszystkie były research-blocking,
- helper z `AccountStateCore` ukrycie klasyfikował próbkę jako `PRE_DECISION`, co tworzyło ryzyko temporal leakage.

## D5. Corrective Action

PR2 ustanawia invariant:

```text
one position_id -> one canonical event stream -> max one terminal truth
```

PR2 odrzuca:

- puste `run_id`,
- puste `session_id`,
- puste `position_id`,
- puste `pool_id`,
- puste `base_mint`,
- puste `event_id`,
- zdublowany `event_id`,
- drugi terminal truth dla tej samej pozycji,
- niejednoznaczny exact join key,
- silent fallback join.

PR2 utrwala invariant również po restarcie przez odbudowę indeksu z istniejącego JSONL. Writer nie aktualizuje in-memory stream przed skutecznym durable append.

PR3 ustanawia invariant:

```text
research-ready price sample must point to timestamped and slotted pool_state_sample_v2 with explicit source and normalization
```

PR3 blokuje research-ready sample, gdy brakuje slotu, wall time, source, event order, decimals, lamports albo reserve provenance. `ShadowLedgerDiagnostic` pozostaje diagnostyką, nie canonical live truth.

PR3 blokuje również:

- brak `account_data_hash` dla account-backed sample,
- brak jawnego `staleness_ms`,
- brak jawnego `staleness_slots`,
- `TemporalClass::Unknown`,
- brak signature albo intra-slot index fields w `event_order_key`.

Gdy chain-order jest nieznany, musi być zapisany jako explicit UNKNOWN i otrzymuje ambiguity label, a nie cichą research-ready kompletność.

## D6. Rejected Alternatives

Odrzucono:

- naprawianie `shadow_lifecycle` i `shadow_exit_replay_v1` jako równorzędnych źródeł prawdy,
- fallback join po samym `pool_id` albo `base_mint`,
- akceptowanie duplicate terminal rows bez typed sub-event,
- promowanie `ShadowLedger` do źródła live truth,
- zapisywanie pool price bez slot/time/source,
- podłączenie V2 writerów do runtime w tym PR.

## D7. Consequences

PR2/PR3 tworzą podstawę pod późniejsze PR8/PR9, gdzie replay i lifecycle mają być widokami pochodnymi z canonical stream. Sama obecna zmiana nie oznacza, że Shadow V2 jest research-grade ani live-equivalence-grade.

Granice pozostają:

```text
runtime_approval=false
shadow_close_only_approval=false
active_close_approval=false
strategy_research_unblocked=false
live_equivalence_claim=false
```

## D8. Verification

Wykonane komendy:

```text
cargo test -p ghost-brain shadow_v2 -- --nocapture
cargo test -q -p ghost-launcher --lib restore_legacy_buy
python3 scripts/guard_restore_shadow_lifecycle.py --skip-runtime --output-dir /tmp/restore_guard_static_local --json
cargo fmt --check
git diff --check
```

Wyniki:

```text
shadow_v2: 19 passed; 0 failed
restore_legacy_buy: 2 passed; 0 failed
guard_restore_shadow_lifecycle --skip-runtime: PASS; 8 targeted commands passed; runtime_smoke SKIPPED
cargo fmt --check: passed
git diff --check: passed
```

Uwaga CI:

```text
Restore Lifecycle Guard był czerwony na wcześniejszym headzie PR.
Lokalny targeted test z tego guarda (`cargo test -q -p ghost-launcher --lib restore_legacy_buy`) przechodzi.
Merge pozostaje zależny od zielonego CI na nowym headzie albo jawnej klasyfikacji failure jako unrelated.
```

Dodatkowe guardy przed commitem:

```text
git diff --cached --name-only
```

Runtime boundary:

```text
NO_RUNTIME_SEMANTICS_CHANGED
NO_BUY_REJECT_CHANGE
NO_GATEKEEPER_POLICY_CHANGE
NO_SELECTOR_RUNTIME_CHANGE
NO_TX_JITO_LIVE_PATH_CHANGE
NO_SHADOW_CLOSE_ONLY_CHANGE
NO_ACTIVE_CLOSE_CHANGE
NO_RUN_STARTED
NO_R51_TOUCH
```
