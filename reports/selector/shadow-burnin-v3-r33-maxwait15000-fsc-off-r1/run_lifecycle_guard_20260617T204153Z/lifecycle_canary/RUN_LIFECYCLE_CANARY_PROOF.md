# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_CANARY_PASS`
- phase: `lifecycle`
- scope: `shadow-burnin-v3-r33-maxwait15000-fsc-off-r1`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r33-maxwait15000-fsc-off-r1.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 358, "NewPoolDetected": 418, "PoolTransaction": 18003, "unknown": 142}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `35850`

## Lifecycle Canary

- status: `PASS`
- accepted_lifecycle_plane: `shadow`
- shadow_status: `PASS`
- shadow_buys_delta: `138`
- shadow_entries_delta: `134`
- shadow_lifecycle_delta: `210`
- legacy_buy_executable_rows: `25`
- shadow_dispatch_closed_rows: `24`
- position_closed_rows: `25`
- exit_filled_rows: `24`
- truth_status_resolved_rows: `49`
- truth_source_canonical_rows: `49`
- final_pnl_pct_present_rows: `49`
- accepted_close_reason_rows: `25`

## Probe Lifecycle Canary

- status: `PASS`
- probe_transport_delta: `1`
- probe_entries_delta: `1`
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
- log_path: `/root/Gho/reports/selector/shadow-burnin-v3-r33-maxwait15000-fsc-off-r1/run_lifecycle_guard_20260617T204153Z/lifecycle_canary/commands/reporter.log`
- output: `/root/Gho/reports/selector/shadow-burnin-v3-r33-maxwait15000-fsc-off-r1/run_lifecycle_guard_20260617T204153Z/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
- outcome_summary_output: `/root/Gho/reports/selector/shadow-burnin-v3-r33-maxwait15000-fsc-off-r1/run_lifecycle_guard_20260617T204153Z/lifecycle_canary/selector_lifecycle_canary_summary.json`
- rows_written: `36`
- close_truth_coverage: `36/36`
- truth_status_resolved_rows: `36`
- truth_source_canonical_rows: `36`
- gatekeeper_buy_context_found_rows: `2`
- final_pnl_pct_present_rows: `36`
- exit_fills_total: `36`
- accepted_close_reason_rows: `36`
