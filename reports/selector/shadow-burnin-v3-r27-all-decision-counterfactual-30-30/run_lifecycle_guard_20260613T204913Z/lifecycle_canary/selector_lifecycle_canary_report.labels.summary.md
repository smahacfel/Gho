# P3.7 Shadow Lifecycle Labels

Source: `/root/Gho/reports/selector/shadow-burnin-v3-r27-all-decision-counterfactual-30-30/run_lifecycle_guard_20260613T204913Z/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
Output: `/root/Gho/reports/selector/shadow-burnin-v3-r27-all-decision-counterfactual-30-30/run_lifecycle_guard_20260613T204913Z/lifecycle_canary/selector_lifecycle_canary_report.labels.jsonl`
Phase F label status: `accepted`

## Counts

- `rows_total`: `2`
- `all_lifecycle_rows`: `2`
- `analysis_status_counts`: `{"ok": 2}`
- `truth_status_counts`: `{"resolved": 2}`
- `market_outcome_class_counts`: `{"market_bad_clean": 1, "market_good_clean": 1}`
- `execution_verification_class_counts`: `{"shadow_onchain_speculative_snapshot_verified": 2}`
- `execution_verification_class_hint_counts`: `{"shadow_onchain_speculative_snapshot_verified": 2}`
- `truth_gap_class_counts`: `{"truth_gap_degraded_acceptable": 2}`
- `entry_truth_gap_class_counts`: `{"truth_gap_degraded_acceptable": 2}`
- `exit_truth_gap_class_counts`: `{"truth_gap_clean": 1, "truth_gap_degraded_acceptable": 1}`
- `buy_quality_class_counts`: `{"buy_quality_bad": 1, "buy_quality_dirty_good": 1}`
- `buy_quality_denominator_rows`: `2`
- `execution_feasibility_reject_rows`: `0`
- `execution_feasibility_status_counts`: `{"unknown": 2}`
- `execution_feasibility_reason_counts`: `{"unknown": 2}`
- `label_quality_counts`: `{"degraded": 2}`
- `close_reason_counts`: `{"Target": 1, "TimeStop": 1}`
- `curve_finality_entry_counts`: `{"speculative": 2}`
- `curve_finality_exit_counts`: `{"speculative": 2}`
- `truth_dataset_kind_counts`: `{"shadow_burnin_lifecycle_onchain": 2}`
- `collection_plane_counts`: `{"active_shadow": 2}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 2, "no_gatekeeper_context_rows": 0}`
- `close_reason_by_buy_quality`: `{"Target": {"buy_quality_dirty_good": 1}, "TimeStop": {"buy_quality_bad": 1}}`
- `degraded_reason_counts`: `{"entry_truth_gap_degraded_acceptable": 2, "exit_truth_gap_degraded_acceptable": 1, "speculative_curve_finality": 2}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 2, "max": 7930.0, "mean": 5959.5, "min": 3989.0, "p50": 5959.5, "p90": 7535.9, "p99": 7890.59}`
- `exit_truth_gap_ms`: `{"count": 2, "max": 38361.0, "mean": 19180.5, "min": 0.0, "p50": 19180.5, "p90": 34524.9, "p99": 37977.39}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 2, "max": 0.0, "mean": 0.0, "min": 0.0, "p50": 0.0, "p90": 0.0, "p99": 0.0}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 2, "max": 1.4582452612099672e-05, "mean": 1.2269211657756074e-05, "min": 9.955970703412476e-06, "p50": 1.2269211657756074e-05, "p90": 1.4119804421230953e-05, "p99": 1.45361877930128e-05}`
- `decision_to_execution_ms`: `{"count": 2, "max": 791.0, "mean": 743.5, "min": 696.0, "p50": 743.5, "p90": 781.5, "p99": 790.0500000000001}`
- `detection_to_execution_ms`: `{"count": 2, "max": 10911.0, "mean": 10822.5, "min": 10734.0, "p50": 10822.5, "p90": 10893.3, "p99": 10909.23}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
