# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-burnin-v3-r31-maxwait15000-fsc-off-r4`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r31-maxwait15000-fsc-off.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 14, "NewPoolDetected": 18, "PoolTransaction": 495, "unknown": 16}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `1015`

## Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- accepted_lifecycle_plane: `None`
- shadow_status: `FAIL_LIFECYCLE_PROOF`
- shadow_buys_delta: `5`
- shadow_entries_delta: `5`
- shadow_lifecycle_delta: `7`
- legacy_buy_executable_rows: `0`
- shadow_dispatch_closed_rows: `0`
- position_closed_rows: `0`
- exit_filled_rows: `0`
- truth_status_resolved_rows: `0`
- truth_source_canonical_rows: `0`
- final_pnl_pct_present_rows: `0`
- accepted_close_reason_rows: `0`

## Probe Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- probe_transport_delta: `1`
- probe_entries_delta: `1`
- probe_lifecycle_delta: `0`
- position_closed_rows: `0`
- exit_filled_rows: `0`
- truth_status_resolved_rows: `0`
- truth_source_canonical_rows: `0`
- final_pnl_pct_present_rows: `0`
- accepted_close_reason_rows: `0`
- simulated_transport_rows: `0`

## Reporter

- status: `SKIPPED`
