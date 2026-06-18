# Selector Lifecycle Canary Proof

- status: `FAIL_LIFECYCLE_PROOF`
- claim: `SELECTOR_LIFECYCLE_CANARY_FAIL:FAIL_LIFECYCLE_PROOF`
- phase: `lifecycle`
- scope: `shadow-burnin-v3-r33-maxwait15000-fsc-off-r1`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r33-maxwait15000-fsc-off-r1.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 764, "NewPoolDetected": 794, "PoolTransaction": 46821, "unknown": 754}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `88723`

## Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- accepted_lifecycle_plane: `None`
- shadow_status: `FAIL_LIFECYCLE_PROOF`
- shadow_buys_delta: `340`
- shadow_entries_delta: `218`
- shadow_lifecycle_delta: `718`
- legacy_buy_executable_rows: `190`
- shadow_dispatch_closed_rows: `167`
- position_closed_rows: `174`
- exit_filled_rows: `173`
- truth_status_resolved_rows: `347`
- truth_source_canonical_rows: `347`
- final_pnl_pct_present_rows: `347`
- accepted_close_reason_rows: `174`

## Probe Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- probe_transport_delta: `46`
- probe_entries_delta: `45`
- probe_lifecycle_delta: `90`
- position_closed_rows: `45`
- exit_filled_rows: `45`
- truth_status_resolved_rows: `90`
- truth_source_canonical_rows: `90`
- final_pnl_pct_present_rows: `90`
- accepted_close_reason_rows: `45`
- simulated_transport_rows: `45`

## Reporter

- status: `SKIPPED`

## Errors

- AccountNotFound_delta > 0
- AccountNotFound_delta > 0
