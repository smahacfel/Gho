# Selector Lifecycle Canary Proof

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_PASS`
- phase: `event`
- scope: `shadow-v2-l2-f-research-codex-20260707-r4-compact-headroom-12h`
- config: `/root/Gho/configs/rollout/shadow-v2-l2-f-research-codex-20260707-r4-compact-headroom-12h.local.toml`

## Event Canary

- status: `PASS`
- event_delta: `{"Candidate": 37, "NewPoolDetected": 42, "PoolTransaction": 1195, "unknown": 12}`
- bad_event_json_delta: `0`
- diag_account_update_relay_delta: `2406`

## Lifecycle Canary

- status: `FAIL_LIFECYCLE_PROOF`
- shadow_buys_delta: `16`
- shadow_entries_delta: `16`
- shadow_lifecycle_delta: `16`
- legacy_buy_executable_rows: `3`
- shadow_dispatch_closed_rows: `12`
- position_closed_rows: `0`
- exit_filled_rows: `0`
- truth_status_resolved_rows: `0`
- truth_source_canonical_rows: `0`
- final_pnl_pct_present_rows: `0`
- accepted_close_reason_rows: `0`

## Reporter

- status: `SKIPPED`
