# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_CANARY_PASS`
- phase: `lifecycle`
- scope: `shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a`
- config: `/tmp/gho-het-pm-v2-validation-runtime/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 222, "NewPoolDetected": 249, "PoolTransaction": 24798, "unknown": 515}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `44272`

## Lifecycle Canary

- status: `PASS`
- shadow_buys_delta: `142`
- shadow_entries_delta: `141`
- shadow_lifecycle_delta: `2840`
- legacy_buy_executable_rows: `3`
- shadow_dispatch_closed_rows: `134`
- position_closed_rows: `128`
- exit_filled_rows: `129`
- truth_status_resolved_rows: `2233`
- truth_source_canonical_rows: `2708`
- final_pnl_pct_present_rows: `257`
- accepted_close_reason_rows: `128`

## Reporter

- status: `PASS`
- exit_code: `0`
- log_path: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a/run_lifecycle_guard_20260718T150247Z/lifecycle_canary/commands/reporter.log`
- output: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a/run_lifecycle_guard_20260718T150247Z/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
- outcome_summary_output: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a/run_lifecycle_guard_20260718T150247Z/lifecycle_canary/selector_lifecycle_canary_summary.json`
- rows_written: `129`
- close_truth_coverage: `129/129`
- truth_status_resolved_rows: `129`
- truth_source_canonical_rows: `129`
- gatekeeper_buy_context_found_rows: `67`
- final_pnl_pct_present_rows: `129`
- exit_fills_total: `129`
- accepted_close_reason_rows: `129`
