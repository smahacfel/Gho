# P3.7 Shadow Lifecycle Labels

Source: `/root/Gho/reports/selector/shadow-burnin-v3-r31-maxwait15000-fsc-off-r4/run_lifecycle_guard_20260617T015811Z/lifecycle_canary/probe_selector_lifecycle_canary_report.jsonl`
Output: `/root/Gho/reports/selector/shadow-burnin-v3-r31-maxwait15000-fsc-off-r4/run_lifecycle_guard_20260617T015811Z/lifecycle_canary/probe_selector_lifecycle_canary_report.labels.jsonl`
Phase F label status: `accepted`

## Counts

- `rows_total`: `27`
- `all_lifecycle_rows`: `27`
- `analysis_status_counts`: `{"ok": 27}`
- `truth_status_counts`: `{"resolved": 27}`
- `market_outcome_class_counts`: `{"market_bad_clean": 25, "market_good_clean": 2}`
- `execution_verification_class_counts`: `{"shadow_onchain_degraded": 14, "shadow_onchain_speculative_snapshot_verified": 13}`
- `execution_verification_class_hint_counts`: `{"shadow_onchain_snapshot_verified_non_final": 14, "shadow_onchain_speculative_snapshot_verified": 13}`
- `truth_gap_class_counts`: `{"truth_gap_degraded_acceptable": 22, "truth_gap_too_large": 5}`
- `entry_truth_gap_class_counts`: `{"truth_gap_clean": 2, "truth_gap_degraded_acceptable": 20, "truth_gap_too_large": 5}`
- `exit_truth_gap_class_counts`: `{"truth_gap_clean": 1, "truth_gap_degraded_acceptable": 26}`
- `buy_quality_class_counts`: `{"buy_quality_bad": 25, "buy_quality_dirty_good": 2}`
- `buy_quality_denominator_rows`: `27`
- `execution_feasibility_reject_rows`: `0`
- `execution_feasibility_status_counts`: `{"unknown": 27}`
- `execution_feasibility_reason_counts`: `{"unknown": 27}`
- `label_quality_counts`: `{"degraded": 27}`
- `close_reason_counts`: `{"Target": 1, "TimeStop": 26}`
- `curve_finality_entry_counts`: `{"provisional": 16, "speculative": 11}`
- `curve_finality_exit_counts`: `{"provisional": 14, "speculative": 13}`
- `truth_dataset_kind_counts`: `{"shadow_burnin_lifecycle_onchain": 27}`
- `collection_plane_counts`: `{"counterfactual_shadow_probe": 27}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 4, "no_gatekeeper_context_rows": 23}`
- `close_reason_by_buy_quality`: `{"Target": {"buy_quality_dirty_good": 1}, "TimeStop": {"buy_quality_bad": 25, "buy_quality_dirty_good": 1}}`
- `degraded_reason_counts`: `{"entry_truth_gap_degraded_acceptable": 20, "entry_truth_gap_too_large": 5, "exit_truth_gap_degraded_acceptable": 26, "missing_gatekeeper_buy_context": 23, "nonstandard_curve_finality": 14, "speculative_curve_finality": 13}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 27, "max": 11011.0, "mean": 6076.814814814815, "min": 119.0, "p50": 5428.0, "p90": 10439.2, "p99": 10986.56}`
- `exit_truth_gap_ms`: `{"count": 27, "max": 42635.0, "mean": 34892.74074074074, "min": 344.0, "p50": 36701.0, "p90": 41443.600000000006, "p99": 42552.06}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 27, "max": 3.9337834064908317, "mean": 0.20076042217874818, "min": 0.0, "p50": 0.0, "p90": 0.0, "p99": 3.2975541988104076}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 27, "max": 1.946774409455898e-05, "mean": 3.836088469100218e-06, "min": 0.0, "p50": 0.0, "p90": 1.4737982452217581e-05, "p99": 1.85744820084821e-05}`
- `decision_to_execution_ms`: `{"count": 27, "max": 12452.0, "mean": 5443.407407407408, "min": 2947.0, "p50": 3572.0, "p90": 9351.8, "p99": 11747.919999999996}`
- `detection_to_execution_ms`: `{"count": 4, "max": 14448.0, "mean": 9079.25, "min": 6034.0, "p50": 7917.5, "p90": 12635.400000000001, "p99": 14266.739999999998}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
