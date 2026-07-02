# Plan PR33-PR36: Shadow V2 Execution Simulation L1

Status planu:

```text
ACCEPTANCE_READY_FOR_OPERATOR_REVIEW
```

Ten plan jest zaakceptowanym kierunkiem resetu priorytetow po serii prac nad
Shadow V2 logging harness, canonical events, manifestami i audytami. Nie jest
to zgoda na automatyczne wlaczenie runtime approval, `shadow_close_only`,
active close, strategy proof ani live-equivalence.

## 1. Cel i zasada nadrzedna

Zamrazamy L0 jako infrastrukture:

```text
canonical recorder
manifesty
replay/lifecycle
density rows
temporal/order audits
shutdown/flush
```

Nie rozwijamy dalej miernika jako glownego celu. Nastepny etap to L1:
deterministic execution simulation.

Docelowy podzial jakosci:

```text
L0 = logging harness / recorder / manifest infrastructure
L1 = deterministic execution simulation
L2 = research-grade shadow po L1 + offline audits + sample size
L3 = live-equivalent shadow po live-confirmed calibration dataset
```

Najwazniejsza regula:

```text
execution simulation != research provenance
```

L1 moze policzyc diagnostic deterministic fill, nawet jezeli provenance nie
jest jeszcze research-grade. Brak `account_data_hash` nie moze automatycznie
blokowac samego deterministic fill, jezeli do formuly sa dostepne:

```text
reserves
token_decimals
sol_lamports
input_amount
fee_bps
slippage_tolerance_bps / min_out
pool_phase
causal event_order_key
```

Brak `account_data_hash` blokuje:

```text
research_provenance_ready = false
```

ale nie musi blokowac:

```text
execution_simulation_ready = true
```

## 2. Kontrakt L1: engine, labels, provenance

### 2.1. Canonical formula source

PR33 musi uzywac istniejacej biblioteki jako jedynego canonical formula
source:

```text
ghost-core/src/shadow_v2_price.rs
```

Nie kopiowac formul do nowego modulu.

`ShadowV2FillEngine` ma tylko opakowac te biblioteke w Ghost-specific
execution contract:

```text
input validation
causal boundary validation
execution/provenance grade classification
typed no-fill/block reasons
conversion into ShadowEntryFillV2 / ShadowExitFillV2
derived after-state construction
```

Formula source:

```text
shadow_v2_constant_product_price_v1
```

Model versions:

```text
shadow_v2_entry_fill_static_constant_product_v1
shadow_v2_exit_fill_static_constant_product_v1
```

### 2.2. Nowy modul

Dodac centralny modul:

```text
ghost-brain/src/guardian/post_buy/shadow_v2_execution.rs
```

`shadow_v2.rs` ma przestac puchnac jako miejsce losowych helperow. Zostaje
schema/canonical event surface.

### 2.3. Typy L1

Dodac typy:

```rust
ShadowV2ExecutionSide = Buy | Sell

ShadowV2BoundaryKind =
  EntryBefore
  EntryAfterDerived
  ExitBefore
  ExitAfterDerived

ShadowV2ExecutionLabelGrade =
  DiagnosticSim
  ResearchCandidate
  LiveConfirmed

ShadowV2NoFillReason =
  MinOutNotMet
  ZeroOutput
  InsufficientReserves
  PoolCompleteOrMigrated
  UnsupportedPoolPhase
  StalePoolState
  OrderingAmbiguity
  TokenAmountMissingForSell

ShadowV2BlockedReason =
  PoolStateMissing
  PoolStateIncomplete
  PoolStateHashMissing
  PoolStateStalenessUnknownOrReversed
  OrderingAmbiguity
  TokenAmountMissing
  FeeModelMissing
  FormulaUnsupported
  UnsupportedPoolPhase
  MissingTokenDecimals
  MissingLamportsNormalization
```

Dodac input/output engine:

