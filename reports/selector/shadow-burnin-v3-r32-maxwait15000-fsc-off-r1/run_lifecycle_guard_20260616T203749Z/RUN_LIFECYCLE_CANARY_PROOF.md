# Selector Lifecycle Canary Proof

- status: `FAIL_LIFECYCLE_PROOF`
- claim: `SELECTOR_LIFECYCLE_CANARY_FAIL:FAIL_LIFECYCLE_PROOF`
- phase: `full`
- scope: `shadow-burnin-v3-r32-maxwait15000-fsc-off-r1`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r32-maxwait15000-fsc-off.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 2253, "NewPoolDetected": 2331, "PoolTransaction": 221837, "unknown": 3870}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `278012`

## Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- accepted_lifecycle_plane: `None`
- shadow_status: `FAIL_LIFECYCLE_PROOF`
- shadow_buys_delta: `1385`
- shadow_entries_delta: `1169`
- shadow_lifecycle_delta: `3335`
- legacy_buy_executable_rows: `1087`
- shadow_dispatch_closed_rows: `971`
- position_closed_rows: `974`
- exit_filled_rows: `961`
- truth_status_resolved_rows: `1921`
- truth_source_canonical_rows: `1949`
- final_pnl_pct_present_rows: `1921`
- accepted_close_reason_rows: `974`

## Probe Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- probe_transport_delta: `208`
- probe_entries_delta: `207`
- probe_lifecycle_delta: `354`
- position_closed_rows: `177`
- exit_filled_rows: `177`
- truth_status_resolved_rows: `354`
- truth_source_canonical_rows: `354`
- final_pnl_pct_present_rows: `354`
- accepted_close_reason_rows: `177`
- simulated_transport_rows: `177`

## Reporter

- status: `SKIPPED`

## Errors

- AccountNotFound_delta > 0
- AccountNotFound_delta > 0
