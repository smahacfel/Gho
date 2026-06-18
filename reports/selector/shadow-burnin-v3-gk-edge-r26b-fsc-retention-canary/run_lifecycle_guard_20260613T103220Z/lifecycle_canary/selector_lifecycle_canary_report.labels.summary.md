# P3.7 Shadow Lifecycle Labels

Source: `/root/Gho/reports/selector/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary/run_lifecycle_guard_20260613T103220Z/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
Output: `/root/Gho/reports/selector/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary/run_lifecycle_guard_20260613T103220Z/lifecycle_canary/selector_lifecycle_canary_report.labels.jsonl`
Phase F label status: `accepted`

## Counts

- `rows_total`: `31`
- `all_lifecycle_rows`: `31`
- `analysis_status_counts`: `{"ok": 31}`
- `truth_status_counts`: `{"resolved": 31}`
- `market_outcome_class_counts`: `{"market_bad_clean": 22, "market_good_clean": 9}`
- `execution_verification_class_counts`: `{"shadow_onchain_speculative_snapshot_verified": 31}`
- `execution_verification_class_hint_counts`: `{"shadow_onchain_speculative_snapshot_verified": 31}`
- `truth_gap_class_counts`: `{"truth_gap_clean": 11, "truth_gap_degraded_acceptable": 20}`
- `entry_truth_gap_class_counts`: `{"truth_gap_clean": 21, "truth_gap_degraded_acceptable": 10}`
- `exit_truth_gap_class_counts`: `{"truth_gap_clean": 13, "truth_gap_degraded_acceptable": 18}`
- `buy_quality_class_counts`: `{"buy_quality_bad": 22, "buy_quality_dirty_good": 9}`
- `buy_quality_denominator_rows`: `31`
- `execution_feasibility_reject_rows`: `0`
- `execution_feasibility_status_counts`: `{"unknown": 31}`
- `execution_feasibility_reason_counts`: `{"unknown": 31}`
- `label_quality_counts`: `{"degraded": 31}`
- `close_reason_counts`: `{"StopLoss": 5, "Target": 8, "TimeStop": 18}`
- `curve_finality_entry_counts`: `{"speculative": 31}`
- `curve_finality_exit_counts`: `{"speculative": 31}`
- `truth_dataset_kind_counts`: `{"shadow_burnin_lifecycle_onchain": 31}`
- `collection_plane_counts`: `{"active_shadow": 31}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 31, "no_gatekeeper_context_rows": 0}`
- `close_reason_by_buy_quality`: `{"StopLoss": {"buy_quality_bad": 5}, "Target": {"buy_quality_dirty_good": 8}, "TimeStop": {"buy_quality_bad": 17, "buy_quality_dirty_good": 1}}`
- `degraded_reason_counts`: `{"entry_drift_degraded": 1, "entry_truth_gap_degraded_acceptable": 10, "exit_drift_degraded": 1, "exit_truth_gap_degraded_acceptable": 18, "speculative_curve_finality": 31}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 31, "max": 8755.0, "mean": 2043.4516129032259, "min": 17.0, "p50": 725.0, "p90": 6773.0, "p99": 8744.8}`
- `exit_truth_gap_ms`: `{"count": 31, "max": 30858.0, "mean": 17707.322580645163, "min": 0.0, "p50": 30109.0, "p90": 30794.0, "p99": 30856.8}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 31, "max": 23.96779890174887, "mean": 3.364121895010902, "min": 0.0, "p50": 0.42764389250031787, "p90": 10.930643975029097, "p99": 20.813439308276273}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 31, "max": 5.950266105174606, "mean": 0.25982643156083, "min": 7.1530057210722475e-06, "p50": 1.5517333940007205e-05, "p90": 4.986237982507191e-05, "p99": 4.796323473740825}`
- `decision_to_execution_ms`: `{"count": 31, "max": 945.0, "mean": 699.6451612903226, "min": 598.0, "p50": 639.0, "p90": 860.0, "p99": 925.1999999999999}`
- `detection_to_execution_ms`: `{"count": 31, "max": 11916.0, "mean": 10969.516129032258, "min": 10640.0, "p50": 10681.0, "p90": 11881.0, "p99": 11910.0}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
