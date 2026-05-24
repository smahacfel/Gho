# RAPORT P3.7-E2S Legacy Buy Executable Route Smoke

Data: 2026-05-24

## Werdykt

**P3.7-E2S: PASS-B / correct fail-closed**

`legacy_buy` został sprawdzony w bounded smoke, ale nie odblokował execution:

- `strict replay = full_replay_ok`
- `diagnostic_quality.status = PASS`
- `r16_artifact_identity_status = PASS`
- `single_active_hash_status = PASS`
- `post-simulation AccountNotFound = 0`
- `route_executable_rows = 0`
- `successful_entry_rows = 0`
- `lifecycle_eligible_rows = 0`

Wniosek: E2S nie jest regresją. System nadal fail-closuje przed symulacją, bez ślepego `AccountNotFound`, ale `legacy_buy` nie ma jeszcze kompletnego, autorytatywnego executable account set.

## Kontekst runu

Config:

`configs/rollout/shadow-burnin-v3-p37-e2s-legacy-buy-executable-route-smoke.toml`

Namespace:

`shadow-burnin-v3-p37-e2s-legacy-buy-executable-route-smoke`

Runtime:

`timeout 20m env RUST_LOG=info cargo run --release -p ghost-launcher --bin ghost-launcher -- --config /root/Gho/configs/rollout/shadow-burnin-v3-p37-e2s-legacy-buy-executable-route-smoke.toml`

Artefakty runtime były świeże względem configu; główne pliki powstały w oknie około `2026-05-24 19:24:20` - `2026-05-24 19:39:25 UTC`.

Uwaga repo: E2 został zacommitowany i wypchnięty jako `3244ab7`, ale smoke wystartował po późniejszym commitcie `dad1774` (`Add coordination risk evidence shell`). Wynik należy czytać jako E2 plus późniejszy commit coordination-risk shell; nie jest to czysty E2-only HEAD.

## Artefakty i liczby

Główne liczniki:

- decision rows: `392`
- active shadow BUY rows: `8`
- active shadow entry artifact rows: `8`
- active shadow lifecycle artifact rows: `8`
- probe selection rows: `44`
- probe skip rows: `392`
- probe transport rows: `0`
- probe successful entry rows: `0`

`v3_shadow_report.py`:

- `status = ok`
- `artifact_freshness.stale_against_config = false`
- `replay.status = full`
- `raw_rows = 392`
- `deduped_rows = 392`
- `v3_rows = 392`
- `bad_rows = 0`

`v3_full_replay_report.py --strict`:

- `replay_status = full_replay_ok`
- `total_rows = 392`
- `status_counts.full_replay_ok = 392`

`v3_p37_l1_reject_diagnostics.py`:

- `diagnostic_quality.status = PASS`
- `r16_artifact_identity_status = PASS`
- `single_active_hash_status = PASS`
- `r16_buy_verdict_count = 8`
- `active_shadow_account_not_found_rows = 0`
- `active_shadow_bonding_curve_v2_account_not_found_after_simulation_rows = 0`
- `active_shadow_runtime_simulation_error_rows = 0`
- `active_shadow_lifecycle_eligible_failure_rows = 0`
- `active_shadow_lifecycle_eligibility_status_counts.not_lifecycle_eligible = 16`

Join-key / feasibility audit:

- `decision_rows_total = 400`
- `probe_selected_rows = 44`
- `route_executable_rows = 0`
- `route_non_executable_rows = 306`
- `execution_feasibility_reject_rows = 68`
- `active_buy_execution_infeasible_rows = 24`
- `successful_entry_rows = 0`
- `lifecycle_eligible_rows = 0`
- `lifecycle_labeled_rows = 8`
- `buy_quality_labeled_rows = 8`
- `execution_feasibility_rate = 0.0`

## Active Shadow Route Result

Unikalne active BUY surfaces w JSONL: `8`.

