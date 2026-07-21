# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-het-pm-v2-authoritative-20260720-retry12`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 38, "NewPoolDetected": 55, "PoolTransaction": 3462, "unknown": 44}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `6305`

## Lifecycle Canary

- status: `PASS`
- shadow_buys_delta: `28`
- shadow_entries_delta: `27`
- shadow_lifecycle_delta: `446`
- legacy_buy_executable_rows: `2`
- shadow_dispatch_closed_rows: `23`
- position_closed_rows: `7`
- exit_filled_rows: `7`
- truth_status_resolved_rows: `420`
- truth_source_canonical_rows: `420`
- final_pnl_pct_present_rows: `14`
- accepted_close_reason_rows: `7`

## Reporter

- status: `SKIPPED`
