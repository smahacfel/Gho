# TimeStop V2 Counterfactual Report

This is observe-only counterfactual evidence. It is not active-exit proof and not a production promotion.

## Scope and Inputs
- scope: `shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2`
- generated_at: `2026-06-25T23:43:38.052055+00:00`
- recommendation: `TIMESTOP_V2_NO_WINDOWS`

## Coverage
- simulated_positions: `3380`
- positions_with_exit_replay: `2686`
- positions_with_tsv2_windows: `0` (0.00%)
- candidate_positions: `0`
- candidate_before_terminal: `0`
- stale_only_candidate_rate: `0.00%`

## Join Quality
- exact_join_count: `2577`
- fallback_unique_join_count: `104`
- unmatched_exit_replay_count: `0`
- unmatched_lifecycle_position_count: `694`
- duplicate_fallback_key_count: `5`

## TimeStop V2 Status Distribution
```json
{}
```

## Candidate Class Distribution
```json
{
  "no_candidate": 3380
}
```

## Candidate Before Terminal Outcome
```json
{
  "false": 0,
  "true": 0,
  "unknown": 3380
}
```

## Counterfactual Economics vs Baseline Barrier
- active_exit_eligible_positions: `0`
- saved_stop_count: `0`
- targets_cut_by_tsv2: `0`
- timeout_improved_count: `0`
- delta_sum_bps: `0`
- delta_avg_bps: `0.00`

## Matrix: Baseline vs With TimeStop V2
| target_bps | stop_bps | max_hold_ms | total | baseline TARGET/STOP/TIMEOUT | TSV2 exits | pnl_delta_sum_bps | exact/path |
|---:|---:|---:|---:|---|---:|---:|---|
| 6000 | -6000 | 120000 | 2686 | 156/150/2380 | 0 | 0 | 2686/0 |

## False-Close Accounting
```json
{
  "no_candidate": 2686,
  "no_exit_replay": 694
}
```

## Resurrection Checks
```json
{
  "alive_within_4000ms_after_candidate": {
    "alive_count": 0,
    "alive_rate": 0.0,
    "candidate_rows": 0
  },
  "alive_within_8000ms_after_candidate": {
    "alive_count": 0,
    "alive_rate": 0.0,
    "candidate_rows": 0
  }
}
```

## Stale/Missing-Data Safety
- `stale_data_no_action` candidates are excluded from active-exit eligibility.
- Missing or stale TimeStop V2 evidence can support data-quality diagnosis only.

## Recommendation
`TIMESTOP_V2_NO_WINDOWS`
