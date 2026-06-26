# RAPORT: Organic Pool Candidate Policy A0 Offline Proof

Generated UTC: `2026-06-26T15:02:50Z`
Scope: `shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2`
Decision lane: `v2.2/legacy_live`
Profile: `medium`
Final verdict: `REJECTED_FOR_RUNTIME / INCONCLUSIVE_RESEARCH`

## Scope boundaries

- Offline/read-only proof only.
- No Gatekeeper BUY/REJECT change.
- No `v25_confidence`, V3 promotion, selector runtime policy, TX builder, sender, Jito path, live execution, or existing log mutation.
- `selector_shadow_score` is used only as equal-count diagnostic baseline.
- `shadow_exit_replay_v1` is used only after candidate cohort selection, for identical Target/Stop/max_hold grids.
- Identifiers and timestamps are used only for join/dedup/sorting, not predictive features.

## Closure verdict

`REJECTED_FOR_RUNTIME / INCONCLUSIVE_RESEARCH`

There is no basis for:

- runtime change,
- `shadow_close_only`,
- Gatekeeper policy change,
- selector change,
- `alpha_31100`,
- XGBoost,
- another threshold-tuning pass on R48/R2.

Hard closure findings:

1. C1 does not beat F5 on holdout cost100 avg or sum: F5 avg `188.659`, sum `86406`; C1 avg `186.637`, sum `57111`.
2. C2-C5 have `0%` Target on holdout.
3. All cost-adjusted medians are negative for S1/F5 and C1-C5.
4. Positive avg comes from a sparse right tail of large hits and positive TimeOuts, not from stable precision.
5. This candidate family does not satisfy the organic edge gate.

## Files checked

| kind | path | exists | size_bytes |
| --- | --- | --- | --- |
| decision_log | logs/rollout/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/decisions/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/v2.2/legacy_live/8b506cc2b631260ea2f828e5fe1dc15b58c79efa2e4ce7a3cca675e057d87051/gatekeeper_v2_decisions.jsonl | True | 1741769868 |
| selector_score | logs/rollout/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/decisions/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/v2.2/legacy_live/8b506cc2b631260ea2f828e5fe1dc15b58c79efa2e4ce7a3cca675e057d87051/selector_shadow_score_v1.jsonl | True | 59359054 |
| shadow_lifecycle | logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/shadow_lifecycle.jsonl | True | 40259676 |
| probe_lifecycle | logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/probe_shadow_lifecycle.jsonl | True | 7060621 |
| exit_replay | logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/shadow_exit_replay_v1.jsonl | True | 10973969 |

## Data controls

