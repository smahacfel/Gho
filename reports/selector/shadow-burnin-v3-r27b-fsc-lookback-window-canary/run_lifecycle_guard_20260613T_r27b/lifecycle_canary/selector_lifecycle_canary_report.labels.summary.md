# P3.7 Shadow Lifecycle Labels

Source: `/root/Gho/reports/selector/shadow-burnin-v3-r27b-fsc-lookback-window-canary/run_lifecycle_guard_20260613T_r27b/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
Output: `/root/Gho/reports/selector/shadow-burnin-v3-r27b-fsc-lookback-window-canary/run_lifecycle_guard_20260613T_r27b/lifecycle_canary/selector_lifecycle_canary_report.labels.jsonl`
Phase F label status: `not_accepted`

## Counts

- `rows_total`: `2`
- `all_lifecycle_rows`: `2`
- `analysis_status_counts`: `{"ok": 2}`
- `truth_status_counts`: `{"resolved": 2}`
- `market_outcome_class_counts`: `{"market_bad_clean": 2}`
- `execution_verification_class_counts`: `{"shadow_onchain_speculative_snapshot_verified": 2}`
- `execution_verification_class_hint_counts`: `{"shadow_onchain_speculative_snapshot_verified": 2}`
- `truth_gap_class_counts`: `{"truth_gap_degraded_acceptable": 2}`
- `entry_truth_gap_class_counts`: `{"truth_gap_degraded_acceptable": 2}`
- `exit_truth_gap_class_counts`: `{"truth_gap_degraded_acceptable": 2}`
- `buy_quality_class_counts`: `{"buy_quality_bad": 2}`
- `buy_quality_denominator_rows`: `2`
- `execution_feasibility_reject_rows`: `0`
- `execution_feasibility_status_counts`: `{"unknown": 2}`
- `execution_feasibility_reason_counts`: `{"unknown": 2}`
- `label_quality_counts`: `{"degraded": 2}`
- `close_reason_counts`: `{"TimeStop": 2}`
- `curve_finality_entry_counts`: `{"speculative": 2}`
- `curve_finality_exit_counts`: `{"speculative": 2}`
- `truth_dataset_kind_counts`: `{"shadow_burnin_lifecycle_onchain": 2}`
- `collection_plane_counts`: `{"active_shadow": 2}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 2, "no_gatekeeper_context_rows": 0}`
- `close_reason_by_buy_quality`: `{"TimeStop": {"buy_quality_bad": 2}}`
- `degraded_reason_counts`: `{"entry_truth_gap_degraded_acceptable": 2, "exit_truth_gap_degraded_acceptable": 2, "speculative_curve_finality": 2}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 2, "max": 6742.0, "mean": 4952.5, "min": 3163.0, "p50": 4952.5, "p90": 6384.1, "p99": 6706.21}`
- `exit_truth_gap_ms`: `{"count": 2, "max": 30358.0, "mean": 30300.5, "min": 30243.0, "p50": 30300.5, "p90": 30346.5, "p99": 30356.85}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 2, "max": 0.0, "mean": 0.0, "min": 0.0, "p50": 0.0, "p90": 0.0, "p99": 0.0}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 2, "max": 1.8809526269869536e-05, "mean": 1.6789305806730326e-05, "min": 1.4769085343591115e-05, "p50": 1.6789305806730326e-05, "p90": 1.8405482177241694e-05, "p99": 1.8769121860606752e-05}`
- `decision_to_execution_ms`: `{"count": 2, "max": 644.0, "mean": 613.5, "min": 583.0, "p50": 613.5, "p90": 637.9, "p99": 643.39}`
- `detection_to_execution_ms`: `{"count": 2, "max": 10677.0, "mean": 10649.5, "min": 10622.0, "p50": 10649.5, "p90": 10671.5, "p99": 10676.449999999999}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
