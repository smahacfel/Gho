# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-burnin-v3-r44-timestop-v2-observe-target50-stop50-fsc-off-r1`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r44-timestop-v2-observe-target50-stop50-fsc-off-r1.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 304, "NewPoolDetected": 319, "PoolTransaction": 16787, "unknown": 54}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `29548`

## Lifecycle Canary

- status: `PASS`
- shadow_buys_delta: `16`
- shadow_entries_delta: `16`
- shadow_lifecycle_delta: `44`
- legacy_buy_executable_rows: `2`
- shadow_dispatch_closed_rows: `14`
- position_closed_rows: `14`
- exit_filled_rows: `14`
- truth_status_resolved_rows: `28`
- truth_source_canonical_rows: `28`
- final_pnl_pct_present_rows: `28`
- accepted_close_reason_rows: `14`

## Reporter

- status: `SKIPPED`
