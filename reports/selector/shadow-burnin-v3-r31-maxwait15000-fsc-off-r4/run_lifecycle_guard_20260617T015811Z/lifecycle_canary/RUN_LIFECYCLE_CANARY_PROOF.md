# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_CANARY_PASS`
- phase: `lifecycle`
- scope: `shadow-burnin-v3-r31-maxwait15000-fsc-off-r4`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r31-maxwait15000-fsc-off.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 99, "NewPoolDetected": 104, "PoolTransaction": 3513, "unknown": 105}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `6689`

## Lifecycle Canary

- status: `PASS`
- accepted_lifecycle_plane: `probe`
- shadow_status: `FAIL_LIFECYCLE_PROOF`
- shadow_buys_delta: `23`
- shadow_entries_delta: `23`
- shadow_lifecycle_delta: `57`
- legacy_buy_executable_rows: `0`
- shadow_dispatch_closed_rows: `0`
- position_closed_rows: `0`
- exit_filled_rows: `0`
- truth_status_resolved_rows: `0`
- truth_source_canonical_rows: `0`
- final_pnl_pct_present_rows: `0`
- accepted_close_reason_rows: `0`

## Probe Lifecycle Canary

- status: `PASS`
- probe_transport_delta: `25`
- probe_entries_delta: `25`
- probe_lifecycle_delta: `42`
- position_closed_rows: `21`
- exit_filled_rows: `21`
- truth_status_resolved_rows: `42`
- truth_source_canonical_rows: `42`
- final_pnl_pct_present_rows: `42`
- accepted_close_reason_rows: `21`
- simulated_transport_rows: `26`

## Reporter

- status: `PASS`
- artifact_plane: `probe`
- exit_code: `0`
- log_path: `/root/Gho/reports/selector/shadow-burnin-v3-r31-maxwait15000-fsc-off-r4/run_lifecycle_guard_20260617T015811Z/lifecycle_canary/commands/reporter.log`
- output: `/root/Gho/reports/selector/shadow-burnin-v3-r31-maxwait15000-fsc-off-r4/run_lifecycle_guard_20260617T015811Z/lifecycle_canary/probe_selector_lifecycle_canary_report.jsonl`
- outcome_summary_output: `/root/Gho/reports/selector/shadow-burnin-v3-r31-maxwait15000-fsc-off-r4/run_lifecycle_guard_20260617T015811Z/lifecycle_canary/probe_selector_lifecycle_canary_summary.json`
- rows_written: `27`
- close_truth_coverage: `27/27`
- truth_status_resolved_rows: `27`
- truth_source_canonical_rows: `27`
- gatekeeper_buy_context_found_rows: `4`
- final_pnl_pct_present_rows: `27`
- exit_fills_total: `27`
- accepted_close_reason_rows: `27`
