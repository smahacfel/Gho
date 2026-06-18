# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_CANARY_PASS`
- phase: `lifecycle`
- scope: `shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 194, "NewPoolDetected": 195, "PoolTransaction": 11473, "unknown": 124}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `22901`

## Lifecycle Canary

- status: `PASS`
- shadow_buys_delta: `38`
- shadow_entries_delta: `38`
- shadow_lifecycle_delta: `100`
- legacy_buy_executable_rows: `3`
- shadow_dispatch_closed_rows: `33`
- position_closed_rows: `31`
- exit_filled_rows: `31`
- truth_status_resolved_rows: `62`
- truth_source_canonical_rows: `62`
- final_pnl_pct_present_rows: `62`
- accepted_close_reason_rows: `31`

## Reporter

- status: `PASS`
- exit_code: `0`
- log_path: `/root/Gho/reports/selector/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary/run_lifecycle_guard_20260613T103220Z/lifecycle_canary/commands/reporter.log`
- output: `/root/Gho/reports/selector/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary/run_lifecycle_guard_20260613T103220Z/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
- outcome_summary_output: `/root/Gho/reports/selector/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary/run_lifecycle_guard_20260613T103220Z/lifecycle_canary/selector_lifecycle_canary_summary.json`
- rows_written: `31`
- close_truth_coverage: `31/31`
- truth_status_resolved_rows: `31`
- truth_source_canonical_rows: `31`
- gatekeeper_buy_context_found_rows: `31`
- final_pnl_pct_present_rows: `31`
- exit_fills_total: `31`
- accepted_close_reason_rows: `31`