```json
{
  "blockers": [
    "C1: no full F5 beat (mix=True, cost100=False, nonnegative_segments=3/3)",
    "C2: no full F5 beat (mix=False, cost100=False, nonnegative_segments=3/3)",
    "C3: no full F5 beat (mix=False, cost100=False, nonnegative_segments=3/3)",
    "C4: no full F5 beat (mix=False, cost100=False, nonnegative_segments=3/3)",
    "C5: retained cohort too small (60)"
  ],
  "decision_lane": "v2.2/legacy_live",
  "decision_log": {
    "duplicate_target_rows": 0,
    "joined_mints": 4317,
    "missing_base_mint": 0,
    "missing_target_mints": 0,
    "rows_scanned": 17194,
    "target_rows_seen": 4317
  },
  "exit_replay": {
    "damage_reasons": {
      "quality:unavailable": 24
    },
    "duplicate_keys": 0,
    "horizon_ms_counts": {
      "120000": 4341
    },
    "qualified_records": 4317,
    "quality_counts": {
      "clean": 4317,
      "unavailable": 24
    },
    "total_records": 4341,
    "truncated_counts": {
      "False": 4341
    }
  },
  "joins": {
    "joined_decision_records": 4317,
    "joined_selector_scores": 4317,
    "missing_decision_records": 0,
    "replay_records": 4317
  },
  "probe_shadow_lifecycle": {
    "exists": true,
    "path": "logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/probe_shadow_lifecycle.jsonl",
    "record_type_counts": {
      "exit_filled": 1211,
      "position_closed": 1211
    },
    "records_with_final_pnl_pct": 2422,
    "rows": 2422
  },
  "profile": "medium",
  "scope": "shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2",
  "selected_exits": {
    "C1": [
      7500,
      -100,
      30000
    ],
    "C2": [
      10000,
      -200,
      40000
    ],
    "C3": [
      10000,
      -200,
      40000
    ],
    "C4": [
      10000,
      -200,
      30000
    ],
    "C5": [
      10000,
      -200,
      30000
    ],
    "S0": [
      10000,
      -200,
      120000
    ],
    "S1_F5": [
      7500,
      -100,
      30000
    ],
    "SEL_EQ_C1": [
      6000,
      -100,
      40000
    ],
    "SEL_EQ_C2": [
      6000,
      -100,
      40000
    ],
    "SEL_EQ_C3": [
      6000,
      -100,
      40000
    ],
    "SEL_EQ_C4": [
      10000,
      -100,
      10000
    ],
    "SEL_EQ_C5": [
      10000,
      -200,
      40000
    ],
    "SEL_EQ_S1_F5": [
      7500,
      -100,
      90000
    ]
  },
  "selector_shadow_score": {
    "joined_mints": 4317,
    "missing_or_nonfinite_scores": 0,
    "rows_scanned": 17194,
    "target_rows_seen": 4317,
    "valid_scores": 4317
  },
  "shadow_lifecycle": {
    "exists": true,
    "path": "logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/shadow_lifecycle.jsonl",
    "record_type_counts": {
      "exit_blocked": 28,
      "exit_filled": 4313,
      "position_closed": 4341,
      "shadow_dispatch": 5060
    },
    "records_with_final_pnl_pct": 8626,
    "rows": 13742
  },
  "split": "chronological_terciles_single_run_weak_evidence",
  "base_script_verdict": "INCONCLUSIVE",
  "closure_verdict": "REJECTED_FOR_RUNTIME / INCONCLUSIVE_RESEARCH",
  "verdict": "REJECTED_FOR_RUNTIME / INCONCLUSIVE_RESEARCH"
}
```

## Decision-time field inventory

| field | family | finite_count | coverage | primary_source | note |
| --- | --- | --- | --- | --- | --- |
| buy_count | traction | 4317 | 1 | buy_count | top-level Gatekeeper decision row / MFS tx_intel |
| sol_buy_ratio | traction | 4317 | 1 | sol_buy_ratio | top-level / MFS tx_intel |
| current_market_cap_sol | overextension | 4317 | 1 | current_market_cap_sol | decision snapshot account market cap |
| bonding_progress_pct | overextension | 4317 | 1 | bonding_progress_pct | decision snapshot bonding progress |
| price_change_ratio | overextension | 4317 | 1 | price_change_ratio | top-level Gatekeeper decision row |
| max_single_tx_price_impact_pct_observed | overextension | 4317 | 1 | max_single_tx_price_impact_pct_observed | observed pre-entry price impact cap |
| unique_ratio | organicity | 4317 | 1 | unique_ratio | top-level or tx_intel unique signer ratio |
| hhi | organicity | 4317 | 1 | hhi | top-level / tx_intel concentration |
| top3_signer_volume_ratio | organicity | 4317 | 1 | top3_signer_volume_ratio | preferred PR4 ratio-scale field; observed scale=ratio_0_1 |
| avg_cpi_depth_50tx | execution_toxicity | 4313 | 0.999073 | avg_cpi_depth_50tx | pre-entry alpha fingerprint diagnostic field, not alpha_31100 |
| compute_unit_cluster_dominance | execution_toxicity | 3461 | 0.801714 | compute_unit_cluster_dominance | optional toxicity cap when coverage is adequate |
| dev_tx_ratio | dev_cross_pool_guard | 4317 | 1 | dev_tx_ratio | optional C5 guard |
| dev_volume_ratio | dev_cross_pool_guard | 4317 | 1 | dev_volume_ratio | optional C5 guard |
| signer_cross_pool_velocity | dev_cross_pool_guard | 4317 | 1 | signer_cross_pool_velocity | optional C5 guard when coverage is adequate |
| cpv_other_pool_activity | dev_cross_pool_guard | 4317 | 1 | cpv_other_pool_activity | optional C5 guard when coverage is adequate |
| flipper_presence_ratio | dev_cross_pool_guard | 3264 | 0.756081 | flipper_presence_ratio | optional C5 guard |

