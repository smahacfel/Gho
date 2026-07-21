# P3.7 Shadow Lifecycle Labels

Source: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a/run_lifecycle_guard_20260718T150247Z/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
Output: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a/run_lifecycle_guard_20260718T150247Z/lifecycle_canary/selector_lifecycle_canary_report.labels.jsonl`
Phase F label status: `accepted`

## Counts

- `rows_total`: `129`
- `all_lifecycle_rows`: `129`
- `analysis_status_counts`: `{"ok": 129}`
- `truth_status_counts`: `{"resolved": 129}`
- `market_outcome_class_counts`: `{"market_bad_clean": 115, "market_good_clean": 14}`
- `execution_verification_class_counts`: `{"shadow_onchain_speculative_snapshot_verified": 129}`
- `execution_verification_class_hint_counts`: `{"shadow_onchain_speculative_snapshot_verified": 129}`
- `truth_gap_class_counts`: `{"truth_gap_clean": 13, "truth_gap_degraded_acceptable": 28, "truth_gap_too_large": 88}`
- `entry_truth_gap_class_counts`: `{"truth_gap_clean": 22, "truth_gap_degraded_acceptable": 19, "truth_gap_too_large": 88}`
- `exit_truth_gap_class_counts`: `{"truth_gap_clean": 20, "truth_gap_degraded_acceptable": 50, "truth_gap_too_large": 59}`
- `buy_quality_class_counts`: `{"buy_quality_bad": 115, "buy_quality_dirty_good": 11, "buy_quality_unknown": 3}`
- `buy_quality_denominator_rows`: `129`
- `execution_feasibility_reject_rows`: `0`
- `execution_feasibility_status_counts`: `{"unknown": 129}`
- `execution_feasibility_reason_counts`: `{"unknown": 129}`
- `label_quality_counts`: `{"degraded": 126, "unknown": 3}`
- `close_reason_counts`: `{"StopLoss": 9, "Target": 6, "TimeStop": 114}`
- `curve_finality_entry_counts`: `{"speculative": 129}`
- `curve_finality_exit_counts`: `{"speculative": 129}`
- `truth_dataset_kind_counts`: `{"shadow_burnin_lifecycle_onchain": 129}`
- `collection_plane_counts`: `{"active_shadow": 129}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 67, "no_gatekeeper_context_rows": 62}`
- `close_reason_by_buy_quality`: `{"StopLoss": {"buy_quality_bad": 9}, "Target": {"buy_quality_dirty_good": 5, "buy_quality_unknown": 1}, "TimeStop": {"buy_quality_bad": 106, "buy_quality_dirty_good": 6, "buy_quality_unknown": 2}}`
- `degraded_reason_counts`: `{"entry_drift_degraded": 8, "entry_truth_gap_degraded_acceptable": 19, "entry_truth_gap_too_large": 88, "exit_truth_gap_degraded_acceptable": 50, "exit_truth_gap_too_large": 59, "missing_gatekeeper_buy_context": 62, "speculative_curve_finality": 129}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 129, "max": 74604.0, "mean": 26879.565891472866, "min": 19.0, "p50": 26096.0, "p90": 57069.2, "p99": 71243.84}`
- `exit_truth_gap_ms`: `{"count": 129, "max": 104873.0, "mean": 45083.38759689922, "min": 0.0, "p50": 40580.0, "p90": 84655.2, "p99": 101604.95999999999}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 129, "max": 301.4595772396568, "mean": 5.9224495676385125, "min": 0.0, "p50": 0.0, "p90": 10.638299676940292, "p99": 88.49232313399924}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 129, "max": 2.9969210462900797, "mean": 0.058127938233528485, "min": 0.0, "p50": 1.4582501517423907e-05, "p90": 3.403565562054567e-05, "p99": 2.393635497849603}`
- `decision_to_execution_ms`: `{"count": 129, "max": 17153.0, "mean": 8671.573643410853, "min": 2267.0, "p50": 7945.0, "p90": 11338.0, "p99": 15886.32}`
- `detection_to_execution_ms`: `{"count": 67, "max": 83526.0, "mean": 75562.73134328358, "min": 72482.0, "p50": 74639.0, "p90": 78687.0, "p99": 83284.44}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
