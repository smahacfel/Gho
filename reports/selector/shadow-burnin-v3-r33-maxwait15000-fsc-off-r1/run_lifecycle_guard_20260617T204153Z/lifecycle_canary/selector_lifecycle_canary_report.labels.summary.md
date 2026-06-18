# P3.7 Shadow Lifecycle Labels

Source: `/root/Gho/reports/selector/shadow-burnin-v3-r33-maxwait15000-fsc-off-r1/run_lifecycle_guard_20260617T204153Z/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
Output: `/root/Gho/reports/selector/shadow-burnin-v3-r33-maxwait15000-fsc-off-r1/run_lifecycle_guard_20260617T204153Z/lifecycle_canary/selector_lifecycle_canary_report.labels.jsonl`
Phase F label status: `not_accepted`

## Counts

- `rows_total`: `36`
- `all_lifecycle_rows`: `36`
- `analysis_status_counts`: `{"ok": 36}`
- `truth_status_counts`: `{"resolved": 36}`
- `market_outcome_class_counts`: `{"market_bad_clean": 35, "market_good_clean": 1}`
- `execution_verification_class_counts`: `{"shadow_onchain_speculative_snapshot_verified": 36}`
- `execution_verification_class_hint_counts`: `{"shadow_onchain_speculative_snapshot_verified": 36}`
- `truth_gap_class_counts`: `{"truth_gap_clean": 4, "truth_gap_degraded_acceptable": 6, "truth_gap_too_large": 26}`
- `entry_truth_gap_class_counts`: `{"truth_gap_clean": 5, "truth_gap_degraded_acceptable": 5, "truth_gap_too_large": 26}`
- `exit_truth_gap_class_counts`: `{"truth_gap_clean": 6, "truth_gap_degraded_acceptable": 6, "truth_gap_too_large": 24}`
- `buy_quality_class_counts`: `{"buy_quality_bad": 35, "buy_quality_unknown": 1}`
- `buy_quality_denominator_rows`: `36`
- `execution_feasibility_reject_rows`: `0`
- `execution_feasibility_status_counts`: `{"unknown": 36}`
- `execution_feasibility_reason_counts`: `{"unknown": 36}`
- `label_quality_counts`: `{"degraded": 35, "unknown": 1}`
- `close_reason_counts`: `{"StopLoss": 28, "TimeStop": 8}`
- `curve_finality_entry_counts`: `{"speculative": 36}`
- `curve_finality_exit_counts`: `{"speculative": 36}`
- `truth_dataset_kind_counts`: `{"shadow_burnin_lifecycle_onchain": 36}`
- `collection_plane_counts`: `{"active_shadow": 36}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 2, "no_gatekeeper_context_rows": 34}`
- `close_reason_by_buy_quality`: `{"StopLoss": {"buy_quality_bad": 28}, "TimeStop": {"buy_quality_bad": 7, "buy_quality_unknown": 1}}`
- `degraded_reason_counts`: `{"entry_drift_degraded": 31, "entry_truth_gap_degraded_acceptable": 5, "entry_truth_gap_too_large": 26, "exit_drift_degraded": 1, "exit_truth_gap_degraded_acceptable": 6, "exit_truth_gap_too_large": 24, "missing_gatekeeper_buy_context": 34, "speculative_curve_finality": 36}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 36, "max": 142068.0, "mean": 46474.88888888889, "min": 92.0, "p50": 34173.0, "p90": 109255.5, "p99": 136611.84999999998}`
- `exit_truth_gap_ms`: `{"count": 36, "max": 142562.0, "mean": 52126.47222222222, "min": 0.0, "p50": 35405.5, "p90": 117487.0, "p99": 142052.4}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 36, "max": 1859.702800170057, "mean": 343.89139517061744, "min": 7.671938424341618, "p50": 192.26996996263503, "p90": 869.5445000335377, "p99": 1697.3313064500896}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 36, "max": 7.053536040296759, "mean": 0.19599296641845398, "min": 0.0, "p50": 3.8880515623729295e-05, "p90": 0.00016623998231879433, "p99": 4.584898500533825}`
- `decision_to_execution_ms`: `{"count": 36, "max": 140164.0, "mean": 78375.47222222222, "min": 9678.0, "p50": 77881.0, "p90": 132779.0, "p99": 139627.45}`
- `detection_to_execution_ms`: `{"count": 2, "max": 154596.0, "mean": 146531.0, "min": 138466.0, "p50": 146531.0, "p90": 152983.0, "p99": 154434.7}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
