# TimeStop V2 Target-Cut Attribution and Selective No-Harm Mask Proof A2

Report date: `2026-06-26`
Scope: `shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1`
Final verdict: `PROMISING_OFFLINE_ONLY`

No runtime change was made.
No Gatekeeper, BUY/REJECT, selector, TX builder, sender, Jito, live execution, `alpha_31100`, XGBoost, or new sidecar change was made.

## Research Question

Can TimeStop V2 target cuts be constrained by a simple candidate-time-safe mask without killing most saved STOP/TIMEOUT actions?

This report evaluates only exit-side action precision. It does not optimize buying and does not use entry precision as an acceptance metric.

## Methodology

- M0-M8 are predeclared masks; no free-form mask grid search is used.
- `alive_within_4000/8000/12000ms_after_candidate` is not used as an immediate lookahead filter.
- M5/M6/M8 simulate waiting 4000/8000/12000 ms and making the decision at delayed decision time from replay path state available at that later time.
- Full-grid diagnostic best is diagnostic only and has zero runtime implication.
- Train-selected row is selected on train only, then reported on validation and holdout.

## Result Levels

- canonical_row_verdict: `TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH`
- train_selected_row_verdict: `PROMISING_OFFLINE_ONLY`
- diagnostic_grid_best_verdict: `TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH`

## Coverage and Join Quality

```json
{
  "candidate_positions": 4746,
  "exact_join_rate_over_exit_replay": 1.0,
  "join_quality": {
    "duplicate_fallback_key_count": 0,
    "entry_ts_ms_source_counts": {
      "window_timestamp_minus_age": 4831
    },
    "exact_join_count": 3656,
    "fallback_unique_join_count": 0,
    "unmatched_exit_replay_count": 0,
    "unmatched_lifecycle_position_count": 1175
  },
  "positions": 4831,
  "positions_with_exit_replay": 3656,
  "positions_with_tsv2_windows": 4831,
  "scope": "shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1",
  "stale_data_no_action_candidates": 13
}
```

## Missing Evidence

| class | field | status |
| --- | --- | --- |
| pre_entry | buy_count | missing evidence: field unavailable |
| pre_entry | sol_buy_ratio | missing evidence: field unavailable |
| pre_entry | current_market_cap_sol | missing evidence: field unavailable |
| pre_entry | bonding_progress_pct | missing evidence: field unavailable |
| pre_entry | price_change_ratio | missing evidence: field unavailable |
| pre_entry | max_single_tx_price_impact_pct_observed | missing evidence: field unavailable |
| pre_entry | unique_ratio | missing evidence: field unavailable |
| pre_entry | hhi | missing evidence: field unavailable |
| pre_entry | top3_signer_volume_ratio | missing evidence: field unavailable |
| pre_entry | avg_cpi_depth_50tx | missing evidence: field unavailable |
| pre_entry | compute_unit_cluster_dominance | missing evidence: field unavailable |
| pre_entry | dev_tx_ratio | missing evidence: field unavailable |
| pre_entry | dev_volume_ratio | missing evidence: field unavailable |
| pre_entry | signer_cross_pool_velocity | missing evidence: field unavailable |
| pre_entry | cpv_other_pool_activity | missing evidence: field unavailable |

## Canonical Row: 6000/-6000/120000

