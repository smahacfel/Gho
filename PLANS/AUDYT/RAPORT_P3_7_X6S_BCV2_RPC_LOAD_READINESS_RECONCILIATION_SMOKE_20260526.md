# RAPORT P3.7-X6S - BCV2 RPC-Load Readiness Reconciliation Smoke - 2026-05-26

## Status

Decision: PASS-B

Meaning: X6S preserved the working-builder parity path and produced a concrete
BCV2 reconciliation classification, but did not unlock executable entries.

The working builder path remained structurally clean:

- `legacy_buy_route_attempted_rows = 0`
- `legacy_fallback_attempted_rows = 0`
- `selected_route_handoff_mismatch_rows = 0`
- `post_simulation_account_not_found_rows = 0`
- `working_builder_buy_variant_counts = {"routed_exact_sol_in": 379}` probe
- `active_shadow_working_builder_buy_variant_counts = {"routed_exact_sol_in": 24}`

Execution is still blocked because the final working builder manifest is not
ready. The dominant blocker is `bonding_curve_v2`: pubkey identity is consistent
across observed tx, builder manifest, and precheck, but the account remains
`missing_on_rpc_precheck`.

## Runtime

- namespace: `shadow-burnin-v3-p37-x6s-bcv2-readiness-reconciliation-smoke`
- config: `configs/rollout/shadow-burnin-v3-p37-x6s-bcv2-readiness-reconciliation-smoke.local.toml`
- code checkpoint: `6eb1274 Implement P3.7 X6 BCV2 readiness reconciliation diagnostics`
- env source: `/root/Gho/.env`, mapped into `CHAINSTACK_*` env vars for runtime
- preflight: PASS before smoke
- start: `2026-05-25T23:31:00Z`
- stop: `2026-05-26T00:01:00Z`
- duration: about 30m
- termination: `timeout` cap, `X6S_EXIT_STATUS=124`

Operator note: when the user requested a 20-minute restart, the process was
still alive, so it was not restarted. It completed the already-running timeout
window and was closed by timeout.

Runtime mode evidence:

- config log: `execution_mode=Shadow entry_mode=shadow_only`
- trigger init: `TriggerComponent initialized (execution_mode: Shadow, entry_mode: ShadowOnly)`
- `LiveSellHandle: skipped (no live transport required at startup)`
- local config uses placeholders for `CHAINSTACK_GRPC_ENDPOINT`, `CHAINSTACK_GRPC_TOKEN`, and `CHAINSTACK_RPC_URL`

No live Sender submission evidence was found:

- `send_transaction(` matches: 0
- `LiveTxSender::send_transaction` matches: 0
- `SUBMITTED` matches: 0
- `trigger.live_sender` matches: 0
- `Sender transport is unavailable` warnings: 8

The `Sender transport is unavailable` rows are refusal warnings from shadow mode
tip-resolution code. They are not Sender submissions.

## Artifacts

- console log: `logs/rollout/shadow-burnin-v3-p37-x6s-bcv2-readiness-reconciliation-smoke/x6s_background_console.log`
- system logs: `logs/rollout/shadow-burnin-v3-p37-x6s-bcv2-readiness-reconciliation-smoke/system.log.2026-05-25`, `logs/rollout/shadow-burnin-v3-p37-x6s-bcv2-readiness-reconciliation-smoke/system.log.2026-05-26`
- oracle logs: `logs/rollout/shadow-burnin-v3-p37-x6s-bcv2-readiness-reconciliation-smoke/oracle.log.2026-05-25`, `logs/rollout/shadow-burnin-v3-p37-x6s-bcv2-readiness-reconciliation-smoke/oracle.log.2026-05-26`
- probe selection: `logs/shadow_run/shadow-burnin-v3-p37-x6s-bcv2-readiness-reconciliation-smoke/probe_selection.jsonl`
- probe transport: `logs/shadow_run/shadow-burnin-v3-p37-x6s-bcv2-readiness-reconciliation-smoke/probe_transport.jsonl`
- probe skips: `logs/shadow_run/shadow-burnin-v3-p37-x6s-bcv2-readiness-reconciliation-smoke/probe_skips.jsonl`
- active shadow transport: `logs/shadow_run/shadow-burnin-v3-p37-x6s-bcv2-readiness-reconciliation-smoke/buys.jsonl`
- active shadow entries: `logs/shadow_run/shadow-burnin-v3-p37-x6s-bcv2-readiness-reconciliation-smoke/shadow_entries.jsonl`
- active shadow lifecycle: `logs/shadow_run/shadow-burnin-v3-p37-x6s-bcv2-readiness-reconciliation-smoke/shadow_lifecycle.jsonl`
- offline audit JSON: `logs/shadow_run/shadow-burnin-v3-p37-x6s-bcv2-readiness-reconciliation-smoke/v3_p37_x6s_join_key_audit.json`
- offline audit MD: `logs/shadow_run/shadow-burnin-v3-p37-x6s-bcv2-readiness-reconciliation-smoke/v3_p37_x6s_join_key_audit.md`

