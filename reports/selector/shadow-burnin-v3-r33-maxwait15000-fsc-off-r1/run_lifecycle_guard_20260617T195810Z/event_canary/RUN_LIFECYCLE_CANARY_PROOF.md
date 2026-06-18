# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-burnin-v3-r33-maxwait15000-fsc-off-r1`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r33-maxwait15000-fsc-off-r1.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 448, "NewPoolDetected": 448, "PoolTransaction": 22850, "unknown": 699}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `44589`

## Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- accepted_lifecycle_plane: `None`
- shadow_status: `FAIL_LIFECYCLE_PROOF`
- shadow_buys_delta: `217`
- shadow_entries_delta: `203`
- shadow_lifecycle_delta: `563`
- legacy_buy_executable_rows: `176`
- shadow_dispatch_closed_rows: `158`
- position_closed_rows: `162`
- exit_filled_rows: `161`
- truth_status_resolved_rows: `323`
- truth_source_canonical_rows: `323`
- final_pnl_pct_present_rows: `323`
- accepted_close_reason_rows: `162`

## Probe Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- probe_transport_delta: `44`
- probe_entries_delta: `44`
- probe_lifecycle_delta: `80`
- position_closed_rows: `40`
- exit_filled_rows: `40`
- truth_status_resolved_rows: `80`
- truth_source_canonical_rows: `80`
- final_pnl_pct_present_rows: `80`
- accepted_close_reason_rows: `40`
- simulated_transport_rows: `44`

## Reporter

- status: `SKIPPED`
