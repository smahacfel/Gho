# Raport Shadow V2 PR38-A0: EXIT_BEFORE Source Audit

Data: 2026-07-03

Finalny werdykt:

```text
EXIT_BOUNDARY_SOURCE_PRESENT
```

Ten werdykt oznacza tylko, ze obecny runtime ma deterministyczny kandydat na
`EXIT_BEFORE` dla diagnostic Shadow V2 L1 sell simulation. Nie oznacza, ze
obecny runtime juz generuje executable exit fill. Obecny `shadow_exit_fill_v2`
nadal pozostaje blokowanym recordem pochodzacym z legacy lifecycle evidence.

Approval flags pozostaja bez zmian:

```text
RUNTIME_APPROVAL=false
SHADOW_CLOSE_ONLY_APPROVAL=false
ACTIVE_CLOSE_APPROVAL=false
RESEARCH_GRADE=false
LIVE_EQUIVALENCE=false
STRATEGY_RESEARCH_UNBLOCKED=false
```

## 1. Zakres

PR38-A0 jest read-only audit / report-only. Nie uruchomiono burnina,
validation runu ani PR38-B implementation. Nie zmieniono Rust runtime.

Audyt odpowiada na pytanie:

```text
Czy runtime ma uczciwe, deterministyczne, no-lookahead zrodlo pool_state_before
dla exit fill, czyli EXIT_BEFORE, ktore moze zasilic ShadowV2FillEngine SELL?
```

## 2. Definicja EXIT_BEFORE

`EXIT_BEFORE` musi byc stanem poola, ktory:

- jest dostepny przed diagnostic/simulated exit fill;
- reprezentuje rynek na lub przed exit boundary, nie po fakcie;
- nie pochodzi z terminal truth, final PnL, outcome, replay-derived lifecycle
  ani post-exit danych;
- ma raw reserves wymagane przez `ShadowV2FillEngine` sell path;
- ma albo moze dostac `token_decimals` i `sol_lamports` normalization;
- ma `event_order_key` / slot / sequence wystarczajace do causal guard;
- moze byc zdegradowany do `DIAGNOSTIC_SIM`, jezeli brakuje
  `account_data_hash` albo pelnego chain-order;
- nie jest late-readem aktualnego `AccountStateCore` w `PostBuyRuntime` bez
  historycznego powiazania z exit boundary.

## 3. Najwazniejsze ustalenie

Primary source istnieje w `MonitoringEngine`, nie w `TerminalTruth` i nie w
derived replay.

Kod:

```text
ghost-brain/src/guardian/post_buy/engine.rs
MonitoringEngine::append_shadow_v2_lifecycle_record()
MonitoringEngine::shadow_v2_exit_pool_state_sample_from_lifecycle()
MonitoringEngine::shadow_v2_exit_fill_from_lifecycle()
```

Sciezka obecna:

1. Dla `ExitFilled`, `ExitBlocked` oraz niektorych `PositionClosed`
   `append_shadow_v2_lifecycle_record()` emituje `ShadowPathSampleV2`,
   `ShadowExitAttemptV2`, opcjonalny `PoolStateSampleV2`,
   `ShadowExitFillV2` i `ShadowTerminalTruthV2`.
2. `shadow_v2_exit_pool_state_sample_from_lifecycle()` pobiera
   `CanonicalPoolState` przez `current_canonical_state(base_mint)`.
3. Z tego stanu buduje `pool_state_sample_v2` przez
   `PoolStateSampleV2::from_account_state_core(...)`.
4. Sample dostaje `TemporalClass::PostEntry`, `ClockDomain::StreamObservedMs`,
   `token_decimals=6`, `sol_lamports=1_000_000_000`, reserves i slot
   `state.last_update_slot`.
5. Brak `account_data_hash` jest jawnie oznaczony:

```text
POOL_STATE_SAMPLE_FROM_ACCOUNT_STATE_CORE_WITHOUT_RAW_ACCOUNT_HASH
POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME
TOKEN_DECIMALS_ASSUMED_PUMPFUN_6
```

To wystarcza jako `DIAGNOSTIC_SIM` source dla PR38-B, jezeli PR38-B zachowa
causal guards i nie nada research-grade. Brak account hash ma blokowac
`research_provenance_ready`, ale nie musi blokowac samego deterministic L1 sell
simulation.

