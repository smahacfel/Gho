# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-het-pm-v2-authoritative-20260720-retry11`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 35, "NewPoolDetected": 71, "PoolTransaction": 4308, "unknown": 58}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `8665`

## Lifecycle Canary

- status: `PASS`
- shadow_buys_delta: `26`
- shadow_entries_delta: `26`
- shadow_lifecycle_delta: `412`
- legacy_buy_executable_rows: `2`
- shadow_dispatch_closed_rows: `22`
- position_closed_rows: `12`
- exit_filled_rows: `12`
- truth_status_resolved_rows: `390`
- truth_source_canonical_rows: `390`
- final_pnl_pct_present_rows: `24`
- accepted_close_reason_rows: `12`

## Reporter

- status: `SKIPPED`
