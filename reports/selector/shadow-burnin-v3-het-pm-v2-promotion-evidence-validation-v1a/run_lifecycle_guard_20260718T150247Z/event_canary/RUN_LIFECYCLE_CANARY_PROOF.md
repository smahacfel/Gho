# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a`
- config: `/tmp/gho-het-pm-v2-validation-runtime/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 221, "NewPoolDetected": 249, "PoolTransaction": 24649, "unknown": 513}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `44086`

## Lifecycle Canary

- status: `PASS`
- shadow_buys_delta: `141`
- shadow_entries_delta: `140`
- shadow_lifecycle_delta: `2824`
- legacy_buy_executable_rows: `3`
- shadow_dispatch_closed_rows: `133`
- position_closed_rows: `127`
- exit_filled_rows: `127`
- truth_status_resolved_rows: `2219`
- truth_source_canonical_rows: `2691`
- final_pnl_pct_present_rows: `254`
- accepted_close_reason_rows: `127`

## Reporter

- status: `SKIPPED`
