# P3.7 Shadow Lifecycle Labels

Source: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-r1/run_lifecycle_guard_20260717T080700Z/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
Output: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-r1/run_lifecycle_guard_20260717T080700Z/lifecycle_canary/selector_lifecycle_canary_report.labels.jsonl`
Phase F label status: `accepted`

## Counts

- `rows_total`: `5`
- `all_lifecycle_rows`: `5`
- `analysis_status_counts`: `{"ok": 5}`
- `truth_status_counts`: `{"resolved": 5}`
- `market_outcome_class_counts`: `{"market_bad_clean": 4, "market_good_clean": 1}`
- `execution_verification_class_counts`: `{"shadow_onchain_speculative_snapshot_verified": 5}`
- `execution_verification_class_hint_counts`: `{"shadow_onchain_speculative_snapshot_verified": 5}`
- `truth_gap_class_counts`: `{"truth_gap_degraded_acceptable": 1, "truth_gap_too_large": 4}`
- `entry_truth_gap_class_counts`: `{"truth_gap_degraded_acceptable": 1, "truth_gap_too_large": 4}`
- `exit_truth_gap_class_counts`: `{"truth_gap_clean": 1, "truth_gap_too_large": 4}`
- `buy_quality_class_counts`: `{"buy_quality_bad": 4, "buy_quality_dirty_good": 1}`
- `buy_quality_denominator_rows`: `5`
- `execution_feasibility_reject_rows`: `0`
- `execution_feasibility_status_counts`: `{"unknown": 5}`
- `execution_feasibility_reason_counts`: `{"unknown": 5}`
- `label_quality_counts`: `{"degraded": 5}`
- `close_reason_counts`: `{"Target": 1, "TimeStop": 4}`
- `curve_finality_entry_counts`: `{"speculative": 5}`
- `curve_finality_exit_counts`: `{"speculative": 5}`
- `truth_dataset_kind_counts`: `{"shadow_burnin_lifecycle_onchain": 5}`
- `collection_plane_counts`: `{"active_shadow": 5}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 2, "no_gatekeeper_context_rows": 3}`
- `close_reason_by_buy_quality`: `{"Target": {"buy_quality_dirty_good": 1}, "TimeStop": {"buy_quality_bad": 4}}`
- `degraded_reason_counts`: `{"entry_truth_gap_degraded_acceptable": 1, "entry_truth_gap_too_large": 4, "exit_truth_gap_too_large": 4, "missing_gatekeeper_buy_context": 3, "speculative_curve_finality": 5}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 5, "max": 48418.0, "mean": 29808.8, "min": 5480.0, "p50": 29738.0, "p90": 43327.6, "p99": 47908.96}`
- `exit_truth_gap_ms`: `{"count": 5, "max": 78694.0, "mean": 53008.4, "min": 328.0, "p50": 60206.0, "p90": 73642.8, "p99": 78188.87999999999}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 5, "max": 7.488476595856963, "mean": 1.4976953191713926, "min": 0.0, "p50": 0.0, "p90": 4.493085957514179, "p99": 7.188937532022685}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 5, "max": 1.4582429219700543e-05, "mean": 1.3360861883526809e-05, "min": 8.477250190708219e-06, "p50": 1.4582335661206258e-05, "p90": 1.4582398599749524e-05, "p99": 1.4582426157705441e-05}`
- `decision_to_execution_ms`: `{"count": 5, "max": 10133.0, "mean": 8103.0, "min": 6354.0, "p50": 7509.0, "p90": 9941.0, "p99": 10113.800000000001}`
- `detection_to_execution_ms`: `{"count": 2, "max": 75870.0, "mean": 74982.5, "min": 74095.0, "p50": 74982.5, "p90": 75692.5, "p99": 75852.25}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