Full inventory is written to CSV.

## Candidate ladder

- `S0`: clean joined `shadow_exit_replay_v1` acted/broad sampler cohort.
- `S1_F5`: `current_market_cap_sol >= 30.2`, `bonding_progress_pct >= 36.5`, `price_change_ratio >= 1.012`, `buy_count >= 8`, `sol_buy_ratio >= 0.520`.
- `C1`: S1 + train-only anti-overextension caps.
- `C2`: C1 + train-only low execution-toxicity caps.
- `C3`: C2 + train-only organic broadening floor.
- `C4`: C3 + train-only concentration guard.
- `C5`: C4 + optional train-only dev/cross-pool guards when decision-time coverage is adequate.

## Threshold source

Thresholds are distribution cuts from the chronological train segment only; no final outcome or holdout metric is used for threshold selection.

| field | stage | direction | quantile | threshold | train_s1_coverage | used | source |
| --- | --- | --- | --- | --- | --- | --- | --- |
| current_market_cap_sol | C1 | cap | 0.75 | 78.0263 | 1 | True | train_s1_distribution_cut |
| bonding_progress_pct | C1 | cap | 0.75 | 60 | 1 | True | train_s1_distribution_cut |
| price_change_ratio | C1 | cap | 0.75 | 1.82959 | 1 | True | train_s1_distribution_cut |
| avg_cpi_depth_50tx | C2 | cap | 0.75 | 2.62791 | 1 | True | train_s1_distribution_cut |
| max_single_tx_price_impact_pct_observed | C2 | cap | 0.75 | 66.5047 | 1 | True | train_s1_distribution_cut |
| compute_unit_cluster_dominance | C2 | cap | 0.75 | 0.445312 | 0.888889 | True | train_s1_distribution_cut |
| unique_ratio | C3 | floor | 0.25 | 0.5 | 1 | True | train_s1_distribution_cut |
| hhi | C4 | cap | 0.75 | 0.0619835 | 1 | True | train_s1_distribution_cut |
| top3_signer_volume_ratio | C4 | cap | 0.75 | 0.536819 | 1 | True | train_s1_distribution_cut |
| top3_volume_pct | C4 | cap | 0.75 | 0.536819 | 1 | False | train_s1_distribution_cut |
| dev_tx_ratio | C5 | cap | 0.75 | 0.0526316 | 1 | True | train_s1_distribution_cut |
| dev_volume_ratio | C5 | cap | 0.75 | 0.165852 | 1 | True | train_s1_distribution_cut |
| signer_cross_pool_velocity | C5 | cap | 0.75 | 0.552239 | 1 | True | train_s1_distribution_cut |
| cpv_other_pool_activity | C5 | cap | 0.75 | 1.62963 | 1 | True | train_s1_distribution_cut |
| flipper_presence_ratio | C5 | cap | 0.75 | 0.413793 | 0.86532 | True | train_s1_distribution_cut |

## Summary metrics

