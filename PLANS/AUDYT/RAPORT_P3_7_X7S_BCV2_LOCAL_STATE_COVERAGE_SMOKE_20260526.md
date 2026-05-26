# RAPORT P3.7-X7S - BCV2 Local State Coverage Smoke - 2026-05-26

## Status

Decision: PASS-B

Meaning: X7S preserved the working-builder parity path and proved that execution
is still blocked. The run did not produce manifest-ready rows, probe entries, or
lifecycle-eligible rows. It did, however, classify the BCV2 local coverage gap:
probe rows are `observed_only_no_account_state` and no row has BCV2 present in
AccountStateCore/MFS/DIAG evidence.

R18 remains NO-GO.

## Runtime

- namespace: `shadow-burnin-v3-p37-x7s-bcv2-local-coverage-smoke`
- config: `configs/rollout/shadow-burnin-v3-p37-x7s-bcv2-local-coverage-smoke.local.toml`
- code checkpoint: `5cc6a54 Implement P3.7 X7 BCV2 local coverage diagnostics`
- preflight: PASS
- runtime mode: `execution_mode=Shadow`, `entry_mode=shadow_only`
- working builder mode: `p37_execution_builder_mode = "working_builder_parity"`
- start marker: `X7S_START_UTC=2026-05-26T01:04:25Z`
- last console timestamp: `2026-05-26T01:34:25Z`
- process status after operator stop request: no matching `timeout` or `ghost-launcher` process remained
- explicit stop marker/status file: not captured

The run reached the intended 30 minute window by log timestamps. The wrapper did
not write `X7S_STOP_UTC` or `X7S_EXIT_STATUS`, so the exact exit status is not
claimed.

No live Sender submission evidence was found:

- `send_transaction(` matches: 0
- `LiveTxSender::send_transaction` matches: 0
- `SUBMITTED` matches: 0
- `trigger.live_sender` matches: 0

`Sender transport is unavailable` appeared 14 times. Those rows are shadow-mode
refusal/warning evidence and not Sender submissions.

## Artifacts

- console log: `logs/rollout/shadow-burnin-v3-p37-x7s-bcv2-local-coverage-smoke/x7s_background_console.log`
- preflight log: `logs/rollout/shadow-burnin-v3-p37-x7s-bcv2-local-coverage-smoke/x7s_preflight.log`
- system log: `logs/rollout/shadow-burnin-v3-p37-x7s-bcv2-local-coverage-smoke/system.log.2026-05-26`
- oracle log: `logs/rollout/shadow-burnin-v3-p37-x7s-bcv2-local-coverage-smoke/oracle.log.2026-05-26`
- probe selection: `logs/shadow_run/shadow-burnin-v3-p37-x7s-bcv2-local-coverage-smoke/probe_selection.jsonl`
- probe transport: `logs/shadow_run/shadow-burnin-v3-p37-x7s-bcv2-local-coverage-smoke/probe_transport.jsonl`
- probe skips: `logs/shadow_run/shadow-burnin-v3-p37-x7s-bcv2-local-coverage-smoke/probe_skips.jsonl`
- active shadow transport: `logs/shadow_run/shadow-burnin-v3-p37-x7s-bcv2-local-coverage-smoke/buys.jsonl`
- active shadow entries: `logs/shadow_run/shadow-burnin-v3-p37-x7s-bcv2-local-coverage-smoke/shadow_entries.jsonl`
- active shadow lifecycle: `logs/shadow_run/shadow-burnin-v3-p37-x7s-bcv2-local-coverage-smoke/shadow_lifecycle.jsonl`
- offline audit JSON: `logs/shadow_run/shadow-burnin-v3-p37-x7s-bcv2-local-coverage-smoke/v3_p37_x7s_join_key_audit.json`
- offline audit MD: `logs/shadow_run/shadow-burnin-v3-p37-x7s-bcv2-local-coverage-smoke/v3_p37_x7s_join_key_audit.md`