```rust
ShadowV2ExecutionInput {
  side,
  pool_state_before,
  input_amount_raw,
  min_out_raw,
  fee_bps,
  slippage_tolerance_bps,
  pool_phase,
  token_decimals,
  event_order_key,
  boundary_ts_ms,
  boundary_slot,
  model_version,
}

ShadowV2ExecutionOutcome {
  fill_status,
  execution_simulation_ready,
  research_provenance_ready,
  execution_label_grade,
  provenance_ready,
  provenance_blockers,
  no_fill_reason,
  blocked_reasons,
  fail_reason,
  fill_price,
  fill_price_source,
  fill_amount_sol,
  fill_amount_tokens,
  expected_output_raw,
  min_out_raw,
  slippage_tolerance_bps,
  deterministic_price_impact_bps,
  realized_slippage_bps,
  own_impact_bps,
  fee_bps,
  quote_fill_divergence_bps,
  pool_state_before_ref,
  pool_state_after_derived,
  reconstruction_status,
  quality,
  limitations,
}
```

Zachowac obecny `FillStatus` dla kompatybilnosci:

```text
FILLED
NO_FILL
FAILED
BLOCKED_BY_DATA
```

Nie dodawac wariantow typu `NO_FILL_MIN_OUT` do `FillStatus`. Powod ma isc do
`no_fill_reason`.

### 2.4. Rozdzielenie execution vs provenance

Kazdy entry/exit fill musi miec jawne pola:

```text
execution_simulation_ready: bool
research_provenance_ready: bool
execution_label_grade: DIAGNOSTIC_SIM | RESEARCH_CANDIDATE | LIVE_CONFIRMED
provenance_ready: bool
provenance_blockers: [...]
```

Interpretacja:

```text
DIAGNOSTIC_SIM
  deterministic fill policzony z dostepnych causal inputs,
  ale brakuje pelnej research provenance, np. account_data_hash.

RESEARCH_CANDIDATE
  deterministic fill policzony,
  pool_state_before ma wystarczajaca provenance,
  ordering i no-lookahead sa zgodne,
  nadal nie jest live-equivalent.

LIVE_CONFIRMED
  nieuzywane w PR33-PR36,
  zarezerwowane dla L3 calibration/live-confirmed dataset.
```

Brak `account_data_hash`:

```text
execution_simulation_ready moze byc true
research_provenance_ready = false
execution_label_grade = DIAGNOSTIC_SIM
provenance_blockers includes POOL_STATE_ACCOUNT_DATA_HASH_MISSING
```

### 2.5. Nomenklatura slippage

Nie uzywac golego `slippage_bps` jako znaczacego live realized slippage.

W L1 rozdzielic:

```text
slippage_tolerance_bps
deterministic_price_impact_bps
realized_slippage_bps = None
```

Jezeli istniejace pole JSON `slippage_bps` zostaje dla kompatybilnosci, schema
i raport musza mowic:

```text
slippage_bps_compat = configured tolerance, not realized live slippage
```

Preferowane docelowe pola:

```text
slippage_tolerance_bps = configured tolerance
deterministic_price_impact_bps = model impact from formula
realized_slippage_bps = null in L1
```

### 2.6. Quote/fill divergence

W L1 static simulation:

```text
quote_fill_divergence_bps = None
```

Nie wpisywac `0`.

`0` oznaczaloby zmierzona zerowa dywergencje. L1 nie ma dwoch realnych zrodel
quote-vs-fill, wiec brak danych ma byc `None` z limitation:

```text
QUOTE_FILL_DIVERGENCE_NOT_MEASURED_IN_L1_STATIC_SIM
```

### 2.7. No-fill

`NO_FILL` wolno zwrocic tylko wtedy, gdy engine ma komplet danych potrzebnych
do symulacji i formula mowi, ze transakcja nie przechodzi.

Przyklad:

```text
fill_status = NO_FILL
no_fill_reason = MIN_OUT_NOT_MET
expected_output_raw = Some(...)
min_out_raw = Some(...)
fill_price = None
pool_state_after = None
```

