# P3.7 Shadow Lifecycle Labels

Source: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a/run_lifecycle_guard_20260717T155637Z/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
Output: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a/run_lifecycle_guard_20260717T155637Z/lifecycle_canary/selector_lifecycle_canary_report.labels.jsonl`
Phase F label status: `accepted`

## Counts

- `rows_total`: `79`
- `all_lifecycle_rows`: `79`
- `analysis_status_counts`: `{"ok": 79}`
- `truth_status_counts`: `{"resolved": 79}`
- `market_outcome_class_counts`: `{"market_bad_clean": 71, "market_good_clean": 8}`
- `execution_verification_class_counts`: `{"shadow_onchain_speculative_snapshot_verified": 79}`
- `execution_verification_class_hint_counts`: `{"shadow_onchain_speculative_snapshot_verified": 79}`
- `truth_gap_class_counts`: `{"truth_gap_clean": 9, "truth_gap_degraded_acceptable": 15, "truth_gap_too_large": 55}`
- `entry_truth_gap_class_counts`: `{"truth_gap_clean": 16, "truth_gap_degraded_acceptable": 8, "truth_gap_too_large": 55}`
- `exit_truth_gap_class_counts`: `{"truth_gap_clean": 11, "truth_gap_degraded_acceptable": 34, "truth_gap_too_large": 34}`
- `buy_quality_class_counts`: `{"buy_quality_bad": 71, "buy_quality_dirty_good": 5, "buy_quality_unknown": 3}`
- `buy_quality_denominator_rows`: `79`
- `execution_feasibility_reject_rows`: `0`
- `execution_feasibility_status_counts`: `{"unknown": 79}`
- `execution_feasibility_reason_counts`: `{"unknown": 79}`
- `label_quality_counts`: `{"degraded": 76, "unknown": 3}`
- `close_reason_counts`: `{"StopLoss": 7, "Target": 3, "TimeStop": 69}`
- `curve_finality_entry_counts`: `{"speculative": 79}`
- `curve_finality_exit_counts`: `{"speculative": 79}`
- `truth_dataset_kind_counts`: `{"shadow_burnin_lifecycle_onchain": 79}`
- `collection_plane_counts`: `{"active_shadow": 79}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 48, "no_gatekeeper_context_rows": 31}`
- `close_reason_by_buy_quality`: `{"StopLoss": {"buy_quality_bad": 7}, "Target": {"buy_quality_dirty_good": 2, "buy_quality_unknown": 1}, "TimeStop": {"buy_quality_bad": 64, "buy_quality_dirty_good": 3, "buy_quality_unknown": 2}}`
- `degraded_reason_counts`: `{"entry_drift_degraded": 7, "entry_truth_gap_degraded_acceptable": 8, "entry_truth_gap_too_large": 55, "exit_truth_gap_degraded_acceptable": 34, "exit_truth_gap_too_large": 34, "missing_gatekeeper_buy_context": 31, "speculative_curve_finality": 79}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 79, "max": 69967.0, "mean": 26845.607594936708, "min": 90.0, "p50": 24515.0, "p90": 55444.8, "p99": 69883.54000000001}`
- `exit_truth_gap_ms`: `{"count": 79, "max": 99889.0, "mean": 44454.81012658228, "min": 0.0, "p50": 37549.0, "p90": 83595.2, "p99": 95835.34}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 79, "max": 536.8805980870858, "mean": 12.220871434385254, "min": 0.0, "p50": 0.0, "p90": 13.909334038365076, "p99": 290.812770255343}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 79, "max": 0.3569328215689116, "mean": 0.004533818499901847, "min": 0.0, "p50": 1.4582441987265327e-05, "p90": 2.280871809201559e-05, "p99": 0.07860232327176413}`
- `decision_to_execution_ms`: `{"count": 79, "max": 11850.0, "mean": 8366.481012658227, "min": 6248.0, "p50": 7924.0, "p90": 10445.4, "p99": 11361.719999999998}`
- `detection_to_execution_ms`: `{"count": 48, "max": 81713.0, "mean": 74839.33333333333, "min": 72459.0, "p50": 73912.0, "p90": 77373.8, "p99": 80084.92000000001}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
