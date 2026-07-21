# P3.7 Shadow Lifecycle Labels

Source: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a-1h/run_lifecycle_guard_20260719T0039Z_c02b49c/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
Output: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a-1h/run_lifecycle_guard_20260719T0039Z_c02b49c/lifecycle_canary/selector_lifecycle_canary_report.labels.jsonl`
Phase F label status: `accepted`

## Counts

- `rows_total`: `86`
- `all_lifecycle_rows`: `86`
- `analysis_status_counts`: `{"ok": 86}`
- `truth_status_counts`: `{"resolved": 86}`
- `market_outcome_class_counts`: `{"market_bad_clean": 69, "market_good_clean": 17}`
- `execution_verification_class_counts`: `{"shadow_onchain_speculative_snapshot_verified": 86}`
- `execution_verification_class_hint_counts`: `{"shadow_onchain_speculative_snapshot_verified": 86}`
- `truth_gap_class_counts`: `{"truth_gap_clean": 16, "truth_gap_degraded_acceptable": 27, "truth_gap_too_large": 43}`
- `entry_truth_gap_class_counts`: `{"truth_gap_clean": 20, "truth_gap_degraded_acceptable": 23, "truth_gap_too_large": 43}`
- `exit_truth_gap_class_counts`: `{"truth_gap_clean": 24, "truth_gap_degraded_acceptable": 35, "truth_gap_too_large": 27}`
- `buy_quality_class_counts`: `{"buy_quality_bad": 69, "buy_quality_dirty_good": 16, "buy_quality_unknown": 1}`
- `buy_quality_denominator_rows`: `86`
- `execution_feasibility_reject_rows`: `0`
- `execution_feasibility_status_counts`: `{"unknown": 86}`
- `execution_feasibility_reason_counts`: `{"unknown": 86}`
- `label_quality_counts`: `{"degraded": 85, "unknown": 1}`
- `close_reason_counts`: `{"StopLoss": 15, "Target": 6, "TimeStop": 65}`
- `curve_finality_entry_counts`: `{"speculative": 86}`
- `curve_finality_exit_counts`: `{"speculative": 86}`
- `truth_dataset_kind_counts`: `{"shadow_burnin_lifecycle_onchain": 86}`
- `collection_plane_counts`: `{"active_shadow": 86}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 37, "no_gatekeeper_context_rows": 49}`
- `close_reason_by_buy_quality`: `{"StopLoss": {"buy_quality_bad": 15}, "Target": {"buy_quality_dirty_good": 6}, "TimeStop": {"buy_quality_bad": 54, "buy_quality_dirty_good": 10, "buy_quality_unknown": 1}}`
- `degraded_reason_counts`: `{"entry_drift_degraded": 16, "entry_truth_gap_degraded_acceptable": 23, "entry_truth_gap_too_large": 43, "exit_truth_gap_degraded_acceptable": 35, "exit_truth_gap_too_large": 27, "missing_gatekeeper_buy_context": 49, "speculative_curve_finality": 86}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 86, "max": 63994.0, "mean": 16806.86046511628, "min": 18.0, "p50": 10135.5, "p90": 42480.5, "p99": 62517.55000000001}`
- `exit_truth_gap_ms`: `{"count": 86, "max": 94429.0, "mean": 33802.348837209305, "min": 0.0, "p50": 30497.0, "p90": 72060.0, "p99": 92627.85}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 86, "max": 495.7014855571237, "mean": 22.59564766677598, "min": 0.0, "p50": 0.0, "p90": 37.70113010059886, "p99": 346.96191030396255}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 86, "max": 3.301678883914727, "mean": 0.03841041368357328, "min": 0.0, "p50": 1.4584406216044954e-05, "p90": 4.1878884304846764e-05, "p99": 0.4953258180721486}`
- `decision_to_execution_ms`: `{"count": 86, "max": 17273.0, "mean": 7716.488372093023, "min": 3379.0, "p50": 6829.0, "p90": 10767.5, "p99": 16168.850000000006}`
- `detection_to_execution_ms`: `{"count": 37, "max": 84481.0, "mean": 74108.5945945946, "min": 69587.0, "p50": 73097.0, "p90": 77421.6, "p99": 83704.84}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