Artifact row counts:

| Artifact | Rows |
|---|---:|
| `decision` | 506 |
| `probe_selection` | 398 |
| `probe_skip` | 94 |
| `probe_transport` | 398 |
| `probe_entry` | 0 |
| `probe_lifecycle` | 0 |
| `shadow_transport` | 14 |
| `shadow_entry` | 14 |
| `shadow_lifecycle` | 14 |

## Execution Feasibility

| Field | Value |
|---|---:|
| `decision_rows_total` | 506 |
| `probe_selected_rows` | 398 |
| `route_executable_rows` | 0 |
| `route_non_executable_rows` | 440 |
| `successful_entry_rows` | 0 |
| `lifecycle_eligible_rows` | 0 |
| `execution_feasibility_reject_rows` | 440 |
| `active_buy_execution_infeasible_rows` | 42 |
| `execution_feasibility_rate` | 0.0 |

Feasibility status counts:

- probe: `{"not_executable_route": 398, "unknown": 94}`
- active shadow: `{"not_executable_route": 42}`

## Hard Invariants

| Invariant | Probe | Active shadow |
|---|---:|---:|
| `working_builder_parity_rows` | 398 | 42 |
| `working_builder_request_built_rows` | 398 | 42 |
| `working_builder_buy_variant_counts` | `{"routed_exact_sol_in": 398}` | `{"routed_exact_sol_in": 42}` |
| `probe_working_builder_legacy_variant_rows` | 0 | 0 |
| `probe_working_builder_selected_legacy_handoff_rows` | 0 | 0 |
| `probe_working_builder_stale_route_diagnostics_rows` | 0 | n/a |
| `legacy_fallback_attempted_rows` | 0 | 0 |
| `selected_route_handoff_mismatch_rows` | 0 | 0 |
| `legacy_buy_route_attempted_rows` | 0 | 0 |
| `account_not_found_after_simulation_rows` | 0 | 0 |
| `bonding_curve_v2_account_not_found_after_simulation_rows` | 0 | 0 |

The working-builder parity contract stayed clean. There was no legacy/fallback
pollution and no selected route handoff mismatch.

## Working Builder Manifest

| Field | Probe | Active shadow |
|---|---:|---:|
| `working_builder_manifest_ready_rows` | 0 | 0 |
| `working_builder_manifest_missing_required_rows` | 398 | 42 |
| `working_builder_manifest_contains_bcv2_rows` | 398 | 42 |
| `working_builder_manifest_ready_after_account_source_repair_rows` | 0 | 0 |
| `working_builder_manifest_still_not_ready_after_account_source_repair_rows` | 398 | 42 |

Final manifest blocked rows in console:

- `P37_SHADOW_PROBE_SELECTED_ROUTE_FINAL_MANIFEST_BLOCKED`: 398

## BCV2 Local Coverage

Probe:

| Field | Value |
|---|---:|
| `working_builder_bcv2_local_coverage_class_counts` | `{"observed_only_no_account_state": 398}` |
| `working_builder_bcv2_account_state_lookup_performed_counts` | `{"true": 398}` |
| `working_builder_bcv2_account_state_age_bucket_counts` | `{"missing": 398}` |
| `working_builder_bcv2_mfs_seen_reason_counts` | `{"mfs_missing_bonding_curve_v2_identity": 398}` |
| `working_builder_bcv2_diag_seen_reason_counts` | `{"diag_missing_bonding_curve_v2_identity": 398}` |
| `working_builder_bcv2_account_state_lookup_performed_rows` | 398 |
| `working_builder_bcv2_account_state_seen_rows` | 0 |
| `working_builder_bcv2_account_state_seen_slot_rows` | 0 |
| `working_builder_bcv2_account_state_age_slots_rows` | 0 |
| `working_builder_bcv2_account_state_owner_rows` | 0 |
| `working_builder_bcv2_account_state_data_len_rows` | 0 |

Active shadow:

| Field | Value |
|---|---:|
| `active_shadow_working_builder_bcv2_local_coverage_class_counts` | `{"missing": 14, "observed_only_no_account_state": 28}` |
| `active_shadow_working_builder_bcv2_account_state_lookup_performed_counts` | `{"missing": 14, "true": 28}` |
| `active_shadow_working_builder_bcv2_account_state_age_bucket_counts` | `{"missing": 42}` |
| `active_shadow_working_builder_bcv2_mfs_seen_reason_counts` | `{"mfs_missing_bonding_curve_v2_identity": 28, "missing": 14}` |
| `active_shadow_working_builder_bcv2_diag_seen_reason_counts` | `{"diag_missing_bonding_curve_v2_identity": 28, "missing": 14}` |
| `active_shadow_working_builder_bcv2_account_state_lookup_performed_rows` | 28 |
| `active_shadow_working_builder_bcv2_account_state_seen_rows` | 0 |
| `active_shadow_working_builder_bcv2_account_state_seen_slot_rows` | 0 |
| `active_shadow_working_builder_bcv2_account_state_age_slots_rows` | 0 |
| `active_shadow_working_builder_bcv2_account_state_owner_rows` | 0 |
| `active_shadow_working_builder_bcv2_account_state_data_len_rows` | 0 |

This is the key X7S result: BCV2 is not seen in AccountStateCore/MFS/DIAG for
any probe or active-shadow row. X7 did not repair coverage; it confirmed and
named the gap.

## BCV2 Readiness And Reconciliation

Probe:

| Field | Value |
|---|---:|
| `working_builder_bcv2_source_authority_counts` | `{"authoritative_observed_tx": 398}` |
| `working_builder_bcv2_rpc_load_status_counts` | `{"missing_on_rpc_precheck": 398}` |
| `working_builder_bcv2_reconciliation_class_counts` | `{"commitment_or_timing_suspected": 106, "local_state_gap": 292}` |
| `working_builder_bcv2_pubkey_consistency_status_counts` | `{"builder_observed_precheck_match": 398}` |
| `working_builder_bcv2_precheck_commitment_counts` | `{"processed": 398}` |
| `working_builder_bcv2_rpc_error_class_counts` | `{"account_missing": 398}` |
| `working_builder_bcv2_loaded_address_source_counts` | `{"resolved_transaction_account_keys": 398}` |
| `working_builder_bcv2_precheck_age_bucket_counts` | `{"0": 7, "1_2": 99, "3_8": 194, "9_32": 98}` |
| `working_builder_bcv2_authoritative_and_load_ready_rows` | 0 |
| `working_builder_bcv2_authoritative_but_missing_on_rpc_rows` | 398 |
| `working_builder_bcv2_pubkey_mismatch_rows` | 0 |
| `working_builder_bcv2_observed_tx_missing_on_rpc_rows` | 398 |
| `working_builder_bcv2_account_state_missing_rows` | 398 |

Active shadow:

| Field | Value |
|---|---:|
| `active_shadow_working_builder_bcv2_source_authority_counts` | `{"authoritative_observed_tx": 42}` |
| `active_shadow_working_builder_bcv2_rpc_load_status_counts` | `{"missing_on_rpc_precheck": 42}` |
| `active_shadow_working_builder_bcv2_reconciliation_class_counts` | `{"commitment_or_timing_suspected": 9, "local_state_gap": 33}` |
| `active_shadow_working_builder_bcv2_pubkey_consistency_status_counts` | `{"builder_observed_precheck_match": 42}` |
| `active_shadow_working_builder_bcv2_precheck_commitment_counts` | `{"processed": 42}` |
| `active_shadow_working_builder_bcv2_rpc_error_class_counts` | `{"account_missing": 42}` |
| `active_shadow_working_builder_bcv2_loaded_address_source_counts` | `{"resolved_transaction_account_keys": 42}` |
| `active_shadow_working_builder_bcv2_precheck_age_bucket_counts` | `{"1_2": 9, "3_8": 33}` |
| `active_shadow_working_builder_bcv2_authoritative_and_load_ready_rows` | 0 |
| `active_shadow_working_builder_bcv2_authoritative_but_missing_on_rpc_rows` | 42 |
| `active_shadow_working_builder_bcv2_pubkey_mismatch_rows` | 0 |
| `active_shadow_working_builder_bcv2_observed_tx_missing_on_rpc_rows` | 42 |
| `active_shadow_working_builder_bcv2_account_state_missing_rows` | 42 |