Audit liczy trzy powierzchnie active (`buys`, `shadow_entries`, `shadow_lifecycle`), dlatego route-resolution counters w audycie mają `24`.

Active route resolver:

- `primary_route_kind = routed_exact_sol_in`
- `primary_route_ready = false`
- `primary_route_not_ready_reason = bonding_curve_v2_observed_meta_missing_on_rpc`
- `fallback_route_kind = legacy_buy`
- `fallback_route_attempted = true`
- `fallback_route_ready = false`
- `fallback_route_success_rows = 0`
- `route_resolution_status = no_executable_route_account_set`

Legacy buy active counters:

- `active_shadow_legacy_buy_route_attempted_rows = 24`
- `active_shadow_legacy_buy_route_ready_rows = 0`
- `active_shadow_legacy_buy_route_not_ready_rows = 24`
- `active_shadow_legacy_buy_route_not_ready_reason_counts.legacy_buy_missing_core_curve_account = 24`
- `active_shadow_legacy_buy_missing_core_curve_account_rows = 24`
- `active_shadow_legacy_buy_authoritative_curve_rows = 0`
- `active_shadow_legacy_buy_curve_source_counts.route_builder = 24`
- `active_shadow_legacy_buy_curve_authority_status_counts.derived_unverified = 24`
- `active_shadow_legacy_buy_curve_rpc_load_status_counts.present_on_rpc_precheck = 24`
- `active_shadow_legacy_buy_rpc_load_ready_rows = 24`
- `active_shadow_legacy_buy_successful_entry_rows = 0`

Interpretacja: aktywny fallback znalazł curve, które jest RPC-loadable, ale jego authority status nadal jest `derived_unverified`, więc nie może zostać potraktowane jako autorytatywny core curve account dla `legacy_buy`.

## Probe Route Result

Probe selected rows: `44`.

Probe fallback:

- `route_fallback_attempted_rows = 44`
- `route_fallback_success_rows = 0`
- `route_resolution_status_counts.no_executable_route_account_set = 44`
- `legacy_buy_route_attempted_rows = 44`
- `legacy_buy_route_ready_rows = 0`
- `legacy_buy_route_not_ready_rows = 44`
- `legacy_buy_route_not_ready_reason_counts.legacy_buy_simulation_load_not_ready = 44`
- `legacy_buy_authoritative_curve_rows = 44`
- `legacy_buy_curve_source_counts.account_state_core = 44`
- `legacy_buy_curve_authority_status_counts.authoritative_account_state = 44`
- `legacy_buy_curve_rpc_load_status_counts.not_checked = 44`
- `legacy_buy_rpc_load_ready_rows = 0`
- `legacy_buy_successful_entry_rows = 0`

Interpretacja: probe ma autorytatywne identity z `account_state_core`, ale nie ma potwierdzonego RPC/local simulation-load readiness dla `legacy_buy`, więc fallback poprawnie pozostaje not-ready.

## AccountNotFound

Przeszukanie artefaktów runu dla:

- `AccountNotFound`
- `account_not_found`
- `simulation_account_not_found`
- `runtime_simulation_error`

dało `0` wystąpień.

To potwierdza, że E2S nie cofnął L1R5/L1R9/L1R14/L1R16: brak route-ready account set nie przechodzi do runtime simulation jako ślepy błąd.

## Decyzja

**E2S = PASS-B / clean fail-closed.**

Nie ma podstaw do L2D2, progów, Phase B, P2/live ani Gatekeeper changes.

`legacy_buy` nadal nie odblokował executable universe:

- active: curve jest RPC-loadable, ale nieautorytatywna (`derived_unverified`)
- probe: curve jest autorytatywna z `account_state_core`, ale RPC readiness nie została sprawdzona/potwierdzona

Następny sensowny krok to route-support follow-up zawężony do `legacy_buy` account authority/readiness reconciliation, albo wybór kolejnego route-support targetu z E1/E2 danych. Nie wracać do BCV2, snapshotów ani policy tuning bez egzekwowalnej trasy.
