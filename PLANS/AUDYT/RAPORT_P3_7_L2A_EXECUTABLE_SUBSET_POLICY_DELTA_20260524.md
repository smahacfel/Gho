# P3.7-L2A Executable Subset Policy Delta Analysis

## Verdict

- status: `pass`
- final_decision: `GO_L2B_AXIS_ABLATION_PREP`
- manifest_path: `PLANS/AUDYT/MANIFEST_P3_7_L1R21_L2_INPUT_DATASET_CONTRACT_20260524.json`

## Input Contract

- buy_quality_denominator_rows: `85`
- buy_quality_dirty_good: `4`
- dirty_good_rate: `0.0471`
- allowed_runs: `["shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1", "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1"]`
- blocked_runs_count: `11`

## Per-Run Delta

| namespace | denominator | bad | dirty_good | good | dirty_good_rate | decision_join_rate |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1` | 42 | 42 | 0 | 0 | 0.0000 | 1.0000 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1` | 43 | 39 | 4 | 0 | 0.0930 | 1.0000 |

## Combined Allowed Subset

- buy_quality_denominator_rows: `85`
- buy_quality_bad: `81`
- buy_quality_dirty_good: `4`
- buy_quality_good: `0`
- dirty_good_rate: `0.0471`

## Policy Delta

- status: `computed_unpaired_delta`
- note: J4C and R16-r1 are different sampled universes; this is directional policy delta, not causal attribution.
- dirty_good_delta: `4`
- bad_delta: `-3`
- dirty_good_rate_delta: `0.0930`
- l2b_candidate_axes: `["standard_mode_shorter_window", "soft_pdd_instead_of_hard_pdd", "prosperity_filter_disabled", "hhi_hard_fail_relaxed", "elapsed_aware_entry_drift"]`

## Distributions: shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1

- buy_quality_class: `{"buy_quality_bad": 42}`
- decision_verdict: `{"TIMEOUT_PHASE1_INSUFFICIENT": 42}`
- reason_code: `{"TIMEOUT_PHASE1_INSUFFICIENT": 42}`
- first_kill_gate: `{"unknown": 42}`
- terminal_gate: `{"TIMEOUT_PHASE1_INSUFFICIENT": 42}`
- pdd_entry_drift_bucket: `{"abs_pct_0_3": 15, "abs_pct_3_6": 3, "unavailable": 24}`
- hhi_bucket: `{"0_20_0_35": 3, "gt_0_35": 28, "unavailable": 11}`
- top3_volume_pct_bucket: `{"gt_0_95": 31, "unavailable": 11}`
- same_ms_tx_ratio_bucket: `{"0_25_0_50": 1, "lte_0_25": 30, "unavailable": 11}`
- pdd_hard_fail: `{"ENTRY_DRIFT": 1, "WHALE": 41}`
- pdd_soft_flags: `{"unknown": 1, "whale": 41}`
- alpha_actionable: `{"false": 42}`
- prosperity_filter_enabled: `{"true": 42}`
- aps_shadow_prosperity_would_pass: `{"false": 42}`
- pdd_spike_detected: `{"false": 42}`
- pdd_ramping_detected: `{"false": 42}`

### Feature Availability: shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1

- `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/p3_7_probe_shadow_lifecycle_feature_availability.json` status=`insufficient_for_selector` rows_total=`42` dirty_good_with_features=`0` bad_with_features=`42`

## Distributions: shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1

- buy_quality_class: `{"buy_quality_bad": 39, "buy_quality_dirty_good": 4}`
- decision_verdict: `{"BUY": 5, "REJECT_CORE_FAIL": 2, "REJECT_HARD_FAIL": 1, "TIMEOUT_PHASE1_INSUFFICIENT": 35}`
- reason_code: `{"BUY_EARLY": 1, "BUY_NORMAL": 4, "HARD_FAIL_PRICE_CHANGE": 1, "REJECT_CORE_FAIL": 2, "TIMEOUT_PHASE1_INSUFFICIENT": 35}`
- first_kill_gate: `{"pdd": 40, "unknown": 3}`
- terminal_gate: `{"buy": 5, "core": 2, "hard_fail": 1, "timeout": 35}`
- pdd_entry_drift_bucket: `{"abs_pct_0_3": 7, "abs_pct_10_15": 3, "abs_pct_3_6": 2, "abs_pct_gt_15": 10, "unavailable": 21}`
- hhi_bucket: `{"0_10_0_155": 1, "0_20_0_35": 4, "gt_0_35": 26, "lte_0_10": 7, "unavailable": 5}`
- top3_volume_pct_bucket: `{"0_70_0_90": 1, "gt_0_95": 30, "lte_0_70": 7, "unavailable": 5}`
- same_ms_tx_ratio_bucket: `{"0_25_0_50": 7, "lte_0_25": 31, "unavailable": 5}`
- pdd_hard_fail: `{"ENTRY_DRIFT": 10, "WHALE": 30, "unknown": 3}`
- pdd_soft_flags: `{"unknown": 13, "whale": 30}`
- alpha_actionable: `{"false": 43}`
- prosperity_filter_enabled: `{"false": 43}`
- aps_shadow_prosperity_would_pass: `{"false": 36, "true": 7}`
- pdd_spike_detected: `{"false": 43}`
- pdd_ramping_detected: `{"false": 43}`

### Feature Availability: shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1

- `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/p3_7_shadow_lifecycle_feature_availability.json` status=`insufficient_for_selector` rows_total=`39` dirty_good_with_features=`2` bad_with_features=`37`

## Non-Goals

- `no_runtime`
- `no_threshold_tuning`
- `no_phase_b`
- `no_p2_live`
- `no_new_runs`
- `no_full_r16_route_universe`
