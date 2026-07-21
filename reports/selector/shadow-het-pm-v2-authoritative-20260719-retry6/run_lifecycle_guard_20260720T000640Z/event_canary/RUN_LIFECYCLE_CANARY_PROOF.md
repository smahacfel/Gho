# Selector Lifecycle Canary Proof

- status: `FAIL_EVENT_CANARY`
- claim: `SELECTOR_LIFECYCLE_CANARY_FAIL:FAIL_EVENT_CANARY`
- phase: `event`
- scope: `shadow-het-pm-v2-authoritative-20260719-retry6`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`

## Event Canary

- status: `FAIL_EVENT_CANARY`
- event_delta: `{"NewPoolDetected": 4, "PoolTransaction": 111}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `228`

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

## Errors

- Candidate_delta <= 0
