# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-het-pm-v2-authoritative-20260720-retry7`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 26, "NewPoolDetected": 41, "PoolTransaction": 2334, "unknown": 27}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `4682`

## Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- shadow_buys_delta: `10`
- shadow_entries_delta: `10`
- shadow_lifecycle_delta: `129`
- legacy_buy_executable_rows: `0`
- shadow_dispatch_closed_rows: `9`
- position_closed_rows: `6`
- exit_filled_rows: `6`
- truth_status_resolved_rows: `119`
- truth_source_canonical_rows: `119`
- final_pnl_pct_present_rows: `12`
- accepted_close_reason_rows: `6`

## Reporter

- status: `SKIPPED`
