# RAPORT P3.7-X3S - Working Builder Account Source Smoke - 2026-05-25

## Status

Decision: FAIL

Meaning: X3S was started in shadow-only `working_builder_parity` mode and did not use live Sender submission, but the acceptance contract is not met. Active-shadow rows stayed on the working-builder boundary, but counterfactual probe transport reintroduced a selected legacy handoff mismatch:

- `selected_route_handoff_mismatch_rows = 340` in probe materialization
- `selected_legacy_handoff_claimed_rows = 340`
- `selected_legacy_handoff_mismatch_rows = 340`
- `working_builder_buy_variant = "legacy_buy"` in 340/371 probe transport rows

This is a FAIL by the X3S contract because selected-route handoff mismatch returned. The next step is to fix X2/X3 probe dispatch parity before another account-source smoke. Do not move to R18.

## Runtime

- namespace: `shadow-burnin-v3-p37-x3s-working-builder-account-source-smoke`
- config: `configs/rollout/shadow-burnin-v3-p37-x3s-working-builder-account-source-smoke.local.toml`
- env source: `/root/Gho/.env`, mapped into `CHAINSTACK_*` env vars for runtime
- start: `2026-05-25T16:48:58Z`
- stopped: process was still alive at check, SIGINT was attempted, then the process group was terminated
- last console timestamp: `2026-05-25T17:16:30Z`
- process status after termination: not running

Runtime mode evidence:

- config log: `execution_mode=Shadow entry_mode=shadow_only`
- trigger init: `TriggerComponent initialized (execution_mode: Shadow, entry_mode: ShadowOnly)`
- local config uses env placeholders for `CHAINSTACK_GRPC_ENDPOINT`, `CHAINSTACK_GRPC_TOKEN`, and `CHAINSTACK_RPC_URL`

No live Sender submission evidence was found:

- `LiveTxSender` matches: 0
- `trigger.live_sender` matches: 0
- `send_transaction(` matches: 0
- `SUBMITTED` matches: 0
- `Sender transport is unavailable` warnings: 19

## Artifacts

- console log: `logs/rollout/shadow-burnin-v3-p37-x3s-working-builder-account-source-smoke/x3s_background_console.log`
- probe selection: `logs/shadow_run/shadow-burnin-v3-p37-x3s-working-builder-account-source-smoke/probe_selection.jsonl`
- probe transport: `logs/shadow_run/shadow-burnin-v3-p37-x3s-working-builder-account-source-smoke/probe_transport.jsonl`
- probe skips: `logs/shadow_run/shadow-burnin-v3-p37-x3s-working-builder-account-source-smoke/probe_skips.jsonl`
- active shadow entries: `logs/shadow_run/shadow-burnin-v3-p37-x3s-working-builder-account-source-smoke/shadow_entries.jsonl`
- active shadow lifecycle: `logs/shadow_run/shadow-burnin-v3-p37-x3s-working-builder-account-source-smoke/shadow_lifecycle.jsonl`
- offline audit JSON: `logs/shadow_run/shadow-burnin-v3-p37-x3s-working-builder-account-source-smoke/v3_p37_x3s_join_key_audit.json`
- offline audit MD: `logs/shadow_run/shadow-burnin-v3-p37-x3s-working-builder-account-source-smoke/v3_p37_x3s_join_key_audit.md`

## Required X3S Fields

| Field | Value |
|---|---:|
| `working_builder_parity_rows` | 371 probe / 57 active shadow |
| `working_builder_request_built_rows` | 371 probe / 57 active shadow |
| `working_builder_buy_variant_counts` | `{"legacy_buy": 340, "routed_exact_sol_in": 31}` probe / `{"routed_exact_sol_in": 19}` active entry artifacts |
| `working_builder_rpc_manifest_hash_coverage` | 371/371 probe transport / 19/19 active entries |
| `working_builder_sender_manifest_hash_coverage` | 371/371 probe transport / 19/19 active entries |
| `working_builder_manifest_contains_bcv2_rows` | 371 probe / 57 active shadow |
| `working_builder_manifest_ready_rows` | 0 probe / 0 active shadow |
| `working_builder_manifest_missing_required_rows` | 72 probe / 57 active shadow |
| `working_builder_bcv2_source_authority_counts` | `{"authoritative_observed_tx": 371}` probe / `{"authoritative_observed_tx": 57}` active shadow |
| `working_builder_bcv2_rpc_load_status_counts` | `{"missing_on_rpc_precheck": 371}` probe / `{"missing_on_rpc_precheck": 57}` active shadow |
| `working_builder_creator_vault_source_authority_counts` | `{"authoritative_detected_pool_creator": 31, "creator_vault_source_not_authoritative": 340}` probe / `{"authoritative_account_overrides_creator_pubkey": 57}` active shadow |
| `working_builder_creator_vault_rpc_load_status_counts` | `{"missing_on_rpc_precheck": 47, "rpc_load_ready": 324}` probe / `{"missing_on_rpc_precheck": 24, "rpc_load_ready": 33}` active shadow |
| `legacy_fallback_attempted_rows` | 0 probe / 0 active shadow |
| `legacy_buy_route_attempted_rows` | 340 probe / 0 active shadow |
| `selected_route_handoff_mismatch_rows` | 340 probe / 0 active shadow |
| `successful_entry_rows` | 0 |
| `successful_probe_entry_rows` | 0 |
| `active_shadow_successful_entry_rows` | 0 |
| `lifecycle_eligible_rows` | 0 |
| `post_simulation_account_not_found_rows` | 0 probe / 0 active shadow |

