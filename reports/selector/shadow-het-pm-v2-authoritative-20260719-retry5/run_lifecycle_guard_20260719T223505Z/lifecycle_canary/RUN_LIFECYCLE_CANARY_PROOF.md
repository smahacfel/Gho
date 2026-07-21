# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_CANARY_PASS`
- phase: `lifecycle`
- scope: `shadow-het-pm-v2-authoritative-20260719-retry5`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 28, "NewPoolDetected": 72, "PoolTransaction": 3182, "unknown": 28}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `6396`

## Lifecycle Canary

- status: `PASS`
- shadow_buys_delta: `16`
- shadow_entries_delta: `16`
- shadow_lifecycle_delta: `235`
- legacy_buy_executable_rows: `1`
- shadow_dispatch_closed_rows: `15`
- position_closed_rows: `5`
- exit_filled_rows: `5`
- truth_status_resolved_rows: `221`
- truth_source_canonical_rows: `221`
- final_pnl_pct_present_rows: `10`
- accepted_close_reason_rows: `5`

## Reporter

- status: `PASS`
- exit_code: `0`
- log_path: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-het-pm-v2-authoritative-20260719-retry5/run_lifecycle_guard_20260719T223505Z/lifecycle_canary/commands/reporter.log`
- output: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-het-pm-v2-authoritative-20260719-retry5/run_lifecycle_guard_20260719T223505Z/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
- outcome_summary_output: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-het-pm-v2-authoritative-20260719-retry5/run_lifecycle_guard_20260719T223505Z/lifecycle_canary/selector_lifecycle_canary_summary.json`
- rows_written: `5`
- close_truth_coverage: `5/5`
- truth_status_resolved_rows: `5`
- truth_source_canonical_rows: `5`
- gatekeeper_buy_context_found_rows: `2`
- final_pnl_pct_present_rows: `5`
- exit_fills_total: `5`
- accepted_close_reason_rows: `5`
