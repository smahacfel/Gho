# Raport Shadow V2 L2-C: Solana temporal source proof closure 20260704

## Status

```text
final_verdict=L2_C_TEMPORAL_SOURCE_PROOF_CLOSED_STILL_DENSITY_BLOCKED
runtime_decision_behavior_changes=NONE
runtime_evidence_schema_changes=NO
audit_output_schema_changes=YES
new_provider_streams=NONE
approval_flags=false
```

L2-C dodaje Solana-aware klasyfikacje temporal/source proof dla Shadow V2.
Zmiana nie przyznaje L2 i nie zmienia runtime decyzji. Celem jest zamkniecie
bledu semantycznego, w ktorym account-state pool-state sample moglby byc
oceniany tym samym proofem co transaction-like event.

## Zakres

W zakresie:

- osobny transaction source proof dla eventow transaction-like;
- osobny account-state source proof dla `pool_state_sample_v2`;
- terminal truth traktowany jako `DERIVED`, nie chain-observed source;
- audyt temporal/no-lookahead raportujacy wymagane L2-C metryki;
- testy blokujace uzycie `event_seq_in_process` jako exact chain order;
- brak luzowania `has_complete_chain_order()`.

Poza zakresem:

- brak nowych NLN/provider subscriptions;
- brak zmian `BUY/REJECT`;
- brak zmian Gatekeeper policy;
- brak zmian selector runtime;
- brak zmian TX/Jito/live path;
- brak `shadow_close_only` albo active close;
- brak density/horizon contract;
- brak Gatekeeper denominator audit;
- brak dedicated research validation run;
- brak `runtime_approval`, `research_grade`, `live_equivalence` albo strategy
  unlock.

## Implementowany Kontrakt

Transaction-like events wymagaja:

```text
slot
block_time
source_tx_signature
transaction_index / tx_index
instruction_index / outer_instruction_index
```

Account-state pool-state samples wymagaja osobnego proof:

```text
account_data_hash = BLAKE3(original raw account update bytes)
source_account_pubkey
source_account_slot
source_write_version
```

To sa dwa rozne proofy:

```text
transaction_source_proof_complete != account_state_source_proof_complete
```

`has_complete_chain_order()` pozostaje niezmieniony i nadal wymaga starego
pelnego kompletu chain-order components. Account-state proof nie sprawia, ze
`EventOrderKey::has_complete_chain_order()` zaczyna zwracac `true`.

## Szczegoly Zmian

### `ghost-brain/src/guardian/post_buy/shadow_v2.rs`

Dodane zostaly diagnostyczne helpery:

```text
EventOrderKey::solana_transaction_source_proof_blockers()
EventOrderKey::has_complete_solana_transaction_source_proof()
PoolStateSampleV2::account_state_source_proof_blockers()
PoolStateSampleV2::has_complete_account_state_source_proof()
```

Helpery sa evidence-only. Nie sa konsumowane przez Gatekeeper, selector,
TX/Jito, active close ani live decision path.

### `scripts/shadow_v2_temporal_no_lookahead_audit.py`

Audyt rozdziela teraz rodziny dowodowe:

- transaction-like:
  - `shadow_entry_attempt_v2`
  - `shadow_entry_fill_v2`
  - `shadow_path_sample_v2`
  - `shadow_exit_attempt_v2`
  - `shadow_exit_fill_v2`
- account-state:
  - `pool_state_sample_v2`
- derived terminal:
  - `shadow_terminal_truth_v2`

Nowe metryki:

```text
temporal_audit_verdict
transaction_source_proof_complete_count
transaction_source_proof_missing_rows
account_state_source_proof_complete_count
account_state_source_proof_missing_rows
unknown_required_source_count
unknown_required_source_rows
not_applicable_accepted_count
fake_handoff_signature_count
event_seq_chain_order_substitute_count
terminal_truth_derived_count
terminal_truth_not_derived_count
raw_unknown_chain_order_component_count
```

`raw_unknown_chain_order_component_count` pozostaje informacyjny. Blockerem L2-C
jest `unknown_required_source_count`, czyli braki wymagane przez wlasciwy
proof dla danej rodziny eventow.

## Semantyka `log_index` i `inner_instruction_index`

Solana-native EVM-style `logIndex` nie istnieje. Dlatego:

```text
log_index_or_unknown = NOT_APPLICABLE
```

moze byc akceptowane jako Solana-specific non-chain-order classification, ale
nie moze sprawic, ze `has_complete_chain_order()` przejdzie.

`inner_instruction_index` nie jest wypelniany z `inner_group_index`.
`inner_instruction_index_or_unknown` pozostaje `UNKNOWN`, dopoki nie ma
zaakceptowanego kontraktu dokladnej semantyki CPI/inner instruction index.

## Ochrona Przed Fake Proof

Audyt blokuje:

- fake handoff signature jako source signature;
- `event_seq_in_process` uzyte jako `EXACT_EVENT_ORDER`;
- terminal truth z chain-observed transaction proof zamiast `DERIVED`;
- transaction-like rows bez wymaganych source fields;
- account-state rows bez hash/pubkey/slot/write_version proof.

## Co Jest Nadal Blokowane

L2 nie jest przyznane. Po L2-C nadal wymagane sa:

- temporal audit PASS na realnym runie;
- density/horizon/retention contract;
- Gatekeeper coverage / denominator / starvation audit;
- dedicated research validation run;
- sample-size gate;
- manifest/replay/lifecycle PASS.

Jesli realny run nie dostarczy source transaction metadata dla transaction-like
rows, nowy audyt zwroci:

```text
BLOCKED_TEMPORAL_TRANSACTION_SOURCE_JOIN
```

Jesli account-state row nie ma hash/pubkey/slot/write_version, zwroci:

```text
BLOCKED_TEMPORAL_ACCOUNT_STATE_SOURCE_PROOF
```

## Weryfikacja

Dodane/regresyjne testy:

```text
shadow_v2_solana_transaction_source_proof_is_not_complete_chain_order
shadow_v2_event_seq_does_not_complete_solana_transaction_source_proof
shadow_v2_account_state_source_proof_is_separate_from_transaction_order
test_shadow_v2_temporal_audit_separates_account_state_proof_from_tx_order
test_shadow_v2_temporal_audit_blocks_transaction_like_missing_source
test_shadow_v2_temporal_audit_rejects_event_seq_as_exact_order_substitute
test_shadow_v2_temporal_audit_counts_terminal_truth_derived_order
```

Minimalne checki wykonane podczas implementacji:

```bash
cargo test -p ghost-brain shadow_v2_solana_transaction_source_proof -- --nocapture
cargo test -p ghost-brain shadow_v2_account_state_source_proof -- --nocapture
python3 tests/test_shadow_v2_temporal_no_lookahead_audit.py
python3 -m py_compile scripts/shadow_v2_temporal_no_lookahead_audit.py tests/test_shadow_v2_temporal_no_lookahead_audit.py
```

## Final Verdict

```text
final_verdict=L2_C_TEMPORAL_SOURCE_PROOF_CLOSED_STILL_DENSITY_BLOCKED
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
next_stage=L2-D_DENSITY_HORIZON_RETENTION_CONTRACT
```
