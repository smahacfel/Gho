# P3.7 Shadow Lifecycle Labels

Source: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-r1b/run_lifecycle_guard_20260717T085914Z/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
Output: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-r1b/run_lifecycle_guard_20260717T085914Z/lifecycle_canary/selector_lifecycle_canary_report.labels.jsonl`
Phase F label status: `accepted`

## Counts

- `rows_total`: `19`
- `all_lifecycle_rows`: `19`
- `analysis_status_counts`: `{"ok": 19}`
- `truth_status_counts`: `{"resolved": 19}`
- `market_outcome_class_counts`: `{"market_bad_clean": 18, "market_good_clean": 1}`
- `execution_verification_class_counts`: `{"shadow_onchain_speculative_snapshot_verified": 19}`
- `execution_verification_class_hint_counts`: `{"shadow_onchain_speculative_snapshot_verified": 19}`
- `truth_gap_class_counts`: `{"truth_gap_clean": 2, "truth_gap_degraded_acceptable": 5, "truth_gap_too_large": 12}`
- `entry_truth_gap_class_counts`: `{"truth_gap_clean": 3, "truth_gap_degraded_acceptable": 4, "truth_gap_too_large": 12}`
- `exit_truth_gap_class_counts`: `{"truth_gap_clean": 2, "truth_gap_degraded_acceptable": 7, "truth_gap_too_large": 10}`
- `buy_quality_class_counts`: `{"buy_quality_bad": 18, "buy_quality_dirty_good": 1}`
- `buy_quality_denominator_rows`: `19`
- `execution_feasibility_reject_rows`: `0`
- `execution_feasibility_status_counts`: `{"unknown": 19}`
- `execution_feasibility_reason_counts`: `{"unknown": 19}`
- `label_quality_counts`: `{"degraded": 19}`
- `close_reason_counts`: `{"StopLoss": 1, "Target": 1, "TimeStop": 17}`
- `curve_finality_entry_counts`: `{"speculative": 19}`
- `curve_finality_exit_counts`: `{"speculative": 19}`
- `truth_dataset_kind_counts`: `{"shadow_burnin_lifecycle_onchain": 19}`
- `collection_plane_counts`: `{"active_shadow": 19}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 6, "no_gatekeeper_context_rows": 13}`
- `close_reason_by_buy_quality`: `{"StopLoss": {"buy_quality_bad": 1}, "Target": {"buy_quality_dirty_good": 1}, "TimeStop": {"buy_quality_bad": 17}}`
- `degraded_reason_counts`: `{"entry_drift_degraded": 2, "entry_truth_gap_degraded_acceptable": 4, "entry_truth_gap_too_large": 12, "exit_truth_gap_degraded_acceptable": 7, "exit_truth_gap_too_large": 10, "missing_gatekeeper_buy_context": 13, "speculative_curve_finality": 19}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 19, "max": 71358.0, "mean": 25120.42105263158, "min": 454.0, "p50": 20090.0, "p90": 52547.799999999996, "p99": 68766.18000000001}`
- `exit_truth_gap_ms`: `{"count": 19, "max": 101525.0, "mean": 50272.42105263158, "min": 382.0, "p50": 49381.0, "p90": 82822.4, "p99": 98960.18000000001}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 19, "max": 242.9072516682116, "mean": 16.36274511069149, "min": 0.0, "p50": 0.0, "p90": 17.637804247389603, "p99": 206.92832805106266}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 19, "max": 2.3194656347737763, "mean": 0.12209101433978335, "min": 0.0, "p50": 1.4586794039317397e-05, "p90": 2.998916972751784e-05, "p99": 1.9019708278690015}`
- `decision_to_execution_ms`: `{"count": 19, "max": 16396.0, "mean": 8239.473684210527, "min": 6266.0, "p50": 7167.0, "p90": 10685.399999999998, "p99": 15898.300000000001}`
- `detection_to_execution_ms`: `{"count": 6, "max": 76188.0, "mean": 73616.5, "min": 72477.0, "p50": 73287.5, "p90": 74782.5, "p99": 76047.45000000001}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
