# P3.7-L1 Reject Diagnostics Summary

- namespace: `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution`
- config_path: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution.toml`
- expected_brain_config_path: `/root/Gho/configs/rollout/ghost_brain_v3_p37_l1_standard_softpdd.toml`
- expected_brain_config_hash: `None`
- expected_run_id: `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution`
- expected_session_id: `r16-standard-softpdd-r4-account-attribution`
- decision_log: `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution/decisions/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution/v2.2/legacy_live/00b3d576e6ddfaefe5f738ef016d91e644fe3c67269a7cb058b29e4c75a2087d/gatekeeper_v2_decisions.jsonl`
- decision_rows: 423
- malformed_rows: 0
- terminal_reject_or_timeout_rows: 419
- r16_buy_verdict_count: 4
- r16_buy_shadow_entry_count: 4
- r16_buy_lifecycle_close_count: 4
- r16_buy_shadow_entry_unmatched_count: 0
- r16_buy_lifecycle_unmatched_count: 0
- r16_reject_pending_probe_lifecycle_count: 0
- r16_artifact_identity_status: PASS
- single_active_hash_status: PASS
- shadow_payer_strategy: ephemeral
- shadow_payer_pubkey: FTVK9eRXXnux2GL6jq4DRiv9HFrbNhFPdmB1PXyFkgX2
- shadow_payer_account_status: ephemeral_not_rpc_required
- shadow_payer_account_error: AccountNotFound
- diagnostic_quality_status: PASS
- pdd_drift_evaluated_rows: 222
- pdd_drift_anchor_hydrated_rows: 222
- pdd_drift_anchor_coverage_pct_among_evaluated: 100.0
- pdd_drift_threshold_source_rows: 423
- pdd_drift_threshold_source_only_rows: 201
- pdd_entry_drift_anchor_coverage_pct: 100.0
- spike_ratio_coverage_pct: 100.0
- spike_ratio_quality_coverage_pct: 100.0
- whale_single_max_pct_coverage_pct: 100.0
- gatekeeper_first_or_terminal_gate_coverage_pct: 100.0

## Policy Hashes

### Decision Rows

- v3_policy_config_hash `55416d4c7ef23a0aaea0c5b3bb4da0abc6564ce7059049c49a8bf80b07170fdc`: 423
- brain_config_hash `b41923673eacd484bd2178c6c7eb6782c5d90a9755f12ad68f1e625a0b658388`: 423

### All R16 Artifacts

- v3_policy_config_hash `55416d4c7ef23a0aaea0c5b3bb4da0abc6564ce7059049c49a8bf80b07170fdc`: 487
- brain_config_hash `b41923673eacd484bd2178c6c7eb6782c5d90a9755f12ad68f1e625a0b658388`: 487

## Artifact Identity Coverage

### decisions
- status: PASS
- rows: 423
- namespace_coverage_pct: 100.0
- run_id_coverage_pct: 100.0
- session_id_coverage_pct: 100.0
- brain_config_path_coverage_pct: 100.0
- brain_config_hash_coverage_pct: 100.0
- v3_policy_config_hash_coverage_pct: 100.0

### active_shadow_buys
- status: PASS
- rows: 4
- namespace_coverage_pct: 100.0
- run_id_coverage_pct: 100.0
- session_id_coverage_pct: 100.0
- brain_config_path_coverage_pct: 100.0
- brain_config_hash_coverage_pct: 100.0
- v3_policy_config_hash_coverage_pct: 100.0

### probe_selection
- status: PASS
- rows: 22
- namespace_coverage_pct: 100.0
- run_id_coverage_pct: 100.0
- session_id_coverage_pct: 100.0
- brain_config_path_coverage_pct: 100.0
- brain_config_hash_coverage_pct: 100.0
- v3_policy_config_hash_coverage_pct: 100.0

### probe_transport
- status: PASS
- rows: 15
- namespace_coverage_pct: 100.0
- run_id_coverage_pct: 100.0
- session_id_coverage_pct: 100.0
- brain_config_path_coverage_pct: 100.0
- brain_config_hash_coverage_pct: 100.0
- v3_policy_config_hash_coverage_pct: 100.0

### probe_entries
- status: PASS
- rows: 15
- namespace_coverage_pct: 100.0
- run_id_coverage_pct: 100.0
- session_id_coverage_pct: 100.0
- brain_config_path_coverage_pct: 100.0
- brain_config_hash_coverage_pct: 100.0
- v3_policy_config_hash_coverage_pct: 100.0

### probe_lifecycle
- status: PASS
- rows: 0
- namespace_coverage_pct: 100.0
- run_id_coverage_pct: 100.0
- session_id_coverage_pct: 100.0
- brain_config_path_coverage_pct: 100.0
- brain_config_hash_coverage_pct: 100.0
- v3_policy_config_hash_coverage_pct: 100.0

### active_shadow_entries
- status: PASS
- rows: 4
- namespace_coverage_pct: 100.0
- run_id_coverage_pct: 100.0
- session_id_coverage_pct: 100.0
- brain_config_path_coverage_pct: 100.0
- brain_config_hash_coverage_pct: 100.0
- v3_policy_config_hash_coverage_pct: 100.0

### active_shadow_lifecycle
- status: PASS
- rows: 4
- namespace_coverage_pct: 100.0
- run_id_coverage_pct: 100.0
- session_id_coverage_pct: 100.0
- brain_config_path_coverage_pct: 100.0
- brain_config_hash_coverage_pct: 100.0
- v3_policy_config_hash_coverage_pct: 100.0


## Artifact Rows

- active_shadow_buys: 4 rows, malformed=0
- probe_selection: 22 rows, malformed=0
- probe_skips: 408 rows, malformed=0
- probe_transport: 15 rows, malformed=0
- probe_entries: 15 rows, malformed=0
- probe_lifecycle: 0 rows, malformed=0
- active_shadow_entries: 4 rows, malformed=0
- active_shadow_lifecycle: 4 rows, malformed=0
- lifecycle_labels: 0 rows, malformed=0

## Active Shadow Payer

- strategy: ephemeral
- status: ephemeral_not_rpc_required
- first_pubkey: FTVK9eRXXnux2GL6jq4DRiv9HFrbNhFPdmB1PXyFkgX2
- first_error: AccountNotFound
- account_not_found_rows: 0

## Lifecycle Labels


## Baseline-Left Gate Distribution

### max_hhi
- fail: 257
- fail:observed_vs_threshold: 257
- pass: 76
- pass:observed_vs_threshold: 76

### min_bonding_progress_pct

### min_market_cap_sol

### min_tx_count
- fail: 373
- pass: 50

### min_unique_signers
- fail: 373
- pass: 50

### alpha
- pass: 423

## First Kill Gates

- missing: 42
- pdd: 376
- velocity: 1

## Terminal Gates

- core: 26
- hard_fail: 20
- timeout: 373

## Reason Codes

- HARD_FAIL_MARKET_CAP: 11
- HARD_FAIL_PRICE_CHANGE: 3
- HARD_FAIL_SLOW_POOL: 6
- REJECT_CORE_FAIL: 26
- TIMEOUT_PHASE1_INSUFFICIENT: 325
- TIMEOUT_PHASE1_NO_DATA: 48
