# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 19, "NewPoolDetected": 23, "PoolTransaction": 1261, "unknown": 8}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `2504`

## Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- shadow_buys_delta: `5`
- shadow_entries_delta: `5`
- shadow_lifecycle_delta: `7`
- legacy_buy_executable_rows: `0`
- shadow_dispatch_closed_rows: `5`
- position_closed_rows: `1`
- exit_filled_rows: `1`
- truth_status_resolved_rows: `2`
- truth_source_canonical_rows: `2`
- final_pnl_pct_present_rows: `2`
- accepted_close_reason_rows: `1`

## Reporter

- status: `SKIPPED`
