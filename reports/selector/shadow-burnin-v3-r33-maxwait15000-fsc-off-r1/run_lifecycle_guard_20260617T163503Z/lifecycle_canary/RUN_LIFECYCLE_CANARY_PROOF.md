# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_CANARY_PASS`
- phase: `lifecycle`
- scope: `shadow-burnin-v3-r33-maxwait15000-fsc-off-r1`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r33-maxwait15000-fsc-off-r1.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 4, "NewPoolDetected": 7, "PoolTransaction": 148, "unknown": 6}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `302`

## Lifecycle Canary

- status: `PASS`
- accepted_lifecycle_plane: `shadow`
- shadow_status: `PASS`
- shadow_buys_delta: `2`
- shadow_entries_delta: `2`
- shadow_lifecycle_delta: `6`
- legacy_buy_executable_rows: `2`
- shadow_dispatch_closed_rows: `2`
- position_closed_rows: `2`
- exit_filled_rows: `2`
- truth_status_resolved_rows: `4`
- truth_source_canonical_rows: `4`
- final_pnl_pct_present_rows: `4`
- accepted_close_reason_rows: `2`

## Probe Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- probe_transport_delta: `0`
- probe_entries_delta: `0`
- probe_lifecycle_delta: `0`
- position_closed_rows: `0`
- exit_filled_rows: `0`
- truth_status_resolved_rows: `0`
- truth_source_canonical_rows: `0`
- final_pnl_pct_present_rows: `0`
- accepted_close_reason_rows: `0`
- simulated_transport_rows: `0`

## Reporter

- status: `PASS`
- artifact_plane: `shadow`
- exit_code: `0`
- log_path: `/root/Gho/reports/selector/shadow-burnin-v3-r33-maxwait15000-fsc-off-r1/run_lifecycle_guard_20260617T163503Z/lifecycle_canary/commands/reporter.log`
- output: `/root/Gho/reports/selector/shadow-burnin-v3-r33-maxwait15000-fsc-off-r1/run_lifecycle_guard_20260617T163503Z/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
- outcome_summary_output: `/root/Gho/reports/selector/shadow-burnin-v3-r33-maxwait15000-fsc-off-r1/run_lifecycle_guard_20260617T163503Z/lifecycle_canary/selector_lifecycle_canary_summary.json`
- rows_written: `2`
- close_truth_coverage: `2/2`
- truth_status_resolved_rows: `2`
- truth_source_canonical_rows: `2`
- gatekeeper_buy_context_found_rows: `2`
- final_pnl_pct_present_rows: `2`
- exit_fills_total: `2`
- accepted_close_reason_rows: `2`
