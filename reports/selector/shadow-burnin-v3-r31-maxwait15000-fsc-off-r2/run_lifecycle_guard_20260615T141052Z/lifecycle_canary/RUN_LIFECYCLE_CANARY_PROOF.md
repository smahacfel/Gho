# Selector Lifecycle Canary Proof

- status: `FAIL_LIFECYCLE_PROOF`
- claim: `SELECTOR_LIFECYCLE_CANARY_FAIL:FAIL_LIFECYCLE_PROOF`
- phase: `lifecycle`
- scope: `shadow-burnin-v3-r31-maxwait15000-fsc-off-r2`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r31-maxwait15000-fsc-off.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 22, "NewPoolDetected": 48, "PoolTransaction": 5276}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `4990`

## Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- accepted_lifecycle_plane: `None`
- shadow_status: `FAIL_LIFECYCLE_PROOF`
- shadow_buys_delta: `15`
- shadow_entries_delta: `0`
- shadow_lifecycle_delta: `15`
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
- probe_transport_delta: `0`
- probe_entries_delta: `0`
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

## Errors

- shadow_entries_delta <= 0
- legacy_buy executable rows <= 0
- shadow_dispatch closed rows <= 0
- position_closed rows <= 0
- exit_filled rows <= 0
- truth_status=resolved lifecycle rows <= 0
- truth_source=canonical_account_state_snapshot lifecycle rows <= 0
- final_pnl_pct lifecycle rows <= 0
- accepted close_reason lifecycle rows <= 0
- probe_transport_delta <= 0
- probe_entries_delta <= 0
- probe_lifecycle_delta <= 0
- probe simulated transport rows <= 0
- probe position_closed rows <= 0
- probe exit_filled rows <= 0
- probe truth_status=resolved lifecycle rows <= 0
- probe truth_source=canonical_account_state_snapshot lifecycle rows <= 0
- probe final_pnl_pct lifecycle rows <= 0
- probe accepted close_reason lifecycle rows <= 0
