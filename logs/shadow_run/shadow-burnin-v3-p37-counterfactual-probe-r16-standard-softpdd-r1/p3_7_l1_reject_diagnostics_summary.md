# P3.7-L1 Reject Diagnostics Summary

- namespace: `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1`
- config_path: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1.toml`
- expected_brain_config_path: `/root/Gho/configs/rollout/ghost_brain_v3_p37_l1_standard_softpdd.toml`
- expected_brain_config_hash: `None`
- expected_run_id: `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1`
- expected_session_id: `r16-standard-softpdd-r1`
- decision_log: `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/decisions/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/v2.2/legacy_live/00b3d576e6ddfaefe5f738ef016d91e644fe3c67269a7cb058b29e4c75a2087d/gatekeeper_v2_decisions.jsonl`
- decision_rows: 1972
- malformed_rows: 0
- terminal_reject_or_timeout_rows: 1966
- r16_buy_verdict_count: 6
- r16_buy_shadow_entry_count: 5
- r16_buy_lifecycle_close_count: 13
- r16_buy_shadow_entry_unmatched_count: 0
- r16_buy_lifecycle_unmatched_count: 1
- r16_reject_pending_probe_lifecycle_count: 6
- r16_artifact_identity_status: FAIL
- single_active_hash_status: FAIL
- diagnostic_quality_status: FAIL
- pdd_entry_drift_anchor_coverage_pct: 0.0
- spike_ratio_coverage_pct: 100.0
- spike_ratio_quality_coverage_pct: 0.0
- whale_single_max_pct_coverage_pct: 0.0
- gatekeeper_first_or_terminal_gate_coverage_pct: 100.0

## Policy Hashes

### Decision Rows

- v3_policy_config_hash `55416d4c7ef23a0aaea0c5b3bb4da0abc6564ce7059049c49a8bf80b07170fdc`: 1972
- brain_config_hash `b41923673eacd484bd2178c6c7eb6782c5d90a9755f12ad68f1e625a0b658388`: 1972

### All R16 Artifacts

- v3_policy_config_hash `55416d4c7ef23a0aaea0c5b3bb4da0abc6564ce7059049c49a8bf80b07170fdc`: 2331
- v3_policy_config_hash `missing`: 1
- brain_config_hash `b41923673eacd484bd2178c6c7eb6782c5d90a9755f12ad68f1e625a0b658388`: 2331
- brain_config_hash `missing`: 1

## Artifact Identity Coverage

### decisions
- status: PASS
- rows: 1972
- namespace_coverage_pct: 100.0
- run_id_coverage_pct: 100.0
- session_id_coverage_pct: 100.0
- brain_config_path_coverage_pct: 100.0
- brain_config_hash_coverage_pct: 100.0
- v3_policy_config_hash_coverage_pct: 100.0

### probe_selection
- status: PASS
- rows: 163
- namespace_coverage_pct: 100.0
- run_id_coverage_pct: 100.0
- session_id_coverage_pct: 100.0
- brain_config_path_coverage_pct: 100.0
- brain_config_hash_coverage_pct: 100.0
- v3_policy_config_hash_coverage_pct: 100.0

### probe_transport
- status: PASS
- rows: 50
- namespace_coverage_pct: 100.0
- run_id_coverage_pct: 100.0
- session_id_coverage_pct: 100.0
- brain_config_path_coverage_pct: 100.0
- brain_config_hash_coverage_pct: 100.0
- v3_policy_config_hash_coverage_pct: 100.0

### probe_entries
- status: PASS
- rows: 50
- namespace_coverage_pct: 100.0
- run_id_coverage_pct: 100.0
- session_id_coverage_pct: 100.0
- brain_config_path_coverage_pct: 100.0
- brain_config_hash_coverage_pct: 100.0
- v3_policy_config_hash_coverage_pct: 100.0

### probe_lifecycle
- status: PASS
- rows: 78
- namespace_coverage_pct: 100.0
- run_id_coverage_pct: 100.0
- session_id_coverage_pct: 100.0
- brain_config_path_coverage_pct: 100.0
- brain_config_hash_coverage_pct: 100.0
- v3_policy_config_hash_coverage_pct: 100.0

### active_shadow_entries
- status: PASS
- rows: 5
- namespace_coverage_pct: 100.0
- run_id_coverage_pct: 100.0
- session_id_coverage_pct: 100.0
- brain_config_path_coverage_pct: 100.0
- brain_config_hash_coverage_pct: 100.0
- v3_policy_config_hash_coverage_pct: 100.0

### active_shadow_lifecycle
- status: FAIL
- rows: 14
- namespace_coverage_pct: 100.0
- run_id_coverage_pct: 92.857
- session_id_coverage_pct: 92.857
- brain_config_path_coverage_pct: 92.857
- brain_config_hash_coverage_pct: 92.857
- v3_policy_config_hash_coverage_pct: 92.857


## Artifact Rows

- probe_selection: 163 rows, malformed=0
- probe_skips: 1922 rows, malformed=0
- probe_transport: 50 rows, malformed=0
- probe_entries: 50 rows, malformed=0
- probe_lifecycle: 78 rows, malformed=0
- active_shadow_entries: 5 rows, malformed=0
- active_shadow_lifecycle: 14 rows, malformed=0
- lifecycle_labels: 39 rows, malformed=0

## Lifecycle Labels

- buy_quality_bad: 37
- buy_quality_dirty_good: 2
## First Kill Gates

- core3: 11
- hard_fail: 1
- market_cap: 1
- missing: 421
- pdd: 1531
- velocity: 1

## Terminal Gates

- core: 72
- hard_fail: 70
- timeout: 1824

## Reason Codes

- HARD_FAIL_EXTREME_BUNDLING: 2
- HARD_FAIL_EXTREME_TOP3: 12
- HARD_FAIL_MARKET_CAP: 36
- HARD_FAIL_PRICE_CHANGE: 13
- HARD_FAIL_SLOW_POOL: 7
- REJECT_CORE_FAIL: 72
- TIMEOUT_PHASE1_INSUFFICIENT: 1331
- TIMEOUT_PHASE1_NO_DATA: 493

## Quality Failures

- pdd_entry_drift_anchor_coverage_below_95pct
- pdd_spike_ratio_quality_coverage_below_95pct
- pdd_whale_single_max_pct_coverage_below_95pct
