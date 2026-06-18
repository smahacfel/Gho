# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_CANARY_PASS`
- phase: `lifecycle`
- scope: `shadow-burnin-v3-r27b-fsc-lookback-window-canary`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r27b-fsc-lookback-window-canary.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 29, "NewPoolDetected": 33, "PoolTransaction": 773, "unknown": 10}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `1532`

## Lifecycle Canary

- status: `PASS`
- shadow_buys_delta: `4`
- shadow_entries_delta: `4`
- shadow_lifecycle_delta: `8`
- legacy_buy_executable_rows: `4`
- shadow_dispatch_closed_rows: `4`
- position_closed_rows: `2`
- exit_filled_rows: `2`
- truth_status_resolved_rows: `4`
- truth_source_canonical_rows: `4`
- final_pnl_pct_present_rows: `4`
- accepted_close_reason_rows: `2`

## Reporter

- status: `PASS`
- exit_code: `0`
- log_path: `/root/Gho/reports/selector/shadow-burnin-v3-r27b-fsc-lookback-window-canary/run_lifecycle_guard_20260613T_r27b/lifecycle_canary/commands/reporter.log`
- output: `/root/Gho/reports/selector/shadow-burnin-v3-r27b-fsc-lookback-window-canary/run_lifecycle_guard_20260613T_r27b/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
- outcome_summary_output: `/root/Gho/reports/selector/shadow-burnin-v3-r27b-fsc-lookback-window-canary/run_lifecycle_guard_20260613T_r27b/lifecycle_canary/selector_lifecycle_canary_summary.json`
- rows_written: `2`
- close_truth_coverage: `2/2`
- truth_status_resolved_rows: `2`
- truth_source_canonical_rows: `2`
- gatekeeper_buy_context_found_rows: `2`
- final_pnl_pct_present_rows: `2`
- exit_fills_total: `2`
- accepted_close_reason_rows: `2`