| policy | policy_kind | count | retained_pct | selected_target_bps | selected_stop_bps | selected_max_hold_ms | gross_target_rate | gross_stop_rate | gross_timeout_rate | gross_negative_timeout_rate | gross_avg_pnl_bps | gross_median_pnl_bps | gross_sum_pnl_bps | cost100_avg_pnl_bps | cost100_median_pnl_bps | cost100_sum_pnl_bps | cost100_max_consecutive_losses |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| S0 | candidate_ladder | 4317 | 1 | 10000 | -200 | 120000 | 0.0132036 | 0.589761 | 0.397035 | 0.879813 | 69.5685 | -200 | 300327 | -30.4315 | -300 | -131373 | 99 |
| S1_F5 | candidate_ladder | 1154 | 0.267315 | 7500 | -100 | 30000 | 0.0129983 | 0.82669 | 0.160312 | 0.0378378 | 293.943 | -100 | 339210 | 193.943 | -200 | 223810 | 41 |
| C1 | candidate_ladder | 768 | 0.177901 | 7500 | -100 | 30000 | 0.0130208 | 0.811198 | 0.175781 | 0.037037 | 305.465 | -100 | 234597 | 205.465 | -200 | 157797 | 31 |
| C2 | candidate_ladder | 323 | 0.0748205 | 10000 | -200 | 40000 | 0.00928793 | 0.752322 | 0.23839 | 0.233766 | 377.576 | -200 | 121957 | 277.576 | -300 | 89657 | 19 |
| C3 | candidate_ladder | 273 | 0.0632384 | 10000 | -200 | 40000 | 0.00732601 | 0.74359 | 0.249084 | 0.25 | 359.813 | -200 | 98229 | 259.813 | -300 | 70929 | 16 |
| C4 | candidate_ladder | 203 | 0.0470234 | 10000 | -200 | 30000 | 0.00492611 | 0.743842 | 0.251232 | 0.117647 | 352.32 | -200 | 71521 | 252.32 | -300 | 51221 | 13 |
| C5 | candidate_ladder | 60 | 0.0138985 | 10000 | -200 | 30000 | 0.0166667 | 0.716667 | 0.266667 | 0 | 426.183 | -200 | 25571 | 326.183 | -300 | 19571 | 8 |
| SEL_EQ_S1_F5 | diagnostic_selector_shadow_score_equal_count | 1154 | 0.267315 | 7500 | -100 | 90000 | 0.0329289 | 0.898614 | 0.0684575 | 0.0632911 | 304.389 | -100 | 351265 | 204.389 | -200 | 235865 | 60 |
| SEL_EQ_C1 | diagnostic_selector_shadow_score_equal_count | 768 | 0.177901 | 6000 | -100 | 40000 | 0.03125 | 0.839844 | 0.128906 | 0.0606061 | 323.384 | -100 | 248359 | 223.384 | -200 | 171559 | 37 |
| SEL_EQ_C2 | diagnostic_selector_shadow_score_equal_count | 323 | 0.0748205 | 6000 | -100 | 40000 | 0.0247678 | 0.866873 | 0.108359 | 0.0571429 | 330.263 | -100 | 106675 | 230.263 | -200 | 74375 | 36 |
| SEL_EQ_C3 | diagnostic_selector_shadow_score_equal_count | 273 | 0.0632384 | 6000 | -100 | 40000 | 0.025641 | 0.875458 | 0.0989011 | 0.037037 | 347.403 | -100 | 94841 | 247.403 | -200 | 67541 | 30 |
| SEL_EQ_C4 | diagnostic_selector_shadow_score_equal_count | 203 | 0.0470234 | 10000 | -100 | 10000 | 0 | 0.630542 | 0.369458 | 0.08 | 312.039 | -100 | 63344 | 212.039 | -200 | 43044 | 14 |
| SEL_EQ_C5 | diagnostic_selector_shadow_score_equal_count | 60 | 0.0138985 | 10000 | -200 | 40000 | 0 | 0.816667 | 0.183333 | 0 | 337.4 | -200 | 20244 | 237.4 | -300 | 14244 | 17 |

## Stability

