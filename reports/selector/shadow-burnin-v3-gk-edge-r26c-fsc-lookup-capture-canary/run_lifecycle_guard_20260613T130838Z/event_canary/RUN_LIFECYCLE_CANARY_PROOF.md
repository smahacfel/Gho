# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 12, "NewPoolDetected": 14, "PoolTransaction": 381, "unknown": 2}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `786`

## Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- shadow_buys_delta: `2`
- shadow_entries_delta: `2`
- shadow_lifecycle_delta: `2`
- legacy_buy_executable_rows: `0`
- shadow_dispatch_closed_rows: `2`
- position_closed_rows: `0`
- exit_filled_rows: `0`
- truth_status_resolved_rows: `0`
- truth_source_canonical_rows: `0`
- final_pnl_pct_present_rows: `0`
- accepted_close_reason_rows: `0`

## Reporter

- status: `SKIPPED`
