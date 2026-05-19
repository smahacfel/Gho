# P3.7 Shadow Lifecycle Labels - Buy Heavy Rerun

Source: `logs/shadow_run/shadow-burnin-buy-heavy-rerun/shadow_onchain_lifecycle_report_all.jsonl`
Output: `logs/shadow_run/shadow-burnin-buy-heavy-rerun/p3_7_shadow_lifecycle_labels.jsonl`
Phase F label status: `accepted`

## Counts

- `rows_total`: `2386`
- `all_lifecycle_rows`: `2386`
- `analysis_status_counts`: `{"ok": 2386}`
- `truth_status_counts`: `{"resolved": 2386}`
- `market_outcome_class_counts`: `{"market_bad_clean": 1807, "market_good_clean": 579}`
- `execution_verification_class_counts`: `{"shadow_onchain_speculative_snapshot_verified": 2386}`
- `truth_gap_class_counts`: `{"truth_gap_clean": 872, "truth_gap_degraded_acceptable": 1514}`
- `entry_truth_gap_class_counts`: `{"truth_gap_clean": 1422, "truth_gap_degraded_acceptable": 964}`
- `exit_truth_gap_class_counts`: `{"truth_gap_clean": 997, "truth_gap_degraded_acceptable": 1389}`
- `buy_quality_class_counts`: `{"buy_quality_bad": 1807, "buy_quality_dirty_good": 579}`
- `label_quality_counts`: `{"degraded": 2386}`
- `close_reason_counts`: `{"StopLoss": 497, "Target": 500, "TimeStop": 1389}`
- `curve_finality_entry_counts`: `{"speculative": 2386}`
- `curve_finality_exit_counts`: `{"speculative": 2386}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 571, "no_gatekeeper_context_rows": 1815}`
- `close_reason_by_buy_quality`: `{"StopLoss": {"buy_quality_bad": 497}, "Target": {"buy_quality_dirty_good": 500}, "TimeStop": {"buy_quality_bad": 1310, "buy_quality_dirty_good": 79}}`
- `degraded_reason_counts`: `{"entry_drift_degraded": 101, "entry_truth_gap_degraded_acceptable": 964, "exit_drift_degraded": 12, "exit_truth_gap_degraded_acceptable": 1389, "missing_gatekeeper_buy_context": 1815, "speculative_curve_finality": 2386}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 2386, "max": 9788.0, "mean": 1948.1974015088012, "min": 0.0, "p50": 928.5, "p90": 5637.5, "p99": 8083.15}`
- `exit_truth_gap_ms`: `{"count": 2386, "max": 40135.0, "mean": 18204.778290025148, "min": 0.0, "p50": 30107.0, "p90": 32529.5, "p99": 37516.05}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 2386, "max": 101.1912107164755, "mean": 1.718053729278956, "min": 0.0, "p50": 0.0, "p90": 2.0365643635255393, "p99": 36.16134322564436}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 2386, "max": 29.625039710742595, "mean": 0.0877463848024166, "min": 0.0, "p50": 1.4749289733995141e-05, "p90": 3.2653765569268245e-05, "p99": 1.3174437410355675}`
- `decision_to_execution_ms`: `{"count": 2386, "max": 3960.0, "mean": 246.8134953897737, "min": 116.0, "p50": 228.0, "p90": 279.0, "p99": 801.0}`
- `detection_to_execution_ms`: `{"count": 571, "max": 9198.0, "mean": 8292.45183887916, "min": 8178.0, "p50": 8274.0, "p90": 8320.0, "p99": 8900.9}`

## Thresholds

- `thresholds`: `{"entry_drift_acceptable_abs_pct": 15.0, "entry_truth_gap_clean_ms": 1500, "entry_truth_gap_degraded_acceptable_ms": 10000, "exit_drift_acceptable_abs_pct": 5.0, "exit_truth_gap_clean_ms": 5000, "exit_truth_gap_other_acceptable_ms": 15000, "exit_truth_gap_timestop_acceptable_ms": 45000}`

## Interpretation

- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.
- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.
- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.
- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.
- Phase B remains blocked until feature availability is audited on these labels.
