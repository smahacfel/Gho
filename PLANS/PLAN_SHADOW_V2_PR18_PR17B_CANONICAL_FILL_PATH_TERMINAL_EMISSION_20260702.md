# Plan Shadow V2 PR18 / PR17B Canonical Fill Path Terminal Emission 2026-07-02

## 1. Cel

PR18 / PR17B ma domknac lukę ujawniona przez PR17-r2:

`PASS_REAL_SHADOW_V2_POSITION_EVIDENCE_PRESENT_WITH_LIMITATIONS`

PR17-r2 potwierdzil realny accepted shadow handoff i canonical `shadow_position_event_v2`, ale entry/exit/path/terminal evidence pozostalo poza pelnym canonical V2 contract:

- entry evidence bylo w legacy `shadow_entries.jsonl` i `buys.jsonl`;
- exit evidence bylo w legacy `shadow_lifecycle.jsonl`;
- canonical V2 mial tylko `POSITION_CREATED`;
- V2 path density bylo `NOT_EVALUABLE_NO_COVERAGE`;
- V2 replay/lifecycle byly derived open-only, bez terminal truth.

Cel PR18 / PR17B:

W logging-only Shadow V2 validation mode emitowac canonical V2 records:

- `shadow_entry_attempt_v2`;
- `shadow_entry_fill_v2`;
- `shadow_path_sample_v2`;
- `shadow_exit_attempt_v2`;
- `shadow_exit_fill_v2`;
- `shadow_terminal_truth_v2`.

Plan verdict:

`PR18_PR17B_PLAN_READY_FOR_IMPLEMENTATION`

To jest plan implementacyjny. Nie uruchamia validation burnin i nie przyznaje zadnego approval.

## 2. Granice

In scope:

- side-by-side canonical V2 emission w `PostBuyRuntime` / Shadow V2 harness;
- korelacja z realnym `position_id`, `candidate_id`, `pool_id`, `base_mint`, slotami, timestampami i source refs;
- no-lookahead metadata dla kazdego nowego rekordu;
- typed missing reasons, gdy dane sa niepelne;
- testy jednostkowe/komponentowe dla emisji i derived replay/lifecycle;
- aktualizacja manifest/gate rows, jezeli schema audit wymaga jawnych expectations.

Out of scope:

- BUY/REJECT change;
- Gatekeeper policy change;
- selector runtime change;
- TX/Jito/live path change;
- `shadow_close_only`;
- active close;
- runtime approval;
- live-equivalence claim;
- strategy proof;
- edge proof;
- RCE proof;
- R51;
- stage raw JSONL/log/runtime artifacts.

## 3. Aktualny kod i luka

Istniejace typy i helpery sa w:

`ghost-brain/src/guardian/post_buy/shadow_v2.rs`

Istnieja juz:

- `ShadowV2Record`;
- `PoolStateSampleV2`;
- `ShadowEntryDecisionV2`;
- `ShadowEntryAttemptV2`;
- `ShadowEntryFillV2`;
- `ShadowPathSampleV2`;
- `ShadowExitAttemptV2`;
- `ShadowExitFillV2`;
- `ShadowTerminalTruthV2`;
- `ShadowReplayV2`;
- `ShadowLifecycleV2`;
- `ShadowV2ValidationHarness`.

Istniejace helpery do wykorzystania:

- `ShadowEntryFillV2::from_static_buy_model(...)`;
- `ShadowExitFillV2::from_static_sell_model(...)`;
- `ShadowPathSampleV2::from_pool_state_mark(...)`;
- `ShadowExitAttemptV2::from_mark_path_trigger(...)`;
- `ShadowReplayV2::derive_from_canonical_stream(...)`;
- `ShadowLifecycleV2::derive_from_canonical_stream(...)`.

Aktualny runtime adapter w:

`ghost-launcher/src/components/post_buy_runtime.rs`

emituje tylko:

- smoke marker;
- `ShadowPositionV2` dla accepted shadow handoff.

Najwazniejszy obecny helper:

`maybe_emit_shadow_v2_position_created(...)`

dodaje limitations:

- `PR15_MINIMAL_POSITION_CREATED_ONLY`;
- `NO_ENTRY_FILL_EXIT_FILL_OR_PATH_INFERENCE_IN_PR15`;
- `SHADOW_V2_RECORD_NOT_CONSUMED_BY_DECISIONS`.