Brak danych zawsze:

```text
fill_status = BLOCKED_BY_DATA
blocked_reasons = [...]
```

Nie wolno mieszac:

```text
NO_FILL = policzylismy i nie przeszlo
BLOCKED_BY_DATA = nie mozemy policzyc
```

## 3. PR33-PR35: implementacja L1

### PR33 - Core ShadowV2FillEngine, bez runtime burnina

Cel: zbudowac silnik i testy syntetyczne. Bez runtime wiring jako glownego
dowodu.

Zmiany:

- Dodac `shadow_v2_execution.rs`.
- `ShadowV2FillEngine::simulate_buy(input)` uzywa
  `quote_constant_product(..., Buy, ...)`.
- `ShadowV2FillEngine::simulate_sell(input)` uzywa
  `quote_constant_product(..., Sell, ...)`.
- `ShadowEntryFillV2` i `ShadowExitFillV2` dostaja additive optional fields:
  - `execution_simulation_ready`
  - `research_provenance_ready`
  - `execution_label_grade`
  - `provenance_ready`
  - `provenance_blockers`
  - `no_fill_reason`
  - `fail_reason`
  - `blocked_reasons`
  - `expected_output_raw`
  - `slippage_tolerance_bps`
  - `deterministic_price_impact_bps`
  - `realized_slippage_bps`
  - `quote_fill_divergence_bps`
  - `pool_state_after_source`
- Zachowac backward compatibility przez `Option` / `#[serde(default)]`.
- Nie zmieniac BUY/REJECT, Gatekeeper policy, selector runtime, TX/Jito/live
  path.

PR33 acceptance:

```text
BUY FILLED fixture passes
SELL FILLED fixture passes
MIN_OUT fixture returns NO_FILL, not BLOCKED_BY_DATA
missing pool state returns BLOCKED_BY_DATA
missing reserves returns BLOCKED_BY_DATA
missing account_data_hash can still return DIAGNOSTIC_SIM FILLED if execution inputs are complete
missing account_data_hash sets research_provenance_ready=false
same-slot ordering ambiguity returns BLOCKED_BY_DATA
realized_slippage_bps is None in all L1 static sim records
quote_fill_divergence_bps is None in all L1 static sim records
formula source is ghost-core/src/shadow_v2_price.rs
```

### PR34 - Entry boundary source i entry runtime wiring

PR34 nie zaczyna od "uzyj engine, jesli cos jest". PR34 zaczyna od twardego
rozstrzygniecia:

```text
czy runtime ma deterministyczne zrodlo ENTRY_BEFORE?
```

Kandydaci do sprawdzenia w kodzie:

```text
PostBuyRuntime accepted shadow handoff
entry_pool_state_before argument
AccountStateCore / CanonicalPoolState availability at entry boundary
SnapshotEngine materialized/decision state
entry_simulation_rpc_slot / buy_landed_slot
```

Jezeli nie ma realnego zrodla `ENTRY_BEFORE`, PR34 konczy sie bez udawania
postepu:

```text
verdict = BLOCKED_ENTRY_BOUNDARY_SOURCE_MISSING
```

Wtedy nie uruchamiac kolejnego burnina. Nastepna decyzja operatora:

```text
wzbogacic handoff / SnapshotEngine / AccountStateCore boundary capture
albo zaakceptowac, ze entry executable sim nie powstanie
```

Jezeli `ENTRY_BEFORE` istnieje:

- Zbudowac `ShadowV2PoolStateBoundary { boundary_kind = EntryBefore }`.
- Wywolac `ShadowV2FillEngine::simulate_buy`.
- Jesli outcome `FILLED`, emitowac:
  - `shadow_entry_attempt_v2`
  - `pool_state_sample_v2 ENTRY_BEFORE`
  - `shadow_entry_fill_v2 FILLED`
  - `pool_state_sample_v2 ENTRY_AFTER_DERIVED`
