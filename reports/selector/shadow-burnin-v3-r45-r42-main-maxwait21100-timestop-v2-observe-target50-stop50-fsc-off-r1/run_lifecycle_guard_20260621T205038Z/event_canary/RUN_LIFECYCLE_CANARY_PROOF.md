# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-burnin-v3-r45-r42-main-maxwait21100-timestop-v2-observe-target50-stop50-fsc-off-r1`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r45-r42-main-maxwait21100-timestop-v2-observe-target50-stop50-fsc-off-r1.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 383, "NewPoolDetected": 388, "PoolTransaction": 14496}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `27255`

## Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- shadow_buys_delta: `0`
- shadow_entries_delta: `0`
- shadow_lifecycle_delta: `0`
- legacy_buy_executable_rows: `0`
- shadow_dispatch_closed_rows: `0`
- position_closed_rows: `0`
- exit_filled_rows: `0`
- truth_status_resolved_rows: `0`
- truth_source_canonical_rows: `0`
- final_pnl_pct_present_rows: `0`
- accepted_close_reason_rows: `0`

## Reporter

- status: `SKIPPED`
