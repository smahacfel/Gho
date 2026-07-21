# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_CANARY_PASS`
- phase: `lifecycle`
- scope: `shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 168, "NewPoolDetected": 210, "PoolTransaction": 12787, "unknown": 320}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `19696`

## Lifecycle Canary

- status: `PASS`
- shadow_buys_delta: `91`
- shadow_entries_delta: `90`
- shadow_lifecycle_delta: `1784`
- legacy_buy_executable_rows: `1`
- shadow_dispatch_closed_rows: `85`
- position_closed_rows: `78`
- exit_filled_rows: `78`
- truth_status_resolved_rows: `1410`
- truth_source_canonical_rows: `1701`
- final_pnl_pct_present_rows: `156`
- accepted_close_reason_rows: `78`

## Reporter

- status: `PASS`
- exit_code: `0`
- log_path: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a/run_lifecycle_guard_20260717T155637Z/lifecycle_canary/commands/reporter.log`
- output: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a/run_lifecycle_guard_20260717T155637Z/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
- outcome_summary_output: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a/run_lifecycle_guard_20260717T155637Z/lifecycle_canary/selector_lifecycle_canary_summary.json`
- rows_written: `79`
- close_truth_coverage: `79/79`
- truth_status_resolved_rows: `79`
- truth_source_canonical_rows: `79`
- gatekeeper_buy_context_found_rows: `48`
- final_pnl_pct_present_rows: `79`
- exit_fills_total: `79`
- accepted_close_reason_rows: `79`
