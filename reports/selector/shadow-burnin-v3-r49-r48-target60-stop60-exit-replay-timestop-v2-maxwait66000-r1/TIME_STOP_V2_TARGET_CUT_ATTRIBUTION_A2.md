# TimeStop V2 Target-Cut Attribution and Selective No-Harm Mask Proof A2

Report date: `2026-06-26`
Scope: `shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1`
Final verdict: `TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH`

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
- train_selected_row_verdict: `TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH`
- diagnostic_grid_best_verdict: `TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH`

## Coverage and Join Quality

```json
{
  "candidate_positions": 5485,
  "exact_join_rate_over_exit_replay": 1.0,
  "join_quality": {
    "duplicate_fallback_key_count": 0,
    "entry_ts_ms_source_counts": {
      "window_timestamp_minus_age": 5604
    },
    "exact_join_count": 4748,
    "fallback_unique_join_count": 0,
    "unmatched_exit_replay_count": 0,
    "unmatched_lifecycle_position_count": 856
  },
  "positions": 5604,
  "positions_with_exit_replay": 4748,
  "positions_with_tsv2_windows": 5604,
  "scope": "shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1",
  "stale_data_no_action_candidates": 15
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
| M0_ALL | 6000 | -6000 | 120000 | 4618 | 426059 | 0 | 0.729516 | 0.710333 | 129 | 0.406331 | False | False | train,validation,holdout | True | TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH |
| M1_NEGATIVE_OR_FLAT_ONLY | 6000 | -6000 | 120000 | 4245 | 324492 | 0 | 0.77479 | 0.754833 | 63 | 0.362372 | False | False | train,validation,holdout | True | TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH |
| M2_LOW_PROFIT_ONLY | 6000 | -6000 | 120000 | 4371 | 349672 | 0 | 0.759308 | 0.739611 | 75 | 0.368677 | False | False | train,validation,holdout | True | TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH |
| M3_EXCLUDE_HEARTBEAT_ONLY | 6000 | -6000 | 120000 | 4611 | 419893 | 0 | 0.730159 | 0.710961 | 129 | 0.408534 | False | False | train,validation,holdout | True | TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH |
| M4_CONFIRM_2_WINDOWS | 6000 | -6000 | 120000 | 4608 | 414733 | 0 | 0.725955 | 0.70633 | 125 | 0.38719 | False | False | train,validation,holdout | True | TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH |
| M5_DELAY_4000MS_CONFIRM | 6000 | -6000 | 120000 | 4607 | 413612 | 0 | 0.726789 | 0.707176 | 125 | 0.38596 | False | False | train,validation,holdout | True | TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH |
| M6_DELAY_8000MS_CONFIRM | 6000 | -6000 | 120000 | 4595 | 411949 | 0 | 0.720581 | 0.700492 | 120 | 0.364799 | False | False | train,validation,holdout | True | TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH |
| M7_CLASS_RESTRICTED | 6000 | -6000 | 120000 | 4611 | 419893 | 0 | 0.730159 | 0.710961 | 129 | 0.408534 | False | False | train,validation,holdout | True | TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH |
| M8_DELAY_12000MS_CONFIRM | 6000 | -6000 | 120000 | 4577 | 347063 | 0 | 0.71317 | 0.692535 | 115 | 0.359695 | False | False | validation,holdout | True | TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH |

## Train-Selected Row

Train-only selection: require positive train paired delta_sum/avg, non-negative train median, train action_precision >= 0.70, Wilson lower 95% >= 0.65, both target-cut guards pass, and at least 100 train action rows; choose max train delta_sum, then Wilson lower, precision, and lower target-cut damage. If no row passes, choose the best train delta row and mark selection_failed.

- selection_passed_train_gate: `True`
- train_failures: `[]`

| mask_name | target_bps | stop_bps | max_hold_ms | cost100_action_taken_count | cost100_delta_sum_bps | cost100_delta_avg_bps | cost100_delta_median_bps | cost100_exit_action_precision | cost100_exit_action_precision_wilson95_lower | cost100_target_cut_count | cost100_target_cut_damage_ratio | cost100_aggregate_target_cut_damage_guard_pass | cost100_segment_target_cut_damage_guard_pass | cost100_segment_target_cut_damage_fail_segments | cost100_target_cut_count_guard_pass | cost100_public_row_verdict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| M4_CONFIRM_2_WINDOWS | 10000 | -6000 | 120000 | 4609 | 442815 | 93.2635 | 0 | 0.735138 | 0.715701 | 44 | 0.223912 | True | False | validation,holdout | True | TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH |

M4_CONFIRM_2_WINDOWS is the best train-selected diagnostic mask on R49.
It is not eligible for shadow_close_only because only one full TSV2-window scope exists, holdout target-cut damage ratio exceeds 25%, absolute TSV2 PnL after costs remains negative, and no R50 independent validation exists.

## Train-Selected Cost Sensitivity

| mask_name | target_bps | stop_bps | max_hold_ms | paired_delta_cost0 | paired_delta_cost50 | paired_delta_cost100 | paired_delta_cost150 | paired_delta_cost200 | absolute_baseline_pnl_cost100 | absolute_tsv2_pnl_cost100 | absolute_baseline_pnl_cost200 | absolute_tsv2_pnl_cost200 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| M4_CONFIRM_2_WINDOWS | 10000 | -6000 | 120000 | 442815 | 442815 | 442815 | 442815 | 442815 | -2031481 | -1588666 | -2506281 | -2063466 |

TSV2 improves a losing baseline, but does not make the selected cohort profitable after costs.
Therefore this is an exit-damage-reduction signal, not a standalone profitable strategy proof.

## Train/Validation/Holdout Stability

| segment | action_taken_count | delta_sum_bps | delta_avg_bps | delta_median_bps | exit_action_precision | exit_action_precision_wilson95_lower | beneficial_exit_count | harmful_exit_count | target_cut_count | target_cut_damage_ratio | target_cut_damage_guard_pass | max_consecutive_harmful_actions |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| train | 1548 | 254608 | 160.839 | 0 | 0.726287 | 0.69301 | 536 | 202 | 13 | 0.16026 | True | 4 |
| validation | 1521 | 87842 | 55.4908 | 0 | 0.725 | 0.691256 | 522 | 198 | 17 | 0.25707 | False | 3 |
| holdout | 1540 | 100365 | 63.4418 | 0 | 0.757774 | 0.722261 | 463 | 148 | 14 | 0.27819 | False | 2 |

## Grid-Neighborhood Around Train-Selected Row

| mask_name | target_bps | stop_bps | max_hold_ms | is_selected | cost100_delta_sum_bps | cost100_exit_action_precision | cost100_exit_action_precision_wilson95_lower | cost100_target_cut_damage_ratio | positive_delta |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| M4_CONFIRM_2_WINDOWS | 7500 | -6000 | 60000 | False | 74253 | 0.634279 | 0.610803 | 0.26932 | True |
| M4_CONFIRM_2_WINDOWS | 7500 | -6000 | 120000 | False | 401131 | 0.731141 | 0.711617 | 0.358155 | True |
| M4_CONFIRM_2_WINDOWS | 7500 | -5000 | 60000 | False | 32357 | 0.635417 | 0.611772 | 0.284297 | True |
| M4_CONFIRM_2_WINDOWS | 7500 | -5000 | 120000 | False | 346024 | 0.734704 | 0.715132 | 0.37621 | True |
| M4_CONFIRM_2_WINDOWS | 10000 | -6000 | 60000 | False | 50153 | 0.635103 | 0.611643 | 0.203737 | True |
| M4_CONFIRM_2_WINDOWS | 10000 | -6000 | 120000 | True | 442815 | 0.735138 | 0.715701 | 0.223912 | True |
| M4_CONFIRM_2_WINDOWS | 10000 | -5000 | 60000 | False | 12826 | 0.636865 | 0.613244 | 0.214005 | True |
| M4_CONFIRM_2_WINDOWS | 10000 | -5000 | 120000 | False | 399708 | 0.739237 | 0.719767 | 0.228938 | True |

## Full-Grid Diagnostic Best

Diagnostic only. This row has zero runtime implication and is not a policy candidate.

| mask_name | target_bps | stop_bps | max_hold_ms | cost100_action_taken_count | cost100_delta_sum_bps | cost100_delta_median_bps | cost100_exit_action_precision | cost100_exit_action_precision_wilson95_lower | cost100_target_cut_count | cost100_target_cut_damage_ratio | cost100_aggregate_target_cut_damage_guard_pass | cost100_segment_target_cut_damage_guard_pass | cost100_segment_target_cut_damage_fail_segments | cost100_target_cut_count_guard_pass | cost100_public_row_verdict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| M0_ALL | 1500 | -6000 | 120000 | 4455 | 616817 | 0 | 0.707623 | 0.687274 | 350 | 0.443004 | False | False | train,validation,holdout | False | TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH |

## Verdict Blockers

- validation: target_cut_damage_ratio > 0.25
- holdout: target_cut_damage_ratio > 0.25

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
| report | reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/TIME_STOP_V2_TARGET_CUT_ATTRIBUTION_A2.md |
| target_cut_attribution | reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_target_cut_attribution_a2.csv |
| mask_summary | reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_mask_summary_a2.csv |
| mask_stability | reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_mask_stability_a2.csv |
| mask_cost_sensitivity | reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_mask_cost_sensitivity_a2.csv |
| mask_grid_neighborhood | reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_mask_grid_neighborhood_a2.csv |
| summary_json | reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_target_cut_attribution_a2_summary.json |
| adr | docs/ADR/ADR_8D_TIMESTOP_V2_TARGET_CUT_ATTRIBUTION_A2_20260626.md |