- Jesli outcome `NO_FILL`, emitowac no-fill z `no_fill_reason`.
- Jesli outcome `BLOCKED_BY_DATA`, emitowac typed blockers.
- `ENTRY_AFTER_DERIVED` ma miec source:
  ```text
  PoolStateSource::DeterministicDerived
  ```
- Nie wpisywac fake `account_data_hash`.

PR34 acceptance:

```text
entry source audit returns ENTRY_BOUNDARY_SOURCE_PRESENT or BLOCKED_ENTRY_BOUNDARY_SOURCE_MISSING
if present: component test produces DIAGNOSTIC_SIM entry FILLED from synthetic runtime boundary
if missing: no burnin and no fake progress
entry without account_data_hash can be DIAGNOSTIC_SIM but not RESEARCH_CANDIDATE
entry missing reserves remains BLOCKED_BY_DATA
entry no-fill min_out produces NO_FILL with fill_price=None
```

### PR35 - Exit boundary, terminal executable PnL

Exit ma uzywac tego samego engine.

Zmiany:

- W `shadow_v2_exit_fill_from_lifecycle()` zastapic obecne "always blocked
  with pool state" sciezka engine.
- `exit_pool_state_before` z AccountStateCore moze dac `DIAGNOSTIC_SIM`,
  nawet jezeli brakuje `account_data_hash`.
- Jesli token amount raw jest znany i state ma reserves/decimals/order,
  wywolac `ShadowV2FillEngine::simulate_sell`.
- Jesli outcome `FILLED`, emitowac:
  - `shadow_exit_attempt_v2`
  - `pool_state_sample_v2 EXIT_BEFORE`
  - `shadow_exit_fill_v2 FILLED`
  - `pool_state_sample_v2 EXIT_AFTER_DERIVED`
- Jesli outcome `NO_FILL`, emitowac no-fill z reason.
- Jesli outcome `BLOCKED_BY_DATA`, emitowac typed blockers.
- `final_pnl_executable_bps` ustawiac tylko wtedy, gdy ten sam `position_id`
  ma:
  ```text
  entry_fill FILLED
  exit_fill FILLED
  ```
- Jesli exit fill jest blocked/no-fill, terminal executable PnL zostaje
  `None`.

PR35 acceptance:

```text
exit synthetic boundary produces DIAGNOSTIC_SIM exit FILLED
exit without token amount returns BLOCKED_BY_DATA
exit min_out failure returns NO_FILL with fill_price=None
terminal executable PnL appears only when same position_id has entry FILLED and exit FILLED
replay/lifecycle reconciliation remains PASS
derived replay/lifecycle are not canonical input
```

## 4. PR36: validation burnin i offline audits

PR36 mozna uruchomic dopiero po PR33-PR35. Nie wczesniej.

Run type:

```text
validation/fidelity-only
logging_only=true
runtime_approval=false
shadow_close_only_approval=false
active_close_approval=false
no strategy proof
no live-equivalence claim
```

### 4.1. PR36 PASS gates

PR36 PASS wymaga pelnego roundtripu per ten sam `position_id`.

Twarde warunki:

```text
real_shadow_v2_positions > 50
entry_fill_FILLED_count > 0
exit_fill_FILLED_count > 0
complete_executable_roundtrip_positions > 0
terminal_truth_with_final_pnl_executable_bps > 0
```

`complete_executable_roundtrip_positions` oznacza:

```text
same position_id:
  shadow_entry_fill_v2.fill_status = FILLED
  shadow_exit_fill_v2.fill_status = FILLED
  shadow_terminal_truth_v2.final_pnl_executable_bps != null
```

Nie wystarczy, ze entry FILLED i exit FILLED wystapia na roznych pozycjach.

Dodatkowe gates:

