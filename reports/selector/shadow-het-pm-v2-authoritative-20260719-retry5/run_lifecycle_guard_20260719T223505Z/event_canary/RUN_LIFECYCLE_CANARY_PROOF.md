# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-het-pm-v2-authoritative-20260719-retry5`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 10, "NewPoolDetected": 35, "PoolTransaction": 1058, "unknown": 3}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `2198`

## Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- shadow_buys_delta: `3`
- shadow_entries_delta: `3`
- shadow_lifecycle_delta: `16`
- legacy_buy_executable_rows: `0`
- shadow_dispatch_closed_rows: `3`
- position_closed_rows: `0`
- exit_filled_rows: `0`
- truth_status_resolved_rows: `13`
- truth_source_canonical_rows: `13`
- final_pnl_pct_present_rows: `0`
- accepted_close_reason_rows: `0`

## Reporter

- status: `SKIPPED`