## 4. Co obecnie NIE jest jeszcze zrobione

Obecny runtime nie uruchamia `ShadowV2FillEngine` dla exit lifecycle evidence.

Kod:

```text
ghost-brain/src/guardian/post_buy/engine.rs
MonitoringEngine::shadow_v2_exit_fill_from_lifecycle()
```

Funkcja:

- zbiera blockery typu:

```text
EXIT_FILL_DERIVED_FROM_LEGACY_LIFECYCLE_EVIDENCE
EXIT_POOL_STATE_AFTER_UNAVAILABLE
FILL_PRICE_UNAVAILABLE
SLIPPAGE_BPS_UNAVAILABLE
OWN_IMPACT_BPS_UNAVAILABLE
FEE_BPS_UNAVAILABLE
LANDING_TELEMETRY_UNAVAILABLE
QUOTE_FILL_DIVERGENCE_UNAVAILABLE
```

- jezeli pool state istnieje, wywoluje:

```text
ShadowExitFillV2::blocked_with_pool_state(...)
```

- jezeli pool state nie istnieje, wywoluje:

```text
ShadowExitFillV2::blocked_without_pool_state(...)
```

Nie wywoluje jeszcze:

```text
ShadowExitFillV2::from_static_sell_model(...)
ShadowV2FillEngine::simulate(... side=Sell, boundary_kind=ExitBefore ...)
```

Dlatego executable exit fill i executable roundtrip PnL nadal nie sa
udowodnione.

## 5. Kandydaci EXIT_BEFORE

Pelna macierz:

```text
reports/selector/shadow_v2_pr38_a0_exit_before_candidate_matrix.csv
```

Podsumowanie:

| Verdict | Count |
|---|---:|
| `PRESENT` | 4 |
| `MISSING` | 3 |
| `AMBIGUOUS` | 2 |
| `REJECTED` | 5 |

Najwazniejsze klasyfikacje:

| Candidate | Verdict | Uzasadnienie |
|---|---|---|
| `exit_account_state_core_lifecycle_boundary` | `PRESENT` | `MonitoringEngine` buduje `PoolStateSampleV2` z `CanonicalPoolState` w lifecycle/exit evidence path. |
| `exit_fill_pool_state_before_ref` | `PRESENT` | `shadow_exit_fill_v2` potrafi linkowac `pool_state_before`, ale obecnie nadal jest blocked fill. |
| `remaining_token_amount_raw_for_sell` | `PRESENT` | Sell input amount istnieje w `MonitoredPosition` i `record.exit_token_amount_raw`. |
| `static_exit_fee_slippage_model` | `PRESENT` | PR33/PR6 static model ma fee/slippage assumptions; to nie jest live realized slippage. |
| `selected_shadow_path_sample_mark_boundary` | `AMBIGUOUS` | Obecny path sample jest legacy lifecycle mark i nie niesie `pool_state_ref`; moze wskazywac timing, ale nie jest samodzielnym pool-state source. |
| `latest_post_entry_account_state_core_state` | `AMBIGUOUS` | Mutable latest state jest akceptowalny dopiero wtedy, gdy jest uchwycony jako exit boundary sample, nie jako dowolny late read. |
| `sell_min_out_raw` | `MISSING` | Nie znaleziono dedykowanego sell `min_out_raw`; L1 moze policzyc modelowy `min_output_amount` ze slippage tolerance. |
| `current_exit_fill_constructor_path` | `MISSING` | Obecny constructor path nie uruchamia sell engine. |
| `terminal_truth_v2` | `REJECTED` | Outcome/post-exit record; nie moze byc `pool_state_before`. |
| `shadowledger_replay_lifecycle_state` | `REJECTED` | Mark/replay/fallback evidence; nie canonical EXIT_BEFORE. |
| `oracle_runtime_price_context` | `REJECTED` | Price context nie jest exit boundary i nie jest przenoszony do exit fill writer. |

## 6. Wymagane pola przyszlego exit fill

