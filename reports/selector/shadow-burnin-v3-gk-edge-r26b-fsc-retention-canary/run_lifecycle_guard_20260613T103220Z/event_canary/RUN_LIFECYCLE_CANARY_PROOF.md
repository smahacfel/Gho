# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 193, "NewPoolDetected": 195, "PoolTransaction": 11453, "unknown": 124}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `22860`

## Lifecycle Canary

- status: `PASS`
- shadow_buys_delta: `38`
- shadow_entries_delta: `38`
- shadow_lifecycle_delta: `100`
- legacy_buy_executable_rows: `3`
- shadow_dispatch_closed_rows: `33`
- position_closed_rows: `31`
- exit_filled_rows: `31`
- truth_status_resolved_rows: `62`
- truth_source_canonical_rows: `62`
- final_pnl_pct_present_rows: `62`
- accepted_close_reason_rows: `31`

## Reporter

- status: `SKIPPED`
