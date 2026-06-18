# P3.7 Shadow Lifecycle Labels

Source: `/root/Gho/reports/selector/shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary/run_lifecycle_guard_20260613T153924Z/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
Output: `/root/Gho/reports/selector/shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary/run_lifecycle_guard_20260613T153924Z/lifecycle_canary/selector_lifecycle_canary_report.labels.jsonl`
Phase F label status: `not_accepted`

## Counts

- `rows_total`: `2`
- `all_lifecycle_rows`: `2`
- `analysis_status_counts`: `{"ok": 2}`
- `truth_status_counts`: `{"resolved": 2}`
- `market_outcome_class_counts`: `{"market_bad_clean": 2}`
- `execution_verification_class_counts`: `{"shadow_onchain_speculative_snapshot_verified": 2}`
- `execution_verification_class_hint_counts`: `{"shadow_onchain_speculative_snapshot_verified": 2}`
- `truth_gap_class_counts`: `{"truth_gap_clean": 1, "truth_gap_degraded_acceptable": 1}`
- `entry_truth_gap_class_counts`: `{"truth_gap_clean": 2}`
- `exit_truth_gap_class_counts`: `{"truth_gap_clean": 1, "truth_gap_degraded_acceptable": 1}`
- `buy_quality_class_counts`: `{"buy_quality_bad": 2}`
- `buy_quality_denominator_rows`: `2`
- `execution_feasibility_reject_rows`: `0`
- `execution_feasibility_status_counts`: `{"unknown": 2}`
- `execution_feasibility_reason_counts`: `{"unknown": 2}`
- `label_quality_counts`: `{"degraded": 2}`
- `close_reason_counts`: `{"StopLoss": 1, "TimeStop": 1}`
- `curve_finality_entry_counts`: `{"speculative": 2}`
- `curve_finality_exit_counts`: `{"speculative": 2}`
- `truth_dataset_kind_counts`: `{"shadow_burnin_lifecycle_onchain": 2}`
- `collection_plane_counts`: `{"active_shadow": 2}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 2, "no_gatekeeper_context_rows": 0}`
- `close_reason_by_buy_quality`: `{"StopLoss": {"buy_quality_bad": 1}, "TimeStop": {"buy_quality_bad": 1}}`
- `degraded_reason_counts`: `{"exit_truth_gap_degraded_acceptable": 1, "speculative_curve_finality": 2}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 2, "max": 374.0, "mean": 270.0, "min": 166.0, "p50": 270.0, "p90": 353.20000000000005, "p99": 371.92}`
- `exit_truth_gap_ms`: `{"count": 2, "max": 30453.0, "mean": 15226.5, "min": 0.0, "p50": 15226.5, "p90": 27407.7, "p99": 30148.47}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 2, "max": 3.1552494342280246, "mean": 1.740697391976287, "min": 0.3261453497245492, "p50": 1.740697391976287, "p90": 2.872339025777677, "p99": 3.12695839338299}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 2, "max": 2.932920299958397e-05, "mean": 2.193204231004664e-05, "min": 1.453488162050931e-05, "p50": 2.193204231004664e-05, "p90": 2.7849770861676504e-05, "p99": 2.9181259785793223e-05}`
- `decision_to_execution_ms`: `{"count": 2, "max": 850.0, "mean": 729.0, "min": 608.0, "p50": 729.0, "p90": 825.8, "p99": 847.58}`
- `detection_to_execution_ms`: `{"count": 2, "max": 11903.0, "mean": 11335.5, "min": 10768.0, "p50": 11335.5, "p90": 11789.5, "p99": 11891.65}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