PR18 / PR17B ma usunac te ograniczenia dla nowych runow przez emisje pelnych V2 eventow, ale nadal bez konsumpcji tych eventow przez decyzje runtime.

## 4. Architektura docelowa PR18 / PR17B

### 4.1 Single canonical event stream

Wszystkie nowe records ida przez:

`ShadowV2ValidationHarness::append_record(ShadowV2Record::...)`

Nie wolno pisac rownoleglego V2 truth file poza harness.

Kazdy event musi miec:

- ten sam `run_id`;
- ten sam `position_id`;
- ten sam `candidate_id`;
- ten sam `pool_id`;
- ten sam `base_mint`;
- source refs do runtime eventu / legacy evidence / pool state;
- event id stabilny i deterministic.

### 4.2 Minimalny event id contract

Proponowany schemat:

```text
shadow_v2_entry_attempt:{position_id}:{decision_ts_ms}
shadow_v2_entry_fill:{position_id}:{sim_finished_ts_ms}
shadow_v2_path_sample:{position_id}:{sample_ts_ms}:{source_seq}
shadow_v2_exit_attempt:{position_id}:{trigger_ts_ms}:{trigger}
shadow_v2_exit_fill:{position_id}:{timestamp_ms}
shadow_v2_terminal_truth:{position_id}:{timestamp_ms}:{close_reason}
```

Jesli timestamp jest niedostepny:

- nie wolno losowac;
- uzyc produced_at_ms tylko z limitation `EVENT_TIME_MISSING_USED_PRODUCED_AT_MS`;
- event musi miec typed blocker.

### 4.3 Event order key

Kazdy rekord z `event_order_key` musi miec jawne komponenty:

- slot;
- block_time;
- signature;
- transaction_index_or_unknown;
- instruction_index_or_unknown;
- inner_instruction_index_or_unknown;
- log_index_or_unknown;
- event_seq_in_process;
- observed_at_wall_ms.

Brak danych chain-order nie moze byc pusty. Musi byc explicit `UNKNOWN`, a rekord musi dostac ambiguity limitation:

- `EVENT_ORDER_SIGNATURE_UNKNOWN`;
- `EVENT_ORDER_TX_INDEX_UNKNOWN`;
- `EVENT_ORDER_INSTRUCTION_INDEX_UNKNOWN`;
- `EVENT_ORDER_SAME_SLOT_INCOMPLETE_ORDERING`.

Slot `UNKNOWN` blokuje research-ready fill/path. Non-slot `UNKNOWN` moze przejsc tylko jako ambiguous/non-exact evidence i nie moze rozstrzygac target/stop bez tie-break policy.

## 5. Entry emission

### 5.1 ShadowEntryAttemptV2

Emitowac przy accepted shadow handoff / shadow simulation start.

Zrodla:

- `candidate_id`;
- `position_id`;
- `pool_amm_id`;
- `base_mint`;
- `decision_ts_ms`;
- `entry_simulation_rpc_slot`;
- `sim_started_ts_ms`;
- `sim_finished_ts_ms`;
- prepared quote/min_out, jesli dostepne;
- entry price z runtime shadow entry evidence, jesli dostepne.

Wymagane pola:

- `intended_entry_ts_ms`: clock domain `DECISION_TS_MS` albo `WALL_CLOCK_MS` z causal boundary;
- `intended_entry_slot`: decision/entry slot jezeli znany;
- `decision_mark_price`: jezeli znany;
- `entry_quote_price`: jezeli znany;
- `entry_quote_tokens_out`: jezeli znany;
- `entry_quote_min_out`: jezeli znany;
- `simulated_submit_ts_ms`: `sim_started_ts_ms`;
- `simulated_landing_slot`: synthetic/observed slot z explicit source;
- `entry_failure_mode`: `None` dla simulation success, typed reason dla failure.

Limitations przy brakach:

- `ENTRY_ATTEMPT_DECISION_TS_MISSING`;
- `ENTRY_ATTEMPT_SLOT_UNKNOWN`;
- `ENTRY_ATTEMPT_QUOTE_PRICE_MISSING`;
- `ENTRY_ATTEMPT_MIN_OUT_MISSING`;
- `ENTRY_ATTEMPT_LANDING_SLOT_SYNTHETIC`.

### 5.2 PoolStateSampleV2 for entry

