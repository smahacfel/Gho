# P3.7 Shadow Lifecycle Labels

Source: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1/probe_shadow_onchain_lifecycle_report.jsonl`
Output: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1/p3_7_probe_shadow_lifecycle_labels.jsonl`
Phase F label status: `accepted`

## Counts

- `rows_total`: `24`
- `all_lifecycle_rows`: `24`
- `analysis_status_counts`: `{"ok": 24}`
- `truth_status_counts`: `{"resolved": 24}`
- `market_outcome_class_counts`: `{"market_bad_clean": 23, "market_good_clean": 1}`
- `execution_verification_class_counts`: `{"shadow_onchain_speculative_snapshot_verified": 24}`
- `truth_gap_class_counts`: `{"truth_gap_degraded_acceptable": 24}`
- `entry_truth_gap_class_counts`: `{"truth_gap_clean": 1, "truth_gap_degraded_acceptable": 23}`
- `exit_truth_gap_class_counts`: `{"truth_gap_clean": 1, "truth_gap_degraded_acceptable": 23}`
- `buy_quality_class_counts`: `{"buy_quality_bad": 23, "buy_quality_dirty_good": 1}`
- `label_quality_counts`: `{"degraded": 24}`
- `close_reason_counts`: `{"Target": 1, "TimeStop": 23}`
- `curve_finality_entry_counts`: `{"speculative": 24}`
- `curve_finality_exit_counts`: `{"speculative": 24}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 0, "no_gatekeeper_context_rows": 24}`
- `close_reason_by_buy_quality`: `{"Target": {"buy_quality_dirty_good": 1}, "TimeStop": {"buy_quality_bad": 23}}`
- `degraded_reason_counts`: `{"entry_truth_gap_degraded_acceptable": 23, "exit_truth_gap_degraded_acceptable": 23, "missing_gatekeeper_buy_context": 24, "speculative_curve_finality": 24}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 24, "max": 9452.0, "mean": 7727.833333333333, "min": 873.0, "p50": 8045.0, "p90": 8789.0, "p99": 9337.92}`
- `exit_truth_gap_ms`: `{"count": 24, "max": 40619.0, "mean": 35778.125, "min": 0.0, "p50": 39138.5, "p90": 39610.2, "p99": 40428.33}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 24, "max": 6.915953493490257, "mean": 0.2881647288954274, "min": 0.0, "p50": 0.0, "p90": 0.0, "p99": 5.325284189987495}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 24, "max": 0.00037416409404134043, "mean": 2.9369694599934437e-05, "min": 9.149225421722917e-06, "p50": 1.4587616475880694e-05, "p90": 1.4595973074627011e-05, "p99": 0.00029162014979777757}`
- `decision_to_execution_ms`: `{"count": 24, "max": 10547.0, "mean": 9151.333333333334, "min": 8951.0, "p50": 9061.5, "p90": 9388.6, "p99": 10361.849999999999}`
- `detection_to_execution_ms`: `{"count": 0, "max": null, "mean": null, "min": null, "p50": null, "p90": null, "p99": null}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
