# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-burnin-v3-r33-maxwait15000-fsc-off-r1`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r33-maxwait15000-fsc-off-r1.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 339, "NewPoolDetected": 411, "PoolTransaction": 17522, "unknown": 142}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `35366`

## Lifecycle Canary

- status: `PASS`
- accepted_lifecycle_plane: `shadow`
- shadow_status: `PASS`
- shadow_buys_delta: `129`
- shadow_entries_delta: `126`
- shadow_lifecycle_delta: `199`
- legacy_buy_executable_rows: `25`
- shadow_dispatch_closed_rows: `24`
- position_closed_rows: `25`
- exit_filled_rows: `24`
- truth_status_resolved_rows: `49`
- truth_source_canonical_rows: `49`
- final_pnl_pct_present_rows: `49`
- accepted_close_reason_rows: `25`

## Probe Lifecycle Canary

- status: `PASS`
- probe_transport_delta: `1`
- probe_entries_delta: `1`
- probe_lifecycle_delta: `2`
- position_closed_rows: `1`
- exit_filled_rows: `1`
- truth_status_resolved_rows: `2`
- truth_source_canonical_rows: `2`
- final_pnl_pct_present_rows: `2`
- accepted_close_reason_rows: `1`
- simulated_transport_rows: `1`

## Reporter

- status: `SKIPPED`
