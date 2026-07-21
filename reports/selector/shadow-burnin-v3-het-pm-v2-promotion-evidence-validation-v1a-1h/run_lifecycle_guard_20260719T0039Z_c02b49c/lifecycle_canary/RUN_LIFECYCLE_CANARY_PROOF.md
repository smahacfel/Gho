# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_CANARY_PASS`
- phase: `lifecycle`
- scope: `shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a-1h`
- config: `/tmp/het-pm-v2-runtime-c02b49c/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 334, "NewPoolDetected": 353, "PoolTransaction": 20642, "unknown": 347}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `36846`

## Lifecycle Canary

- status: `PASS`
- shadow_buys_delta: `94`
- shadow_entries_delta: `94`
- shadow_lifecycle_delta: `1656`
- legacy_buy_executable_rows: `2`
- shadow_dispatch_closed_rows: `89`
- position_closed_rows: `86`
- exit_filled_rows: `86`
- truth_status_resolved_rows: `1310`
- truth_source_canonical_rows: `1564`
- final_pnl_pct_present_rows: `172`
- accepted_close_reason_rows: `86`

## Reporter

- status: `PASS`
- exit_code: `0`
- log_path: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a-1h/run_lifecycle_guard_20260719T0039Z_c02b49c/lifecycle_canary/commands/reporter.log`
- output: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a-1h/run_lifecycle_guard_20260719T0039Z_c02b49c/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
- outcome_summary_output: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a-1h/run_lifecycle_guard_20260719T0039Z_c02b49c/lifecycle_canary/selector_lifecycle_canary_summary.json`
- rows_written: `86`
- close_truth_coverage: `86/86`
- truth_status_resolved_rows: `86`
- truth_source_canonical_rows: `86`
- gatekeeper_buy_context_found_rows: `37`
- final_pnl_pct_present_rows: `86`
- exit_fills_total: `86`
- accepted_close_reason_rows: `86`
