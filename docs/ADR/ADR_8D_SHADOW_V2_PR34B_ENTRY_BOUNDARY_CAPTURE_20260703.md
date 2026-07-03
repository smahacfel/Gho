# ADR-8D: Shadow V2 PR34-B Entry Boundary Capture

Data: 2026-07-03

## Status

```text
PROPOSED_FOR_PR34_B
```

Finalny verdict implementacyjny:

```text
PR34_B_IMPLEMENTATION_READY_FOR_VALIDATION
```

## D1. Problem

PR34-A0 potwierdzil, ze deterministyczne zrodlo `ENTRY_BEFORE` istnieje
upstream w Trigger boundary:

```text
TriggerComponent -> AccountStateCore -> CanonicalPoolState
```

Jednoczesnie obecny Shadow V2 writer w `PostBuyRuntime` nie mial tego stanu w
handoffie. W praktyce entry fill pozostawal `BLOCKED_BY_DATA`, bo produkcyjna
sciezka przekazywala `entry_pool_state_before=None`.

Kluczowe ryzyko dla PR34-B:

```text
nie wolno odczytac AccountStateCore pozno w PostBuyRuntime jako skrotu,
bo bylby to aktualny reducer state, nie historyczny snapshot ENTRY_BEFORE.
```

## D2. Decyzja

Dodajemy addytywny, shadow-only payload:

```text
ShadowV2EntryBoundaryPayload
```

Payload jest capture'owany w Trigger boundary podczas budowy
`PreparedBuyRequest`, przed shadow simulation i przed post-buy handoff. Niesie:

- `CanonicalPoolState`;
- capture wall-clock ms;
- state slot/time;
- latest observed slot, jezeli dostepny;
- amount lamports;
- min_out;
- jawne fee/slippage assumptions;
- token decimals i lamports normalization;
- limitations, w tym brak `account_data_hash`.

Payload jest przenoszony przez:

```text
PreparedBuyRequest
-> ShadowBuySimulationReport
-> OracleRuntime PostBuySubmitted
-> PostBuyRuntime
```

W `PostBuyRuntime` payload jest mapowany na `PoolStateSampleV2`, a entry fill
jest liczony przez istniejacy PR33 L1 engine poprzez:

```text
ShadowEntryFillV2::from_static_buy_model(...)
```

Brak payloadu nadal daje typed:

```text
BLOCKED_BY_DATA
ENTRY_POOL_STATE_BEFORE_UNAVAILABLE
ENTRY_FILL_POOL_STATE_SAMPLE_NOT_AVAILABLE_IN_RUNTIME_HANDOFF
```

## D3. Evidence

Implementacja PR34-B obejmuje:

- `ghost-launcher/src/events.rs`
  - `ShadowV2EntryBoundaryPayload`;
  - addytywne pole `shadow_v2_entry_boundary` w `PostBuySubmitted`;
  - builder `with_shadow_v2_entry_boundary(...)`.
- `ghost-launcher/src/components/trigger/component.rs`
  - capture `CanonicalPoolState` w Trigger boundary;
  - przeniesienie payloadu w `PreparedBuyRequest`.
- `ghost-launcher/src/components/trigger/shadow_run.rs`
  - przeniesienie payloadu przez shadow simulation report.
- `ghost-launcher/src/oracle_runtime.rs`
  - przekazanie payloadu do `PostBuySubmitted`.
- `ghost-launcher/src/components/post_buy_runtime.rs`
  - budowa `PoolStateSampleV2` z boundary payload;
  - entry fill przez PR33 L1 model, jezeli boundary jest obecny;
  - fallback `BLOCKED_BY_DATA`, jezeli boundary nie istnieje.
- `ghost-brain/src/guardian/post_buy/shadow_v2.rs`
  - addytywne `min_out_raw` w `ShadowEntryFillModelConfig`.

Testy potwierdzaja:

- capture przed shadow simulation:
  `shadow_v2_prepared_buy_request_captures_entry_boundary_before_shadow_simulation`;
- brak late `AccountStateCore` shortcut w PostBuyRuntime:
  `shadow_v2_postbuy_does_not_late_read_account_state_for_entry_boundary`;
- payload -> `PoolStateSampleV2` -> diagnostic entry fill:
  `shadow_v2_postbuy_entry_fill_executes_diagnostic_sim_from_entry_boundary_payload`;
- brak payloadu nadal daje blocked entry fill:
  `shadow_v2_entry_evidence_writes_attempt_and_blocked_fill`;
- dostarczony pool state jest linkowany do entry fill:
  `shadow_v2_postbuy_entry_fill_uses_available_pool_state_refs`.

## D4. Runtime Boundary

PR34-B nie zmienia:

- BUY/REJECT;
- Gatekeeper policy;
- selector runtime;
- TX/Jito/live path;
- `shadow_close_only`;
- active close;
- R51.

Shadow V2 boundary payload i wynikowe records sa diagnostic/shadow-only.
Nie sa konsumowane przez decision path, policy path, selector, TX submit ani
live path.

Nie uruchomiono burnina ani validation runu.

## D5. Measurement Semantics

Jezeli boundary payload istnieje i zawiera reserves/amount/min_out/fee/slippage
/decimals/order wystarczajace do formuly, entry fill moze dostac:

```text
fill_status = FILLED
execution_simulation_ready = true
execution_label_grade = DIAGNOSTIC_SIM
```

Bez pelnej provenance nie wolno podnosic wyniku do live-equivalence ani
automatycznie do research-grade.

Brak `account_data_hash` jest zapisywany jako provenance blocker:

```text
POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME
```

To blokuje research provenance, ale nie musi blokowac samej deterministic
diagnostic symulacji.

## D6. Ograniczenia

PR34-B nadal nie dowodzi:

- realnego landed fill;
- realized live slippage;
- quote/fill divergence;
- landing latency;
- failed/no-fill tx telemetry;
- live-confirmed calibration;
- executable exit fill;
- executable roundtrip PnL.

Fee `100 bps`, token decimals `6` i lamports normalization sa jawnie opisanymi
assumptions dla pump.fun bonding-curve diagnostic sim. Brak raw account hash i
niepelne chain-order components pozostaja ograniczeniami provenance.

## D7. Rejected Alternatives

Odrzucono:

1. Pozny odczyt `PostBuyRuntime.account_state_core.get_canonical_state(...)`.
   Powod: lookahead risk przez aktualny reducer state.
2. Fake `account_data_hash`.
   Powod: falszuje provenance.
3. Podlaczenie Shadow V2 payloadu do BUY/REJECT albo policy.
   Powod: PR34-B jest shadow-only evidence path.
4. Runtime burnin w PR34-B.
   Powod: walidacja runtime wymaga osobnej decyzji operatora po merge.

## D8. Konsekwencje

Po PR34-B mozna dopiero zaplanowac kolejny validation/fidelity burnin, ale tylko
po osobnej decyzji operatora.

Minimalna oczekiwana poprawa w kolejnym burninie:

```text
shadow_entry_fill_v2 execution_simulation_ready=true
entry_fill FILLED count > 0, jezeli boundary payload dociera z realnych handoffow
entry_fill max grade bez pelnej provenance = DIAGNOSTIC_SIM
```

Nie zmieniaja sie approval flags:

```text
runtime_approval = false
shadow_close_only_approval = false
active_close_approval = false
research_grade = false
live_equivalence = false
strategy_research_unblocked = false
```
