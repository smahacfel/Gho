# RAPORT SHADOW V2 FIDELITY VALIDATION BURNIN PR17 R2 RELAXED SAMPLING 20260701

## 1. Werdykt wykonawczy

Werdykt PR17-r2:

`PASS_REAL_SHADOW_V2_POSITION_EVIDENCE_PRESENT_WITH_LIMITATIONS`

Glowny warunek sukcesu zostal spelniony:

`real_shadow_v2_positions > 0`

W runie `shadow-burnin-v2-fidelity-validation-pr17-r2-relaxed-sampling` powstala jedna realna pozycja Shadow V2 ponad diagnostycznym markerem:

- real_shadow_v2_positions: `1`;
- diagnostic_marker_positions: `1`;
- `shadow_position_event_v2.jsonl` rows: `2`;
- `shadow_replay_v2.jsonl` rows: `2`;
- `shadow_lifecycle_v2.jsonl` rows: `2`;
- `shadow_path_density_v2.jsonl` rows: `14`;
- `post_run_manifest.status=PASS`;
- post-run strict audit: `PASS`;
- clean shutdown: `PASS`;
- `SIGTERM=false`;
- reconnect/disconnect flood: `false`.

To nadal nie jest research-grade, strategy proof, edge proof, runtime approval ani live-equivalence. Profil byl celowo samplingowy:

`RELAXED_VALIDATION_SAMPLING_PROFILE`

i musi byc traktowany jako:

- `NOT_PRODUCTION_POLICY`;
- `NOT_STRATEGY_EVIDENCE`;
- `NOT_EDGE_PROOF`;
- `NOT_RUNTIME_APPROVAL`;
- `NOT_LIVE_EQUIVALENT`.

Najwazniejsze ograniczenie: realna pozycja istnieje w canonical Shadow V2 jako `shadow_position_v2`, ale PR15/PR16 harness nadal materializuje tylko minimalny event `POSITION_CREATED`. Entry/exit evidence istnieje w lokalnych legacy shadow logs, nie jako pelne canonical V2 `shadow_entry_fill_v2`, `shadow_exit_fill_v2` ani `shadow_path_sample_v2`.

## 2. Zakres i granice

Zakres wykonany:

- uruchomiono osobny lokalny run:
  `shadow-burnin-v2-fidelity-validation-pr17-r2-relaxed-sampling`;
- uzyto osobnego scope:
  `reports/selector/shadow-v2-fidelity-validation-pr17-r2-relaxed-sampling`;
- zmieniono wylacznie lokalne, niestage'owane configi walidacyjne;
- nie zmieniono kodu runtime;
- nie zmieniono produkcyjnych rolloutow.

Granice nienaruszone:

- validation/fidelity-only;
- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- brak strategy proof;
- brak RCE proof;
- brak selector/edge proof;
- brak live-equivalence claim;
- brak zmian BUY/REJECT runtime code;
- brak zmian Gatekeeper policy code;
- brak zmian selector runtime;
- brak zmian TX/Jito/live path;
- brak R51;
- raw JSONL/log/runtime artifacts nie sa commitowane.

## 3. Lokalny relaxed validation config

Baseline main:

`286a1f76f5c5fd632800f60afa6b4be98066eec7`

Lokalne, niestage'owane configi:

- `configs/rollout/shadow-v2-fidelity-validation-pr17-r2-relaxed-sampling.local.toml`;
- `configs/rollout/ghost_brain_shadow_v2_fidelity_validation_pr17_r2_relaxed_sampling.local.toml`.

Launcher preflight potwierdzil:

- execution mode: `Shadow`;
- entry mode: `shadow_only`;
- Ghost Brain config:
  `/root/Gho/configs/rollout/ghost_brain_shadow_v2_fidelity_validation_pr17_r2_relaxed_sampling.local.toml`;
- `min_tx=10`;
- `min_unique=6`;
- `min_buy=5`;
- `max_wait_ms=10000`;
- Spectrum RPC: `PASS`;
- NLN gRPC app probe: `PASS`.

Samplingowe progi ustawione lokalnie:

| Field | Value |
|---|---:|
| `min_tx_count` | `10` |
| `min_unique_signers` | `6` |
| `min_buy_count` | `5` |
| `max_wait_time_ms` | `10000` |
| `min_interval_cv` | `0.0` |
| `max_burst_ratio` | `9999.0` |
| `min_avg_interval_ms` | `0.0` |
| `max_avg_interval_ms` | `1200.0` |
| `min_timing_entropy` | `0.0` |
| `min_dust_filtered_count` | `0` |
| `max_interval_cv` | `9.3` |
| `max_timing_entropy` | `9999.0` |
| `max_dev_volume_ratio` | `0.99` |
| `min_bonding_progress_pct` | `10.0` |
| `min_market_cap_sol` | `10.0` |

