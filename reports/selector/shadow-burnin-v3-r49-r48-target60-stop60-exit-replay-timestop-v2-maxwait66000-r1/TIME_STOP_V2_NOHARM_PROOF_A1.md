# TimeStop V2 No-Harm / Action-Precision Proof A1

Generated UTC: `2026-06-26T16:48:01.953398+00:00`
Scope: `shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1`
Final verdict: `INCONCLUSIVE_RESEARCH / REJECTED_FOR_RUNTIME`
No basis for runtime change.
No basis for shadow_close_only plan.
Positive action precision is blocked by target-cut guard.

## PR-ORG-A0 Closure

`DONE / REJECTED_FOR_RUNTIME / INCONCLUSIVE_RESEARCH / KEEP_AS_NEGATIVE_EVIDENCE`

Do not continue ORG-A0 as PR-ORG-A0b, C6/C7/C8, more R48/R2 threshold tuning, organic hard gates, selector reranker, `alpha_31100`, XGBoost, Gatekeeper BUY/REJECT change, or `shadow_close_only` based on ORG-A0.

Reason: ORG-A0 showed that the F5/C1 positive avg came from a sparse right tail, not a stable organic edge. After removing top 5%, S1_F5 and C1 are negative; C1 does not beat F5 on holdout; C2-C5 have 0% Target on holdout; all cost-adjusted medians are negative.

## Scope Boundaries

- Offline/read-only proof only.
- No Gatekeeper runtime change.
- No BUY/REJECT change.
- No `v25_confidence`, V3, selector runtime, `alpha_31100`, TX builder, sender, Jito path, live execution, or existing log mutation.
- This proof evaluates only `exit_action_precision = beneficial_exit / (beneficial_exit + harmful_exit)`.
- It does not use or report entry target precision as an acceptance metric.

## Inputs

```json
{
  "probe_shadow_lifecycle": "/root/Gho/logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/probe_shadow_lifecycle.jsonl",
  "shadow_exit_replay": "/root/Gho/logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/shadow_exit_replay_v1.jsonl",
  "shadow_lifecycle": "/root/Gho/logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/shadow_lifecycle.jsonl"
}
```

## Coverage and Join Quality

```json
{
  "candidate_positions": 3517,
  "exact_join_rate_over_exit_replay": 1.0,
  "join_quality": {
    "duplicate_fallback_key_count": 0,
    "entry_ts_ms_source_counts": {
      "window_timestamp_minus_age": 3594
    },
    "exact_join_count": 3079,
    "fallback_unique_join_count": 0,
    "unmatched_exit_replay_count": 0,
    "unmatched_lifecycle_position_count": 515
  },
  "positions": 3594,
  "positions_with_exit_replay": 3079,
  "positions_with_tsv2_windows": 3594,
  "scope": "shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1",
  "stale_data_no_action_candidates": 8
}
```

## R48/R2 Negative Coverage Control

R48/R2 is used only as a no-window coverage control.

```json
{
  "candidate_positions": 0,
  "input_paths": {
    "probe_shadow_lifecycle": "/root/Gho/logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/probe_shadow_lifecycle.jsonl",
    "shadow_exit_replay": "/root/Gho/logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/shadow_exit_replay_v1.jsonl",
    "shadow_lifecycle": "/root/Gho/logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/shadow_lifecycle.jsonl"
  },
  "join_quality": {
    "duplicate_fallback_key_count": 6,
    "entry_ts_ms_source_counts": {
      "closed_minus_duration": 5552
    },
    "exact_join_count": 4145,
    "fallback_unique_join_count": 190,
    "unmatched_exit_replay_count": 0,
    "unmatched_lifecycle_position_count": 1217
  },
  "load_stats": [
    {
      "malformed_rows": 0,
      "path": "/root/Gho/logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/shadow_exit_replay_v1.jsonl",
      "rows": 4341
    },
    {
      "malformed_rows": 0,
      "path": "/root/Gho/logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/shadow_lifecycle.jsonl",
      "rows": 13742
    },
    {
      "malformed_rows": 0,
      "path": "/root/Gho/logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/probe_shadow_lifecycle.jsonl",
      "rows": 2422
    }
  ],
  "positions": 5558,
  "positions_with_exit_replay": 4341,
  "positions_with_tsv2_windows": 0,
  "scope": "shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2",
  "stale_data_no_action_candidates": 0
}
```

## Resurrection Checks

```json
{
  "alive_within_12000ms_after_candidate": {
    "alive_count": 356,
    "alive_rate": 0.10122263292578902,
    "candidate_rows": 3517
  },
  "alive_within_4000ms_after_candidate": {
    "alive_count": 121,
    "alive_rate": 0.034404321865226045,
    "candidate_rows": 3517
  },
  "alive_within_8000ms_after_candidate": {
    "alive_count": 268,
    "alive_rate": 0.07620130793289735,
    "candidate_rows": 3517
  }
}
```

## Best Variant

- target_bps: `2000`
- stop_bps: `-6000`
- max_hold_ms: `120000`
- selection: max `cost100_delta_sum_bps`, then Wilson lower bound, action precision, and lower target-cut damage.