| policy | segment | count | gross_target_rate | gross_stop_rate | gross_negative_timeout_rate | gross_avg_pnl_bps | cost100_avg_pnl_bps | cost100_sum_pnl_bps | cost100_max_consecutive_losses |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| S0 | train | 1439 | 0.0145935 | 0.562891 | 0.873355 | 83.1487 | -16.8513 | -24249 | 61 |
| S0 | validation | 1439 | 0.0132036 | 0.587213 | 0.89913 | 59.3231 | -40.6769 | -58534 | 99 |
| S0 | holdout | 1439 | 0.0118138 | 0.61918 | 0.86629 | 66.2335 | -33.7665 | -48590 | 96 |
| S1_F5 | train | 297 | 0.023569 | 0.767677 | 0.0483871 | 431.562 | 331.562 | 98474 | 25 |
| S1_F5 | validation | 399 | 0.0075188 | 0.879699 | 0.0444444 | 197.569 | 97.5689 | 38930 | 41 |
| S1_F5 | holdout | 458 | 0.010917 | 0.818777 | 0.025641 | 288.659 | 188.659 | 86406 | 26 |
| C1 | train | 201 | 0.0348259 | 0.726368 | 0.0416667 | 495.557 | 395.557 | 79507 | 18 |
| C1 | validation | 261 | 0 | 0.881226 | 0.0322581 | 181.146 | 81.1456 | 21179 | 31 |
| C1 | holdout | 306 | 0.00980392 | 0.80719 | 0.0357143 | 286.637 | 186.637 | 57111 | 28 |
| C2 | train | 100 | 0.03 | 0.68 | 0.241379 | 591.88 | 491.88 | 49188 | 12 |
| C2 | validation | 105 | 0 | 0.780952 | 0.304348 | 226.724 | 126.724 | 13306 | 18 |
| C2 | holdout | 118 | 0 | 0.788136 | 0.16 | 330.195 | 230.195 | 27163 | 17 |
| C3 | train | 87 | 0.0229885 | 0.666667 | 0.222222 | 580.954 | 480.954 | 41843 | 9 |
| C3 | validation | 91 | 0 | 0.769231 | 0.333333 | 232.67 | 132.67 | 12073 | 16 |
| C3 | holdout | 95 | 0 | 0.789474 | 0.2 | 279.084 | 179.084 | 17013 | 15 |
| C4 | train | 67 | 0.0149254 | 0.686567 | 0.1 | 479.493 | 379.493 | 25426 | 5 |
| C4 | validation | 63 | 0 | 0.777778 | 0.214286 | 206.683 | 106.683 | 6721 | 13 |
| C4 | holdout | 73 | 0 | 0.767123 | 0.0588235 | 361.288 | 261.288 | 19074 | 11 |
| C5 | train | 16 | 0.0625 | 0.625 | 0 | 1176.44 | 1076.44 | 17223 | 4 |
| C5 | validation | 24 | 0 | 0.791667 | 0 | -33.3333 | -133.333 | -3200 | 6 |
| C5 | holdout | 20 | 0 | 0.7 | 0 | 377.4 | 277.4 | 5548 | 6 |
| SEL_EQ_S1_F5 | train | 363 | 0.0358127 | 0.900826 | 0 | 337.259 | 237.259 | 86125 | 56 |
| SEL_EQ_S1_F5 | validation | 396 | 0.030303 | 0.919192 | 0.15 | 256.313 | 156.313 | 61900 | 60 |
| SEL_EQ_S1_F5 | holdout | 395 | 0.0329114 | 0.875949 | 0.0555556 | 322.38 | 222.38 | 87840 | 37 |
| SEL_EQ_C1 | train | 262 | 0.0343511 | 0.858779 | 0 | 312.931 | 212.931 | 55788 | 22 |
| SEL_EQ_C1 | validation | 251 | 0.0318725 | 0.856574 | 0.107143 | 304.243 | 204.243 | 51265 | 37 |
| SEL_EQ_C1 | holdout | 255 | 0.027451 | 0.803922 | 0.0697674 | 352.965 | 252.965 | 64506 | 26 |
| SEL_EQ_C2 | train | 117 | 0.025641 | 0.888889 | 0 | 287.316 | 187.316 | 21916 | 36 |
| SEL_EQ_C2 | validation | 116 | 0.0344828 | 0.862069 | 0.166667 | 353.784 | 253.784 | 29439 | 21 |
| SEL_EQ_C2 | holdout | 90 | 0.0111111 | 0.844444 | 0 | 355.778 | 255.778 | 23020 | 11 |
| SEL_EQ_C3 | train | 97 | 0.0309278 | 0.896907 | 0 | 327.144 | 227.144 | 22033 | 28 |
| SEL_EQ_C3 | validation | 100 | 0.03 | 0.88 | 0.111111 | 305.6 | 205.6 | 20560 | 30 |
| SEL_EQ_C3 | holdout | 76 | 0.0131579 | 0.842105 | 0 | 428.263 | 328.263 | 24948 | 14 |
| SEL_EQ_C4 | train | 80 | 0 | 0.625 | 0.0666667 | 291.137 | 191.137 | 15291 | 14 |
| SEL_EQ_C4 | validation | 65 | 0 | 0.753846 | 0.1875 | 158.215 | 58.2154 | 3784 | 13 |
| SEL_EQ_C4 | holdout | 58 | 0 | 0.5 | 0.0344828 | 513.259 | 413.259 | 23969 | 7 |
| SEL_EQ_C5 | train | 22 | 0 | 0.818182 | 0 | 316.591 | 216.591 | 4765 | 6 |
| SEL_EQ_C5 | validation | 25 | 0 | 0.84 | 0 | 263.28 | 163.28 | 4082 | 16 |
| SEL_EQ_C5 | holdout | 13 | 0 | 0.769231 | 0 | 515.154 | 415.154 | 5397 | 6 |

