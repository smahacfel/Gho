# P3.7 Shadow Lifecycle Labels

Source: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/active_shadow_onchain_lifecycle_report.jsonl`
Output: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/active_p3_7_shadow_lifecycle_labels.jsonl`
Phase F label status: `accepted`

## Counts

- `rows_total`: `4`
- `all_lifecycle_rows`: `4`
- `analysis_status_counts`: `{"ok": 4}`
- `truth_status_counts`: `{"resolved": 4}`
- `market_outcome_class_counts`: `{"market_bad_clean": 2, "market_good_clean": 2}`
- `execution_verification_class_counts`: `{"shadow_onchain_speculative_snapshot_verified": 4}`
- `truth_gap_class_counts`: `{"truth_gap_clean": 3, "truth_gap_degraded_acceptable": 1}`
- `entry_truth_gap_class_counts`: `{"truth_gap_clean": 4}`
- `exit_truth_gap_class_counts`: `{"truth_gap_clean": 3, "truth_gap_degraded_acceptable": 1}`
- `buy_quality_class_counts`: `{"buy_quality_bad": 2, "buy_quality_dirty_good": 2}`
- `label_quality_counts`: `{"degraded": 4}`
- `close_reason_counts`: `{"StopLoss": 1, "Target": 2, "TimeStop": 1}`
- `curve_finality_entry_counts`: `{"speculative": 4}`
- `curve_finality_exit_counts`: `{"speculative": 4}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 4, "no_gatekeeper_context_rows": 0}`
- `close_reason_by_buy_quality`: `{"StopLoss": {"buy_quality_bad": 1}, "Target": {"buy_quality_dirty_good": 2}, "TimeStop": {"buy_quality_bad": 1}}`
- `degraded_reason_counts`: `{"exit_truth_gap_degraded_acceptable": 1, "speculative_curve_finality": 4}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 4, "max": 462.0, "mean": 312.25, "min": 241.0, "p50": 273.0, "p90": 414.30000000000007, "p99": 457.22999999999996}`
- `exit_truth_gap_ms`: `{"count": 4, "max": 30045.0, "mean": 7635.5, "min": 3.0, "p50": 247.0, "p90": 21178.800000000003, "p99": 29158.379999999994}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 4, "max": 0.0, "mean": 0.0, "min": 0.0, "p50": 0.0, "p90": 0.0, "p99": 0.0}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 4, "max": 3.374310032884431, "mean": 0.8435890240802796, "min": 9.104062803633184e-06, "p50": 1.8479686941796913e-05, "p90": 2.3620252947783764, "p99": 3.2730815590738245}`
- `decision_to_execution_ms`: `{"count": 4, "max": 253.0, "mean": 224.75, "min": 207.0, "p50": 219.5, "p90": 246.70000000000002, "p99": 252.37}`
- `detection_to_execution_ms`: `{"count": 4, "max": 2754.0, "mean": 1150.75, "min": 589.0, "p50": 630.0, "p90": 2128.2000000000003, "p99": 2691.419999999999}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
