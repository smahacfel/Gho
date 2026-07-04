# ADR-8D: Shadow V2 L2-C Solana Temporal Source Proof Closure 20260704

## Status

Accepted implementation stage.

## Decision

Dodajemy Solana-aware, evidence-only temporal source proof classification dla
Shadow V2:

```text
final_verdict=L2_C_TEMPORAL_SOURCE_PROOF_CLOSED_STILL_DENSITY_BLOCKED
```

Decyzja architektoniczna: transaction proof i account-state proof sa dwoma
roznymi kontraktami. Account-state proof nie moze udawac transaction
chain-order proof.

## Context

Po L2-B `account_data_hash` i metadata account-state sa propagowane z raw
Seer/Geyser account update bytes do Shadow V2 pool-state evidence. Nadal
istnial problem interpretacyjny: stary `EventOrderKey::has_complete_chain_order`
jest zbyt transaction-oriented dla account-state source proof, ale nie wolno go
luzowac, bo jest backward-compatible guardem intra-slot ordering.

L2-C wprowadza osobna klasyfikacje, zamiast zmieniac stare predicate.

## Implemented Contract

Transaction-like events wymagaja:

```text
slot
block_time
source_tx_signature
transaction_index / tx_index
instruction_index / outer_instruction_index
```

Account-state pool-state samples wymagaja:

```text
account_data_hash
source_account_pubkey
source_account_slot
source_write_version
```

Terminal truth pozostaje:

```text
DERIVED
```

Nie jest source dla fill boundary ani pool-state boundary.

## Code-Level Changes

W `ghost-brain/src/guardian/post_buy/shadow_v2.rs` dodano:

```text
EventOrderKey::solana_transaction_source_proof_blockers()
EventOrderKey::has_complete_solana_transaction_source_proof()
PoolStateSampleV2::account_state_source_proof_blockers()
PoolStateSampleV2::has_complete_account_state_source_proof()
```

`EventOrderKey::has_complete_chain_order()` pozostaje niezmieniony.

W `scripts/shadow_v2_temporal_no_lookahead_audit.py` dodano proof-aware audit
logic i nowe metryki:

```text
transaction_source_proof_complete_count
account_state_source_proof_complete_count
unknown_required_source_count
not_applicable_accepted_count
fake_handoff_signature_count
event_seq_chain_order_substitute_count
terminal_truth_derived_count
```

## Rejected Alternatives

### Loosen `has_complete_chain_order()`

Rejected. To stworzyłoby falszywy PASS dla account-state samples i oslabiloby
stary guard same-slot/intra-transaction ambiguity.

### Treat `event_seq_in_process` as chain order

Rejected. `event_seq_in_process` jest local process order. Nie dowodzi Solana
chain order ani same-slot tie-breakera.

### Use handoff signature as source transaction signature

Rejected. Handoff signature jest runtime handoff identity, nie transaction
source proof dla pool-state boundary.

### Treat terminal truth as chain-observed proof

Rejected. Terminal truth jest derived outcome/reconciliation evidence. Nie moze
byc uzyty jako boundary-before source.

## Consequences

1. `pool_state_sample_v2` moze miec complete account-state proof bez
   complete transaction chain-order.
2. Transaction-like events nadal musza miec transaction source metadata.
3. `log_index_or_unknown=NOT_APPLICABLE` nie daje complete chain order.
4. `inner_instruction_index` pozostaje `UNKNOWN`, jezeli nie ma exact CPI/inner
   semantics.
5. L2 pozostaje zablokowane do czasu density, denominator i research validation
   run.

## Compatibility

Nie zmieniono runtime evidence JSON schema. Dodane Rust helpery sa
diagnostyczne. Zmieniono output schema offline temporal audit przez dodanie
metryk L2-C.

## Verification

```bash
cargo test -p ghost-brain shadow_v2_solana_transaction_source_proof -- --nocapture
cargo test -p ghost-brain shadow_v2_account_state_source_proof -- --nocapture
python3 tests/test_shadow_v2_temporal_no_lookahead_audit.py
python3 -m py_compile scripts/shadow_v2_temporal_no_lookahead_audit.py tests/test_shadow_v2_temporal_no_lookahead_audit.py
```

## Final Decision

```text
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
next_stage=L2-D_DENSITY_HORIZON_RETENTION_CONTRACT
```