Artifact row counts:

| Artifact | Rows |
|---|---:|
| `decision` | 602 |
| `probe_selection` | 380 |
| `probe_skip` | 214 |
| `probe_transport` | 379 |
| `probe_entry` | 0 |
| `probe_lifecycle` | 0 |
| `shadow_transport` | 8 |
| `shadow_entry` | 8 |
| `shadow_lifecycle` | 8 |

## Execution Feasibility

| Field | Value |
|---|---:|
| `decision_rows_total` | 602 |
| `probe_selected_rows` | 380 |
| `route_executable_rows` | 0 |
| `route_non_executable_rows` | 403 |
| `successful_entry_rows` | 0 |
| `lifecycle_eligible_rows` | 0 |
| `execution_feasibility_reject_rows` | 403 |
| `active_buy_execution_infeasible_rows` | 24 |
| `execution_feasibility_rate` | 0.0 |

Feasibility status counts:

- probe: `{"not_executable_route": 379, "unknown": 214}`
- active shadow: `{"not_executable_route": 24}`

## Hard Invariants

| Invariant | Probe | Active shadow |
|---|---:|---:|
| `working_builder_parity_rows` | 379 | 24 |
| `working_builder_request_built_rows` | 379 | 24 |
| `working_builder_buy_variant_counts` | `{"routed_exact_sol_in": 379}` | `{"routed_exact_sol_in": 24}` |
| `probe_working_builder_variant_drift_rows` | 0 | 0 |
| `probe_working_builder_legacy_variant_rows` | 0 | 0 |
| `probe_working_builder_selected_legacy_handoff_rows` | 0 | 0 |
| `probe_working_builder_stale_route_diagnostics_rows` | 0 | 0 |
| `legacy_fallback_attempted_rows` | 0 | 0 |
| `selected_route_handoff_mismatch_rows` | 0 | 0 |
| `legacy_buy_route_attempted_rows` | 0 | 0 |
| `account_not_found_after_simulation_rows` | 0 | 0 |
| `bonding_curve_v2_account_not_found_after_simulation_rows` | 0 | 0 |

The active-shadow columns use the audit's `active_shadow_*` counters.

## Working Builder Manifest

| Field | Probe | Active shadow |
|---|---:|---:|
| `working_builder_manifest_ready_rows` | 0 | 0 |
| `working_builder_manifest_missing_required_rows` | 379 | 24 |
| `working_builder_manifest_contains_bcv2_rows` | 379 | 24 |
| `working_builder_manifest_ready_after_account_source_repair_rows` | 0 | 0 |
| `working_builder_manifest_still_not_ready_after_account_source_repair_rows` | 379 | 24 |

Final manifest blocked rows in console:

- `P37_SHADOW_PROBE_SELECTED_ROUTE_FINAL_MANIFEST_BLOCKED`: 379

## BCV2 Reconciliation

Probe:

| Field | Value |
|---|---:|
| `working_builder_bcv2_source_authority_counts` | `{"authoritative_observed_tx": 379}` |
| `working_builder_bcv2_rpc_load_status_counts` | `{"missing_on_rpc_precheck": 379}` |
| `working_builder_bcv2_reconciliation_class_counts` | `{"local_state_gap": 288, "commitment_or_timing_suspected": 91}` |
| `working_builder_bcv2_pubkey_consistency_status_counts` | `{"builder_observed_precheck_match": 379}` |
| `working_builder_bcv2_precheck_commitment_counts` | `{"processed": 379}` |
| `working_builder_bcv2_rpc_error_class_counts` | `{"account_missing": 379}` |
| `working_builder_bcv2_loaded_address_source_counts` | `{"resolved_transaction_account_keys": 379}` |
| `working_builder_bcv2_precheck_age_bucket_counts` | `{"0": 5, "1_2": 86, "3_8": 180, "9_32": 108}` |
| `working_builder_bcv2_precheck_pubkey_rows` | 379 |
| `working_builder_bcv2_builder_pubkey_rows` | 379 |
| `working_builder_bcv2_observed_pubkey_rows` | 379 |
| `working_builder_bcv2_observed_slot_rows` | 379 |
| `working_builder_bcv2_observed_tx_signature_rows` | 379 |
| `working_builder_bcv2_precheck_context_slot_rows` | 379 |
| `working_builder_bcv2_precheck_attempt_count_rows` | 379 |
| `working_builder_bcv2_precheck_latency_rows` | 379 |
| `working_builder_bcv2_precheck_age_from_observed_slot_rows` | 379 |
| `working_builder_bcv2_loaded_address_source_missing_rows` | 0 |
| `working_builder_bcv2_authoritative_and_load_ready_rows` | 0 |
| `working_builder_bcv2_authoritative_but_missing_on_rpc_rows` | 379 |
| `working_builder_bcv2_pubkey_mismatch_rows` | 0 |
| `working_builder_bcv2_observed_tx_missing_on_rpc_rows` | 379 |
| `working_builder_bcv2_account_state_missing_rows` | 379 |

Active shadow:

| Field | Value |
|---|---:|
| `active_shadow_working_builder_bcv2_source_authority_counts` | `{"authoritative_observed_tx": 24}` |
| `active_shadow_working_builder_bcv2_rpc_load_status_counts` | `{"missing_on_rpc_precheck": 24}` |
| `active_shadow_working_builder_bcv2_reconciliation_class_counts` | `{"local_state_gap": 21, "commitment_or_timing_suspected": 3}` |
| `active_shadow_working_builder_bcv2_pubkey_consistency_status_counts` | `{"builder_observed_precheck_match": 24}` |
| `active_shadow_working_builder_bcv2_precheck_commitment_counts` | `{"processed": 24}` |
| `active_shadow_working_builder_bcv2_rpc_error_class_counts` | `{"account_missing": 24}` |
| `active_shadow_working_builder_bcv2_loaded_address_source_counts` | `{"resolved_transaction_account_keys": 24}` |
| `active_shadow_working_builder_bcv2_precheck_age_bucket_counts` | `{"1_2": 3, "3_8": 18, "9_32": 3}` |
| `active_shadow_working_builder_bcv2_authoritative_and_load_ready_rows` | 0 |
| `active_shadow_working_builder_bcv2_authoritative_but_missing_on_rpc_rows` | 24 |
| `active_shadow_working_builder_bcv2_pubkey_mismatch_rows` | 0 |
| `active_shadow_working_builder_bcv2_observed_tx_missing_on_rpc_rows` | 24 |
| `active_shadow_working_builder_bcv2_account_state_missing_rows` | 24 |

Raw age check:

- probe `working_builder_bcv2_precheck_age_from_observed_slot`: min 0, max 19, negative rows 0
- active shadow `working_builder_bcv2_precheck_age_from_observed_slot`: min 2, max 10, negative rows 0
- precheck attempt count: 1 for every probe and active-shadow row
- probe precheck latency: min 15 ms, max 1220 ms, average about 50.54 ms
- active-shadow precheck latency: min 22 ms, max 114 ms, average about 36.25 ms

## Creator Vault Readiness

Probe:

