# RAPORT P3.7-L1R6 R16-r8 ACTIVE SHADOW REPORT ATTRIBUTION

Data: 2026-05-22

## Status

R16-r8 zostal uruchomiony jako waski smoke dla naprawy L1R6:

`shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r8-active-shadow-report-attribution`

Werdykt:

- L1R6 active shadow report-error attribution: `PASS`
- Strict replay: `PASS`
- L1 diagnostics quality: `PASS`
- Identity/hash contract: `PASS`
- Active shadow AccountNotFound attribution: `PASS`
- Active shadow execution: `BLOCKED`
- L2 ablation / collection / Phase B / P2 / live: `HOLD / NO-GO`

## Zakres

Smoke nie zmienial progow, polityki, IWIM, live/P2 ani sampling semantics.

Config runu:

`configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r8-active-shadow-report-attribution.toml`

Raporty wygenerowane z artefaktow:

- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r8-active-shadow-report-attribution/v3_shadow_report.json`
- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r8-active-shadow-report-attribution/v3_full_replay_report_strict.json`
- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r8-active-shadow-report-attribution/p3_7_l1r8_join_key_audit.json`
- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r8-active-shadow-report-attribution/p3_7_l1r8_join_key_audit.md`
- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r8-active-shadow-report-attribution/p3_7_l1r8_reject_diagnostics.json`
- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r8-active-shadow-report-attribution/shadow_onchain_lifecycle_report.jsonl`

## Wyniki runtime

Run zostal zatrzymany po uzyskaniu jednoznacznego dowodu dla celu smoke.

Liczniki:

- `raw_rows`: 222
- `v3_rows`: 222
- `bad_rows`: 0
- `strict replay`: `full_replay_ok`
- `r16_buy_verdict_count`: 1
- `r16_buy_shadow_entry_count`: 1
- `r16_buy_lifecycle_close_count`: 1
- `probe_selection_rows`: 15
- `probe_transport_rows`: 0
- `probe_entry_rows`: 0
- `bonding_curve_v2_precheck_skipped_before_simulation_rows`: 15
- `bonding_curve_v2_account_not_found_after_simulation_rows`: 0

L1 diagnostics:

- `diagnostic_quality.status`: `PASS`
- `pdd_entry_drift_anchor_coverage_pct`: 100.0
- `spike_ratio_quality_coverage_pct`: 100.0
- `whale_single_max_pct_coverage_pct`: 100.0
- `r16_artifact_identity_status`: `PASS`
- `single_active_hash_status`: `PASS`

Active shadow dispatch:

- `active_shadow_account_not_found_rows`: 3
- `active_shadow_account_not_found_attributed_rows`: 3
- `active_shadow_account_not_found_unattributed_rows`: 0
- `active_shadow_lifecycle_eligible_failure_rows`: 0
- `active_shadow_account_set_match_counts`: `{"true": 3}`
- `active_shadow_account_narrowing_status_counts`: `{"exact_after_narrowing": 3}`
- `active_shadow_account_candidate_raw_counts`: `{"payer_pubkey": 3, "user_ata": 3, "user_volume_accumulator": 3, "bonding_curve_v2": 3}`
- `active_shadow_account_candidate_narrowed_counts`: `{"bonding_curve_v2": 3}`

Po dodatkowym reporting fixie w join-key audit canonical `shadow_entries.jsonl` row z
`execution_outcome=shadow_data_problem` jest liczony jako dispatch failure, a nie
jako successful entry artifact:

- `active_shadow_dispatch_failure_rows`: 3
- `active_shadow_successful_entry_rows`: 0
- `active_shadow_lifecycle_eligible_rows`: 0
- `active_shadow_account_candidate_raw_counts`: `{"payer_pubkey": 3, "user_ata": 3, "user_volume_accumulator": 3, "bonding_curve_v2": 3}`
- `active_shadow_account_candidate_narrowed_counts`: `{"bonding_curve_v2": 3}`

## Dowod atrybucji

Canonical active shadow artifacts (`buys.jsonl`, `shadow_entries.jsonl`, `shadow_lifecycle.jsonl`) zawieraja teraz atrybucje AccountNotFound.

Przykladowy row:

- `err`: `AccountNotFound`
- `simulation_error_kind`: `AccountNotFound`
- `simulation_error_category`: `simulation_account_not_found_attributed`
- `simulation_error_account_role`: `bonding_curve_v2`
- `simulation_error_account_pubkey`: `UQK1akiBpJe8rxiwQYMaKMJmUxDFMNJuLgiitLpw6rV`
- `simulation_error_account_source`: `route_builder`
- `simulation_error_account_narrowing_status`: `exact_after_narrowing`
- `active_shadow_lifecycle_eligibility_status`: `not_lifecycle_eligible`
- `account_set_match`: `true`

To potwierdza, ze poprzedni blad L1R6 zostal naprawiony: `Ok(ShadowSimulated { report.err = AccountNotFound })` nie traci juz diagnostyki przy konwersji do canonical shadow event / entry / lifecycle failure row.

## Interpretacja

L1R6 code path jest runtime-validated dla glownego celu:

`active shadow AccountNotFound` nie jest juz blind/unattributed.

Jednoczesnie execution nadal jest zablokowane. Brakujacym kontem po zwezeniu jest `bonding_curve_v2`. Failure rows sa poprawnie oznaczone jako `not_lifecycle_eligible`, wiec raport nie miesza failed dispatch z realnym lifecycle close.

Probe plane w tym runie nie doszedl do transport/entry, bo L1R5 fail-closed precheck zadzialal zgodnie z kontraktem:

- `execution_account_not_ready:bonding_curve_v2`: 15
- `creator_vault_source_not_authoritative`: 119
- `missing_execution_route_identity`: 29

## Wniosek

R16-r8 zamyka L1R6 jako naprawe atrybucji active shadow report-error path.

Nastepny waski fix powinien byc active-shadow odpowiednikiem L1R5:

`P3.7-L1R9 Active Shadow BondingCurveV2 Precheck Contract`

Cel: jezeli `bonding_curve_v2` jest w simulation transaction account metas, active shadow path powinien traktowac je jako simulation-load required i fail-closed przed `simulate_buy`, zamiast dopuszczac runtime `AccountNotFound`.

Do czasu tej naprawy:

- nie uruchamiac L2 ablation,
- nie robic collection,
- nie przechodzic do Phase B,
- nie dotykac P2/live,
- nie zmieniac progow.

## Walidacja

Wykonane przed runtime:

- `cargo test -p ghost-launcher --lib active_shadow_report_error_outcome_carries_account_diagnostics -- --nocapture`
- `cargo test -p ghost-launcher --lib active_shadow_account_not_found -- --nocapture`
- `cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture`
- `cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture`
- `python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v`
- `rustfmt --edition 2021 --check ghost-launcher/src/oracle_runtime.rs`
- `git diff --check`

Wykonane po runtime:

- `python3 scripts/v3_shadow_report.py --config <r16-r8-config> --json`
- `python3 scripts/v3_full_replay_report.py --config <r16-r8-config> --strict --json`
- `python3 scripts/v3_p37_mfs_lifecycle_join_key_audit.py --config <r16-r8-config> --output-json ... --output-md ...`
- `python3 scripts/v3_p37_l1_reject_diagnostics.py --config <r16-r8-config> --json`
- `python3 scripts/shadow_onchain_lifecycle_report.py --config <r16-r8-config> --all-sessions --output ...`
- `python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py`
- `python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v`

On-chain lifecycle report zapisal 0 rows, bo jedyny active shadow lifecycle row jest dispatch failure, nie lifecycle-eligible close.
