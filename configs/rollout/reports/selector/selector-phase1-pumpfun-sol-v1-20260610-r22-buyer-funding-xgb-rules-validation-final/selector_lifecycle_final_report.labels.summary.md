# P3.7 Shadow Lifecycle Labels

Source: `/root/Gho/configs/rollout/reports/selector/selector-phase1-pumpfun-sol-v1-20260610-r22-buyer-funding-xgb-rules-validation-final/selector_lifecycle_final_report.jsonl`
Output: `/root/Gho/configs/rollout/reports/selector/selector-phase1-pumpfun-sol-v1-20260610-r22-buyer-funding-xgb-rules-validation-final/selector_lifecycle_final_labels.jsonl`
Phase F label status: `accepted`

## Counts

- `rows_total`: `664`
- `all_lifecycle_rows`: `664`
- `analysis_status_counts`: `{"ok": 664}`
- `truth_status_counts`: `{"resolved": 664}`
- `market_outcome_class_counts`: `{"market_bad_clean": 508, "market_good_clean": 156}`
- `execution_verification_class_counts`: `{"shadow_onchain_speculative_snapshot_verified": 664}`
- `execution_verification_class_hint_counts`: `{"shadow_onchain_speculative_snapshot_verified": 664}`
- `truth_gap_class_counts`: `{"truth_gap_clean": 202, "truth_gap_degraded_acceptable": 448, "truth_gap_too_large": 14}`
- `entry_truth_gap_class_counts`: `{"truth_gap_clean": 395, "truth_gap_degraded_acceptable": 255, "truth_gap_too_large": 14}`
- `exit_truth_gap_class_counts`: `{"truth_gap_clean": 232, "truth_gap_degraded_acceptable": 432}`
- `buy_quality_class_counts`: `{"buy_quality_bad": 508, "buy_quality_dirty_good": 156}`
- `buy_quality_denominator_rows`: `664`
- `execution_feasibility_reject_rows`: `0`
- `execution_feasibility_status_counts`: `{"unknown": 664}`
- `execution_feasibility_reason_counts`: `{"unknown": 664}`
- `label_quality_counts`: `{"degraded": 664}`
- `close_reason_counts`: `{"StopLoss": 103, "Target": 129, "TimeStop": 432}`
- `curve_finality_entry_counts`: `{"speculative": 664}`
- `curve_finality_exit_counts`: `{"speculative": 664}`
- `truth_dataset_kind_counts`: `{"shadow_burnin_lifecycle_onchain": 664}`
- `collection_plane_counts`: `{"active_shadow": 664}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 664, "no_gatekeeper_context_rows": 0}`
- `close_reason_by_buy_quality`: `{"StopLoss": {"buy_quality_bad": 103}, "Target": {"buy_quality_dirty_good": 129}, "TimeStop": {"buy_quality_bad": 405, "buy_quality_dirty_good": 27}}`
- `degraded_reason_counts`: `{"entry_drift_degraded": 67, "entry_truth_gap_degraded_acceptable": 255, "entry_truth_gap_too_large": 14, "exit_drift_degraded": 8, "exit_truth_gap_degraded_acceptable": 432, "speculative_curve_finality": 664}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 664, "max": 13954.0, "mean": 2500.6927710843374, "min": 1.0, "p50": 909.0, "p90": 7987.600000000001, "p99": 10411.95}`
- `exit_truth_gap_ms`: `{"count": 664, "max": 40422.0, "mean": 20243.933734939757, "min": 0.0, "p50": 30188.0, "p90": 30898.699999999997, "p99": 39199.31}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 664, "max": 115.05463126156248, "mean": 4.734841665326076, "min": 0.0, "p50": 0.0, "p90": 14.950926849204382, "p99": 46.38158781548481}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 664, "max": 22.441442169589564, "mean": 0.15823011616009933, "min": 0.0, "p50": 1.5407198317163306e-05, "p90": 3.209419533356517e-05, "p99": 5.6909587236897625}`
- `decision_to_execution_ms`: `{"count": 664, "max": 24610.0, "mean": 756.8629518072289, "min": 554.0, "p50": 671.0, "p90": 887.7, "p99": 991.74}`
- `detection_to_execution_ms`: `{"count": 664, "max": 36194.0, "mean": 11036.085843373494, "min": 10628.0, "p50": 10729.0, "p90": 11934.699999999999, "p99": 12527.990000000002}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
