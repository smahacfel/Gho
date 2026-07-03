# Raport Shadow V2 PR38-B: EXIT L1 Sell Engine Wiring

Data: 2026-07-03

Finalny werdykt PR38-B:

```text
PR38_B_IMPLEMENTATION_READY_FOR_VALIDATION
```

Ten PR jest minimalnym shadow-only implementation PR. Nie uruchomiono burnina,
nie wykonano validation runu i nie nadano zadnego approval.

Approval flags pozostaja:

```text
RUNTIME_APPROVAL=false
SHADOW_CLOSE_ONLY_APPROVAL=false
ACTIVE_CLOSE_APPROVAL=false
RESEARCH_GRADE=false
LIVE_EQUIVALENCE=false
STRATEGY_RESEARCH_UNBLOCKED=false
BURNIN_AUTHORIZATION=false
```

## 1. Cel

PR38-A0 zaakceptowal diagnostic `EXIT_BEFORE` source w:

```text
ghost-brain/src/guardian/post_buy/engine.rs
MonitoringEngine::shadow_v2_exit_pool_state_sample_from_lifecycle()
```

PR38-B podlacza istniejacy po PR33 L1 `ShadowV2FillEngine` sell path do
exit/lifecycle evidence path, ale tylko wtedy, gdy runtime ma:

- `pool_state_before`;
- `record.exit_token_amount_raw`;
- lifecycle record typu `ExitFilled`.

To nadal jest diagnostic deterministic simulation, nie live fill proof.

## 2. Zmieniony kod

Plik:

```text
ghost-brain/src/guardian/post_buy/engine.rs
```

Zmieniona sciezka:

```text
MonitoringEngine::shadow_v2_exit_fill_from_lifecycle(...)
```

Nowa logika:

1. Jezeli `record.record_type == ExitFilled`,
2. oraz `pool_state_before.is_some()`,
3. oraz `record.exit_token_amount_raw.is_some()`,
4. budowany jest:

```text
ShadowExitFillModelConfig::bonding_curve(
    input_token_raw = record.exit_token_amount_raw.unwrap(),
    slippage_bps = 150,
    fee_bps = 100,
    executable_fill_model_version = SHADOW_V2_EXIT_FILL_MODEL_VERSION,
)
```

5. Nastapnie wywolywane jest:

```text
ShadowExitFillV2::from_static_sell_model(envelope, fill_order_key, pool_state_before, &config)
```

Jezeli brakuje `pool_state_before` albo `exit_token_amount_raw`, sciezka
pozostaje `BLOCKED_BY_DATA` z typed limitations.

## 3. Czego PR38-B nie udaje

PR38-B nie wpisuje fake values:

- brak fake `min_out_raw`;
- brak fake `account_data_hash`;
- brak fake `realized_slippage_bps`;
- brak fake `quote_fill_divergence_bps`;
- brak fake live landing / failure telemetry.

Brak `account_data_hash` pozostaje provenance blockerem i ogranicza wynik do:

```text
execution_label_grade = DIAGNOSTIC_SIM
research_provenance_ready = false
measurement_grade = DiagnosticOnly
```

## 4. Ordering i no-lookahead

PR38-B nie obchodzi causal guards z PR33 engine. Jezeli engine wykryje
problem orderingu, exit fill zostaje `BLOCKED_BY_DATA`.

Zachowane blockery:

```text
EXIT_FILL_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS
EXIT_FILL_POOL_STATE_AFTER_FILL_BOUNDARY
EXIT_FILL_POOL_STATE_NOT_STRICTLY_BEFORE_FILL_BOUNDARY
```

Testy pokrywaja:

- causal-safe case daje `FILLED`;
- same-slot ambiguity daje `BLOCKED_BY_DATA`;
- pool state po fill boundary daje `BLOCKED_BY_DATA`;
- brak pool state daje `BLOCKED_BY_DATA`;
- brak token amount daje `BLOCKED_BY_DATA`.

## 5. Terminal PnL

PR38-B nie liczy executable PnL z mark price, terminal truth ani blocked exit
fill.

Obowiazujacy kontrakt pozostaje:

```text
final_pnl_executable_bps != null
```

moze powstac tylko wtedy, gdy ten sam `position_id` ma:

- entry fill `FILLED`;
- exit fill `FILLED`.

Istniejacy test:

```text
cargo test -q -p ghost-brain shadow_v2_terminal_truth_sets_executable_pnl_only_when_exit_fill_executable --lib
```

potwierdza, ze blocked exit fill nie generuje executable terminal PnL.

## 6. Testy

Uruchomione testy:

```text
cargo test -q -p ghost-brain shadow_v2_exit_fill_ --lib
cargo test -q -p ghost-brain shadow_v2_terminal_truth_sets_executable_pnl_only_when_exit_fill_executable --lib
cargo test -q -p ghost-launcher shadow_v2_no_decision_consumption_static_guard --lib
```

Wyniki:

```text
shadow_v2_exit_fill_: 9 passed
terminal executable PnL guard: 1 passed
no-decision-consumption static guard: 1 passed
```

## 7. Granice runtime

PR38-B nie zmienia:

- BUY/REJECT;
- Gatekeeper policy;
- selector runtime;
- TX/Jito/live path;
- R51;
- `shadow_close_only`;
- active close.

Shadow V2 pozostaje evidence-only / diagnostic-only. Static guard potwierdza,
ze Shadow V2 nadal nie jest konsumowany przez BUY/REJECT, Gatekeeper, selector
ani TX/Jito/live path.

## 8. Co nadal wymaga validation burnin

PR38-B jest gotowy do walidacji, ale sam jej nie wykonuje.

Nastepny burnin po osobnej zgodzie operatora powinien zaraportowac co najmniej:

- `exit_pool_state_before_count`;
- `exit_token_amount_raw_count`;
- `exit_fill_FILLED_count`;
- `exit_fill_BLOCKED_BY_DATA_count`;
- `execution_simulation_ready_true_count`;
- `execution_label_grade_DIAGNOSTIC_SIM_count`;
- `research_provenance_ready_false_count`;
- `EXIT_FILL_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS_count`;
- `EXIT_FILL_POOL_STATE_AFTER_FILL_BOUNDARY_count`;
- `EXIT_FILL_POOL_STATE_NOT_STRICTLY_BEFORE_FILL_BOUNDARY_count`;
- `terminal_truth_with_final_pnl_executable_bps_count`;
- `complete_executable_roundtrip_positions`.

## 9. Konkluzja

PR38-B domyka minimalne wiring L1 diagnostic SELL simulation dla exit side.

Nie dowodzi jeszcze:

- realnego executable exit coverage w runtime burninie;
- complete executable roundtrip PnL;
- research-grade provenance;
- live-equivalence;
- runtime approval.

Finalny stan:

```text
PR38_B_IMPLEMENTATION_READY_FOR_VALIDATION
```
