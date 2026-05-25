# RAPORT P3.7-X4S - Probe Working Builder Parity Smoke - 2026-05-25

## Status

Decision: PASS-B

Meaning: X4S confirmed the X4 target for probe parity. In `working_builder_parity` mode, probe transport preserved the post-build working request route identity and did not reintroduce `legacy_buy`, selected legacy handoff, stale route diagnostics, or selected-route handoff mismatch.

Execution is still not unlocked. Remaining blocker is concrete account-source/readiness on the working builder manifest:

- `bonding_curve_v2`: authoritative observed tx source, but `missing_on_rpc_precheck`
- `creator_vault`: authoritative source, partially `missing_on_rpc_precheck`

This is a PASS-B by the X4S contract. Do not go to R18. Next step is X5 account-source repair for the working builder path.

## Runtime

- namespace: `shadow-burnin-v3-p37-x4s-probe-working-builder-parity-smoke`
- config: `configs/rollout/shadow-burnin-v3-p37-x4s-probe-working-builder-parity-smoke.local.toml`
- code checkpoint: `91e9c19 Implement P3.7 X4 probe working builder parity enforcement`
- env source: `/root/Gho/.env`, mapped into `CHAINSTACK_*` env vars for runtime
- start: `2026-05-25T18:53:07Z`
- stop: `2026-05-25T19:16:03Z`
- duration: about 22m 55s
- termination: stopped early on operator instruction; `INT` was sent and `timeout --kill-after` terminated the remaining process group

Runtime mode evidence:

- config log: `execution_mode=Shadow entry_mode=shadow_only`
- trigger init: `TriggerComponent initialized (execution_mode: Shadow, entry_mode: ShadowOnly)`
- local config uses placeholders for `CHAINSTACK_GRPC_ENDPOINT`, `CHAINSTACK_GRPC_TOKEN`, and `CHAINSTACK_RPC_URL`
- `LiveSellHandle: skipped (no live transport required at startup)`

No live Sender submission evidence was found:

- `trigger.live_sender` matches: 0
- `send_transaction(` matches: 0
- `SUBMITTED` matches: 0
- `Sender transport is unavailable` warnings: 22

## Artifacts

- console log: `logs/rollout/shadow-burnin-v3-p37-x4s-probe-working-builder-parity-smoke/x4s_background_console.log`
- probe selection: `logs/shadow_run/shadow-burnin-v3-p37-x4s-probe-working-builder-parity-smoke/probe_selection.jsonl`
- probe transport: `logs/shadow_run/shadow-burnin-v3-p37-x4s-probe-working-builder-parity-smoke/probe_transport.jsonl`
- probe skips: `logs/shadow_run/shadow-burnin-v3-p37-x4s-probe-working-builder-parity-smoke/probe_skips.jsonl`
- active shadow transport: `logs/shadow_run/shadow-burnin-v3-p37-x4s-probe-working-builder-parity-smoke/buys.jsonl`
- active shadow entries: `logs/shadow_run/shadow-burnin-v3-p37-x4s-probe-working-builder-parity-smoke/shadow_entries.jsonl`
- active shadow lifecycle: `logs/shadow_run/shadow-burnin-v3-p37-x4s-probe-working-builder-parity-smoke/shadow_lifecycle.jsonl`
- offline audit JSON: `logs/shadow_run/shadow-burnin-v3-p37-x4s-probe-working-builder-parity-smoke/v3_p37_x4s_join_key_audit.json`
- offline audit MD: `logs/shadow_run/shadow-burnin-v3-p37-x4s-probe-working-builder-parity-smoke/v3_p37_x4s_join_key_audit.md`

## Required X4S Fields

| Field | Value |
|---|---:|
| `probe_working_builder_parity_rows` | 434 |
| `working_builder_request_built_rows` | 434 |
| `working_builder_buy_variant_counts` | `{"routed_exact_sol_in": 434}` |
| `probe_working_builder_variant_drift_rows` | 0 |
| `probe_working_builder_legacy_variant_rows` | 0 |
| `probe_working_builder_selected_legacy_handoff_rows` | 0 |
| `probe_working_builder_stale_route_diagnostics_rows` | 0 |
| `selected_route_handoff_mismatch_rows` | 0 |
| `legacy_buy_route_attempted_rows` | 0 |
| `legacy_fallback_attempted_rows` | 0 |
| `working_builder_manifest_ready_rows` | 0 |
| `working_builder_manifest_missing_required_rows` | 434 |
| `working_builder_manifest_contains_bcv2_rows` | 434 |
| `successful_probe_entry_rows` | 0 |
| `active_shadow_successful_entry_rows` | 0 |
| `successful_entry_rows` | 0 |
| `lifecycle_eligible_rows` | 0 |
| `post_simulation_account_not_found_rows` | 0 probe / 0 active shadow |

