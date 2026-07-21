# P3.7 Shadow Lifecycle Labels

Source: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-het-pm-v2-authoritative-20260719-retry5/run_lifecycle_guard_20260719T223505Z/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
Output: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-het-pm-v2-authoritative-20260719-retry5/run_lifecycle_guard_20260719T223505Z/lifecycle_canary/selector_lifecycle_canary_report.labels.jsonl`
Phase F label status: `not_accepted`

## Counts

- `rows_total`: `5`
- `all_lifecycle_rows`: `5`
- `analysis_status_counts`: `{"ok": 5}`
- `truth_status_counts`: `{"resolved": 5}`
- `market_outcome_class_counts`: `{"market_bad_clean": 5}`
- `execution_verification_class_counts`: `{"shadow_onchain_speculative_snapshot_verified": 5}`
- `execution_verification_class_hint_counts`: `{"shadow_onchain_speculative_snapshot_verified": 5}`
- `truth_gap_class_counts`: `{"truth_gap_clean": 4, "truth_gap_too_large": 1}`
- `entry_truth_gap_class_counts`: `{"truth_gap_clean": 4, "truth_gap_too_large": 1}`
- `exit_truth_gap_class_counts`: `{"truth_gap_clean": 5}`
- `buy_quality_class_counts`: `{"buy_quality_bad": 5}`
- `buy_quality_denominator_rows`: `5`
- `execution_feasibility_reject_rows`: `0`
- `execution_feasibility_status_counts`: `{"unknown": 5}`
- `execution_feasibility_reason_counts`: `{"unknown": 5}`
- `label_quality_counts`: `{"degraded": 5}`
- `close_reason_counts`: `{"StopLoss": 1, "Target": 4}`
- `curve_finality_entry_counts`: `{"speculative": 5}`
- `curve_finality_exit_counts`: `{"speculative": 5}`
- `truth_dataset_kind_counts`: `{"shadow_burnin_lifecycle_onchain": 5}`
- `collection_plane_counts`: `{"active_shadow": 5}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 2, "no_gatekeeper_context_rows": 3}`
- `close_reason_by_buy_quality`: `{"StopLoss": {"buy_quality_bad": 1}, "Target": {"buy_quality_bad": 4}}`
- `degraded_reason_counts`: `{"entry_drift_degraded": 2, "entry_truth_gap_too_large": 1, "missing_gatekeeper_buy_context": 3, "speculative_curve_finality": 5}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 5, "max": 35139.0, "mean": 7268.6, "min": 74.0, "p50": 182.0, "p90": 21423.4, "p99": 33767.44}`
- `exit_truth_gap_ms`: `{"count": 5, "max": 2114.0, "mean": 805.4, "min": 0.0, "p50": 0.0, "p90": 2033.6, "p99": 2105.96}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 5, "max": 59.41116158762454, "mean": 18.594430930232487, "min": 0.0, "p50": 14.942576711435219, "p90": 42.30482127175583, "p99": 57.70052755603767}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 5, "max": 4.6885007010111224e-05, "mean": 2.228403826798342e-05, "min": 1.505395865741832e-05, "p50": 1.656991768239635e-05, "p90": 3.485408988534999e-05, "p99": 4.56819152976351e-05}`
- `decision_to_execution_ms`: `{"count": 5, "max": 9378.0, "mean": 7864.2, "min": 7315.0, "p50": 7420.0, "p90": 8764.8, "p99": 9316.68}`
- `detection_to_execution_ms`: `{"count": 2, "max": 75585.0, "mean": 74618.5, "min": 73652.0, "p50": 74618.5, "p90": 75391.7, "p99": 75565.67}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
