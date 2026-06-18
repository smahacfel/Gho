# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-burnin-v3-r32-maxwait15000-fsc-off-r1`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r32-maxwait15000-fsc-off.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 782, "NewPoolDetected": 793, "PoolTransaction": 61784, "unknown": 1536}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `75299`

## Lifecycle Canary

- status: `PASS`
- accepted_lifecycle_plane: `shadow`
- shadow_status: `PASS`
- shadow_buys_delta: `460`
- shadow_entries_delta: `455`
- shadow_lifecycle_delta: `1232`
- legacy_buy_executable_rows: `432`
- shadow_dispatch_closed_rows: `396`
- position_closed_rows: `389`
- exit_filled_rows: `381`
- truth_status_resolved_rows: `762`
- truth_source_canonical_rows: `778`
- final_pnl_pct_present_rows: `762`
- accepted_close_reason_rows: `389`

## Probe Lifecycle Canary

- status: `PASS`
- probe_transport_delta: `101`
- probe_entries_delta: `101`
- probe_lifecycle_delta: `174`
- position_closed_rows: `88`
- exit_filled_rows: `88`
- truth_status_resolved_rows: `176`
- truth_source_canonical_rows: `176`
- final_pnl_pct_present_rows: `176`
- accepted_close_reason_rows: `88`
- simulated_transport_rows: `91`

## Reporter

- status: `SKIPPED`
