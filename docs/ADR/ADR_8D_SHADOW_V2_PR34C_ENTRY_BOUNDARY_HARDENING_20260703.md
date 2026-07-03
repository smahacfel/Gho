# ADR-8D: Shadow V2 PR34-C Entry Boundary Hardening

Data: 2026-07-03

Status:

```text
PROPOSED
```

## D1. Problem

PR34-B wprowadzil shadow-only `ENTRY_BEFORE` boundary payload przenoszony z TriggerComponent do PostBuyRuntime. Payload zawiera `CanonicalPoolState`, ale przed PR34-C PostBuyRuntime nie sprawdzal twardo, czy `boundary.canonical_pool_state` odpowiada temu samemu `base_mint` i `pool_amm_id`, ktore przyszly w handoff.

To tworzylo ryzyko evidence contamination: bledny lub niespojny payload moglby zostac opakowany jako `pool_state_sample_v2`, a nastepnie przekazany do `ShadowV2FillEngine`.

## D2. Decyzja

Dodajemy shadow-only walidacje handoff boundary:

```text
boundary.canonical_pool_state.base_mint == handoff.base_mint
boundary.canonical_pool_state.pool_amm_id == handoff.pool_amm_id
```

Mismatche sa fail-closed dla Shadow V2 entry fill evidence:

- nie emitujemy `pool_state_sample_v2`;
- nie uruchamiamy L1 fill engine;
- emitujemy `shadow_entry_fill_v2` jako `BLOCKED_BY_DATA`;
- zapisujemy typed limitations z powodem mismatch.

## D3. Evidence

Kod:

- `ghost-launcher/src/components/post_buy_runtime.rs`

Nowe zachowanie:

- `shadow_v2_entry_boundary_handoff_blockers(...)` porownuje boundary state z handoffem.
- `maybe_emit_shadow_v2_entry_evidence(...)` przekazuje payload do budowy `PoolStateSampleV2` tylko przy pustej liscie blockerow.
- `maybe_emit_shadow_v2_entry_evidence_with_pool_state(...)` dolacza blocker labels do `blocked_without_pool_state(...)`.

Testy:

```text
cargo test -p ghost-launcher shadow_v2_postbuy_entry_boundary_blocks_base_mint_mismatch -- --nocapture
cargo test -p ghost-launcher shadow_v2_postbuy_entry_boundary_blocks_pool_id_mismatch -- --nocapture
cargo test -p ghost-launcher shadow_v2_postbuy_entry_boundary_preserves_same_slot_ordering_blocker -- --nocapture
cargo test -p ghost-brain shadow_v2_entry_fill_blocks_future_pool_state_by_process_sequence -- --nocapture
cargo test -p ghost-brain shadow_v2_entry_fill_blocks_same_slot_incomplete_order -- --nocapture
```

## D4. Konsekwencje

Po PR34-C Shadow V2 entry boundary jest nadal diagnostic-only, ale jest bezpieczniejsze jako input do L1 engine:

- zgodny payload moze przejsc do `PoolStateSampleV2` i dalej do `ShadowV2FillEngine`;
- niezgodny payload blokuje fill zamiast produkowac falszywa symulacje;
- brak `account_data_hash` nadal nie jest fake'owany i nadal degraduje research provenance;
- ordering ambiguity pozostaje typed blockerem, a nie cichym sukcesem.

## D5. Odrzucone alternatywy

Odrzucono:

- pozny odczyt `PostBuyRuntime.account_state_core` jako fallback dla mismatchu;
- ignorowanie `base_mint` mismatchu;
- ignorowanie `pool_amm_id` mismatchu;
- emitowanie `pool_state_sample_v2` z payloadu, ktory nie zgadza sie z handoffem;
- nadawanie research-grade lub runtime approval na podstawie PR34-C.

## D6. Runtime boundary

PR34-C nie zmienia:

- BUY/REJECT;
- Gatekeeper policy;
- selector runtime;
- TX/Jito/live path;
- `shadow_close_only`;
- active close;
- R51.

PR34-C nie uruchamia burnina ani validation runu.

Approval flags pozostaja:

```text
RUNTIME_APPROVAL=false
SHADOW_CLOSE_ONLY_APPROVAL=false
ACTIVE_CLOSE_APPROVAL=false
RESEARCH_GRADE=false
LIVE_EQUIVALENCE=false
BURNIN_AUTHORIZATION=false
```

## D7. Acceptance gates

PR34-C jest akceptowalny tylko jezeli:

- base mint mismatch daje `BLOCKED_BY_DATA`;
- pool id mismatch daje `BLOCKED_BY_DATA`;
- mismatch nie emituje `pool_state_sample_v2`;
- mismatch nie odpala `ShadowV2FillEngine`;
- same-slot ordering ambiguity nadal pojawia sie jako `ENTRY_FILL_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS`;
- `ENTRY_FILL_POOL_STATE_AFTER_FILL_BOUNDARY` i `ENTRY_FILL_POOL_STATE_NOT_STRICTLY_BEFORE_FILL_BOUNDARY` pozostaja utrzymane w centralnym engine;
- brak zmian w BUY/REJECT/Gatekeeper/selector/TX/Jito/live path.

## D8. Decyzja koncowa

```text
PR34_C_IMPLEMENTATION_READY_FOR_REVIEW
```

Burnin pozostaje niedozwolony bez osobnej decyzji operatora.
