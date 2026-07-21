# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 29, "NewPoolDetected": 49, "PoolTransaction": 1723, "unknown": 38}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `3276`

## Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- shadow_buys_delta: `16`
- shadow_entries_delta: `16`
- shadow_lifecycle_delta: `191`
- legacy_buy_executable_rows: `0`
- shadow_dispatch_closed_rows: `16`
- position_closed_rows: `9`
- exit_filled_rows: `9`
- truth_status_resolved_rows: `149`
- truth_source_canonical_rows: `175`
- final_pnl_pct_present_rows: `18`
- accepted_close_reason_rows: `9`

## Reporter

- status: `SKIPPED`
