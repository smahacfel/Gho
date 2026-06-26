# ADR-8D: PR-TSV2-A2 TimeStop V2 target-cut attribution and selective no-harm mask proof

Data: 2026-06-26

## 1. Status

`TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH`

Offline-only research evidence. No runtime change, no `shadow_close_only`, no Gatekeeper policy change, no selector change.

## 2. Scope

Scope: `shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1`

A2 checks whether predeclared candidate-time-safe masks M0-M8 can reduce TimeStop V2 target-cut damage while retaining beneficial saved STOP/TIMEOUT exits.

## 3. Safety Boundary

- no Gatekeeper changes
- no BUY/REJECT changes
- no V3/v25 confidence changes
- no selector runtime policy changes
- no TX builder/sender/Jito/live execution changes
- no new sidecar
- no `alpha_31100`, XGBoost, or selector shadow score runtime input

## 4. Coverage

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

## 5. Canonical / Train-Selected / Diagnostic Views

Canonical row is fixed at `6000/-6000/120000`.
canonical_row_verdict: `TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH`
train_selected_row_verdict: `TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH`
diagnostic_grid_best_verdict: `TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH`

Train-selected row:
```json
{
  "cost100_action_taken_count": 4609,
  "cost100_aggregate_target_cut_damage_guard_pass": true,
  "cost100_beneficial_exit_count": 1521,
  "cost100_delta_avg_bps": 93.26347935973041,
  "cost100_delta_median_bps": 0.0,
  "cost100_delta_sum_bps": 442815,
  "cost100_exit_action_precision": 0.7351377477042049,
  "cost100_exit_action_precision_wilson95_lower": 0.7157011038976295,
  "cost100_gross_saved_damage_bps": 1800453,
  "cost100_harmful_exit_count": 548,
  "cost100_public_row_verdict": "TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH",
  "cost100_segment_target_cut_damage_fail_segments": "validation,holdout",
  "cost100_segment_target_cut_damage_guard_pass": false,
  "cost100_target_cut_count": 44,
  "cost100_target_cut_count_guard_pass": true,
  "cost100_target_cut_damage_bps": 403143,
  "cost100_target_cut_damage_guard_pass": true,
  "cost100_target_cut_damage_ratio": 0.22391198215115862,
  "mask_name": "M4_CONFIRM_2_WINDOWS",
  "max_hold_ms": 120000,
  "stop_bps": -6000,
  "target_bps": 10000
}
```

Train-selected cost row:
```json
{
  "absolute_baseline_pnl_cost0": -1556681,
  "absolute_baseline_pnl_cost100": -2031481,
  "absolute_baseline_pnl_cost150": -2268881,
  "absolute_baseline_pnl_cost200": -2506281,
  "absolute_baseline_pnl_cost50": -1794081,
  "absolute_tsv2_pnl_cost0": -1113866,
  "absolute_tsv2_pnl_cost100": -1588666,
  "absolute_tsv2_pnl_cost150": -1826066,
  "absolute_tsv2_pnl_cost200": -2063466,
  "absolute_tsv2_pnl_cost50": -1351266,
  "mask_name": "M4_CONFIRM_2_WINDOWS",
  "max_hold_ms": 120000,
  "paired_delta_avg_cost0": 93.26347935973041,
  "paired_delta_avg_cost100": 93.26347935973041,
  "paired_delta_avg_cost150": 93.26347935973041,
  "paired_delta_avg_cost200": 93.26347935973041,
  "paired_delta_avg_cost50": 93.26347935973041,
  "paired_delta_cost0": 442815,
  "paired_delta_cost100": 442815,
  "paired_delta_cost150": 442815,
  "paired_delta_cost200": 442815,
  "paired_delta_cost50": 442815,
  "paired_delta_median_cost0": 0.0,
  "paired_delta_median_cost100": 0.0,
  "paired_delta_median_cost150": 0.0,
  "paired_delta_median_cost200": 0.0,
  "paired_delta_median_cost50": 0.0,
  "paired_delta_sum_cost0": 442815,
  "paired_delta_sum_cost100": 442815,
  "paired_delta_sum_cost150": 442815,
  "paired_delta_sum_cost200": 442815,
  "paired_delta_sum_cost50": 442815,
  "stop_bps": -6000,
  "target_bps": 10000
}
```

