# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-burnin-v3-r44-timestop-v2-observe-target50-stop50-fsc-off-r1`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r44-timestop-v2-observe-target50-stop50-fsc-off-r1.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 251, "NewPoolDetected": 256, "PoolTransaction": 14159, "unknown": 28}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `28334`

## Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- shadow_buys_delta: `9`
- shadow_entries_delta: `9`
- shadow_lifecycle_delta: `359`
- legacy_buy_executable_rows: `0`
- shadow_dispatch_closed_rows: `9`
- position_closed_rows: `7`
- exit_filled_rows: `7`
- truth_status_resolved_rows: `350`
- truth_source_canonical_rows: `350`
- final_pnl_pct_present_rows: `14`
- accepted_close_reason_rows: `7`

## Reporter

- status: `SKIPPED`
