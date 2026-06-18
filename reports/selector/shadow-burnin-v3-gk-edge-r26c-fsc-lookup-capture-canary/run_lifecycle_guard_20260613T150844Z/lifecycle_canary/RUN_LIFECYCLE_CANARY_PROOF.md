# Selector Lifecycle Canary Proof

- status: `FAIL_LIFECYCLE_PROOF`
- claim: `SELECTOR_LIFECYCLE_CANARY_FAIL:FAIL_LIFECYCLE_PROOF`
- phase: `lifecycle`
- scope: `shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 57, "NewPoolDetected": 65, "PoolTransaction": 3351, "unknown": 37}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `6574`

## Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- shadow_buys_delta: `12`
- shadow_entries_delta: `12`
- shadow_lifecycle_delta: `30`
- legacy_buy_executable_rows: `0`
- shadow_dispatch_closed_rows: `12`
- position_closed_rows: `9`
- exit_filled_rows: `9`
- truth_status_resolved_rows: `18`
- truth_source_canonical_rows: `18`
- final_pnl_pct_present_rows: `18`
- accepted_close_reason_rows: `9`

## Reporter

- status: `SKIPPED`

## Errors

- legacy_buy executable rows <= 0