Dodatkowo, jako samplingowe obnizenie oczywistych blokerow walidacyjnych:

- `min_total_volume_sol=0.0`;
- `min_consecutive_buys=1`;
- `max_bonding_progress_pct=100.0`;
- prosperity market-cap floors obnizone do `10.0`.

Te wartosci nie sa rekomendacja strategii ani produkcji. Ich jedyny cel to wygenerowanie realnego evidence dla Shadow V2 fidelity harness.

## 4. Pre-run gates

Pre-run manifest generation:

- status: `PASS`;
- blockers: `[]`;
- run_id: `shadow-burnin-v2-fidelity-validation-pr17-r2-relaxed-sampling`;
- created_at: `2026-07-01T23:49:33+00:00`.

Pre-run strict manifest audit:

- status: `PASS`;
- blockers: `[]`.

Validation burnin plan audit:

- command: `python3 scripts/shadow_v2_validation_burnin_plan_audit.py --strict`;
- status: `PASS`;
- `validation_mode=FIDELITY_ONLY`;
- `runtime_approval=false`;
- `strategy_proof_enabled=false`.

Legacy downgrade audit:

- command: `python3 scripts/shadow_v2_legacy_downgrade_audit.py --strict`;
- status: `PASS`;
- `v1_live_equivalent_allowed=false`.

Build/preflight:

- `cargo build -p ghost-launcher --bin ghost-launcher --release`: `PASS`;
- launcher `--preflight`: `PASS`.

## 5. Runtime evidence

Run start:

`2026-07-01T23:50:13.944Z`

Shutdown signal:

`2026-07-01T23:51:49.772Z`

Final shutdown:

`2026-07-01T23:52:12.684Z`

Runtime evidence:

- `All components started successfully`: `1`;
- `NewPoolDetected`: observed in runtime stream;
- NLN gRPC primary ingest: `PASS`;
- Program Streams errors: `0`;
- `Transport channel disconnected`: `0`;
- `NLN Subscribe request failed`: `0`;
- `status: 502`: `0`;
- `Bad Gateway`: `0`;
- `SIGTERM`: `0`;
- process exit code: `0`.

Decision evidence:

- gatekeeper decision rows: `114`;
- malformed decision rows: `0`;
- `decision_verdict_buy=true`: `1`;
- decision planes:
  - `legacy_live`: `74`;
  - `v25_shadow`: `40`;
- main BUY row:
  - pool: `2ttPHhPR7rF98eug3F9rjaNR96fsDU2AEArGK5q78Une`;
  - base mint: `HhCYSPYhNWmYkzpcjdWnVoDfgPbuHGVGSyomeei4pump`;
  - decision id: `2ttPHhPR7rF98eug3F9rjaNR96fsDU2AEArGK5q78Une:1782949891366:1782949893366:BUY`;
  - IWIM result: `OK`;
  - n_tx: `30/30`;
  - final verdict: `BUY`.

## 6. Shadow V2 canonical evidence

Post-run strict audit:

- status: `PASS`;
- blockers: `[]`;
- artifact_count strict scan: `7`;
- schema coverage:
  - `shadow_position_event_v2`: `2`;
  - `shadow_replay_v2`: `2`;
  - `shadow_lifecycle_v2`: `2`;
  - `shadow_path_density_v2`: `14`.

Written post-run manifest:

- `post_run_manifest.status=PASS`;
- blockers: `[]`;
- artifact_count in written manifest: `6`;
- total_size_bytes: `36864`.

Canonical Shadow V2 rows:

| Type | Count | Notes |
|---|---:|---|
| diagnostic marker | 1 | `VALIDATION_SMOKE_MARKER` |
| real accepted shadow handoff | 1 | `VALIDATION_HARNESS_POSITION_CREATED` |

Real Shadow V2 position:

- candidate_id:
  `HhCYSPYhNWmYkzpcjdWnVoDfgPbuHGVGSyomeei4pump_2ttPHhPR7rF98eug3F9rjaNR96fsDU2AEArGK5q78Une_1782949902044`;
- position_id:
  `2ttPHhPR7rF98eug3F9rjaNR96fsDU2AEArGK5q78Une:HhCYSPYhNWmYkzpcjdWnVoDfgPbuHGVGSyomeei4pump:1782949910476`;
