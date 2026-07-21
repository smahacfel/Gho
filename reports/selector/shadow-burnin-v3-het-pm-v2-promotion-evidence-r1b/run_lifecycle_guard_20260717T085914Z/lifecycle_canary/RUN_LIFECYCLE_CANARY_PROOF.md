# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_CANARY_PASS`
- phase: `lifecycle`
- scope: `shadow-burnin-v3-het-pm-v2-promotion-evidence-r1b`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-r1b.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 69, "NewPoolDetected": 82, "PoolTransaction": 1863, "unknown": 77}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `3752`

## Lifecycle Canary

- status: `PASS`
- shadow_buys_delta: `25`
- shadow_entries_delta: `25`
- shadow_lifecycle_delta: `287`
- legacy_buy_executable_rows: `2`
- shadow_dispatch_closed_rows: `20`
- position_closed_rows: `19`
- exit_filled_rows: `19`
- truth_status_resolved_rows: `224`
- truth_source_canonical_rows: `264`
- final_pnl_pct_present_rows: `38`
- accepted_close_reason_rows: `19`

## Reporter

- status: `PASS`
- exit_code: `0`
- log_path: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-r1b/run_lifecycle_guard_20260717T085914Z/lifecycle_canary/commands/reporter.log`
- output: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-r1b/run_lifecycle_guard_20260717T085914Z/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
- outcome_summary_output: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-r1b/run_lifecycle_guard_20260717T085914Z/lifecycle_canary/selector_lifecycle_canary_summary.json`
- rows_written: `19`
- close_truth_coverage: `19/19`
- truth_status_resolved_rows: `19`
- truth_source_canonical_rows: `19`
- gatekeeper_buy_context_found_rows: `6`
- final_pnl_pct_present_rows: `19`
- exit_fills_total: `19`
- accepted_close_reason_rows: `19`