| mask_name | target_bps | stop_bps | max_hold_ms | cost100_action_taken_count | cost100_delta_sum_bps | cost100_delta_median_bps | cost100_exit_action_precision | cost100_exit_action_precision_wilson95_lower | cost100_target_cut_count | cost100_target_cut_damage_ratio | cost100_aggregate_target_cut_damage_guard_pass | cost100_segment_target_cut_damage_guard_pass | cost100_segment_target_cut_damage_fail_segments | cost100_target_cut_count_guard_pass | cost100_public_row_verdict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| M0_ALL | 6000 | -6000 | 120000 | 3557 | 571588 | 0 | 0.75977 | 0.739138 | 97 | 0.37669 | False | False | train,validation,holdout | True | TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH |
| M1_NEGATIVE_OR_FLAT_ONLY | 6000 | -6000 | 120000 | 3292 | 400875 | 0 | 0.790007 | 0.768526 | 51 | 0.345301 | False | False | train,validation,holdout | True | TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH |
| M2_LOW_PROFIT_ONLY | 6000 | -6000 | 120000 | 3369 | 442569 | 0 | 0.783001 | 0.761815 | 59 | 0.349346 | False | False | train,validation,holdout | True | TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH |
| M3_EXCLUDE_HEARTBEAT_ONLY | 6000 | -6000 | 120000 | 3554 | 581022 | 0 | 0.759931 | 0.739285 | 96 | 0.370532 | False | False | train,validation,holdout | True | TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH |
| M4_CONFIRM_2_WINDOWS | 6000 | -6000 | 120000 | 3544 | 479949 | 0 | 0.758848 | 0.737733 | 93 | 0.379123 | False | False | train,validation,holdout | True | TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH |
| M5_DELAY_4000MS_CONFIRM | 6000 | -6000 | 120000 | 3543 | 472643 | 0 | 0.758413 | 0.737267 | 93 | 0.380465 | False | False | train,validation,holdout | True | TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH |
| M6_DELAY_8000MS_CONFIRM | 6000 | -6000 | 120000 | 3533 | 410276 | 0 | 0.752837 | 0.731014 | 90 | 0.380647 | False | False | train,validation,holdout | True | TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH |
| M7_CLASS_RESTRICTED | 6000 | -6000 | 120000 | 3554 | 581022 | 0 | 0.759931 | 0.739285 | 96 | 0.370532 | False | False | train,validation,holdout | True | TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH |
| M8_DELAY_12000MS_CONFIRM | 6000 | -6000 | 120000 | 3517 | 348078 | 0 | 0.740716 | 0.718005 | 85 | 0.387061 | False | False | train,validation,holdout | True | TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH |

## Train-Selected Row

Train-only selection: require positive train paired delta_sum/avg, non-negative train median, train action_precision >= 0.70, Wilson lower 95% >= 0.65, both target-cut guards pass, and at least 100 train action rows; choose max train delta_sum, then Wilson lower, precision, and lower target-cut damage. If no row passes, choose the best train delta row and mark selection_failed.

- selection_passed_train_gate: `True`
- train_failures: `[]`

| mask_name | target_bps | stop_bps | max_hold_ms | cost100_action_taken_count | cost100_delta_sum_bps | cost100_delta_avg_bps | cost100_delta_median_bps | cost100_exit_action_precision | cost100_exit_action_precision_wilson95_lower | cost100_target_cut_count | cost100_target_cut_damage_ratio | cost100_aggregate_target_cut_damage_guard_pass | cost100_segment_target_cut_damage_guard_pass | cost100_segment_target_cut_damage_fail_segments | cost100_target_cut_count_guard_pass | cost100_public_row_verdict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| M7_CLASS_RESTRICTED | 10000 | -6000 | 60000 | 3532 | 498371 | 136.316 | 0 | 0.713699 | 0.689975 | 14 | 0.122271 | True | True |  | True | PROMISING_OFFLINE_ONLY |

M4_CONFIRM_2_WINDOWS is the best train-selected diagnostic mask on R49.
It is not eligible for shadow_close_only because only one full TSV2-window scope exists, holdout target-cut damage ratio exceeds 25%, absolute TSV2 PnL after costs remains negative, and no R50 independent validation exists.

## Train-Selected Cost Sensitivity

| mask_name | target_bps | stop_bps | max_hold_ms | paired_delta_cost0 | paired_delta_cost50 | paired_delta_cost100 | paired_delta_cost150 | paired_delta_cost200 | absolute_baseline_pnl_cost100 | absolute_tsv2_pnl_cost100 | absolute_baseline_pnl_cost200 | absolute_tsv2_pnl_cost200 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| M7_CLASS_RESTRICTED | 10000 | -6000 | 60000 | 498371 | 498371 | 498371 | 498371 | 498371 | -1858385 | -1360014 | -2223985 | -1725614 |

TSV2 improves a losing baseline, but does not make the selected cohort profitable after costs.
Therefore this is an exit-damage-reduction signal, not a standalone profitable strategy proof.

## Train/Validation/Holdout Stability

| segment | action_taken_count | delta_sum_bps | delta_avg_bps | delta_median_bps | exit_action_precision | exit_action_precision_wilson95_lower | beneficial_exit_count | harmful_exit_count | target_cut_count | target_cut_damage_ratio | target_cut_damage_guard_pass | max_consecutive_harmful_actions |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| train | 1160 | 157430 | 129.147 | 0 | 0.720657 | 0.676224 | 307 | 119 | 5 | 0.142501 | True | 3 |
| validation | 1186 | 155242 | 127.352 | 0 | 0.683897 | 0.642001 | 344 | 159 | 7 | 0.176263 | True | 3 |
| holdout | 1186 | 185699 | 152.462 | 0 | 0.736347 | 0.697269 | 391 | 140 | 2 | 0.051168 | True | 3 |

