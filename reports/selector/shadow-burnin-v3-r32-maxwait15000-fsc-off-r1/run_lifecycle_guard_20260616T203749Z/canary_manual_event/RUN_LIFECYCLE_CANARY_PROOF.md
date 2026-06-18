# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-burnin-v3-r32-maxwait15000-fsc-off-r1`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r32-maxwait15000-fsc-off.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 759, "NewPoolDetected": 770, "PoolTransaction": 59580, "unknown": 1503}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `72908`

## Lifecycle Canary

- status: `PASS`
- accepted_lifecycle_plane: `shadow`
- shadow_status: `PASS`
- shadow_buys_delta: `452`
- shadow_entries_delta: `447`
- shadow_lifecycle_delta: `1206`
- legacy_buy_executable_rows: `423`
- shadow_dispatch_closed_rows: `389`
- position_closed_rows: `378`
- exit_filled_rows: `370`
- truth_status_resolved_rows: `740`
- truth_source_canonical_rows: `756`
- final_pnl_pct_present_rows: `740`
- accepted_close_reason_rows: `378`

## Probe Lifecycle Canary

- status: `PASS`
- probe_transport_delta: `97`
- probe_entries_delta: `97`
- probe_lifecycle_delta: `174`
- position_closed_rows: `87`
- exit_filled_rows: `87`
- truth_status_resolved_rows: `174`
- truth_source_canonical_rows: `174`
- final_pnl_pct_present_rows: `174`
- accepted_close_reason_rows: `87`
- simulated_transport_rows: `87`

## Reporter

- status: `SKIPPED`