## Tail-source audit: S1/F5 and C1

Audit cell: selected F5/C1 exit `target_bps=7500`, `stop_bps=-100`, `max_hold_ms=30000`, `roundtrip_cost_bps=100`.

### Cost100 contribution by outcome

| policy | segment | count | sum | avg | median | target_count | target_sum | stop_count | stop_sum | timeout_count | timeout_sum |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| S1_F5 | all | 1154 | 223810 | 193.943 | -200 | 15 | 111000 | 954 | -190800 | 185 | 303610 |
| S1_F5 | holdout | 458 | 86406 | 188.659 | -200 | 5 | 37000 | 375 | -75000 | 78 | 124406 |
| C1 | all | 768 | 157797 | 205.465 | -200 | 10 | 74000 | 623 | -124600 | 135 | 208397 |
| C1 | holdout | 306 | 57111 | 186.637 | -200 | 3 | 22200 | 247 | -49400 | 56 | 84311 |

Interpretation:

- For S1/F5, TimeOuts contribute `303610` bps on all rows and `124406` bps on holdout, while Stops remove `-190800` / `-75000` bps.
- For C1, TimeOuts contribute `208397` bps on all rows and `84311` bps on holdout, while Stops remove `-124600` / `-49400` bps.
- The positive avg is therefore not mostly a Target precision story. It is mainly a positive-TimeOut/right-tail story.

### Right-tail dependency

| policy | segment | top_pct | top_k | top_sum | top_labels | rest_sum | rest_avg | rest_median |
| --- | --- | ---: | ---: | ---: | --- | ---: | ---: | ---: |
| S1_F5 | all | 1% | 12 | 88800 | `TARGET=12` | 135010 | 118.222 | -200 |
| S1_F5 | all | 5% | 58 | 290890 | `TARGET=15,TIMEOUT=43` | -67080 | -61.204 | -200 |
| S1_F5 | all | 10% | 116 | 391819 | `TARGET=15,TIMEOUT=101` | -168009 | -161.858 | -200 |
| S1_F5 | holdout | 1% | 5 | 37000 | `TARGET=5` | 49406 | 109.064 | -200 |
| S1_F5 | holdout | 5% | 23 | 111718 | `TARGET=5,TIMEOUT=18` | -25312 | -58.189 | -200 |
| S1_F5 | holdout | 10% | 46 | 153428 | `TARGET=5,TIMEOUT=41` | -67022 | -162.675 | -200 |
| C1 | all | 1% | 8 | 59200 | `TARGET=8` | 98597 | 129.733 | -200 |
| C1 | all | 5% | 39 | 198546 | `TARGET=10,TIMEOUT=29` | -40749 | -55.897 | -200 |
| C1 | all | 10% | 77 | 264438 | `TARGET=10,TIMEOUT=67` | -106641 | -154.329 | -200 |
| C1 | holdout | 1% | 4 | 28918 | `TARGET=3,TIMEOUT=1` | 28193 | 93.354 | -200 |
| C1 | holdout | 5% | 16 | 78508 | `TARGET=3,TIMEOUT=13` | -21397 | -73.783 | -200 |
| C1 | holdout | 10% | 31 | 102114 | `TARGET=3,TIMEOUT=28` | -45003 | -163.647 | -200 |