| Pole | Obecny status |
|---|---|
| `sell input amount raw` | `PRESENT`: `MonitoredPosition.remaining_token_amount_raw`, `entry_token_amount_raw`, `shadow_exit_token_amount_raw(...)`, `record.exit_token_amount_raw`. |
| `min_out raw` | `MISSING`: brak realnego sell min-out; model moze uzyc `None` i policzyc `min_output_amount` ze slippage tolerance, bez live min-out claim. |
| `fee_bps` | `PRESENT_AS_MODEL_ASSUMPTION`: `ShadowExitFillModelConfig::bonding_curve(...)`. |
| `slippage_tolerance_bps` | `PRESENT_AS_MODEL_ASSUMPTION`: nie jest realized slippage. |
| `pool_phase` | `PRESENT_AS_MODEL_ASSUMPTION`: `ShadowV2PoolPhase::BondingCurve` dla pump.fun bonding curve. |
| `pool_state_before` | `PRESENT`: `PoolStateSampleV2` z `shadow_v2_exit_pool_state_sample_from_lifecycle`. |
| `raw reserves` | `PRESENT`: `CanonicalPoolState.virtual_*` i `real_*` reserves. |
| `token_decimals` | `PRESENT_AS_ASSUMPTION`: `6`, jawnie oznaczane. |
| `sol_lamports` | `PRESENT`: `1_000_000_000`, jawnie w sample. |
| `boundary slot / event_order_key` | `PRESENT_WITH_LIMITATIONS`: slot z `state.last_update_slot`; chain order components explicit `UNKNOWN`; fill order z `exit_landed_slot`/`exit_sample_slot` i process seq. |
| `position_id correlation` | `PRESENT`: lifecycle record/envelope carries `position_id`. |
| `selected exit reason` | `PRESENT`: `shadow_v2_exit_trigger_label(record)` maps target/stop/time_stop/etc. |
| `source of selected exit boundary` | `PRESENT_WITH_LIMITATIONS`: lifecycle exit evidence + AccountStateCore current canonical state at emission. |
| `account_data_hash` | `MISSING`: unavailable in runtime; must remain provenance blocker. |

## 7. Why PRESENT, not MISSING

PR18E juz ustalil, ze exit fill rows moga miec `pool_state_before` linked, a
PR18E validation burnin pokazal:

```text
shadow_exit_fill_v2 rows = 185
with pool_state_before = 185
exit fill BLOCKED_BY_DATA = 185
```

To znaczy: source state dotarl do exit fill record, ale fill pozostawal
zablokowany, bo w tamtym kodzie brakowalo L1 execution engine wiring.

Po PR33 engine juz istnieje. Po PR34-A/B/C entry side dziala w realnym shadow
flow. Dla exit side obecny kod nadal nie wykonal analogicznego kroku. To
uzasadnia:

```text
EXIT_BOUNDARY_SOURCE_PRESENT
```

ale tylko jako zgoda na minimalny PR38-B implementation, nie jako gotowy
executable exit proof.

## 8. Minimalny plan PR38-B

PR38-B moze byc malym implementation PR, jezeli zachowa ponizszy kontrakt.

### 8.1. Source

Uzyc tylko:

```text
MonitoringEngine::shadow_v2_exit_pool_state_sample_from_lifecycle(record)
```

jako source `EXIT_BEFORE`.

Nie uzywac:

- `TerminalTruthV2`;
- final PnL;
- derived replay/lifecycle jako canonical input;
- late `PostBuyRuntime.account_state_core` read;
- ShadowLedger/replay-only mark output;
- OracleRuntime price context.

### 8.2. Konstrukcja fill

W `MonitoringEngine::shadow_v2_exit_fill_from_lifecycle(...)`, dla
`pool_state_before.is_some()` i `record.exit_token_amount_raw.is_some()`:

1. zbudowac `ShadowExitFillModelConfig::bonding_curve(...)`;
2. przekazac:
   - `input_token_raw = record.exit_token_amount_raw`;
   - `slippage_bps` jako jawne model assumption;
   - `fee_bps` jako jawne model assumption;
   - `executable_fill_model_version = SHADOW_V2_EXIT_FILL_MODEL_VERSION`;
3. wywolac:

```text
ShadowExitFillV2::from_static_sell_model(envelope, fill_order_key, pool_state_before, &config)
```