## Cost Sensitivity for Best Variant

| roundtrip_cost_bps | supported_rows | action_taken_count | delta_sum_bps | delta_avg_bps | delta_median_bps | exit_action_precision | exit_action_precision_wilson95_lower | beneficial_exit_count | harmful_exit_count | target_cut_count | target_cut_damage_bps | saved_stop_count | saved_stop_damage_bps | timeout_improved_count | timeout_improved_bps | stale_no_action_exclusions | no_candidate_exclusions | ambiguous_unjoined_exclusions | exact_rows | path_approx_rows |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 0 | 3079 | 2914 | 448725 | 145.737 | 0 | 0.702415 | 0.678009 | 989 | 419 | 226 | 461016 | 59 | 246158 | 930 | 795329 | 8 | 74 | 0 | 3079 | 0 |
| 50 | 3079 | 2914 | 448725 | 145.737 | 0 | 0.702415 | 0.678009 | 989 | 419 | 226 | 461016 | 59 | 246158 | 930 | 795329 | 8 | 74 | 0 | 3079 | 0 |
| 100 | 3079 | 2914 | 448725 | 145.737 | 0 | 0.702415 | 0.678009 | 989 | 419 | 226 | 461016 | 59 | 246158 | 930 | 795329 | 8 | 74 | 0 | 3079 | 0 |
| 150 | 3079 | 2914 | 448725 | 145.737 | 0 | 0.702415 | 0.678009 | 989 | 419 | 226 | 461016 | 59 | 246158 | 930 | 795329 | 8 | 74 | 0 | 3079 | 0 |
| 200 | 3079 | 2914 | 448725 | 145.737 | 0 | 0.702415 | 0.678009 | 989 | 419 | 226 | 461016 | 59 | 246158 | 930 | 795329 | 8 | 74 | 0 | 3079 | 0 |

## Chronological Stability for Best Variant

| segment | supported_rows | action_taken_count | delta_sum_bps | delta_avg_bps | exit_action_precision | exit_action_precision_wilson95_lower | beneficial_exit_count | harmful_exit_count | max_consecutive_harmful_actions |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| train | 1027 | 982 | 165837 | 161.477 | 0.672584 | 0.630572 | 341 | 166 | 4 |
| validation | 1026 | 968 | 187164 | 182.421 | 0.727483 | 0.683681 | 315 | 118 | 2 |
| holdout | 1026 | 964 | 95724 | 93.2982 | 0.711538 | 0.668902 | 333 | 135 | 3 |

## Grid-Neighborhood Stability

| target_bps | stop_bps | max_hold_ms | is_best | cost100_delta_sum_bps | cost100_delta_avg_bps | cost100_exit_action_precision | cost100_exit_action_precision_wilson95_lower | positive_delta |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1500 | -5000 | 60000 | False | 267611 | 86.9149 | 0.631626 | 0.60288 | True |
| 1500 | -5000 | 120000 | False | 406707 | 132.091 | 0.69764 | 0.672665 | True |
| 1500 | -6000 | 60000 | False | 302538 | 98.2585 | 0.631439 | 0.602859 | True |
| 1500 | -6000 | 120000 | False | 443408 | 144.01 | 0.694668 | 0.669755 | True |
| 2000 | -5000 | 60000 | False | 249123 | 80.9104 | 0.633362 | 0.605122 | True |
| 2000 | -5000 | 120000 | False | 407245 | 132.265 | 0.705376 | 0.680916 | True |
| 2000 | -6000 | 60000 | False | 287050 | 93.2283 | 0.633162 | 0.605079 | True |
| 2000 | -6000 | 120000 | True | 448725 | 145.737 | 0.702415 | 0.678009 | True |
| 3000 | -5000 | 60000 | False | 199813 | 64.8954 | 0.636824 | 0.60903 | True |
| 3000 | -5000 | 120000 | False | 351767 | 114.247 | 0.712587 | 0.688586 | True |
| 3000 | -6000 | 60000 | False | 241426 | 78.4105 | 0.636591 | 0.608947 | True |
| 3000 | -6000 | 120000 | False | 398475 | 129.417 | 0.709633 | 0.68568 | True |

## Verdict

`INCONCLUSIVE_RESEARCH / REJECTED_FOR_RUNTIME`

No basis for runtime change.
No basis for shadow_close_only plan.
Positive action precision is blocked by target-cut guard.

Runtime blockers:
- target_cut_damage_bps > 25% gross_saved_damage_bps
- target_cut_count exceeds saved_stop_count + 10% timeout_improved_count

Shadow-close-only blockers:
- requires minimum two independent TSV2 scopes; only one full TSV2-window scope is available

## Output Files

| artifact | path |
| --- | --- |
| summary | /root/Gho/reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_noharm_summary_v1.csv |
| cost_sensitivity | /root/Gho/reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_noharm_cost_sensitivity_v1.csv |
| stability | /root/Gho/reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_noharm_stability_v1.csv |
| grid_neighborhood | /root/Gho/reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_noharm_grid_neighborhood_v1.csv |
| report | /root/Gho/reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/TIME_STOP_V2_NOHARM_PROOF_A1.md |