Additional active-shadow parity:

| Field | Value |
|---|---:|
| `active_shadow_working_builder_parity_rows` | 66 |
| `active_shadow_working_builder_request_built_rows` | 66 |
| `active_shadow_working_builder_buy_variant_counts` | `{"routed_exact_sol_in": 66}` |
| `active_shadow_probe_working_builder_variant_drift_rows` | 0 |
| `active_shadow_probe_working_builder_legacy_variant_rows` | 0 |
| `active_shadow_probe_working_builder_selected_legacy_handoff_rows` | 0 |
| `active_shadow_probe_working_builder_stale_route_diagnostics_rows` | 0 |
| `active_shadow_legacy_fallback_attempted_rows` | 0 |
| `active_shadow_selected_route_handoff_mismatch_rows` | 0 |
| `active_shadow_legacy_buy_route_attempted_rows` | 0 |
| `active_shadow_working_builder_manifest_ready_rows` | 0 |
| `active_shadow_working_builder_manifest_missing_required_rows` | 66 |

## Account-Source Blockers

Probe working builder manifest:

- `working_builder_bcv2_source_authority_counts = {"authoritative_observed_tx": 434}`
- `working_builder_bcv2_rpc_load_status_counts = {"missing_on_rpc_precheck": 434}`
- `working_builder_creator_vault_source_authority_counts = {"authoritative_detected_pool_creator": 434}`
- `working_builder_creator_vault_rpc_load_status_counts = {"missing_on_rpc_precheck": 38, "rpc_load_ready": 396}`
- `P37_SHADOW_PROBE_SELECTED_ROUTE_FINAL_MANIFEST_BLOCKED` rows in console: 434

Active-shadow working builder manifest:

- `active_shadow_working_builder_bcv2_source_authority_counts = {"authoritative_observed_tx": 66}`
- `active_shadow_working_builder_bcv2_rpc_load_status_counts = {"missing_on_rpc_precheck": 66}`
- `active_shadow_working_builder_creator_vault_source_authority_counts = {"authoritative_account_overrides_creator_pubkey": 66}`
- `active_shadow_working_builder_creator_vault_rpc_load_status_counts = {"missing_on_rpc_precheck": 24, "rpc_load_ready": 42}`

## Acceptance Check

PASS-A is not met:

- `successful_probe_entry_rows = 0`
- `active_shadow_successful_entry_rows = 0`
- `lifecycle_eligible_rows = 0`
- `working_builder_manifest_ready_rows = 0`

PASS-B is met:

- probe working-builder parity rows exist: 434
- probe legacy/handoff pollution is 0
- `selected_route_handoff_mismatch_rows = 0`
- `legacy_buy_route_attempted_rows = 0`
- post-simulation AccountNotFound is 0
- remaining blockers are concrete working-builder account-source/readiness issues: `bonding_curve_v2` and `creator_vault`

FAIL is not met:

- no probe `legacy_buy` working-builder variant rows
- no selected legacy handoff rows
- no stale route diagnostics rows
- no selected-route handoff mismatch rows

## Notes

The smoke was intentionally stopped before the original 30-minute cap after operator instruction. The collected artifacts are still sufficient for the X4 acceptance question because they include 434 probe transport rows and 66 active-shadow parity rows, all with zero legacy/handoff pollution.

The audit tool reports `probe_readiness = not_ready` because probe entries/lifecycle rows were not produced. That is consistent with PASS-B: route identity is now clean, but the final working builder manifest is not ready because required accounts are missing on RPC precheck.

## Final Decision

X4S = PASS-B

Concrete next step:

P3.7-X5 - Working Builder Account Source Repair

Scope for X5:

- repair `bonding_curve_v2` RPC-load readiness for the working builder manifest
- repair `creator_vault` RPC-load readiness/source handoff for the working builder manifest
- keep `legacy_buy_route_attempted_rows = 0`
- keep `selected_route_handoff_mismatch_rows = 0`
- do not start R18 before X5S returns PASS-A
