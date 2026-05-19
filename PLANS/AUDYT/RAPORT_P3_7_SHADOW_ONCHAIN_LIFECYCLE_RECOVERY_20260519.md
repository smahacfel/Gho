# P3.7 Shadow-Onchain Lifecycle Recovery

Namespace: `shadow-burnin-buy-heavy-rerun`
Config: `configs/rollout/shadow-burnin-buy-heavy-rerun-report-only.toml`
Input: `logs/shadow_run/shadow-burnin-buy-heavy-rerun/shadow_onchain_lifecycle_report_all.jsonl`
Phase E recovery status: `accepted`

## Acceptance Checks

- `rows_total_gt_0`: `true`
- `analysis_status_ok_gt_0`: `true`
- `truth_status_resolved_gt_0`: `true`
- `position_closed_gt_0`: `true`
- `gatekeeper_buy_context_found_gt_0`: `true`
- `entry_truth_gap_distribution_present`: `true`
- `exit_truth_gap_distribution_present`: `true`
- `curve_finality_distribution_present`: `true`
- `pnl_positive_or_negative_counted`: `true`

## Counts

- `rows_total`: `2386`
- `analysis_status_counts`: `{"ok": 2386}`
- `truth_status_counts`: `{"resolved": 2386}`
- `truth_source_counts`: `{"canonical_account_state_snapshot": 2386}`
- `curve_finality_entry_counts`: `{"speculative": 2386}`
- `curve_finality_exit_fill_counts`: `{"speculative": 2386}`
- `execution_verification_class_hint_entry_counts`: `{"shadow_onchain_speculative_snapshot_verified": 2386}`
- `execution_verification_class_hint_exit_fill_counts`: `{"shadow_onchain_speculative_snapshot_verified": 2386}`
- `position_closed_rows`: `2386`
- `exit_filled_rows`: `2386`
- `final_pnl_pct_counts`: `{"negative": 1807, "positive": 579}`
- `close_reason_counts`: `{"StopLoss": 497, "Target": 500, "TimeStop": 1389}`
- `gatekeeper_buy_context_found_count`: `571`
- `gatekeeper_buy_context_missing_count`: `1815`
- `shadow_execution_outcome_counts`: `{"shadow_simulated": 2386}`

## Distributions

- `entry_truth_gap_ms`: `{"count": 2386, "max": 9788.0, "mean": 1948.1974015088012, "min": 0.0, "p50": 928.5, "p90": 5637.5, "p99": 8083.15}`
- `exit_truth_gap_max_per_position_ms`: `{"count": 2386, "max": 40135.0, "mean": 18204.778290025148, "min": 0.0, "p50": 30107.0, "p90": 32529.5, "p99": 37516.05}`
- `exit_truth_gap_fill_ms`: `{"count": 2386, "max": 40135.0, "mean": 18204.778290025148, "min": 0.0, "p50": 30107.0, "p90": 32529.5, "p99": 37516.05}`
- `entry_drift_vs_onchain_executable_pct`: `{"count": 2386, "max": 101.1912107164755, "mean": 0.5920974428804527, "min": -42.56319404397683, "p50": 0.0, "p90": 0.0, "p99": 34.9640859029559}`
- `entry_abs_drift_vs_onchain_executable_pct`: `{"count": 2386, "max": 101.1912107164755, "mean": 1.718053729278956, "min": 0.0, "p50": 0.0, "p90": 2.0365643635255393, "p99": 36.16134322564436}`
- `exit_drift_vs_onchain_executable_pct`: `{"count": 2386, "max": 29.625039710742595, "mean": -0.016199592006433707, "min": -27.23193039277264, "p50": -1.4714239648938587e-05, "p90": -8.451864097480666e-06, "p99": 0.0}`
- `exit_abs_drift_vs_onchain_executable_pct`: `{"count": 2386, "max": 29.625039710742595, "mean": 0.0877463848024166, "min": 0.0, "p50": 1.4749289733995141e-05, "p90": 3.2653765569268245e-05, "p99": 1.3174437410355675}`
- `decision_to_execution_ms`: `{"count": 2386, "max": 3960.0, "mean": 246.8134953897737, "min": 116.0, "p50": 228.0, "p90": 279.0, "p99": 801.0}`
- `detection_to_execution_ms`: `{"count": 571, "max": 9198.0, "mean": 8292.45183887916, "min": 8178.0, "p50": 8274.0, "p90": 8320.0, "p99": 8900.9}`

## Scope Notes

- This report summarizes shadow-onchain lifecycle recovery only.
- Shadow lifecycle proof is not live inclusion and does not prove strategy edge.
- Non-finalized finality values are snapshot/degraded proof hints, not finalized proof.
- Phase B feature prototype remains blocked until labeler and feature availability audit are complete.

## Base Script Notes

Primary `shadow_onchain_lifecycle_report.py` stdout for `shadow-burnin-buy-heavy-rerun` reported:

- `rows_written=2386`
- `scope_candidates=7816`
- `close_truth_coverage=2388/2388 failed=0 pct=100.00`
- `skipped missing_transport_record=2`

The first run with `configs/rollout/shadow-burnin-buy-heavy.local.toml` produced lifecycle truth but `gatekeeper_buy_context_found=0`, because the historical `gatekeeper_v2_buys.jsonl` files are stored under the rollout root `v2.2/` and `v2.5/` directories, while that runtime config points `oracle.decision_log_path` to the `decisions/` subdirectory. The accepted recovery run used `configs/rollout/shadow-burnin-buy-heavy-rerun-report-only.toml`, which is report-only and must not be used for runtime.

Secondary sanity run for `shadow-burnin-v3-p36-sample-r13-primary-only` reported:

- `rows_written=0`
- `scope_candidates=1`
- `close_truth_coverage=0/0 failed=0 pct=n/a`
- `skipped no_closed_positions_in_scope=1`