Interpretation: TSV2 improves a losing baseline, but does not make the selected cohort profitable after costs. This is an exit-damage-reduction signal, not a standalone profitable strategy proof.

M4_CONFIRM_2_WINDOWS is the best train-selected diagnostic mask on R49. It is not eligible for `shadow_close_only` because only one full TSV2-window scope exists, holdout target-cut damage ratio exceeds 25%, absolute TSV2 PnL after costs remains negative, and no R50 independent validation exists.

Full-grid diagnostic best, no runtime implication:
```json
{
  "cost100_action_taken_count": 4455,
  "cost100_aggregate_target_cut_damage_guard_pass": false,
  "cost100_beneficial_exit_count": 1411,
  "cost100_delta_avg_bps": 129.9109098567818,
  "cost100_delta_median_bps": 0.0,
  "cost100_delta_sum_bps": 616817,
  "cost100_exit_action_precision": 0.7076228686058175,
  "cost100_exit_action_precision_wilson95_lower": 0.6872743721210641,
  "cost100_gross_saved_damage_bps": 1327509,
  "cost100_harmful_exit_count": 583,
  "cost100_public_row_verdict": "TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH",
  "cost100_segment_target_cut_damage_fail_segments": "train,validation,holdout",
  "cost100_segment_target_cut_damage_guard_pass": false,
  "cost100_target_cut_count": 350,
  "cost100_target_cut_count_guard_pass": false,
  "cost100_target_cut_damage_bps": 588092,
  "cost100_target_cut_damage_guard_pass": false,
  "cost100_target_cut_damage_ratio": 0.44300415289086553,
  "mask_name": "M0_ALL",
  "max_hold_ms": 120000,
  "stop_bps": -6000,
  "target_bps": 1500
}
```

## 6. Missing Evidence

```json
[
  {
    "class": "pre_entry",
    "field": "buy_count",
    "status": "missing evidence: field unavailable"
  },
  {
    "class": "pre_entry",
    "field": "sol_buy_ratio",
    "status": "missing evidence: field unavailable"
  },
  {
    "class": "pre_entry",
    "field": "current_market_cap_sol",
    "status": "missing evidence: field unavailable"
  },
  {
    "class": "pre_entry",
    "field": "bonding_progress_pct",
    "status": "missing evidence: field unavailable"
  },
  {
    "class": "pre_entry",
    "field": "price_change_ratio",
    "status": "missing evidence: field unavailable"
  },
  {
    "class": "pre_entry",
    "field": "max_single_tx_price_impact_pct_observed",
    "status": "missing evidence: field unavailable"
  },
  {
    "class": "pre_entry",
    "field": "unique_ratio",
    "status": "missing evidence: field unavailable"
  },
  {
    "class": "pre_entry",
    "field": "hhi",
    "status": "missing evidence: field unavailable"
  },
  {
    "class": "pre_entry",
    "field": "top3_signer_volume_ratio",
    "status": "missing evidence: field unavailable"
  },
  {
    "class": "pre_entry",
    "field": "avg_cpi_depth_50tx",
    "status": "missing evidence: field unavailable"
  },
  {
    "class": "pre_entry",
    "field": "compute_unit_cluster_dominance",
    "status": "missing evidence: field unavailable"
  },
  {
    "class": "pre_entry",
    "field": "dev_tx_ratio",
    "status": "missing evidence: field unavailable"
  },
  {
    "class": "pre_entry",
    "field": "dev_volume_ratio",
    "status": "missing evidence: field unavailable"
  },
  {
    "class": "pre_entry",
    "field": "signer_cross_pool_velocity",
    "status": "missing evidence: field unavailable"
  },
  {
    "class": "pre_entry",
    "field": "cpv_other_pool_activity",
    "status": "missing evidence: field unavailable"
  }
]
```

## 7. Verdict Blockers

- validation: target_cut_damage_ratio > 0.25
- holdout: target_cut_damage_ratio > 0.25

## 8. Runtime Decision

No basis for runtime change from A2 alone.
No basis for `shadow_close_only` without a second independent positive TSV2-window scope.

## 9. R50 Requirement

A second scope must be a TSV2 logging-only validation run with `time_stop_v2_window`, `shadow_exit_replay_v1`, no active close, no BUY/REJECT/Gatekeeper/selector/TX/Jito changes, and at least 3000 joined replay/window positions with exact join rate >=98%.