- pool_id:
  `2ttPHhPR7rF98eug3F9rjaNR96fsDU2AEArGK5q78Une`;
- base_mint:
  `HhCYSPYhNWmYkzpcjdWnVoDfgPbuHGVGSyomeei4pump`;
- produced_at_slot: `430194027`;
- temporal_class: `POST_ENTRY`;
- source_refs: `post_buy_runtime:accepted_shadow_handoff`;
- quality: `VALIDATION_HARNESS_POSITION_CREATED`;
- limitations:
  - `PR15_MINIMAL_POSITION_CREATED_ONLY`;
  - `NO_ENTRY_FILL_EXIT_FILL_OR_PATH_INFERENCE_IN_PR15`;
  - `SHADOW_V2_RECORD_NOT_CONSUMED_BY_DECISIONS`;
  - `SESSION_ID_MISSING_FROM_HANDOFF_EXPLICIT_UNKNOWN`.

Wniosek: canonical V2 ma realna pozycje, ale nadal nie ma pelnego V2 fill/path contract.

## 7. Entry evidence

Entry evidence istnieje w lokalnych legacy shadow logs:

- artifact: `logs/shadow_v2/shadow-v2-fidelity-validation-pr17-r2-relaxed-sampling/shadow_entries.jsonl`;
- rows: `1`;
- execution_outcome: `shadow_simulated`;
- entry_price: `7.541744196631089e-08`;
- decision_ts_ms: `1782949902044`;
- timestamp_ms: `1782949902044`;
- slot: `430194027`;
- brain_config_hash: `059f8a837ac33f9ad3f8d637b700482f0155d1a0258d0b6cebf6f2df04f8ae54`.

Shadow simulation buy evidence:

- artifact: `logs/shadow_v2/shadow-v2-fidelity-validation-pr17-r2-relaxed-sampling/buys.jsonl`;
- rows: `1`;
- sim_started_ts_ms: `1782949910214`;
- sim_finished_ts_ms: `1782949910476`;
- decision_to_sim_start_ms: `8170`;
- shadow_duration_ms: `262`;
- amount_lamports: `7000000`;
- entry_token_amount_raw: `92816725382`;
- rpc_slot: `430194027`;
- err: `null`;
- error_class: `null`;
- units_consumed: `98176`;
- live_signature: `null`.

Status:

`ENTRY_EVIDENCE_PRESENT_LEGACY_SHADOW_LOG_ONLY`

Ograniczenie:

Canonical V2 nie emituje jeszcze `shadow_entry_attempt_v2` ani `shadow_entry_fill_v2`. Nie wolno na tej podstawie deklarowac live fill ani live-equivalence.

## 8. Exit evidence

Exit evidence istnieje w lokalnym `shadow_lifecycle.jsonl`:

- rows: `3`;
- record_types:
  - `shadow_dispatch`: `1`;
  - `exit_filled`: `1`;
  - `position_closed`: `1`;
- close_reason: `TimeStop`;
- duration_ms: `30375`;
- entry_price: `7.541744196631089e-08`;
- exit_value_sol: `0.00579915`;
- final_pnl: `-0.0012008499999999998`;
- final_pnl_pct: `-17.154999999999998`;
- truth_source: `canonical_account_state_snapshot`;
- truth_status: `resolved`;
- entry_simulation_rpc_slot: `430194027`;
- entry_landed_slot: `430194028`;
- exit_sample_slot: `430194026`;
- exit_landed_slot: `430194027`;
- sample_age_ms: `0`;
- sample_price_state: `Valid`.

Status:

`EXIT_EVIDENCE_PRESENT_LEGACY_SHADOW_LIFECYCLE_ONLY`

Ograniczenie:

Canonical V2 derived lifecycle ma tylko `POSITION_OPEN` i nie tworzy canonical terminal truth. Nie ma jeszcze pelnego `shadow_exit_attempt_v2`, `shadow_exit_fill_v2` ani `shadow_terminal_truth_v2`.

## 9. Path density

`shadow_path_density_v2.jsonl`:

- rows: `14`;
- verdicts:
  - `NOT_EVALUABLE_NO_COVERAGE`: `14`;
- path_points_sum: `0`.

Status:

`PATH_SAMPLES_NOT_EMITTED_NOT_EVALUABLE_NO_COVERAGE`

Wniosek:

Ten run potwierdza realny accepted shadow handoff i lifecycle close w legacy logs, ale nie potwierdza sampling density ani target/stop/path reconstruction w V2. Horyzonty 2s/3s/120s/300s/500s pozostaja nieewaluowalne z V2 density evidence.

