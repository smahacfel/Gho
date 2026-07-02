# RAPORT SHADOW V2 PR18 CANONICAL FILL PATH TERMINAL EMISSION 20260702

## 1. Werdykt wykonawczy

Werdykt implementacyjny PR18 / PR17B:

`PR18_IMPLEMENTATION_READY_FOR_REVIEW`

PR18 dodaje side-by-side canonical Shadow V2 emission dla:

- `shadow_entry_attempt_v2`;
- `shadow_entry_fill_v2`;
- `shadow_path_sample_v2`;
- `shadow_exit_attempt_v2`;
- `shadow_exit_fill_v2`;
- `shadow_terminal_truth_v2`.

Implementacja pozostaje logging-only. Nowe rekordy ida przez `ShadowV2ValidationHarness::append_record(...)` i sa zapisywane jako canonical `shadow_position_event_v2.jsonl` plus derived `shadow_replay_v2.jsonl`, `shadow_lifecycle_v2.jsonl` i `shadow_path_density_v2.jsonl`.

To nie jest strategy proof, edge proof, runtime approval ani live-equivalence. Rekordy entry/exit fill sa celowo blokowane jako `BLOCKED_BY_DATA`, jezeli runtime nie ma pelnego `pool_state_sample_v2`, telemetry landing/latency albo quote/fill divergence. Terminal truth jest mark/path-only evidence z legacy shadow lifecycle, a nie potwierdzonym live fill.

## 2. Zakres wykonany

Zmiany kodowe:

- `ghost-launcher/src/components/post_buy_runtime.rs`
  - przekazuje wspolny `ShadowV2ValidationHarness` do post-buy runtime i guardian engine;
  - emituje `shadow_entry_attempt_v2` po accepted shadow handoff;
  - emituje `shadow_entry_fill_v2` jako typed `BLOCKED_BY_DATA`, gdy brakuje causal pool-state/fill evidence;
  - zachowuje `position_id`, `candidate_id`, `pool_id`, `base_mint`, slot/timestamp, source refs i limitations.

- `ghost-brain/src/guardian/post_buy/engine.rs`
  - emituje `shadow_path_sample_v2` z legacy lifecycle mark/path evidence;
  - emituje `shadow_exit_attempt_v2`;
  - emituje `shadow_exit_fill_v2` jako typed `BLOCKED_BY_DATA`, gdy brakuje executable sell pool-state evidence;
  - emituje `shadow_terminal_truth_v2` dla `position_closed`;
  - zachowuje conservative terminal linkage: exit fill link jest ustawiany tylko tam, gdzie PR18 emituje terminalowy blocked exit fill dla `total_exits=0`; przy legacy exit timestamp mismatch risk link zostaje jawnie zablokowany limitation.

- `ghost-brain/src/guardian/post_buy/shadow_v2.rs`
  - dodaje helper `ShadowEntryFillV2::blocked_without_pool_state(...)`;
  - dodaje helper `ShadowExitFillV2::blocked_without_pool_state(...)`;
  - dodaje helper `ShadowPathSampleV2::from_legacy_lifecycle_mark(...)`.

## 3. Zachowane granice

PR18 nie zmienia:

- BUY/REJECT;
- Gatekeeper policy;
- selector runtime;
- TX/Jito/live path;
- `shadow_close_only`;
- active close;
- runtime approval;
- live-equivalence claim;
- R51.

Shadow V2 records nie sa konsumowane przez Gatekeeper, selector, BUY/REJECT ani TX/Jito/live path. Sa dodatkowymi artefaktami logging-only dla walidacji fidelity.

## 4. Entry evidence contract

`shadow_entry_attempt_v2` jest emitowany z accepted shadow handoff w `PostBuyRuntime`.

Zachowane pola korelacji:

- `run_id`;
- `session_id`, gdy dostepny;
- `candidate_id`;
- `position_id`;
- `pool_id`;
- `base_mint`;
- slot z `entry_simulation_rpc_slot` / `buy_landed_slot`, jezeli dostepny;
- timestamp z `entry_opened_at_ms`, jezeli dostepny;
- source refs do `post_buy_runtime:accepted_shadow_handoff` i `post_buy_submitted:shadow_simulation`.

`shadow_entry_fill_v2` jest emitowany jako:

`fill_status=BLOCKED_BY_DATA`

z powodami m.in.:

- `ENTRY_FILL_POOL_STATE_SAMPLE_MISSING`;
- `ENTRY_FILL_NOT_EXECUTABLE_WITHOUT_POOL_STATE_PROVENANCE`;
- `ENTRY_FILL_STATIC_MODEL_NOT_LIVE_CONFIRMED`;
- `ENTRY_FILL_DERIVED_FROM_SHADOW_SIMULATION_HANDOFF`;
- `ENTRY_FILL_LATENCY_AND_LANDING_TELEMETRY_NOT_MEASURED`;
- `ENTRY_FILL_QUOTE_FILL_DIVERGENCE_NOT_MEASURED`.

Wniosek: PR18 udostepnia canonical V2 entry evidence, ale nie udowadnia live executable entry fill.

## 5. Exit/path/terminal evidence contract

