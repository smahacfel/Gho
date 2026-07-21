# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-burnin-v3-het-pm-v2-promotion-evidence-r1`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-r1.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 188, "NewPoolDetected": 210, "PoolTransaction": 2770, "unknown": 33}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `5844`

## Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- shadow_buys_delta: `19`
- shadow_entries_delta: `19`
- shadow_lifecycle_delta: `118`
- legacy_buy_executable_rows: `0`
- shadow_dispatch_closed_rows: `19`
- position_closed_rows: `5`
- exit_filled_rows: `5`
- truth_status_resolved_rows: `84`
- truth_source_canonical_rows: `99`
- final_pnl_pct_present_rows: `10`
- accepted_close_reason_rows: `5`

## Reporter

- status: `SKIPPED`
