# RAPORT P3.7-X5S - Working Builder Account Source Smoke - 2026-05-25

## Status

Decision: PASS-B

Meaning: X5S stayed on the working-builder parity path and did not reintroduce
legacy route pollution, selected legacy handoff, selected-route handoff mismatch,
or post-simulation `AccountNotFound`.

Execution is still not unlocked. The final working builder manifest remains not
ready because account-source/readiness blockers are still concrete:

- `bonding_curve_v2`: authoritative observed-tx source, but `missing_on_rpc_precheck`
- `creator_vault`: authoritative source, partially `missing_on_rpc_precheck`

This is a PASS-B by the X5S contract. Do not go to R18. The next step is a
targeted account-source/readiness follow-up for the working builder path, with
`bonding_curve_v2` as the primary blocker.

## Runtime

- namespace: `shadow-burnin-v3-p37-x5s-working-builder-account-source-smoke`
- config: `configs/rollout/shadow-burnin-v3-p37-x5s-working-builder-account-source-smoke.local.toml`
- code checkpoint: `548482c Implement P3.7 X5 working builder account source readiness`
- env source: `/root/Gho/.env`, mapped into `CHAINSTACK_*` env vars for runtime
- start: `2026-05-25T20:13:42Z`
- stop: `2026-05-25T20:43:14Z`
- duration: about 29m 31s
- termination: process reached the 30-minute `timeout` cap and was no longer running after the smoke

Runtime mode evidence:

- config log: `execution_mode=Shadow entry_mode=shadow_only`
- trigger init: `TriggerComponent initialized (execution_mode: Shadow, entry_mode: ShadowOnly)`
- local config uses placeholders for `CHAINSTACK_GRPC_ENDPOINT`, `CHAINSTACK_GRPC_TOKEN`, and `CHAINSTACK_RPC_URL`
- `LiveSellHandle: skipped (no live transport required at startup)`

No live Sender submission evidence was found:

- `trigger.live_sender` matches: 0
- `send_transaction(` matches: 0
- `LiveTxSender::send_transaction` matches: 0
- `SUBMITTED` matches: 0
- `Sender transport is unavailable` warnings: 29

## Artifacts

- console log: `logs/rollout/shadow-burnin-v3-p37-x5s-working-builder-account-source-smoke/x5s_background_console.log`
- process pid file: `logs/rollout/shadow-burnin-v3-p37-x5s-working-builder-account-source-smoke/x5s.pid`
- probe selection: `logs/shadow_run/shadow-burnin-v3-p37-x5s-working-builder-account-source-smoke/probe_selection.jsonl`
- probe transport: `logs/shadow_run/shadow-burnin-v3-p37-x5s-working-builder-account-source-smoke/probe_transport.jsonl`
- probe skips: `logs/shadow_run/shadow-burnin-v3-p37-x5s-working-builder-account-source-smoke/probe_skips.jsonl`
- active shadow transport: `logs/shadow_run/shadow-burnin-v3-p37-x5s-working-builder-account-source-smoke/buys.jsonl`
- active shadow entries: `logs/shadow_run/shadow-burnin-v3-p37-x5s-working-builder-account-source-smoke/shadow_entries.jsonl`
- active shadow lifecycle: `logs/shadow_run/shadow-burnin-v3-p37-x5s-working-builder-account-source-smoke/shadow_lifecycle.jsonl`
- offline audit JSON: `logs/shadow_run/shadow-burnin-v3-p37-x5s-working-builder-account-source-smoke/v3_p37_x5s_join_key_audit.json`
- offline audit MD: `logs/shadow_run/shadow-burnin-v3-p37-x5s-working-builder-account-source-smoke/v3_p37_x5s_join_key_audit.md`

Artifact row counts:

| Artifact | Rows |
|---|---:|
| `probe_selection.jsonl` | 566 |
| `probe_transport.jsonl` | 566 |
| `probe_skips.jsonl` | 225 |
| `buys.jsonl` | 29 |
| `shadow_entries.jsonl` | 29 |
| `shadow_lifecycle.jsonl` | 29 |

## Required X5S Fields

| Field | Value |
|---|---:|
| `working_builder_parity_rows` | 566 probe / 87 active shadow |
| `working_builder_request_built_rows` | 566 probe / 87 active shadow |
| `working_builder_buy_variant_counts` | `{"routed_exact_sol_in": 566}` probe / `{"routed_exact_sol_in": 87}` active shadow |
| `probe_working_builder_variant_drift_rows` | 0 |
| `probe_working_builder_legacy_variant_rows` | 0 |
| `probe_working_builder_selected_legacy_handoff_rows` | 0 |
| `probe_working_builder_stale_route_diagnostics_rows` | 0 |
| `selected_route_handoff_mismatch_rows` | 0 probe / 0 active shadow |
| `legacy_buy_route_attempted_rows` | 0 probe / 0 active shadow |
| `legacy_fallback_attempted_rows` | 0 probe / 0 active shadow |
| `working_builder_manifest_ready_rows` | 0 probe / 0 active shadow |
| `working_builder_manifest_missing_required_rows` | 566 probe / 87 active shadow |
| `working_builder_manifest_contains_bcv2_rows` | 566 probe / 87 active shadow |
| `successful_probe_entry_rows` | 0 |
| `active_shadow_successful_entry_rows` | 0 |
| `successful_entry_rows` | 0 |
| `lifecycle_eligible_rows` | 0 |
| `post_simulation_account_not_found_rows` | 0 probe / 0 active shadow |
| `P37_SHADOW_PROBE_SELECTED_ROUTE_FINAL_MANIFEST_BLOCKED` console rows | 566 |

