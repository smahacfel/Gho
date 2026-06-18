# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 24, "NewPoolDetected": 26, "PoolTransaction": 484, "unknown": 7}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `982`

## Lifecycle Canary

- status: `PASS`
- shadow_buys_delta: `3`
- shadow_entries_delta: `3`
- shadow_lifecycle_delta: `7`
- legacy_buy_executable_rows: `3`
- shadow_dispatch_closed_rows: `3`
- position_closed_rows: `2`
- exit_filled_rows: `2`
- truth_status_resolved_rows: `4`
- truth_source_canonical_rows: `4`
- final_pnl_pct_present_rows: `4`
- accepted_close_reason_rows: `2`

## Reporter

- status: `SKIPPED`
