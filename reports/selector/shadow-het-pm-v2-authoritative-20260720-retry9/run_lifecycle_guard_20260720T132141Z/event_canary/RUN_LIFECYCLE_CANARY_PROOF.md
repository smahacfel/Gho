# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-het-pm-v2-authoritative-20260720-retry9`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 25, "NewPoolDetected": 44, "PoolTransaction": 2832, "unknown": 50}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `5559`

## Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- shadow_buys_delta: `22`
- shadow_entries_delta: `22`
- shadow_lifecycle_delta: `461`
- legacy_buy_executable_rows: `0`
- shadow_dispatch_closed_rows: `24`
- position_closed_rows: `10`
- exit_filled_rows: `10`
- truth_status_resolved_rows: `440`
- truth_source_canonical_rows: `440`
- final_pnl_pct_present_rows: `20`
- accepted_close_reason_rows: `10`

## Reporter

- status: `SKIPPED`
