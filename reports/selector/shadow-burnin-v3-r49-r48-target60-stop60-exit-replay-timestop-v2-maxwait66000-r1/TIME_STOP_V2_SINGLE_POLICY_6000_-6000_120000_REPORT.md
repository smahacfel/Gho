# TimeStop V2 Counterfactual Report

This is observe-only counterfactual evidence. It is not active-exit proof and not a production promotion.

## Scope and Inputs
- scope: `shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1`
- generated_at: `2026-06-26T11:05:34.580476+00:00`
- recommendation: `TIMESTOP_V2_COUNTERFACTUAL_PROMISING`

## Coverage
- simulated_positions: `768`
- positions_with_exit_replay: `648`
- positions_with_tsv2_windows: `768` (100.00%)
- candidate_positions: `753`
- candidate_before_terminal: `631`
- stale_only_candidate_rate: `0.80%`

## Join Quality
- exact_join_count: `648`
- fallback_unique_join_count: `0`
- unmatched_exit_replay_count: `0`
- unmatched_lifecycle_position_count: `120`
- duplicate_fallback_key_count: `0`

## TimeStop V2 Status Distribution
```json
{
  "heartbeat": 32,
  "stale_or_insufficient": 6,
  "weak": 715
}
```

## Candidate Class Distribution
```json
{
  "heartbeat_only_candidate": 3,
  "no_candidate": 15,
  "no_progress_with_volume_candidate": 68,
  "stale_data_no_action": 6,
  "weak_no_progress_candidate": 676
}
```

## Candidate Before Terminal Outcome
```json
{
  "false": 0,
  "true": 631,
  "unknown": 137
}
```

## Counterfactual Economics vs Baseline Barrier
- active_exit_eligible_positions: `625`
- saved_stop_count: `11`
- targets_cut_by_tsv2: `13`
- timeout_improved_count: `226`
- delta_sum_bps: `118578`
- delta_avg_bps: `189.72`

## Matrix: Baseline vs With TimeStop V2
| target_bps | stop_bps | max_hold_ms | total | baseline TARGET/STOP/TIMEOUT | TSV2 exits | pnl_delta_sum_bps | exact/path |
|---:|---:|---:|---:|---|---:|---:|---|
| 6000 | -6000 | 120000 | 648 | 23/15/610 | 625 | 118578 | 648/0 |

## False-Close Accounting
```json
{
  "cut_target": 13,
  "harmful_exit": 82,
  "neutral_exit": 293,
  "no_candidate": 14,
  "no_exit_replay": 120,
  "not_active_exit_eligible": 3,
  "saved_stop": 11,
  "stale_excluded": 6,
  "timeout_improved": 226
}
```

## Resurrection Checks
```json
{
  "alive_within_4000ms_after_candidate": {
    "alive_count": 30,
    "alive_rate": 0.0398406374501992,
    "candidate_rows": 753
  },
  "alive_within_8000ms_after_candidate": {
    "alive_count": 54,
    "alive_rate": 0.07171314741035857,
    "candidate_rows": 753
  }
}
```

## Stale/Missing-Data Safety
- `stale_data_no_action` candidates are excluded from active-exit eligibility.
- Missing or stale TimeStop V2 evidence can support data-quality diagnosis only.

## Recommendation
`TIMESTOP_V2_COUNTERFACTUAL_PROMISING`