4. nie wpisywac fake `min_out_raw`;
5. nie wpisywac fake `account_data_hash`;
6. zachowac explicit `UNKNOWN` dla brakujacych chain-order components.

### 8.3. Grade

Maksymalny grade bez raw account hash i pelnego chain-order:

```text
execution_label_grade = DIAGNOSTIC_SIM
research_provenance_ready = false
measurement_grade = DiagnosticOnly
```

`RESEARCH_CANDIDATE` moze pojawic sie tylko, jezeli
`PoolStateSampleV2::research_blockers()` jest puste. Aktualnie nie nalezy tego
oczekiwac, bo `account_data_hash` jest unavailable.

### 8.4. Causal guards

PR38-B musi zachowac blokery PR33 engine:

```text
EXIT_FILL_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS
EXIT_FILL_POOL_STATE_AFTER_FILL_BOUNDARY
EXIT_FILL_POOL_STATE_NOT_STRICTLY_BEFORE_FILL_BOUNDARY
```

Jesli ordering nie przejdzie, result ma byc `BLOCKED_BY_DATA`, a nie fake
`FILLED`.

### 8.5. Terminal executable PnL

`final_pnl_executable_bps` moze byc ustawione dopiero wtedy, gdy ten sam
`position_id` ma:

```text
entry_fill = FILLED
exit_fill = FILLED
```

Nie wolno liczyc executable PnL z mark price, terminal truth albo blocked exit
fill.

### 8.6. Testy wymagane w PR38-B

Minimalnie:

- exit fill z lifecycle pool sample przechodzi przez `from_static_sell_model`
  i daje `FILLED`/`DIAGNOSTIC_SIM`, gdy ordering jest causal-safe;
- brak `pool_state_before` nadal daje `BLOCKED_BY_DATA`;
- brak `exit_token_amount_raw` nadal daje typed blocker;
- same-slot ambiguous ordering daje `BLOCKED_BY_DATA`;
- terminal executable PnL pozostaje `None`, jezeli exit fill jest blocked;
- terminal executable PnL moze byc ustawiony tylko przy entry+exit `FILLED` dla
  tego samego `position_id`;
- static guard potwierdza brak konsumpcji Shadow V2 przez BUY/REJECT,
  Gatekeeper, selector, TX/Jito/live path.

## 9. Odrzucone zrodla

### TerminalTruthV2

`ShadowTerminalTruthV2` powstaje po lifecycle close i ma
`final_pnl_mark_bps`. To jest outcome. Nie moze byc `EXIT_BEFORE`.

### ShadowExitPathReplay selected point

`replay_exit_from_path_v2(...)` jest derived mark/path logic. Moze sluzyc do
etykiet mark-path, ale nie moze byc canonical pool-state inputem, jezeli nie
odwoluje sie do causal `pool_state_sample_v2`.

### ShadowLedger / legacy lifecycle mark

`ShadowPathSampleV2::from_legacy_lifecycle_mark(...)` jawnie zapisuje brak
pool-state provenance. To nie jest executable fill source.

### OracleRuntime price context

`OracleRuntime::resolve_price_context(...)` jest decision/price context z
fallbackami i nie jest przenoszony do exit fill writer jako exit boundary.

### Late PostBuyRuntime AccountStateCore read

Samo `PostBuyRuntimeConfig.account_state_core` nie jest akceptowalnym
`EXIT_BEFORE`, bo dowolny pozny read moze byc nowszy niz exit boundary.

## 10. Acceptance status

PR38-A0 spelnia:

- report-only;
- brak zmian Rust runtime;
- brak burnina;
- brak validation runu;
- brak raw JSONL/log/runtime scope/local TOML w commit scope;
- kandydaci sklasyfikowani;
- terminal/outcome/replay source jawnie odrzucone;
- late read jawnie odrzucony poza boundary capture;
- finalny verdict jeden i dokladny:

```text
EXIT_BOUNDARY_SOURCE_PRESENT
```

## 11. Konsekwencja

PR38-B jest dopuszczalny jako minimalny shadow-only implementation PR dla
diagnostic exit L1 sell simulation. PR38-B nadal nie moze nadac runtime
approval, research-grade, live-equivalence, `shadow_close_only`, active close,
strategy proof ani executable roundtrip proof bez osobnego validation burnina i
audytu.