`shadow_path_sample_v2` jest emitowany z `shadow_lifecycle` jako mark/path evidence.

Ograniczenia sa jawne:

- `LEGACY_LIFECYCLE_PRICE_TRUTH_NOT_POOL_STATE_SAMPLE`;
- `PATH_SAMPLE_POOL_STATE_PROVENANCE_MISSING`;
- `MARK_PRICE_REPLAY_NOT_EXECUTABLE_FILL`.

`shadow_exit_attempt_v2` jest emitowany z lifecycle trigger evidence i zawiera:

- trigger timestamp z lifecycle;
- target/stop/timeout context, jezeli dostepny;
- tie-break policy `BLOCK_AMBIGUOUS`;
- source ref do konkretnego lifecycle record type.

`shadow_exit_fill_v2` jest emitowany jako:

`fill_status=BLOCKED_BY_DATA`

z powodami m.in.:

- `EXIT_FILL_POOL_STATE_SAMPLE_MISSING`;
- `EXIT_FILL_NOT_EXECUTABLE_WITHOUT_POOL_STATE_PROVENANCE`;
- `EXIT_FILL_STATIC_MODEL_NOT_LIVE_CONFIRMED`;
- `STATIC_EXIT_FILL_DOES_NOT_ENABLE_ACTIVE_CLOSE`;
- `EXIT_FILL_DERIVED_FROM_LEGACY_LIFECYCLE_EVIDENCE`;
- `EXIT_FILL_QUOTE_FILL_DIVERGENCE_NOT_MEASURED`.

`shadow_terminal_truth_v2` jest emitowany dla `position_closed` jako:

- `simulation_level=MARK_ONLY`;
- `measurement_grade=MARK_PRICE_REPLAY`;
- `final_pnl_mark_bps` z lifecycle, jezeli dostepny;
- `final_pnl_executable_bps=None`;
- `terminal_source=shadow_lifecycle.position_closed`;
- `reconciliation_status=TERMINAL_TRUTH_FROM_LEGACY_LIFECYCLE_MARK_ONLY`.

Wniosek: PR18 tworzy canonical V2 terminal truth jako mark/path terminal evidence, ale nie tworzy live executable exit proof.

## 6. Temporal/no-lookahead metadata

Nowe rekordy uzywaja explicit clock-domain metadata:

- entry attempt: `SubmitTsMs`;
- entry fill blocked record: `LandingTsMs`;
- path sample: `StreamObservedMs`;
- exit attempt: `StreamObservedMs`;
- exit fill blocked record: `LandingTsMs`;
- terminal truth: `StreamObservedMs`.

`event_order_key` zawiera:

- slot jako `KNOWN` albo explicit `UNKNOWN`;
- signature jako `KNOWN` albo explicit `UNKNOWN`;
- incomplete chain-order components jako explicit `UNKNOWN`;
- `event_seq_in_process`;
- `observed_at_wall_ms`.

Braki orderingu sa przenoszone do limitations/ambiguity labels i nie sa cicho traktowane jako exact chain order.

## 7. Testy i walidacja

Uruchomione testy:

- `cargo check -p ghost-brain -q` - PASS;
- `cargo check -p ghost-launcher -q` - PASS;
- `cargo test -p ghost-brain shadow_v2_lifecycle_close_emits_path_exit_terminal_records -- --nocapture` - PASS;
- `cargo test -p ghost-launcher shadow_v2_entry_evidence_writes_attempt_and_blocked_fill -- --nocapture` - PASS;
- `cargo test -p ghost-launcher shadow_v2_validation_smoke_marker_writes_required_artifacts_without_handoff -- --nocapture` - PASS;
- `cargo fmt --check` - PASS.

Uwaga: workspace nadal emituje duza liczbe istniejacych ostrzezen kompilatora. W ramach PR18 nie byly one czyszczone ani interpretowane jako acceptance gate.

## 8. Co zostaje nieudowodnione

PR18 nie dowodzi:

- executable entry fill;
- executable exit fill;
- entry slippage;
- exit slippage;
- own buy impact;
- own sell impact;
- actual landing latency;
- failed/no-fill live behavior;
- quote/fill divergence;
- live-equivalence;
- research-grade reconstruction coverage.

Te pola pozostaja blocked/limitations, dopoki kolejny validation burnin i audyt V2-only nie potwierdza danych.

## 9. Nastepny krok

Po review i merge PR18 nalezy uruchomic osobny validation/fidelity burnin. Wymagania dla kolejnego burnina:

- `shadow_entry_attempt_v2` rows > 0;
- `shadow_entry_fill_v2` rows > 0 albo typed missing reason;
- `shadow_path_sample_v2` rows > 0;
- `shadow_exit_attempt_v2` rows > 0;
- `shadow_exit_fill_v2` rows > 0 albo typed missing reason;
- `shadow_terminal_truth_v2` rows > 0;
- density evaluable where coverage exists;
- replay/lifecycle terminal reconciliation;
- post-run strict audit PASS;
- clean shutdown proven.

Do czasu tego burnina:

- `research_grade=NOT_GRANTED`;
- `live_equivalence=NOT_GRANTED`;
- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- PR17 fidelity validation pozostaje zablokowany jako proof.