## Account-Source Repair Counters

Probe working builder manifest:

| Field | Value |
|---|---:|
| `working_builder_bcv2_source_authority_counts` | `{"authoritative_observed_tx": 566}` |
| `working_builder_bcv2_rpc_load_status_counts` | `{"missing_on_rpc_precheck": 566}` |
| `working_builder_bcv2_authoritative_and_load_ready_rows` | 0 |
| `working_builder_bcv2_authoritative_but_missing_on_rpc_rows` | 566 |
| `working_builder_bcv2_pubkey_mismatch_rows` | 0 |
| `working_builder_bcv2_observed_tx_missing_on_rpc_rows` | 566 |
| `working_builder_bcv2_account_state_missing_rows` | 566 |
| `working_builder_creator_vault_source_authority_counts` | `{"authoritative_detected_pool_creator": 566}` |
| `working_builder_creator_vault_rpc_load_status_counts` | `{"missing_on_rpc_precheck": 56, "rpc_load_ready": 510}` |
| `working_builder_creator_vault_authoritative_and_load_ready_rows` | 510 |
| `working_builder_creator_vault_authoritative_but_missing_on_rpc_rows` | 56 |
| `working_builder_creator_vault_source_mismatch_rows` | 0 |
| `working_builder_manifest_ready_after_account_source_repair_rows` | 0 |
| `working_builder_manifest_still_not_ready_after_account_source_repair_rows` | 566 |

Active-shadow working builder manifest:

| Field | Value |
|---|---:|
| `active_shadow_working_builder_bcv2_source_authority_counts` | `{"authoritative_observed_tx": 87}` |
| `active_shadow_working_builder_bcv2_rpc_load_status_counts` | `{"missing_on_rpc_precheck": 87}` |
| `active_shadow_working_builder_bcv2_authoritative_and_load_ready_rows` | 0 |
| `active_shadow_working_builder_bcv2_authoritative_but_missing_on_rpc_rows` | 87 |
| `active_shadow_working_builder_bcv2_pubkey_mismatch_rows` | 0 |
| `active_shadow_working_builder_bcv2_observed_tx_missing_on_rpc_rows` | 87 |
| `active_shadow_working_builder_bcv2_account_state_missing_rows` | 87 |
| `active_shadow_working_builder_creator_vault_source_authority_counts` | `{"authoritative_account_overrides_creator_pubkey": 87}` |
| `active_shadow_working_builder_creator_vault_rpc_load_status_counts` | `{"missing_on_rpc_precheck": 42, "rpc_load_ready": 45}` |
| `active_shadow_working_builder_creator_vault_authoritative_and_load_ready_rows` | 45 |
| `active_shadow_working_builder_creator_vault_authoritative_but_missing_on_rpc_rows` | 42 |
| `active_shadow_working_builder_creator_vault_source_mismatch_rows` | 0 |
| `active_shadow_working_builder_manifest_ready_after_account_source_repair_rows` | 0 |
| `active_shadow_working_builder_manifest_still_not_ready_after_account_source_repair_rows` | 87 |

## Acceptance Check

PASS-A is not met:

- `working_builder_manifest_ready_rows = 0`
- `successful_probe_entry_rows = 0`
- `active_shadow_successful_entry_rows = 0`
- `lifecycle_eligible_rows = 0`

PASS-B is met:

- probe working-builder parity rows exist: 566
- probe and active-shadow buy variant stayed `routed_exact_sol_in`
- `legacy_buy_route_attempted_rows = 0`
- `legacy_fallback_attempted_rows = 0`
- `selected_route_handoff_mismatch_rows = 0`
- `post_simulation_account_not_found_rows = 0`
- remaining blockers are concrete account-source/readiness issues on the working builder manifest:
  - `bonding_curve_v2` observed-tx meta is authoritative and route-compatible, but `missing_on_rpc_precheck`
  - `creator_vault` is authoritative, but partially `missing_on_rpc_precheck`

FAIL is not met:

- no probe `legacy_buy` working-builder variant rows
- no selected legacy handoff rows
- no stale route diagnostics rows
- no selected-route handoff mismatch rows
- no post-simulation `AccountNotFound`
- no `working_builder_manifest_ready=true` row with a missing required account

## Interpretation

X5 code-level instrumentation worked: the smoke can now prove account-source
readiness at the working builder boundary without falling back to legacy route
state. The runtime did not regress to the X3S/X4 failure class.

X5 did not make the final manifest executable. The dominant blocker is
`bonding_curve_v2`: every probe and active-shadow working-builder row has
`authoritative_observed_tx`, no pubkey mismatch, and route-compatible observed
metadata, but the account is still `missing_on_rpc_precheck`. `creator_vault`
is a secondary blocker because most probe rows are load-ready, but a minority
still fail RPC precheck.

## Final Decision

X5S = PASS-B

Concrete next step:

P3.7-X6 - Working Builder BCV2 RPC-Load Readiness Reconciliation

Scope for X6:

- keep `working_builder_parity` as the only execution-builder mode for this path
- keep `legacy_buy_route_attempted_rows = 0`
- keep `selected_route_handoff_mismatch_rows = 0`
- determine why route-compatible observed `bonding_curve_v2` metas are missing on RPC precheck
- compare observed-tx BCV2 pubkey, AccountStateCore state, MFS/DIAG state, and RPC commitment/timing
- only after BCV2 is load-ready, finish the remaining `creator_vault` partial readiness gap
- do not start R18 before X6/X6S returns PASS-A