| Field | Value |
|---|---:|
| `working_builder_creator_vault_source_authority_counts` | `{"authoritative_detected_pool_creator": 379}` |
| `working_builder_creator_vault_rpc_load_status_counts` | `{"rpc_load_ready": 362, "missing_on_rpc_precheck": 17}` |
| `working_builder_creator_vault_authoritative_and_load_ready_rows` | 362 |
| `working_builder_creator_vault_authoritative_but_missing_on_rpc_rows` | 17 |
| `working_builder_creator_vault_source_mismatch_rows` | 0 |

Active shadow:

| Field | Value |
|---|---:|
| `active_shadow_working_builder_creator_vault_source_authority_counts` | `{"authoritative_account_overrides_creator_pubkey": 24}` |
| `active_shadow_working_builder_creator_vault_rpc_load_status_counts` | `{"rpc_load_ready": 24}` |
| `active_shadow_working_builder_creator_vault_authoritative_and_load_ready_rows` | 24 |
| `active_shadow_working_builder_creator_vault_authoritative_but_missing_on_rpc_rows` | 0 |
| `active_shadow_working_builder_creator_vault_source_mismatch_rows` | 0 |

Creator vault is no longer the primary blocker in active shadow. Probe still has
17 missing rows, but BCV2 blocks 379/379 probe rows and 24/24 active-shadow rows.

## Interpretation

X6S did not find a pubkey wiring mismatch:

- `builder_observed_precheck_match = 379/379` probe
- `builder_observed_precheck_match = 24/24` active shadow
- `working_builder_bcv2_pubkey_mismatch_rows = 0`
- `active_shadow_working_builder_bcv2_pubkey_mismatch_rows = 0`

X6S did not find post-simulation `AccountNotFound`; the system fails closed
before simulation when the final working-builder manifest is missing BCV2.

The dominant reconciliation class is `local_state_gap`:

- probe: 288/379 BCV2 rows
- active shadow: 21/24 BCV2 rows

The secondary class is `commitment_or_timing_suspected`:

- probe: 91/379 BCV2 rows
- active shadow: 3/24 BCV2 rows

Because `local_state_gap` dominates and all pubkeys match, the next step should
not be R18 and should not be route discovery. The next targeted fix should
repair BCV2 coverage from observed tx into AccountStateCore/MFS/DIAG/readiness
for the working builder path. The timing/commitment class should remain visible
and can be addressed by a delayed or multi-commitment recheck after local state
coverage is corrected, or earlier if the repair shows it is the remaining class.

## Acceptance Check

PASS-A is not met:

- `working_builder_manifest_ready_rows = 0`
- `successful_probe_entry_rows = 0`
- `active_shadow_successful_entry_rows = 0`
- `lifecycle_eligible_rows = 0`

PASS-B is met:

- working-builder parity rows exist in probe and active shadow
- all working-builder variants are `routed_exact_sol_in`
- legacy route/fallback/handoff pollution is zero
- post-simulation `AccountNotFound` is zero
- BCV2 has a concrete reconciliation classification
- pubkey consistency is clean
- remaining blocker is concrete: `bonding_curve_v2` observed-tx identity is authoritative and consistent, but RPC precheck reports `missing_on_rpc_precheck`

FAIL is not met:

- no live Sender submission
- no `legacy_buy` probe variant
- no selected legacy handoff
- no selected-route handoff mismatch
- no post-simulation `AccountNotFound`
- no pubkey mismatch
- no negative precheck-age rows
- no `working_builder_manifest_ready=true` row with missing required accounts

## Final Decision

X6S = PASS-B

Concrete next step:

P3.7-X7 - Working Builder BCV2 Local State Coverage Repair

Scope:

- keep `working_builder_parity`
- keep `legacy_buy_route_attempted_rows = 0`
- keep `selected_route_handoff_mismatch_rows = 0`
- repair BCV2 coverage from observed tx into AccountStateCore/MFS/DIAG/readiness for the working builder path
- keep processed single-attempt precheck diagnostics visible
- preserve fail-closed behavior when final builder manifest accounts are missing
- do not start R18 until manifest-ready rows and successful shadow/probe entries appear without invariant regressions

NO-GO:

- no R18 before X7/X7S or equivalent targeted readiness repair
- no live/P2/Sender
- no Gatekeeper, threshold, V3 selector, L2D2, or scoring changes
- no legacy_buy fallback or BCV2 handoff patch