```text
runtime post_run_manifest.status = PASS
post-run strict audit = PASS
malformed canonical rows = 0
replay/lifecycle reconciliation PASS
temporal/no-lookahead PASS or BLOCKED only by explicit UNKNOWN chain components
clean shutdown PASS
no SIGTERM
no forced component abort
raw evidence/logs/runtime scopes/local configs not staged
```

### 4.2. PR36 blocked verdicts

Jezeli entry albo exit sa w 100% `BLOCKED_BY_DATA`:

```text
BLOCKED_L1_EXECUTION_INPUTS_STILL_UNAVAILABLE
```

Jezeli nie ma zadnego pelnego roundtripu:

```text
BLOCKED_NO_COMPLETE_EXECUTABLE_ROUNDTRIP
```

Jezeli entry boundary source nie istnieje:

```text
BLOCKED_ENTRY_BOUNDARY_SOURCE_MISSING
```

Jezeli exit boundary source nie istnieje:

```text
BLOCKED_EXIT_BOUNDARY_SOURCE_MISSING
```

Jezeli manifest/shutdown/schema fail:

```text
FAIL_RUNTIME_MANIFEST_OR_SHUTDOWN
```

### 4.3. Offline audit update po PR36

Po PR36 audyty musza raportowac nowe metryki:

```text
diagnostic_sim_filled_count
research_candidate_filled_count
live_confirmed_filled_count
entry_fill_FILLED_count
entry_fill_NO_FILL_count
entry_fill_BLOCKED_BY_DATA_count
exit_fill_FILLED_count
exit_fill_NO_FILL_count
exit_fill_BLOCKED_BY_DATA_count
complete_executable_roundtrip_positions
terminal_executable_pnl_count
no_fill_reason_frequency
blocked_reason_frequency
provenance_blocker_frequency
research_provenance_ready_count
research_provenance_blocked_count
realized_slippage_present_count
quote_fill_divergence_present_count
```

Audyty entry/exit musza rozrozniac:

```text
FILLED_DIAGNOSTIC_SIM
FILLED_RESEARCH_CANDIDATE
NO_FILL_COMPUTED
BLOCKED_BY_DATA
```

Entry audit verdicts:

```text
PASS_ENTRY_EXECUTION_SIM_READY
BLOCKED_ENTRY_BOUNDARY_SOURCE_MISSING
BLOCKED_ENTRY_EXECUTION_INPUTS_UNAVAILABLE
FAIL_ENTRY_SCHEMA_OR_ORDERING_BROKEN
```

Exit audit verdicts:

```text
PASS_EXIT_EXECUTION_SIM_READY
BLOCKED_EXIT_BOUNDARY_SOURCE_MISSING
BLOCKED_EXIT_EXECUTION_INPUTS_UNAVAILABLE
FAIL_EXIT_SCHEMA_OR_ORDERING_BROKEN
```

Roundtrip audit verdicts:

```text
PASS_COMPLETE_EXECUTABLE_ROUNDTRIP_PRESENT
BLOCKED_NO_COMPLETE_EXECUTABLE_ROUNDTRIP
FAIL_TERMINAL_EXECUTABLE_PNL_INCONSISTENT
```

Research provenance audit verdicts:

```text
PASS_RESEARCH_PROVENANCE_READY
BLOCKED_RESEARCH_PROVENANCE_INCOMPLETE
FAIL_PROVENANCE_SCHEMA_OR_CAUSALITY_BROKEN
```

PR36 final verdicts:

```text
PASS_L1_DETERMINISTIC_EXECUTION_SIM_READY
BLOCKED_ENTRY_BOUNDARY_SOURCE_MISSING
BLOCKED_EXIT_BOUNDARY_SOURCE_MISSING
BLOCKED_L1_EXECUTION_INPUTS_STILL_UNAVAILABLE
BLOCKED_NO_COMPLETE_EXECUTABLE_ROUNDTRIP
FAIL_RUNTIME_MANIFEST_OR_SHUTDOWN
FAIL_SCHEMA_ORDERING_OR_LOOKAHEAD
```

