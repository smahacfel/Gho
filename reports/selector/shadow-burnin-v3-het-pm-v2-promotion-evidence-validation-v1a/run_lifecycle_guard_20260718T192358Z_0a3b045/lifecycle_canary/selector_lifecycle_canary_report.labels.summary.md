# P3.7 Shadow Lifecycle Labels

Source: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a/run_lifecycle_guard_20260718T192358Z_0a3b045/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
Output: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a/run_lifecycle_guard_20260718T192358Z_0a3b045/lifecycle_canary/selector_lifecycle_canary_report.labels.jsonl`
Phase F label status: `accepted`

## Counts

- `rows_total`: `124`
- `all_lifecycle_rows`: `124`
- `analysis_status_counts`: `{"ok": 124}`
- `truth_status_counts`: `{"resolved": 124}`
- `market_outcome_class_counts`: `{"market_bad_clean": 108, "market_good_clean": 16}`
- `execution_verification_class_counts`: `{"shadow_onchain_speculative_snapshot_verified": 124}`
- `execution_verification_class_hint_counts`: `{"shadow_onchain_speculative_snapshot_verified": 124}`
- `truth_gap_class_counts`: `{"truth_gap_clean": 26, "truth_gap_degraded_acceptable": 33, "truth_gap_too_large": 65}`
- `entry_truth_gap_class_counts`: `{"truth_gap_clean": 31, "truth_gap_degraded_acceptable": 28, "truth_gap_too_large": 65}`
- `exit_truth_gap_class_counts`: `{"truth_gap_clean": 35, "truth_gap_degraded_acceptable": 47, "truth_gap_too_large": 42}`
- `buy_quality_class_counts`: `{"buy_quality_bad": 108, "buy_quality_dirty_good": 16}`
- `buy_quality_denominator_rows`: `124`
- `execution_feasibility_reject_rows`: `0`
- `execution_feasibility_status_counts`: `{"unknown": 124}`
- `execution_feasibility_reason_counts`: `{"unknown": 124}`
- `label_quality_counts`: `{"degraded": 124}`
- `close_reason_counts`: `{"StopLoss": 18, "Target": 16, "TimeStop": 90}`
- `curve_finality_entry_counts`: `{"speculative": 124}`
- `curve_finality_exit_counts`: `{"speculative": 124}`
- `truth_dataset_kind_counts`: `{"shadow_burnin_lifecycle_onchain": 124}`
- `collection_plane_counts`: `{"active_shadow": 124}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 47, "no_gatekeeper_context_rows": 77}`
- `close_reason_by_buy_quality`: `{"StopLoss": {"buy_quality_bad": 18}, "Target": {"buy_quality_dirty_good": 16}, "TimeStop": {"buy_quality_bad": 90}}`
- `degraded_reason_counts`: `{"entry_drift_degraded": 23, "entry_truth_gap_degraded_acceptable": 28, "entry_truth_gap_too_large": 65, "exit_drift_degraded": 1, "exit_truth_gap_degraded_acceptable": 47, "exit_truth_gap_too_large": 42, "missing_gatekeeper_buy_context": 77, "speculative_curve_finality": 124}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 124, "max": 67547.0, "mean": 18728.959677419356, "min": 15.0, "p50": 10763.0, "p90": 50404.200000000004, "p99": 65236.759999999995}`
- `exit_truth_gap_ms`: `{"count": 124, "max": 97612.0, "mean": 36487.959677419356, "min": 0.0, "p50": 31519.5, "p90": 80509.0, "p99": 95557.93000000001}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 124, "max": 196.6192452104947, "mean": 13.595447659376315, "min": 0.0, "p50": 0.0, "p90": 44.3764989440636, "p99": 172.19835487309246}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 124, "max": 6.712962396146793, "mean": 0.054154403532315425, "min": 0.0, "p50": 1.4588546476401731e-05, "p90": 3.7692443118908514e-05, "p99": 4.869550417108125e-05}`
- `decision_to_execution_ms`: `{"count": 124, "max": 15242.0, "mean": 7089.862903225807, "min": 1331.0, "p50": 6657.5, "p90": 8579.7, "p99": 13889.019999999986}`
- `detection_to_execution_ms`: `{"count": 47, "max": 82451.0, "mean": 73629.0425531915, "min": 68375.0, "p50": 73021.0, "p90": 76317.40000000001, "p99": 80658.84}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
