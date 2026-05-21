# P3.7 Shadow Lifecycle Labels

Source: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/probe_shadow_onchain_lifecycle_report.jsonl`
Output: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/p3_7_probe_shadow_lifecycle_labels.jsonl`
Phase F label status: `not_accepted`

## Counts

- `rows_total`: `42`
- `all_lifecycle_rows`: `42`
- `analysis_status_counts`: `{"ok": 42}`
- `truth_status_counts`: `{"resolved": 42}`
- `market_outcome_class_counts`: `{"market_bad_clean": 42}`
- `execution_verification_class_counts`: `{"shadow_onchain_degraded": 4, "shadow_onchain_speculative_snapshot_verified": 38}`
- `truth_gap_class_counts`: `{"truth_gap_degraded_acceptable": 32, "truth_gap_too_large": 10}`
- `entry_truth_gap_class_counts`: `{"truth_gap_clean": 12, "truth_gap_degraded_acceptable": 20, "truth_gap_too_large": 10}`
- `exit_truth_gap_class_counts`: `{"truth_gap_degraded_acceptable": 38, "truth_gap_too_large": 4}`
- `buy_quality_class_counts`: `{"buy_quality_bad": 42}`
- `label_quality_counts`: `{"degraded": 42}`
- `close_reason_counts`: `{"TimeStop": 42}`
- `curve_finality_entry_counts`: `{"provisional": 4, "speculative": 38}`
- `curve_finality_exit_counts`: `{"provisional": 4, "speculative": 38}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 0, "no_gatekeeper_context_rows": 42}`
- `close_reason_by_buy_quality`: `{"TimeStop": {"buy_quality_bad": 42}}`
- `degraded_reason_counts`: `{"entry_truth_gap_degraded_acceptable": 20, "entry_truth_gap_too_large": 10, "exit_truth_gap_degraded_acceptable": 38, "exit_truth_gap_too_large": 4, "missing_gatekeeper_buy_context": 42, "nonstandard_curve_finality": 4, "speculative_curve_finality": 38}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 42, "max": 53472.0, "mean": 7991.928571428572, "min": 90.0, "p50": 5656.5, "p90": 11004.5, "p99": 49549.939999999966}`
- `exit_truth_gap_ms`: `{"count": 42, "max": 84718.0, "mean": 38148.333333333336, "min": 30030.0, "p50": 34932.0, "p90": 42180.0, "p99": 80710.24999999997}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 42, "max": 8.450055435770532, "mean": 0.92632589887204, "min": 0.0, "p50": 0.0, "p90": 3.3188907765037134, "p99": 6.3462779255165}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 42, "max": 0.0002807636208856934, "mean": 1.7340095839625204e-05, "min": 0.0, "p50": 1.4582488772063584e-05, "p90": 1.506636962966823e-05, "p99": 0.00017308606709798182}`
- `decision_to_execution_ms`: `{"count": 42, "max": 65006.0, "mean": 12610.02380952381, "min": 8893.0, "p50": 8981.5, "p90": 10696.399999999998, "p99": 64021.59}`
- `detection_to_execution_ms`: `{"count": 0, "max": null, "mean": null, "min": null, "p50": null, "p90": null, "p99": null}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
