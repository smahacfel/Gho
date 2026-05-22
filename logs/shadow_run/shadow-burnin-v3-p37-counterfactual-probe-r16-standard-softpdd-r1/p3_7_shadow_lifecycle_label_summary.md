# P3.7 Shadow Lifecycle Labels

Source: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/probe_shadow_onchain_lifecycle_report.jsonl`
Output: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/p3_7_shadow_lifecycle_labels.jsonl`
Phase F label status: `accepted`

## Counts

- `rows_total`: `39`
- `all_lifecycle_rows`: `39`
- `analysis_status_counts`: `{"ok": 39}`
- `truth_status_counts`: `{"resolved": 39}`
- `market_outcome_class_counts`: `{"market_bad_clean": 37, "market_good_clean": 2}`
- `execution_verification_class_counts`: `{"shadow_onchain_degraded": 1, "shadow_onchain_speculative_snapshot_verified": 38}`
- `truth_gap_class_counts`: `{"truth_gap_clean": 3, "truth_gap_degraded_acceptable": 36}`
- `entry_truth_gap_class_counts`: `{"truth_gap_clean": 12, "truth_gap_degraded_acceptable": 27}`
- `exit_truth_gap_class_counts`: `{"truth_gap_clean": 4, "truth_gap_degraded_acceptable": 35}`
- `buy_quality_class_counts`: `{"buy_quality_bad": 37, "buy_quality_dirty_good": 2}`
- `label_quality_counts`: `{"degraded": 39}`
- `close_reason_counts`: `{"StopLoss": 2, "Target": 2, "TimeStop": 35}`
- `curve_finality_entry_counts`: `{"provisional": 4, "speculative": 35}`
- `curve_finality_exit_counts`: `{"provisional": 1, "speculative": 38}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 1, "no_gatekeeper_context_rows": 38}`
- `close_reason_by_buy_quality`: `{"StopLoss": {"buy_quality_bad": 2}, "Target": {"buy_quality_dirty_good": 2}, "TimeStop": {"buy_quality_bad": 35}}`
- `degraded_reason_counts`: `{"entry_truth_gap_degraded_acceptable": 27, "exit_truth_gap_degraded_acceptable": 35, "missing_gatekeeper_buy_context": 38, "nonstandard_curve_finality": 1, "speculative_curve_finality": 38}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 39, "max": 7657.0, "mean": 3898.0, "min": 21.0, "p50": 4593.0, "p90": 6953.000000000001, "p99": 7544.9}`
- `exit_truth_gap_ms`: `{"count": 39, "max": 38899.0, "mean": 29962.615384615383, "min": 0.0, "p50": 30732.0, "p90": 37949.8, "p99": 38696.84}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 39, "max": 7.76780358853677, "mean": 0.6440821369796514, "min": 0.0, "p50": 0.0, "p90": 1.1551611471623033, "p99": 7.6219263917702875}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 39, "max": 3.0206862622339514e-05, "mean": 1.2431117760525759e-05, "min": 0.0, "p50": 1.4609568776968729e-05, "p90": 2.609781113438459e-05, "p99": 2.9743814016880418e-05}`
- `decision_to_execution_ms`: `{"count": 39, "max": 6592.0, "mean": 4093.230769230769, "min": -844.0, "p50": 4005.0, "p90": 6004.2, "p99": 6413.779999999999}`
- `detection_to_execution_ms`: `{"count": 1, "max": 1401.0, "mean": 1401.0, "min": 1401.0, "p50": 1401.0, "p90": 1401.0, "p99": 1401.0}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
