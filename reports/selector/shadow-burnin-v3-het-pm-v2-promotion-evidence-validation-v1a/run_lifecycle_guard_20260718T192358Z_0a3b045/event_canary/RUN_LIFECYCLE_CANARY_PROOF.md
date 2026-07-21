# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a`
- config: `/tmp/gho-het-pm-v2-validation-runtime-v2/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 241, "NewPoolDetected": 261, "PoolTransaction": 28960, "unknown": 500}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `42516`

## Lifecycle Canary

- status: `PASS`
- shadow_buys_delta: `144`
- shadow_entries_delta: `142`
- shadow_lifecycle_delta: `2524`
- legacy_buy_executable_rows: `8`
- shadow_dispatch_closed_rows: `134`
- position_closed_rows: `123`
- exit_filled_rows: `123`
- truth_status_resolved_rows: `1997`
- truth_source_canonical_rows: `2399`
- final_pnl_pct_present_rows: `246`
- accepted_close_reason_rows: `123`

## Reporter

- status: `SKIPPED`
