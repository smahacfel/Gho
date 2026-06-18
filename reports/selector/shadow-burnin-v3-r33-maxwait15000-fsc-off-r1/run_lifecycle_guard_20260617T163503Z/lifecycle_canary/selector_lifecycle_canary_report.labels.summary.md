# P3.7 Shadow Lifecycle Labels

Source: `/root/Gho/reports/selector/shadow-burnin-v3-r33-maxwait15000-fsc-off-r1/run_lifecycle_guard_20260617T163503Z/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
Output: `/root/Gho/reports/selector/shadow-burnin-v3-r33-maxwait15000-fsc-off-r1/run_lifecycle_guard_20260617T163503Z/lifecycle_canary/selector_lifecycle_canary_report.labels.jsonl`
Phase F label status: `accepted`

## Counts

- `rows_total`: `2`
- `all_lifecycle_rows`: `2`
- `analysis_status_counts`: `{"ok": 2}`
- `truth_status_counts`: `{"resolved": 2}`
- `market_outcome_class_counts`: `{"market_bad_clean": 1, "market_good_clean": 1}`
- `execution_verification_class_counts`: `{"shadow_onchain_speculative_snapshot_verified": 2}`
- `execution_verification_class_hint_counts`: `{"shadow_onchain_speculative_snapshot_verified": 2}`
- `truth_gap_class_counts`: `{"truth_gap_clean": 1, "truth_gap_degraded_acceptable": 1}`
- `entry_truth_gap_class_counts`: `{"truth_gap_clean": 1, "truth_gap_degraded_acceptable": 1}`
- `exit_truth_gap_class_counts`: `{"truth_gap_clean": 2}`
- `buy_quality_class_counts`: `{"buy_quality_bad": 1, "buy_quality_dirty_good": 1}`
- `buy_quality_denominator_rows`: `2`
- `execution_feasibility_reject_rows`: `0`
- `execution_feasibility_status_counts`: `{"unknown": 2}`
- `execution_feasibility_reason_counts`: `{"unknown": 2}`
- `label_quality_counts`: `{"degraded": 2}`
- `close_reason_counts`: `{"StopLoss": 1, "Target": 1}`
- `curve_finality_entry_counts`: `{"speculative": 2}`
- `curve_finality_exit_counts`: `{"speculative": 2}`
- `truth_dataset_kind_counts`: `{"shadow_burnin_lifecycle_onchain": 2}`
- `collection_plane_counts`: `{"active_shadow": 2}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 2, "no_gatekeeper_context_rows": 0}`
- `close_reason_by_buy_quality`: `{"StopLoss": {"buy_quality_bad": 1}, "Target": {"buy_quality_dirty_good": 1}}`
- `degraded_reason_counts`: `{"entry_drift_degraded": 1, "entry_truth_gap_degraded_acceptable": 1, "exit_drift_degraded": 1, "speculative_curve_finality": 2}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 2, "max": 1526.0, "mean": 936.5, "min": 347.0, "p50": 936.5, "p90": 1408.1000000000001, "p99": 1514.21}`
- `exit_truth_gap_ms`: `{"count": 2, "max": 305.0, "mean": 152.5, "min": 0.0, "p50": 152.5, "p90": 274.5, "p99": 301.95}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 2, "max": 137.51929356441474, "mean": 68.75964678220737, "min": 0.0, "p50": 68.75964678220737, "p90": 123.76736420797327, "p99": 136.1441006287706}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 2, "max": 5.086469579766684, "mean": 2.5432521165757005, "min": 3.4653384717309166e-05, "p50": 2.5432521165757005, "p90": 4.577826087128487, "p99": 5.035605230502863}`
- `decision_to_execution_ms`: `{"count": 2, "max": 3406.0, "mean": 2335.0, "min": 1264.0, "p50": 2335.0, "p90": 3191.8, "p99": 3384.58}`
- `detection_to_execution_ms`: `{"count": 2, "max": 9089.0, "mean": 7085.0, "min": 5081.0, "p50": 7085.0, "p90": 8688.2, "p99": 9048.92}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