PASS nie nadaje automatycznie:

```text
research_grade=true
live_equivalence=true
runtime_approval=true
shadow_close_only_approval=true
active_close_approval=true
strategy_research_unblocked=true
```

Moze co najwyzej zaproponowac:

```text
runtime_approval_candidate = true_for_shadow_v2_logging_validation_only
strategy_research_unblocked_candidate = true_for_offline_execution_sim_reconstruction_review
```

## 5. Testy, komendy, granice

### PR33 tests

```text
cargo test -p ghost-brain shadow_v2_execution_buy_filled_diagnostic_without_hash -- --nocapture
cargo test -p ghost-brain shadow_v2_execution_buy_filled_research_candidate_with_hash -- --nocapture
cargo test -p ghost-brain shadow_v2_execution_sell_filled_diagnostic_without_hash -- --nocapture
cargo test -p ghost-brain shadow_v2_execution_min_out_returns_no_fill_without_fill_price -- --nocapture
cargo test -p ghost-brain shadow_v2_execution_missing_pool_state_blocks -- --nocapture
cargo test -p ghost-brain shadow_v2_execution_same_slot_ambiguity_blocks -- --nocapture
cargo test -p ghost-brain shadow_v2_execution_quote_fill_divergence_is_none_in_l1 -- --nocapture
cargo test -p ghost-brain shadow_v2_execution_realized_slippage_is_none_in_l1 -- --nocapture
```

### PR34 tests

```text
cargo test -p ghost-launcher shadow_v2_entry_boundary_source_is_detected_or_typed_blocked -- --nocapture
cargo test -p ghost-launcher shadow_v2_postbuy_entry_fill_uses_engine_when_boundary_ready -- --nocapture
cargo test -p ghost-launcher shadow_v2_postbuy_entry_fill_blocks_when_boundary_missing -- --nocapture
cargo test -p ghost-launcher shadow_v2_postbuy_entry_fill_diagnostic_sim_without_hash -- --nocapture
```

### PR35 tests

```text
cargo test -p ghost-brain shadow_v2_exit_fill_uses_engine_when_boundary_ready -- --nocapture
cargo test -p ghost-brain shadow_v2_exit_fill_blocks_when_token_amount_missing -- --nocapture
cargo test -p ghost-brain shadow_v2_exit_fill_no_fill_min_out_has_no_fill_price -- --nocapture
cargo test -p ghost-brain shadow_v2_terminal_executable_pnl_requires_same_position_roundtrip -- --nocapture
cargo test -p ghost-brain shadow_v2_replay_lifecycle_reconcile_after_execution_labels -- --nocapture
```

### Required checks per implementation PR

```text
cargo check -p ghost-brain
cargo check -p ghost-launcher
cargo fmt --check
git diff --check
git diff --cached --check
forbidden staged-file guard
```

Jezeli zmienione skrypty Python:

```text
python3 -m py_compile <changed_script.py>
python3 <changed_test.py>
```

### Twarde granice

```text
No BUY/REJECT change
No Gatekeeper policy change
No selector runtime change
No TX/Jito/live path change
No shadow_close_only
No active close
No runtime approval
No research-grade auto-grant
No live-equivalence claim
No R51 touch
No raw JSONL/log/runtime scope/local config staged
No burnin before PR33-PR35 are merged
No PR33 implementation before operator accepts this plan
```

Definition of Done dla L1:

```text
central ShadowV2FillEngine exists
ghost-core/src/shadow_v2_price.rs is canonical formula source
execution_simulation_ready separated from research_provenance_ready
diagnostic deterministic FILLED can exist without account_data_hash
research provenance remains blocked without account_data_hash
NO_FILL means computed no-fill, not missing data
realized_slippage_bps is None in L1
quote_fill_divergence_bps is None in L1
complete executable roundtrip exists in validation
terminal final_pnl_executable_bps exists only for same-position filled entry+filled exit
recorder measures the simulator, not replaces it
```
