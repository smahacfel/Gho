# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_CANARY_PASS`
- phase: `lifecycle`
- scope: `shadow-burnin-v3-het-pm-v2-promotion-evidence-r1`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-r1.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 199, "NewPoolDetected": 219, "PoolTransaction": 3351, "unknown": 38}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `7004`

## Lifecycle Canary

- status: `PASS`
- shadow_buys_delta: `25`
- shadow_entries_delta: `25`
- shadow_lifecycle_delta: `124`
- legacy_buy_executable_rows: `1`
- shadow_dispatch_closed_rows: `23`
- position_closed_rows: `5`
- exit_filled_rows: `5`
- truth_status_resolved_rows: `84`
- truth_source_canonical_rows: `99`
- final_pnl_pct_present_rows: `10`
- accepted_close_reason_rows: `5`

## Reporter

- status: `PASS`
- exit_code: `0`
- log_path: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-r1/run_lifecycle_guard_20260717T080700Z/lifecycle_canary/commands/reporter.log`
- output: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-r1/run_lifecycle_guard_20260717T080700Z/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
- outcome_summary_output: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-r1/run_lifecycle_guard_20260717T080700Z/lifecycle_canary/selector_lifecycle_canary_summary.json`
- rows_written: `5`
- close_truth_coverage: `5/5`
- truth_status_resolved_rows: `5`
- truth_source_canonical_rows: `5`
- gatekeeper_buy_context_found_rows: `2`
- final_pnl_pct_present_rows: `5`
- exit_fills_total: `5`
- accepted_close_reason_rows: `5`
