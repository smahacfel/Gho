# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-burnin-v3-r31-maxwait15000-fsc-off-r4`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r31-maxwait15000-fsc-off.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 66, "NewPoolDetected": 67, "PoolTransaction": 1515, "unknown": 63}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `3062`

## Lifecycle Canary

- status: `PASS`
- accepted_lifecycle_plane: `probe`
- shadow_status: `FAIL_LIFECYCLE_PROOF`
- shadow_buys_delta: `6`
- shadow_entries_delta: `6`
- shadow_lifecycle_delta: `6`
- legacy_buy_executable_rows: `0`
- shadow_dispatch_closed_rows: `0`
- position_closed_rows: `0`
- exit_filled_rows: `0`
- truth_status_resolved_rows: `0`
- truth_source_canonical_rows: `0`
- final_pnl_pct_present_rows: `0`
- accepted_close_reason_rows: `0`

## Probe Lifecycle Canary

- status: `PASS`
- probe_transport_delta: `20`
- probe_entries_delta: `20`
- probe_lifecycle_delta: `32`
- position_closed_rows: `17`
- exit_filled_rows: `17`
- truth_status_resolved_rows: `34`
- truth_source_canonical_rows: `34`
- final_pnl_pct_present_rows: `34`
- accepted_close_reason_rows: `17`
- simulated_transport_rows: `19`

## Reporter

- status: `SKIPPED`
