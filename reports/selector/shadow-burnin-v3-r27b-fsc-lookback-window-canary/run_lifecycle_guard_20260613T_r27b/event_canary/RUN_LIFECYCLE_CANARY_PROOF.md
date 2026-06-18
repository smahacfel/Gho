# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-burnin-v3-r27b-fsc-lookback-window-canary`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r27b-fsc-lookback-window-canary.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 29, "NewPoolDetected": 33, "PoolTransaction": 764, "unknown": 10}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `1526`

## Lifecycle Canary

- status: `PASS`
- shadow_buys_delta: `4`
- shadow_entries_delta: `4`
- shadow_lifecycle_delta: `8`
- legacy_buy_executable_rows: `4`
- shadow_dispatch_closed_rows: `4`
- position_closed_rows: `2`
- exit_filled_rows: `2`
- truth_status_resolved_rows: `4`
- truth_source_canonical_rows: `4`
- final_pnl_pct_present_rows: `4`
- accepted_close_reason_rows: `2`

## Reporter

- status: `SKIPPED`