Interpretation:

- Removing top 5% turns the remaining S1/F5 cohort negative both overall and on holdout.
- Removing top 5% turns the remaining C1 cohort negative both overall and on holdout.
- Top 10% contributes more than the entire net sum in both S1/F5 and C1; the remaining 90% is materially negative.
- The largest wins are a small mixture of Targets and positive TimeOuts. The TimeOut component is required for the positive net result.

### Winner profile vs rest

Top winner bucket is top 5% by cost100 replay PnL.

| policy | bucket | count | current_market_cap_sol_median | price_change_ratio_median | buy_count_median | unique_signers_median | hhi_median | top3_signer_volume_ratio_median | sol_buy_ratio_median |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| S1_F5 | top5pct | 58 | 59.898 | 1.4797 | 62 | 50.5 | 0.0300 | 0.3506 | 0.6525 |
| S1_F5 | rest | 1096 | 56.043 | 1.3978 | 43 | 35 | 0.0374 | 0.3932 | 0.6602 |
| C1 | top5pct | 39 | 49.902 | 1.2245 | 51 | 42 | 0.0354 | 0.3707 | 0.6328 |
| C1 | rest | 729 | 47.642 | 1.2263 | 31 | 27 | 0.0454 | 0.4406 | 0.6651 |

Winner profile note:

- Winners are somewhat broader and less concentrated than the rest, but that does not translate into a stable organic policy: C1 does not beat F5 on holdout avg/sum, C2-C5 lose Target on holdout, and all medians remain negative after costs.
- This profile is useful only as a tail-source diagnostic. It is not a runtime candidate and must not be converted into another threshold-tuning pass on R48/R2.

## Exit matrix and cost sensitivity

- Full identical Target/Stop/max_hold matrix: `reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_exit_matrix.csv`
- Cost sensitivity at `[0, 50, 100, 150, 200]` bps: `reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_cost_sensitivity.csv`
- Stability by chronological tercile for train-selected exits: `reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_stability.csv`

## Acceptance verdict

`REJECTED_FOR_RUNTIME / INCONCLUSIVE_RESEARCH`

Blockers before runtime:
- C1: no full F5 beat (mix=True, cost100=False, nonnegative_segments=3/3)
- C2: no full F5 beat (mix=False, cost100=False, nonnegative_segments=3/3)
- C3: no full F5 beat (mix=False, cost100=False, nonnegative_segments=3/3)
- C4: no full F5 beat (mix=False, cost100=False, nonnegative_segments=3/3)
- C5: retained cohort too small (60)
- C1 does not beat F5 on holdout cost100 avg or sum.
- C2-C5 have 0% Target on holdout.
- All cost-adjusted medians are negative.
- Positive avg depends on sparse right tail, positive TimeOuts, and large hits, not stable precision.
- Organic edge gate is not met.
- Runtime gate remains closed: this proof does not recommend Gatekeeper, selector, V3, sender, or live-execution changes.
- No runtime proposal follows from PR-ORG-A0.

## Generated outputs

| artifact | path |
| --- | --- |
| summary | reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_summary.csv |
| exit_matrix | reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_exit_matrix.csv |
| cost_sensitivity | reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_cost_sensitivity.csv |
| stability | reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_stability.csv |
| inventory | reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_inventory.csv |
| thresholds | reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_thresholds.csv |
| report | PLANS/AUDYT/RAPORT_ORGANIC_POOL_CANDIDATE_POLICY_A0_20260626.md |