## Grid-Neighborhood Around Train-Selected Row

| mask_name | target_bps | stop_bps | max_hold_ms | is_selected | cost100_delta_sum_bps | cost100_exit_action_precision | cost100_exit_action_precision_wilson95_lower | cost100_target_cut_damage_ratio | positive_delta |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| M7_CLASS_RESTRICTED | 7500 | -6000 | 30000 | False | 229706 | 0.66474 | 0.63546 | 0.088743 | True |
| M7_CLASS_RESTRICTED | 7500 | -6000 | 60000 | False | 517061 | 0.713014 | 0.689275 | 0.128066 | True |
| M7_CLASS_RESTRICTED | 7500 | -6000 | 120000 | False | 619314 | 0.765112 | 0.744605 | 0.298393 | True |
| M7_CLASS_RESTRICTED | 7500 | -5000 | 30000 | False | 205920 | 0.664055 | 0.634516 | 0.0935263 | True |
| M7_CLASS_RESTRICTED | 7500 | -5000 | 60000 | False | 477235 | 0.714286 | 0.690424 | 0.134766 | True |
| M7_CLASS_RESTRICTED | 7500 | -5000 | 120000 | False | 588326 | 0.767483 | 0.746911 | 0.294852 | True |
| M7_CLASS_RESTRICTED | 10000 | -6000 | 30000 | False | 213079 | 0.66474 | 0.63546 | 0.101285 | True |
| M7_CLASS_RESTRICTED | 10000 | -6000 | 60000 | True | 498371 | 0.713699 | 0.689975 | 0.122271 | True |
| M7_CLASS_RESTRICTED | 10000 | -6000 | 120000 | False | 536445 | 0.766264 | 0.745787 | 0.319398 | True |
| M7_CLASS_RESTRICTED | 10000 | -5000 | 30000 | False | 189293 | 0.664055 | 0.634516 | 0.106744 | True |
| M7_CLASS_RESTRICTED | 10000 | -5000 | 60000 | False | 457545 | 0.714979 | 0.691132 | 0.128751 | True |
| M7_CLASS_RESTRICTED | 10000 | -5000 | 120000 | False | 508065 | 0.768648 | 0.748109 | 0.318156 | True |

## Full-Grid Diagnostic Best

Diagnostic only. This row has zero runtime implication and is not a policy candidate.

| mask_name | target_bps | stop_bps | max_hold_ms | cost100_action_taken_count | cost100_delta_sum_bps | cost100_delta_median_bps | cost100_exit_action_precision | cost100_exit_action_precision_wilson95_lower | cost100_target_cut_count | cost100_target_cut_damage_ratio | cost100_aggregate_target_cut_damage_guard_pass | cost100_segment_target_cut_damage_guard_pass | cost100_segment_target_cut_damage_fail_segments | cost100_target_cut_count_guard_pass | cost100_public_row_verdict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| M7_CLASS_RESTRICTED | 3000 | -6000 | 120000 | 3517 | 637901 | 0 | 0.748531 | 0.727375 | 178 | 0.388047 | False | False | train,validation,holdout | True | TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH |

## Verdict Blockers

- none for `PROMISING_OFFLINE_ONLY`; this is not runtime approval.

## R50 / TSV2 Logging-Only Validation Run Requirements

- emits `time_stop_v2_window`
- emits `shadow_exit_replay_v1`
- no active close
- no BUY/REJECT change
- no Gatekeeper policy change
- no selector runtime change
- no `alpha_31100`
- no TX/Jito path changes
- same no-harm report schema as A1/A2
- `positions_with_exit_replay >= 3000`
- `positions_with_tsv2_windows >= 3000`
- `exact_join_rate >= 98%`
- `candidate_positions >= 2500`
- `path_approximate_rows = 0` preferred

## Output Files

| artifact | path |
| --- | --- |
| report | reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/TIME_STOP_V2_TARGET_CUT_ATTRIBUTION_A2.md |
| target_cut_attribution | reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_target_cut_attribution_a2.csv |
| mask_summary | reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_mask_summary_a2.csv |
| mask_stability | reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_mask_stability_a2.csv |
| mask_cost_sensitivity | reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_mask_cost_sensitivity_a2.csv |
| mask_grid_neighborhood | reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_mask_grid_neighborhood_a2.csv |
| summary_json | reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_target_cut_attribution_a2_summary.json |
| adr | docs/ADR/ADR_8D_TIMESTOP_V2_TARGET_CUT_ATTRIBUTION_A2_20260626.md |
