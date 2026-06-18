# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_CANARY_PASS`
- phase: `lifecycle`
- scope: `shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 24, "NewPoolDetected": 27, "PoolTransaction": 503, "unknown": 7}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `1020`

## Lifecycle Canary

- status: `PASS`
- shadow_buys_delta: `3`
- shadow_entries_delta: `3`
- shadow_lifecycle_delta: `7`
- legacy_buy_executable_rows: `3`
- shadow_dispatch_closed_rows: `3`
- position_closed_rows: `2`
- exit_filled_rows: `2`
- truth_status_resolved_rows: `4`
- truth_source_canonical_rows: `4`
- final_pnl_pct_present_rows: `4`
- accepted_close_reason_rows: `2`

## Reporter

- status: `PASS`
- exit_code: `0`
- log_path: `/root/Gho/reports/selector/shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary/run_lifecycle_guard_20260613T153924Z/lifecycle_canary/commands/reporter.log`
- output: `/root/Gho/reports/selector/shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary/run_lifecycle_guard_20260613T153924Z/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
- outcome_summary_output: `/root/Gho/reports/selector/shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary/run_lifecycle_guard_20260613T153924Z/lifecycle_canary/selector_lifecycle_canary_summary.json`
- rows_written: `2`
- close_truth_coverage: `2/2`
- truth_status_resolved_rows: `2`
- truth_source_canonical_rows: `2`
- gatekeeper_buy_context_found_rows: `2`
- final_pnl_pct_present_rows: `2`
- exit_fills_total: `2`
- accepted_close_reason_rows: `2`
