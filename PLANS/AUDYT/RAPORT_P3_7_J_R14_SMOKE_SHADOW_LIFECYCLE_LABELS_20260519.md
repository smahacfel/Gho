# P3.7 Shadow Lifecycle Labels - Buy Heavy Rerun

Source: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/shadow_onchain_lifecycle_report.jsonl`
Output: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/p3_7_shadow_lifecycle_labels.jsonl`
Phase F label status: `not_accepted`

## Counts

- `rows_total`: `0`
- `all_lifecycle_rows`: `0`
- `analysis_status_counts`: `{}`
- `truth_status_counts`: `{}`
- `market_outcome_class_counts`: `{}`
- `execution_verification_class_counts`: `{}`
- `truth_gap_class_counts`: `{}`
- `entry_truth_gap_class_counts`: `{}`
- `exit_truth_gap_class_counts`: `{}`
- `buy_quality_class_counts`: `{}`
- `label_quality_counts`: `{}`
- `close_reason_counts`: `{}`
- `curve_finality_entry_counts`: `{}`
- `curve_finality_exit_counts`: `{}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 0, "no_gatekeeper_context_rows": 0}`
- `close_reason_by_buy_quality`: `{}`
- `degraded_reason_counts`: `{}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 0, "max": null, "mean": null, "min": null, "p50": null, "p90": null, "p99": null}`
- `exit_truth_gap_ms`: `{"count": 0, "max": null, "mean": null, "min": null, "p50": null, "p90": null, "p99": null}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 0, "max": null, "mean": null, "min": null, "p50": null, "p90": null, "p99": null}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 0, "max": null, "mean": null, "min": null, "p50": null, "p90": null, "p99": null}`
- `decision_to_execution_ms`: `{"count": 0, "max": null, "mean": null, "min": null, "p50": null, "p90": null, "p99": null}`
- `detection_to_execution_ms`: `{"count": 0, "max": null, "mean": null, "min": null, "p50": null, "p90": null, "p99": null}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