There is still no pubkey wiring mismatch. The pubkey is consistent across
builder, observed tx, and precheck, but it is missing on RPC precheck and absent
from local state evidence.

## Creator Vault

Probe:

| Field | Value |
|---|---:|
| `working_builder_creator_vault_source_authority_counts` | `{"authoritative_detected_pool_creator": 398}` |
| `working_builder_creator_vault_rpc_load_status_counts` | `{"missing_on_rpc_precheck": 21, "rpc_load_ready": 377}` |
| `working_builder_creator_vault_authoritative_and_load_ready_rows` | 377 |
| `working_builder_creator_vault_authoritative_but_missing_on_rpc_rows` | 21 |
| `working_builder_creator_vault_source_mismatch_rows` | 0 |

Active shadow:

| Field | Value |
|---|---:|
| `active_shadow_working_builder_creator_vault_rpc_load_status_counts` | `{"missing_on_rpc_precheck": 6, "rpc_load_ready": 36}` |
| `active_shadow_working_builder_creator_vault_authoritative_and_load_ready_rows` | 36 |
| `active_shadow_working_builder_creator_vault_authoritative_but_missing_on_rpc_rows` | 6 |

Creator vault remains a secondary blocker. BCV2 blocks every probe and
active-shadow working-builder manifest row.

## Interpretation

X7S confirms that the current working-builder path still does not execute:

- no executable route rows
- no probe entry rows
- no lifecycle-eligible rows
- no manifest-ready rows
- BCV2 is authoritative from observed tx and pubkey-consistent
- BCV2 is not load-ready on RPC precheck
- BCV2 is not present in AccountStateCore/MFS/DIAG evidence

This is not a return of the legacy/fallback bug. X4/X5/X6/X7 kept the working
builder route identity clean. The remaining failure is that the runtime still
does not have a real BCV2 state/readiness source for the working-builder
manifest.

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
- BCV2 local coverage is concretely classified
- probe BCV2 local coverage is `observed_only_no_account_state` for 398/398 rows
- `working_builder_bcv2_account_state_seen_rows = 0`
- remaining blocker is concrete: BCV2 is observed-tx authoritative and
  pubkey-consistent, but absent from local AccountStateCore/MFS/DIAG and missing
  on RPC precheck

FAIL is not met:

- no live Sender submission
- no `legacy_buy` working-builder variant
- no selected legacy handoff
- no selected-route handoff mismatch
- no post-simulation `AccountNotFound`
- no pubkey mismatch
- no manifest-ready row with missing required account

## Final Decision

X7S = PASS-B

R18 = NO-GO

Concrete next step:

P3.7-X8 - Working Builder BCV2 Runtime State Materialization Repair

Scope:

- keep `working_builder_parity`
- keep `legacy_buy_route_attempted_rows = 0`
- keep `selected_route_handoff_mismatch_rows = 0`
- do not treat observed tx meta as load-ready
- repair or explicitly prove missing BCV2 coverage in AccountStateCore/MFS/DIAG
- verify whether BCV2 account updates are unsubscribed, late, ignored, or mapped
  only as observed tx metadata
- keep RPC precheck fail-closed until BCV2 is actually load-ready

NO-GO:

- no R18 before BCV2 local state/readiness is repaired or explicitly excluded
- no live/P2/Sender
- no Gatekeeper, threshold, V3 selector, L2D2, or scoring changes
- no legacy_buy fallback or BCV2 handoff patch
