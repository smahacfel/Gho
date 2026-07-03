# RAPORT: Shadow V2 PR34-B Entry Boundary Capture

Data: 2026-07-03

## 1. Executive Summary

Finalny verdict:

```text
PR34_B_IMPLEMENTATION_READY_FOR_VALIDATION
```

PR34-B implementuje minimalny, shadow-only capture `ENTRY_BEFORE` zgodny z
werdyktem PR34-A0:

```text
ENTRY_BOUNDARY_SOURCE_PRESENT
```

Stan poola jest capture'owany upstream w Trigger boundary z
`AccountStateCore -> CanonicalPoolState`, przenoszony przez shadow-only handoff,
a w `PostBuyRuntime` mapowany na `PoolStateSampleV2` i uzywany do entry fill
przez PR33 L1 deterministic execution engine.

To nadal nie jest:

```text
research_grade
live_equivalent
runtime approval
shadow_close_only approval
active close approval
strategy proof
```

Nie uruchomiono burnina.

## 2. Co Zmieniono

Zakres kodowy:

- `ghost-launcher/src/events.rs`
  - dodano `ShadowV2EntryBoundaryPayload`;
  - dodano optional `shadow_v2_entry_boundary` do `PostBuySubmitted`;
  - dodano `with_shadow_v2_entry_boundary(...)`.
- `ghost-launcher/src/components/trigger/component.rs`
  - capture `CanonicalPoolState` w Trigger boundary;
  - payload dodany do `PreparedBuyRequest`;
  - test capture przed shadow simulation.
- `ghost-launcher/src/components/trigger/shadow_run.rs`
  - payload przenoszony przez `ShadowBuySimulationReport`.
- `ghost-launcher/src/oracle_runtime.rs`
  - payload przekazywany do shadow post-buy handoff.
- `ghost-launcher/src/components/post_buy_runtime.rs`
  - boundary payload mapowany na `PoolStateSampleV2`;
  - `ShadowEntryFillV2::from_static_buy_model(...)` uruchamiany, gdy boundary
    istnieje;
  - brak boundary nadal daje typed `BLOCKED_BY_DATA`.
- `ghost-brain/src/guardian/post_buy/shadow_v2.rs`
  - dodano `min_out_raw` do `ShadowEntryFillModelConfig`.

## 3. Kontrakt ENTRY_BEFORE

Source:

```text
TriggerComponent
-> account_state_core.get_canonical_state(mint)
-> CanonicalPoolState
```

Capture boundary:

```text
PreparedBuyRequest build / Trigger boundary
before shadow simulation / post-buy handoff
```

Przenoszone pola:

- `CanonicalPoolState`;
- `captured_at_wall_ms`;
- `latest_observed_slot`;
- `state_slot`;
- `state_ts_ms`;
- `amount_lamports`;
- `min_tokens_out`;
- `fee_bps`;
- `slippage_tolerance_bps`;
- `token_decimals`;
- `sol_lamports`;
- `limitations`.

Nie wpisano fake `account_data_hash`.

## 4. Entry Fill Behavior

Gdy boundary payload istnieje:

```text
ENTRY_ATTEMPT
POOL_STATE_SAMPLE
ENTRY_FILL
```

`ENTRY_FILL` moze byc deterministic diagnostic fill:

```text
fill_status = FILLED
execution_simulation_ready = true
execution_label_grade = DIAGNOSTIC_SIM
research_provenance_ready = false, jezeli provenance ma blockery
```

Wypelniane pola, gdy formuly maja wystarczajace wejscia:

- `fill_price`;
- `fill_amount_tokens`;
- `fee_bps`;
- `min_out`;
- `own_impact_bps`;
- `deterministic_price_impact_bps`;
- `pool_state_before`;
- `pool_state_after`;
- `execution_simulation_ready`;
- `execution_label_grade`.

Gdy boundary payload nie istnieje:

```text
fill_status = BLOCKED_BY_DATA
ENTRY_POOL_STATE_BEFORE_UNAVAILABLE
ENTRY_FILL_POOL_STATE_SAMPLE_NOT_AVAILABLE_IN_RUNTIME_HANDOFF
```

## 5. Nadal Typed Unavailable / Ograniczenia

Nadal nie sa dowiedzione ani wypelnione jako live evidence:

- `account_data_hash`;
- pelny chain transaction/instruction/log ordering;
- live landing latency;
- failed/no-fill tx telemetry;
- realized live slippage;
- quote/fill divergence;
- live-confirmed fill;
- executable exit fill;
- full executable roundtrip PnL.

Brak `account_data_hash` jest provenance blockerem, nie fake value.

## 6. Granice Runtime

Bez zmian:

- BUY/REJECT;
- Gatekeeper policy;
- selector runtime;
- TX/Jito/live path;
- `shadow_close_only`;
- active close;
- R51.

Shadow V2 boundary evidence nie jest konsumowane przez decision path.

Approval flags pozostaja:

```text
runtime_approval = false
shadow_close_only_approval = false
active_close_approval = false
research_grade = false
live_equivalence = false
strategy_research_unblocked = false
```

## 7. Test Evidence

Uruchomione testy celowane:

```text
cargo test -p ghost-launcher shadow_v2_postbuy_entry_fill_uses_available_pool_state_refs -- --nocapture
cargo test -p ghost-launcher shadow_v2_entry_evidence_writes_attempt_and_blocked_fill -- --nocapture
cargo test -p ghost-launcher shadow_v2_postbuy_does_not_late_read_account_state_for_entry_boundary -- --nocapture
cargo test -p ghost-launcher shadow_v2_prepared_buy_request_captures_entry_boundary_before_shadow_simulation -- --nocapture
cargo test -p ghost-launcher shadow_v2_postbuy_entry_fill_executes_diagnostic_sim_from_entry_boundary_payload -- --nocapture
```

Wynik:

```text
PASS
```

Walidacja finalna PR powinna dodatkowo obejmowac:

```text
cargo check -p ghost-brain
cargo check -p ghost-launcher
cargo fmt --check
git diff --check
git diff --cached --check
forbidden staged-file guard
```

## 8. Decyzja Operacyjna

PR34-B moze zostac zmerge'owany jako implementation PR gotowy do pozniejszej
walidacji. Nie nalezy uruchamiac burnina bez osobnej decyzji operatora.

Nastepny runtime validation burnin, jezeli operator go zatwierdzi, powinien
sprawdzic:

- czy realne handoffy niosa `ShadowV2EntryBoundaryPayload`;
- czy `shadow_entry_fill_v2` ma `execution_simulation_ready=true`;
- czy `entry_fill FILLED count > 0`;
- czy `research_provenance_ready=false` tam, gdzie brakuje hash/order;
- czy brak boundary nadal daje typed `BLOCKED_BY_DATA`;
- czy nadal nie ma zmian BUY/REJECT/Gatekeeper/selector/TX/Jito/live path.
