# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_CANARY_PASS`
- phase: `lifecycle`
- scope: `shadow-burnin-v3-r31-maxwait15000-fsc-off-r4`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r31-maxwait15000-fsc-off.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 43, "NewPoolDetected": 46, "PoolTransaction": 3705, "unknown": 114}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `9042`

## Lifecycle Canary

- status: `PASS`
- accepted_lifecycle_plane: `shadow`
- shadow_status: `PASS`
- shadow_buys_delta: `34`
- shadow_entries_delta: `34`
- shadow_lifecycle_delta: `88`
- legacy_buy_executable_rows: `5`
- shadow_dispatch_closed_rows: `3`
- position_closed_rows: `7`
- exit_filled_rows: `6`
- truth_status_resolved_rows: `13`
- truth_source_canonical_rows: `13`
- final_pnl_pct_present_rows: `13`
- accepted_close_reason_rows: `7`

## Probe Lifecycle Canary

- status: `PASS`
- probe_transport_delta: `4`
- probe_entries_delta: `4`
- probe_lifecycle_delta: `2`
- position_closed_rows: `1`
- exit_filled_rows: `1`
- truth_status_resolved_rows: `2`
- truth_source_canonical_rows: `2`
- final_pnl_pct_present_rows: `2`
- accepted_close_reason_rows: `1`
- simulated_transport_rows: `1`

## Reporter

- status: `PASS`
- artifact_plane: `shadow`
- exit_code: `0`
- log_path: `/root/Gho/reports/selector/shadow-burnin-v3-r31-maxwait15000-fsc-off-r4/run_lifecycle_guard_20260616T083520Z/lifecycle_canary/commands/reporter.log`
- output: `/root/Gho/reports/selector/shadow-burnin-v3-r31-maxwait15000-fsc-off-r4/run_lifecycle_guard_20260616T083520Z/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
- outcome_summary_output: `/root/Gho/reports/selector/shadow-burnin-v3-r31-maxwait15000-fsc-off-r4/run_lifecycle_guard_20260616T083520Z/lifecycle_canary/selector_lifecycle_canary_summary.json`
- rows_written: `37`
- close_truth_coverage: `37/37`
- truth_status_resolved_rows: `37`
- truth_source_canonical_rows: `37`
- gatekeeper_buy_context_found_rows: `37`
- final_pnl_pct_present_rows: `37`
- exit_fills_total: `37`
- accepted_close_reason_rows: `37`
