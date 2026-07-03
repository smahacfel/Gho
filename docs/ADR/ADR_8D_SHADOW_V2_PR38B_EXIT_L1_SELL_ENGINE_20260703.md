# ADR-8D: Shadow V2 PR38-B EXIT L1 Sell Engine Wiring

Data: 2026-07-03

## Status

Accepted for implementation PR, pending runtime validation.

Final PR verdict:

```text
PR38_B_IMPLEMENTATION_READY_FOR_VALIDATION
```

## D1. Context

PR38-A0 zaakceptowal `EXIT_BOUNDARY_SOURCE_PRESENT` dla diagnostic exit
simulation. Akceptowany source to:

```text
ghost-brain/src/guardian/post_buy/engine.rs
MonitoringEngine::shadow_v2_exit_pool_state_sample_from_lifecycle()
```

Ten source buduje `PoolStateSampleV2` z `CanonicalPoolState` w lifecycle/exit
evidence path. Przed PR38-B `shadow_v2_exit_fill_from_lifecycle(...)` linkowal
`pool_state_before`, ale zawsze emitowal blocked exit fill.

## D2. Decision

Podlaczamy PR33 L1 sell engine wylacznie w shadow/lifecycle evidence path:

```text
MonitoringEngine::shadow_v2_exit_fill_from_lifecycle(...)
```

Warunki uruchomienia sell engine:

```text
record.record_type == ExitFilled
pool_state_before.is_some()
record.exit_token_amount_raw.is_some()
```

Dla tego przypadku budowany jest:

```text
ShadowExitFillModelConfig::bonding_curve(...)
```

i wywolywany jest:

```text
ShadowExitFillV2::from_static_sell_model(...)
```

## D3. Invariants

PR38-B zachowuje nastepujace invariants:

- brak zmiany BUY/REJECT;
- brak zmiany Gatekeeper policy;
- brak zmiany selector runtime;
- brak zmiany TX/Jito/live path;
- brak dotkniecia R51;
- brak `shadow_close_only`;
- brak active close;
- brak runtime approval;
- brak research-grade;
- brak live-equivalence;
- brak burnina w PR.

## D4. Non-Fake Data Contract

PR38-B nie wpisuje wartosci syntetycznych jako measured/live evidence.

Nie wypelniamy fake:

```text
min_out_raw
account_data_hash
realized_slippage_bps
quote_fill_divergence_bps
live landing telemetry
live failure telemetry
```

Fee/slippage sa jawnie oznaczone jako model assumptions:

```text
EXIT_FILL_MODEL_FEE_BPS_ASSUMPTION=100
EXIT_FILL_MODEL_SLIPPAGE_BPS_ASSUMPTION=150
```

Brak `account_data_hash` pozostaje provenance blockerem.

## D5. Quality Grade

Bez pelnej provenance wynik moze byc co najwyzej:

```text
execution_label_grade = DIAGNOSTIC_SIM
measurement_grade = DiagnosticOnly
research_provenance_ready = false
```

PR38-B nie nadaje `RESEARCH_CANDIDATE` ani `LIVE_CONFIRMED`.

## D6. Ordering Contract

PR38-B polega na PR33 engine causal guards. Nie wolno obchodzic blockerow:

```text
EXIT_FILL_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS
EXIT_FILL_POOL_STATE_AFTER_FILL_BOUNDARY
EXIT_FILL_POOL_STATE_NOT_STRICTLY_BEFORE_FILL_BOUNDARY
```

Jezeli ordering jest niebezpieczny albo niejednoznaczny dla exact execution
simulation, result pozostaje `BLOCKED_BY_DATA`.

## D7. Terminal Truth Contract

`final_pnl_executable_bps` nie moze byc liczony z:

- mark price;
- terminal truth jako input;
- replay selected point;
- blocked exit fill.

Executable PnL moze powstac tylko dla tego samego `position_id`, gdy:

```text
entry_fill == FILLED
exit_fill == FILLED
```

## D8. Tests

Uruchomione testy:

```text
cargo test -q -p ghost-brain shadow_v2_exit_fill_ --lib
cargo test -q -p ghost-brain shadow_v2_terminal_truth_sets_executable_pnl_only_when_exit_fill_executable --lib
cargo test -q -p ghost-launcher shadow_v2_no_decision_consumption_static_guard --lib
```

Wynik:

```text
PASS
```

## D9. Consequences

PR38-B daje implementation-ready L1 diagnostic sell simulation wiring, ale nie
daje jeszcze runtime proof.

Po merge wymagany jest osobno zatwierdzony validation/fidelity burnin, ktory
sprawdzi realne:

- `exit_fill_FILLED_count`;
- `exit_fill_BLOCKED_BY_DATA_count`;
- ordering blocker distribution;
- `terminal_truth_with_final_pnl_executable_bps_count`;
- `complete_executable_roundtrip_positions`.

Do czasu takiego burnina nadal obowiazuje:

```text
RUNTIME_APPROVAL=false
SHADOW_CLOSE_ONLY_APPROVAL=false
ACTIVE_CLOSE_APPROVAL=false
RESEARCH_GRADE=false
LIVE_EQUIVALENCE=false
STRATEGY_RESEARCH_UNBLOCKED=false
BURNIN_AUTHORIZATION=false
```