Additional coverage:

- decision rows: 572
- probe selections: 372
- probe transport rows: 371
- probe entry rows: 0
- probe lifecycle rows: 0
- probe skips: 181
- active shadow entries: 19
- active shadow lifecycle rows: 19

## Acceptance Check

PASS-A is not met:

- `working_builder_manifest_ready_rows = 0`
- `successful_probe_entry_rows = 0`
- `active_shadow_successful_entry_rows = 0`
- `lifecycle_eligible_rows = 0`

PASS-B is not met:

- `selected_route_handoff_mismatch_rows = 340` in probe materialization
- `working_builder_buy_variant_counts` includes `legacy_buy = 340`
- `creator_vault_source_not_authoritative = 340` in probe materialization

FAIL is met:

- selected-route handoff mismatch returned in probe materialization
- `legacy_buy` route state returned in probe transport, even though `working_builder_parity_mode = "working_builder_parity"` is present on all 371 probe transport rows

## Active Shadow Result

Active-shadow dispatch stayed structurally clean:

- `active_shadow_working_builder_parity_rows = 57`
- `active_shadow_working_builder_request_built_rows = 57`
- `active_shadow_working_builder_manifest_contains_bcv2_rows = 57`
- `active_shadow_legacy_fallback_attempted_rows = 0`
- `active_shadow_route_fallback_attempted_rows = 0`
- `active_shadow_selected_route_handoff_mismatch_rows = 0`
- `active_shadow_account_not_found_rows = 0`

But active shadow did not reach executable entries:

- `active_shadow_working_builder_manifest_ready_rows = 0`
- `active_shadow_working_builder_manifest_missing_required_rows = 57`
- `active_shadow_successful_entry_rows = 0`
- `active_shadow_lifecycle_eligible_rows = 0`

Active-shadow remaining account-source blockers:

- `bonding_curve_v2`: `missing_on_rpc_precheck = 57`
- `creator_vault`: `missing_on_rpc_precheck = 24`, `rpc_load_ready = 33`

## Probe Result

Probe transport built working-builder parity rows, but the route identity was polluted:

- `working_builder_parity_rows = 371`
- `working_builder_request_built_rows = 371`
- `working_builder_manifest_contains_bcv2_rows = 371`
- `working_builder_rpc_manifest_hash_coverage = 371/371`
- `working_builder_sender_manifest_hash_coverage = 371/371`

Probe failure signals:

- `working_builder_buy_variant_counts = {"legacy_buy": 340, "routed_exact_sol_in": 31}`
- `selected_route_handoff_mismatch_rows = 340`
- `selected_legacy_handoff_claimed_rows = 340`
- `selected_legacy_handoff_mismatch_rows = 340`
- `legacy_buy_route_attempted_rows = 340`
- `legacy_buy_route_unsupported_builder_layout_rows = 299`
- `legacy_buy_route_not_ready_reason_counts = {"legacy_buy_simulation_load_not_ready": 41, "legacy_buy_unsupported_builder_layout_requires_bcv2": 299}`

Probe account-source blockers:

- `bonding_curve_v2`: `missing_on_rpc_precheck = 371`
- `creator_vault`: `creator_vault_source_not_authoritative = 340`
- `creator_vault`: `missing_on_rpc_precheck = 47`, `rpc_load_ready = 324`

## Safety

No live/P2 submission evidence was observed. The run stayed in shadow-only runtime mode and Sender transport was unavailable. The local smoke config is not a commit artifact and contains env placeholders instead of endpoint tokens.

## Final Decision

X3S = FAIL

Primary blocker:

- counterfactual probe path still allows selected legacy handoff mismatch under `working_builder_parity`

Secondary blocker after the handoff repair:

- working-builder manifest remains not ready because `bonding_curve_v2` is not RPC-load-ready and `creator_vault` is partially not authoritative/not RPC-load-ready

Concrete next step:

Fix X2/X3 probe dispatch parity so every P3.7 probe row in `working_builder_parity` mode keeps the post-build request variant and selected route on the working `routed_exact_sol_in` family, with `selected_route_handoff_mismatch_rows = 0` and no `legacy_buy` transport rows. Then rerun X3S. Do not start R18 before X3S passes or returns a clean PASS-B account-source-only blocker.