PR18 powinien emitowac `PoolStateSampleV2` przed `ShadowEntryFillV2`, jezeli mozna zbudowac sample z realnego state source.

Source priority:

1. AccountStateCore canonical account state;
2. entry simulation RPC context, jezeli AccountStateCore unavailable;
3. legacy shadow entry price only jako `BLOCKED_BY_DATA_PRICE_ONLY_NO_RESERVES`.

Jesli brakuje reserves/account hash/staleness:

- emitowac `PoolStateSampleV2` z typed blockers tylko gdy schema pozwala;
- albo emitowac `ShadowEntryFillV2` jako `BLOCKED_BY_DATA` z source ref do legacy evidence;
- nie fabricowac reserves.

### 5.3 ShadowEntryFillV2

Uzywac:

`ShadowEntryFillV2::from_static_buy_model(...)`

tylko gdy dostepny jest research-ready `PoolStateSampleV2`.

Jesli pool state/reserves sa niepelne:

- emitowac `ShadowEntryFillV2` z `fill_status=BLOCKED_BY_DATA`;
- typed blockers:
  - `ENTRY_FILL_POOL_STATE_SAMPLE_MISSING`;
  - `ENTRY_FILL_RESERVE_PROVENANCE_MISSING`;
  - `ENTRY_FILL_ACCOUNT_DATA_HASH_MISSING`;
  - `ENTRY_FILL_STALENESS_UNKNOWN`;
  - `ENTRY_FILL_EVENT_ORDER_INCOMPLETE`.

W PR18 nie wolno udawac live fill:

- `simulation_level=FILL_MODEL_STATIC` albo `MARK_ONLY`;
- `measurement_grade=RESEARCH_GRADE_CANDIDATE` tylko przy pelnych inputach;
- inaczej `DIAGNOSTIC_ONLY` albo `BLOCKED_BY_DATA`;
- limitations zawsze musza zawierac `NOT_LIVE_CONFIRMED`.

## 6. Path emission

### 6.1 ShadowPathSampleV2

Emitowac path samples podczas lifecycle monitoring tickow albo przy zmianach state/price.

Minimalny PR18 target:

- co najmniej 1 path sample dla realnej pozycji, jezeli monitoring ma price state;
- typed no-coverage reason, jezeli monitoring nie ma price state.

Zrodla:

- AccountStateCore / canonical account snapshot;
- PostBuyGuardian price sample;
- legacy lifecycle sample fields, jesli zawieraja sample timestamp/slot/price.

Sampling reasons:

- `EVENT_SAMPLE`;
- `HEARTBEAT`;
- `LEVEL_HIT`;
- `TERMINAL`;
- `TIME_STOP_SAMPLE`;
- `BLOCKED_NO_PRICE_STATE`.

Kazdy sample musi:

- wskazywac `pool_state_ref`;
- miec `sample_ts_ms`;
- miec `age_ms`;
- miec `sample_slot` lub explicit `UNKNOWN`;
- miec `mark_price` albo typed reason `PATH_SAMPLE_MARK_PRICE_MISSING`;
- miec `pnl_mark_bps` jezeli entry price znany;
- nie emitowac executable PnL bez exit quote model.

### 6.2 Density implications

Po emisji path samples `shadow_path_density_v2.jsonl` musi przejsc z samego:

`NOT_EVALUABLE_NO_COVERAGE`

do evaluable tam, gdzie coverage istnieje:

- `EVALUABLE_APPROX` albo `SPARSE_APPROX_ONLY` dla pokrytych horyzontow;
- `NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY` dla horyzontow poza runtime;
- `NOT_EVALUABLE_NO_COVERAGE` tylko gdy path sample count = 0.

## 7. Exit emission

### 7.1 ShadowExitAttemptV2

Emitowac przy triggerze:

- target;
- stop;
- time stop;
- manual/forced close;
- blocked/no-price close.

W PR17-r2 realny close byl:

- close_reason: `TimeStop`;
- duration_ms: `30375`;
- truth_source: `canonical_account_state_snapshot`;
- truth_status: `resolved`.

PR18 mapping:

- `exit_trigger=TIME_STOP`;
- `trigger_ts_ms=exit_reason_evaluation_ts_ms` albo lifecycle `timestamp_ms`;
- `trigger_slot=exit_sample_slot`;
- `trigger_source=canonical_account_state_snapshot` albo typed missing reason;
- `max_hold_ms` z runtime config;
- `same_slot_ambiguity` z event_order completeness.