## 10. Replay/lifecycle V2 reconciliation

V2 replay i lifecycle sa tied to real candidate/position:

- `shadow_replay_v2`: `2` rows, w tym 1 real position row;
- `shadow_lifecycle_v2`: `2` rows, w tym 1 real position row;
- oba derived snapshots wskazuja canonical high-watermark realnej pozycji.

Status:

`REPLAY_LIFECYCLE_REAL_POSITION_DERIVED_OPEN_ONLY`

Ograniczenie:

V2 replay/lifecycle nie zawieraja jeszcze entry fill event id, exit fill event id, terminal truth event id ani terminal PnL. Reconciliation dla pelnego lifecycle pozostaje zablokowane do czasu emisji pelnych V2 eventow.

## 11. Clean shutdown

Clean shutdown:

- `SIGINT`: `1`;
- `SIGTERM`: `0`;
- `Oracle Runtime shut down successfully`: observed;
- `PostBuyRuntime: Shadow V2 post-run manifest generated and strict-verified`: `1`;
- `Seer shut down successfully`: observed;
- `Watchdog shut down successfully`: observed;
- `All components shut down successfully`: `1`;
- `Ghost Launcher shutdown complete`: `1`;
- process exit code: `0`.

Nie bylo reconnect/disconnect flood:

- `Transport channel disconnected`: `0`;
- `NLN Subscribe request failed`: `0`;
- `status: 502`: `0`;
- `Bad Gateway`: `0`.

## 12. Znaczenie dla Shadow V2

PR17-r2 przesuwa status z:

`BLOCKED_NO_REAL_SHADOW_V2_POSITION_EVIDENCE`

do:

`PASS_REAL_SHADOW_V2_POSITION_EVIDENCE_PRESENT_WITH_LIMITATIONS`

Udowodnione:

- relaxed validation sampling potrafi wygenerowac realny accepted shadow handoff;
- Shadow V2 canonical writer zapisuje realny `shadow_position_event_v2`;
- derived replay/lifecycle materializuja rows tied to real position;
- post-run manifest i strict audit przechodza;
- legacy shadow logs zawieraja entry simulation, shadow dispatch, exit_filled i position_closed dla tej pozycji;
- clean shutdown dziala.

Nieudowodnione:

- canonical V2 entry fill;
- canonical V2 exit fill;
- canonical V2 terminal truth;
- V2 path samples;
- V2 path density dla realnych horyzontow;
- entry price reconstruction from V2 pool_state_sample_v2;
- exit price reconstruction from V2 path/exit fill;
- live fill;
- live slippage;
- live landing;
- executable sell fill.

## 13. Final decision

| Pytanie | Odpowiedz |
|---|---|
| Czy real_shadow_v2_positions > 0? | Tak, `1` |
| Czy pre-run gates przeszly? | Tak |
| Czy post_run_manifest ma PASS? | Tak |
| Czy post-run strict audit ma PASS? | Tak |
| Czy clean shutdown jest udowodniony? | Tak |
| Czy entry evidence istnieje? | Tak, w legacy shadow logs |
| Czy canonical V2 entry fill istnieje? | Nie |
| Czy exit evidence istnieje? | Tak, w legacy shadow lifecycle |
| Czy canonical V2 exit fill istnieje? | Nie |
| Czy path samples > 0? | Nie |
| Czy density rows sa evaluable? | Nie, `NOT_EVALUABLE_NO_COVERAGE` |
| Czy replay/lifecycle sa tied to real position? | Tak, ale derived open-only |
| Czy runtime approval jest przyznany? | Nie |
| Czy shadow_close_only approval jest przyznany? | Nie |
| Czy active close approval jest przyznany? | Nie |
| Czy strategy proof jest przyznany? | Nie |
| Czy live-equivalence jest przyznane? | Nie |

Finalny verdict:

`PASS_REAL_SHADOW_V2_POSITION_EVIDENCE_PRESENT_WITH_LIMITATIONS`

## 14. Nastepny krok

Nie nalezy przechodzic do strategy proof ani runtime approval.

Nastepny techniczny etap powinien byc waskim PR-em implementacyjnym, ktory emituje pelne canonical V2 records dla realnego handoff:

- `shadow_entry_attempt_v2`;
- `shadow_entry_fill_v2`;
- `shadow_path_sample_v2`;
- `shadow_exit_attempt_v2`;
- `shadow_exit_fill_v2`;
- `shadow_terminal_truth_v2`.

Dopiero po tym run moze przejsc z "real position evidence present" do pelnej fidelity reconstruction.
