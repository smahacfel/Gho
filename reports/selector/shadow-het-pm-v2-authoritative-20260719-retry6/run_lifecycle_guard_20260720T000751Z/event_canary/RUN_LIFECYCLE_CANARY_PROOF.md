# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-het-pm-v2-authoritative-20260719-retry6`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 25, "NewPoolDetected": 44, "PoolTransaction": 1767, "unknown": 39}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `3556`

## Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- shadow_buys_delta: `19`
- shadow_entries_delta: `19`
- shadow_lifecycle_delta: `275`
- legacy_buy_executable_rows: `0`
- shadow_dispatch_closed_rows: `17`
- position_closed_rows: `8`
- exit_filled_rows: `8`
- truth_status_resolved_rows: `258`
- truth_source_canonical_rows: `258`
- final_pnl_pct_present_rows: `16`
- accepted_close_reason_rows: `8`

## Reporter

- status: `SKIPPED`