### 7.2 ShadowExitFillV2

Uzywac:

`ShadowExitFillV2::from_static_sell_model(...)`

tylko gdy jest:

- token amount;
- pool state sample before exit;
- reserves;
- fee/slippage assumptions.

Jesli nie ma pelnych danych:

- emitowac `ShadowExitFillV2` jako `BLOCKED_BY_DATA`;
- typed blockers:
  - `EXIT_FILL_POOL_STATE_SAMPLE_MISSING`;
  - `EXIT_FILL_TOKEN_AMOUNT_MISSING`;
  - `EXIT_FILL_RESERVE_PROVENANCE_MISSING`;
  - `EXIT_FILL_EVENT_ORDER_INCOMPLETE`;
  - `EXIT_FILL_SAMPLE_SLOT_PRECEDES_ENTRY_LANDING_SLOT`;
  - `EXIT_FILL_SYNTHETIC_LANDING_SLOT`.

Jesli exit ma tylko legacy lifecycle price/value:

- zapisac source ref do legacy lifecycle row;
- measurement grade pozostaje `DIAGNOSTIC_ONLY` albo `MARK_PRICE_REPLAY`;
- nie oznaczac jako executable sell fill.

## 8. Terminal truth

Emitowac `ShadowTerminalTruthV2` po terminalnym lifecycle close.

Wymagania:

- dokladnie jeden terminal truth per `position_id`;
- linked_entry_fill = event id entry fill albo `None` z blocker;
- linked_exit_fill = event id exit fill albo `None` z blocker;
- terminal_reason mapped do `TerminalReasonV2`;
- terminal_ts_ms;
- terminal_slot;
- close_age_ms;
- final_pnl_mark_bps;
- final_pnl_executable_bps tylko gdy executable fill model complete;
- reconciliation_status.

Typed statuses:

- `TERMINAL_TRUTH_COMPLETE`;
- `TERMINAL_TRUTH_BLOCKED_MISSING_ENTRY_FILL`;
- `TERMINAL_TRUTH_BLOCKED_MISSING_EXIT_FILL`;
- `TERMINAL_TRUTH_BLOCKED_MISSING_PATH`;
- `TERMINAL_TRUTH_LEGACY_LIFECYCLE_DERIVED_ONLY`.

Duplicate handling:

- `exit_filled` i `position_closed` w legacy lifecycle nie moga tworzyc dwoch terminal truth records;
- `exit_filled` jest sub-event/source ref;
- `position_closed` tworzy terminal truth;
- canonical writer ma zachowac invariant one-terminal-per-position.

## 9. Replay/lifecycle expected result

Po PR18/PR17B derived `shadow_replay_v2` dla realnej pozycji musi zawierac:

- `entry_fill_event_id`;
- `exit_attempt_event_id`;
- `exit_fill_event_id`;
- `terminal_truth_event_id`;
- `mark_path_sample_count > 0`, jezeli path samples istnieja;
- `terminal_reason`;
- `terminal_pnl_mark_bps`;
- `close_age_ms`;
- `replay_derivation_status` bez open-only blockera.

Derived `shadow_lifecycle_v2` musi:

- miec terminal view;
- wskazywac canonical terminal event id;
- nie tworzyc osobnego competing truth.

## 10. Implementation PR structure

### PR18A - runtime adapter helpers

Files likely touched:

- `ghost-launcher/src/components/post_buy_runtime.rs`;
- targeted tests in same module.

Work:

- add helper for common Shadow V2 envelope from handoff;
- add helper for event order key with explicit UNKNOWN fields;
- add append outcome logging helper.

Risk:

- low/medium, logging-only path only.

### PR18B - entry attempt/fill emission

Files likely touched:

- `ghost-launcher/src/components/post_buy_runtime.rs`;
- possibly `ghost-brain/src/guardian/post_buy/shadow_v2.rs` only if blocked constructors are missing.

Work:

- emit `ShadowEntryAttemptV2`;
- emit `PoolStateSampleV2` for entry if state available;
- emit `ShadowEntryFillV2` filled or blocked.

Tests:

- simulated handoff with complete pool state emits entry attempt + fill;
- missing pool state emits blocked entry fill;
- no-decision-consumption guard remains green.

### PR18C - path sample emission

Files likely touched:

- `ghost-launcher/src/components/post_buy_runtime.rs`;
- maybe monitoring tick adapter.

Work:

- emit `ShadowPathSampleV2` on price samples/ticks;
- emit terminal path sample on close;
- keep density rows derived from real path samples.

Tests:

- path sample count > 0 for fixture position;
- missing price state gives typed `PATH_SAMPLE_MARK_PRICE_MISSING`;
- density no longer reports all `NOT_EVALUABLE_NO_COVERAGE` when samples exist.

### PR18D - exit attempt/fill/terminal truth

Files likely touched:

- `ghost-launcher/src/components/post_buy_runtime.rs`;
- targeted tests.

Work:

- emit `ShadowExitAttemptV2`;
- emit `ShadowExitFillV2` filled or blocked;
- emit one `ShadowTerminalTruthV2` per position;
- deduplicate legacy `exit_filled` + `position_closed`.

Tests:

- time stop produces one exit attempt, one exit fill, one terminal truth;
- duplicate terminal attempts fail/are ignored with typed status;
- replay/lifecycle derived rows include terminal ids.

### PR18E - manifest/audit/report updates

Files likely touched:

- `reports/selector/shadow_v2_required_schema_manifest.csv`;
- `reports/selector/shadow_v2_acceptance_gates.csv`;
- docs/ADR;
- optional validation summary script if existing audit needs extension.

Work:

- add explicit gates for V2-only entry/exit/path/terminal evidence;
- update docs to require post-implementation validation burnin.

## 11. Acceptance gates for PR18 implementation

Code/static gates:

- no BUY/REJECT diff;
- no Gatekeeper policy diff;
- no selector runtime diff;
- no TX/Jito/live path diff;
- no `shadow_close_only`;
- no active close;
- no approval flags changed;
- no raw JSONL/log/runtime artifacts staged.

Unit/component gates:

- `cargo test -p ghost-brain shadow_v2`;
- targeted `post_buy_runtime` Shadow V2 tests;
- `cargo fmt --check`;
- `git diff --check`;
- forbidden staged-file guard.

Functional gates before post-implementation burnin:

- disabled config still does not initialize harness;
- enabled logging-only config emits only artifacts, not decisions;
- no Python in hot path;
- no Shadow V2 decision consumption.

Validation burnin gates after implementation:

- `shadow_position_event_v2` real positions > 0;
- `shadow_entry_attempt_v2` rows > 0;
- `shadow_entry_fill_v2` rows > 0 or typed blocked rows;
- `shadow_path_sample_v2` rows > 0;
- `shadow_exit_attempt_v2` rows > 0;
- `shadow_exit_fill_v2` rows > 0 or typed blocked rows;
- `shadow_terminal_truth_v2` rows > 0;
- density rows evaluable where coverage exists;
- replay/lifecycle terminal reconciliation present;
- post_run_manifest `PASS`;
- strict audit `PASS`;
- clean shutdown proven.

## 12. Required validation run after PR18

Po implementation PR nie wolno od razu robic strategy proof.

Nastepny run ma byc:

`Shadow V2 V2-only fidelity validation burnin`

Wymagania:

- validation/fidelity-only;
- relaxed validation sampling allowed tylko z labelami;
- no runtime approval;
- no live-equivalence claim;
- no raw evidence commit.

Required outputs:

- report-only PR;
- summary CSV;
- ADR;
- V2-only evidence matrix:
  - entry fill evidence;
  - exit fill evidence;
  - terminal truth;
  - path samples;
  - density coverage;
  - replay/lifecycle reconciliation.

## 13. Non-goals

PR18 / PR17B nie ma:

- poprawiac strategii;
- stroic progow;
- udowadniac edge;
- zmieniac live path;
- twierdzic live-equivalence;
- rozwiazywac PR14 live-confirmed calibration;
- cleanupowac stare artefakty;
- zmieniac R51.

## 14. Definition of Done

PR18 / PR17B jest gotowy do merge tylko gdy:

- pelne V2 record types sa emitowane side-by-side w logging-only harness;
- kazdy record ma correlation id i no-lookahead metadata;
- missing fields sa typed, nie milczace;
- replay/lifecycle derived rows widza terminal truth;
- tests potwierdzaja happy path i blocked-by-data path;
- staged files nie zawieraja raw JSONL/log/runtime artifacts;
- docs/ADR jasno mowia, ze runtime approval i live-equivalence nadal sa false.
