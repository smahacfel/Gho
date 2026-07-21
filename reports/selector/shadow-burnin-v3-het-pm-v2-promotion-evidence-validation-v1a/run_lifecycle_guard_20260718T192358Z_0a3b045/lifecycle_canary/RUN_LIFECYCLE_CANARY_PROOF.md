# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_CANARY_PASS`
- phase: `lifecycle`
- scope: `shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a`
- config: `/tmp/gho-het-pm-v2-validation-runtime-v2/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 241, "NewPoolDetected": 264, "PoolTransaction": 29229, "unknown": 503}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `42894`

## Lifecycle Canary

- status: `PASS`
- shadow_buys_delta: `145`
- shadow_entries_delta: `143`
- shadow_lifecycle_delta: `2556`
- legacy_buy_executable_rows: `8`
- shadow_dispatch_closed_rows: `134`
- position_closed_rows: `124`
- exit_filled_rows: `124`
- truth_status_resolved_rows: `2021`
- truth_source_canonical_rows: `2429`
- final_pnl_pct_present_rows: `248`
- accepted_close_reason_rows: `124`

## Reporter

- status: `PASS`
- exit_code: `0`
- log_path: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a/run_lifecycle_guard_20260718T192358Z_0a3b045/lifecycle_canary/commands/reporter.log`
- output: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a/run_lifecycle_guard_20260718T192358Z_0a3b045/lifecycle_canary/selector_lifecycle_canary_report.jsonl`
- outcome_summary_output: `/root/Gho_dynamic_exit_v1_pr2b/reports/selector/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a/run_lifecycle_guard_20260718T192358Z_0a3b045/lifecycle_canary/selector_lifecycle_canary_summary.json`
- rows_written: `124`
- close_truth_coverage: `124/124`
- truth_status_resolved_rows: `124`
- truth_source_canonical_rows: `124`
- gatekeeper_buy_context_found_rows: `47`
- final_pnl_pct_present_rows: `124`
- exit_fills_total: `124`
- accepted_close_reason_rows: `124`
