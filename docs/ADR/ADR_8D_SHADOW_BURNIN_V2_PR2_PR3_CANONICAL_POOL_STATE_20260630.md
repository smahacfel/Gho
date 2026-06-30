# ADR-8D: Shadow Burnin V2 Canonical Event Stream i Pool-State Provenance

Data: 2026-06-30

Status:

```text
IMPLEMENTED_LOCAL_PENDING_REVIEW
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

PR3 dodaje:

- konstruktor `PoolStateSampleV2::from_account_state_core`,
- walidację `PoolStateSampleV2::research_blockers`,
- `PoolStateProvenanceRecorder`,
- hash raw account data przez `account_data_hash_blake3`,
- blokady research dla missing slot/time/source oraz `ShadowLedgerDiagnostic`.

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
- `shadow_v2_pool_state_from_account_state_core_carries_provenance`
- `shadow_v2_pool_state_research_sample_blocks_missing_slot_or_unknown_source`
- `shadow_v2_pool_state_shadowledger_source_is_diagnostic_only`
- `shadow_v2_pool_state_recorder_enforces_monotonic_event_sequence`

## D4. Root Cause

Shadow V1 pozwalał traktować replay i lifecycle jak równoległe prawdy, a nie pochodne widoki z jednego strumienia zdarzeń. Brakowało też egzekwowalnego kontraktu, że pool-state sample posiada slot, wall time, source, event order, reserve provenance i jawny status diagnostic-only dla `ShadowLedger`.

## D5. Corrective Action

PR2 ustanawia invariant:

```text
one position_id -> one canonical event stream -> max one terminal truth
```

PR2 odrzuca:

- puste `position_id`,
- puste `event_id`,
- zdublowany `event_id`,
- drugi terminal truth dla tej samej pozycji,
- niejednoznaczny exact join key,
- silent fallback join.

PR3 ustanawia invariant:

```text
research-ready price sample must point to timestamped and slotted pool_state_sample_v2 with explicit source and normalization
```

PR3 blokuje research-ready sample, gdy brakuje slotu, wall time, source, event order, decimals, lamports albo reserve provenance. `ShadowLedgerDiagnostic` pozostaje diagnostyką, nie canonical live truth.

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
cargo test -p ghost-brain shadow_v2_terminal -- --nocapture
cargo test -p ghost-brain shadow_v2_pool_state -- --nocapture
cargo test -p ghost-brain shadow_v2 -- --nocapture
cargo fmt --check
git diff --check
```

Wyniki:

```text
shadow_v2_terminal: 4 passed; 0 failed
shadow_v2_pool_state: 4 passed; 0 failed
shadow_v2: 13 passed; 0 failed
cargo fmt --check: passed
git diff --check: passed
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
