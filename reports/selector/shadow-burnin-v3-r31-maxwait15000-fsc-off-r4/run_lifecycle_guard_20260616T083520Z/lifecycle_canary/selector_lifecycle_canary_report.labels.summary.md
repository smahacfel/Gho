# P3.7 Shadow Lifecycle Labels

Source: `/root/Gho/reports/selector/shadow-burnin-v3-r31-maxwait15000-fsc-off-r4/run_lifecycle_guard_20260616T083520Z/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
Output: `/root/Gho/reports/selector/shadow-burnin-v3-r31-maxwait15000-fsc-off-r4/run_lifecycle_guard_20260616T083520Z/lifecycle_canary/selector_lifecycle_canary_report.labels.jsonl`
Phase F label status: `accepted`

## Counts

- `rows_total`: `37`
- `all_lifecycle_rows`: `37`
- `analysis_status_counts`: `{"ok": 37}`
- `truth_status_counts`: `{"resolved": 37}`
- `market_outcome_class_counts`: `{"market_bad_clean": 27, "market_good_clean": 10}`
- `execution_verification_class_counts`: `{"shadow_onchain_speculative_snapshot_verified": 37}`
- `execution_verification_class_hint_counts`: `{"shadow_onchain_speculative_snapshot_verified": 37}`
- `truth_gap_class_counts`: `{"truth_gap_clean": 15, "truth_gap_degraded_acceptable": 19, "truth_gap_too_large": 3}`
- `entry_truth_gap_class_counts`: `{"truth_gap_clean": 23, "truth_gap_degraded_acceptable": 11, "truth_gap_too_large": 3}`
- `exit_truth_gap_class_counts`: `{"truth_gap_clean": 17, "truth_gap_degraded_acceptable": 20}`
- `buy_quality_class_counts`: `{"buy_quality_bad": 27, "buy_quality_dirty_good": 9, "buy_quality_unknown": 1}`
- `buy_quality_denominator_rows`: `37`
- `execution_feasibility_reject_rows`: `0`
- `execution_feasibility_status_counts`: `{"unknown": 37}`
- `execution_feasibility_reason_counts`: `{"unknown": 37}`
- `label_quality_counts`: `{"degraded": 36, "unknown": 1}`
- `close_reason_counts`: `{"StopLoss": 9, "Target": 8, "TimeStop": 20}`
- `curve_finality_entry_counts`: `{"speculative": 37}`
- `curve_finality_exit_counts`: `{"speculative": 37}`
- `truth_dataset_kind_counts`: `{"shadow_burnin_lifecycle_onchain": 37}`
- `collection_plane_counts`: `{"active_shadow": 37}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 37, "no_gatekeeper_context_rows": 0}`
- `close_reason_by_buy_quality`: `{"StopLoss": {"buy_quality_bad": 9}, "Target": {"buy_quality_dirty_good": 7, "buy_quality_unknown": 1}, "TimeStop": {"buy_quality_bad": 18, "buy_quality_dirty_good": 2}}`
- `degraded_reason_counts`: `{"entry_drift_degraded": 9, "entry_truth_gap_degraded_acceptable": 11, "entry_truth_gap_too_large": 3, "exit_truth_gap_degraded_acceptable": 20, "speculative_curve_finality": 37}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 37, "max": 14739.0, "mean": 3051.0, "min": 27.0, "p50": 701.0, "p90": 8307.199999999999, "p99": 14699.04}`
- `exit_truth_gap_ms`: `{"count": 37, "max": 41763.0, "mean": 16929.297297297297, "min": 0.0, "p50": 30028.0, "p90": 30591.0, "p99": 40517.76}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 37, "max": 74.9480594982576, "mean": 8.752035704400527, "min": 0.0, "p50": 1.0887577441787588, "p90": 27.99578605558746, "p99": 63.37110625375125}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 37, "max": 5.371486644367707e-05, "mean": 1.5754612518774395e-05, "min": 0.0, "p50": 1.5136827824502319e-05, "p90": 2.215848454500957e-05, "p99": 4.3423918054230646e-05}`
- `decision_to_execution_ms`: `{"count": 37, "max": 4842.0, "mean": 903.7297297297297, "min": 574.0, "p50": 661.0, "p90": 975.3999999999999, "p99": 4309.200000000001}`
- `detection_to_execution_ms`: `{"count": 37, "max": 19875.0, "mean": 16025.297297297297, "min": 15602.0, "p50": 15685.0, "p90": 16866.6, "p99": 19338.24}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
