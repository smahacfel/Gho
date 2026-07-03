# Raport: Shadow V2 PR34-C Entry Boundary Hardening

Data: 2026-07-03

Status:

```text
PR34_C_IMPLEMENTATION_READY_FOR_REVIEW
```

## 1. Cel

PR34-C domyka maly hardening po PR34-B: Shadow V2 entry boundary payload z TriggerComponent nie moze zostac uzyty do L1 entry fill simulation, jezeli nie dotyczy tego samego `base_mint` i `pool_amm_id`, ktore PostBuyRuntime otrzymal w accepted shadow handoff.

Ten PR nie jest burninem, nie jest strategy proof i nie nadaje zadnego approval.

## 2. Zmieniony kontrakt

Przed zbudowaniem `pool_state_sample_v2` z `ShadowV2EntryBoundaryPayload` PostBuyRuntime waliduje:

- `boundary.canonical_pool_state.base_mint.to_string() == handoff base_mint`
- `boundary.canonical_pool_state.pool_amm_id.to_string() == handoff pool_amm_id`

Jezeli walidacja nie przejdzie:

- `pool_state_sample_v2` nie jest emitowany;
- `ShadowV2FillEngine` nie jest uruchamiany;
- `shadow_entry_fill_v2` pozostaje `BLOCKED_BY_DATA`;
- record dostaje typed limitations:
  - `ENTRY_BOUNDARY_BASE_MINT_MISMATCH`
  - `ENTRY_BOUNDARY_POOL_ID_MISMATCH`
  - `ENTRY_BOUNDARY_HANDOFF_VALIDATION_FAILED`
  - `ENTRY_POOL_STATE_BEFORE_REJECTED_BY_BOUNDARY_VALIDATION`

## 3. Ordering labels

PR34-C nie zmienia centralnego ordering contract w `ShadowV2FillEngine`. Wymagane labelki sa utrzymane:

```text
ENTRY_FILL_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS
ENTRY_FILL_POOL_STATE_AFTER_FILL_BOUNDARY
ENTRY_FILL_POOL_STATE_NOT_STRICTLY_BEFORE_FILL_BOUNDARY
```

Nowy test PostBuyRuntime potwierdza, ze same-slot ambiguity z realnego entry boundary payloadu trafia do `shadow_entry_fill_v2` jako typed blocker. Istniejace testy `ghost-brain` nadal potwierdzaja `AFTER_FILL_BOUNDARY` i `NOT_STRICTLY_BEFORE_FILL_BOUNDARY` w centralnym engine.

## 4. Dowody z testow

Uruchomione testy targeted:

```text
cargo test -p ghost-launcher shadow_v2_postbuy_entry_boundary_blocks_base_mint_mismatch -- --nocapture
cargo test -p ghost-launcher shadow_v2_postbuy_entry_boundary_blocks_pool_id_mismatch -- --nocapture
cargo test -p ghost-launcher shadow_v2_postbuy_entry_boundary_preserves_same_slot_ordering_blocker -- --nocapture
cargo test -p ghost-brain shadow_v2_entry_fill_blocks_future_pool_state_by_process_sequence -- --nocapture
cargo test -p ghost-brain shadow_v2_entry_fill_blocks_same_slot_incomplete_order -- --nocapture
```

Wynik:

```text
PASS
```

## 5. Granice

Nie zmieniono:

- BUY/REJECT;
- Gatekeeper policy;
- selector runtime;
- TX/Jito/live path;
- R51;
- `shadow_close_only`;
- active close.

Nie uruchomiono:

- burnina;
- validation runu;
- PR17/PR35 runtime proof.

Approval flags pozostaja:

```text
RUNTIME_APPROVAL=false
SHADOW_CLOSE_ONLY_APPROVAL=false
ACTIVE_CLOSE_APPROVAL=false
RESEARCH_GRADE=false
LIVE_EQUIVALENCE=false
BURNIN_AUTHORIZATION=false
```

## 6. Kolejny krok

Po review i merge PR34-C operator moze osobno zdecydowac o kolejnym validation/fidelity burninie. Ten raport sam nie autoryzuje burnina.
