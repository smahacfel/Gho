# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-burnin-v3-het-pm-v2-promotion-evidence-r1b`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-r1b.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 68, "NewPoolDetected": 82, "PoolTransaction": 1855, "unknown": 77}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `3734`

## Lifecycle Canary

- status: `PASS`
- shadow_buys_delta: `25`
- shadow_entries_delta: `25`
- shadow_lifecycle_delta: `286`
- legacy_buy_executable_rows: `2`
- shadow_dispatch_closed_rows: `20`
- position_closed_rows: `19`
- exit_filled_rows: `19`
- truth_status_resolved_rows: `221`
- truth_source_canonical_rows: `261`
- final_pnl_pct_present_rows: `38`
- accepted_close_reason_rows: `19`

## Reporter

- status: `SKIPPED`
