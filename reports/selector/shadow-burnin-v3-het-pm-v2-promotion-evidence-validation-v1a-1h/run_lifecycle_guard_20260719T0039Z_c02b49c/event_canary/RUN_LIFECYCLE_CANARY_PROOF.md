# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a-1h`
- config: `/tmp/het-pm-v2-runtime-c02b49c/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 334, "NewPoolDetected": 350, "PoolTransaction": 20507, "unknown": 347}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `36694`

## Lifecycle Canary

- status: `PASS`
- shadow_buys_delta: `94`
- shadow_entries_delta: `94`
- shadow_lifecycle_delta: `1653`
- legacy_buy_executable_rows: `2`
- shadow_dispatch_closed_rows: `89`
- position_closed_rows: `86`
- exit_filled_rows: `86`
- truth_status_resolved_rows: `1307`
- truth_source_canonical_rows: `1561`
- final_pnl_pct_present_rows: `172`
- accepted_close_reason_rows: `86`

## Reporter

- status: `SKIPPED`
