# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r27-all-decision-counterfactual-30-30.toml`
- readiness: `ready_for_lifecycle_feature_join`
- join_key_acceptance: `degraded`
- join_quality: `mint_only`
- probe_readiness: `ready_for_probe_transport_entry_join`
- probe_join_key_acceptance: `pass`
- probe_join_quality: `exact_probe_id_and_ab_record_id`
- probe_decision_join_acceptance: `pass`
- probe_required_exact_decision_v3_join_coverage: `1.0`
- probe_entry_materialization_status_counts: `{"entry_materialized": 32, "simulation_error": 5}`
- probe_entry_materialization_reason_counts: `{"entry_row_present": 32, "simulation_account_layout_mismatch:custom_2006": 1, "simulation_instruction_error": 4}`
- full_chain_ab_record_id_coverage: `1.0`
- probe_chain_ab_record_id_coverage: `1.0`
- probe_chain_probe_id_coverage: `1.0`
- readiness_reasons: `[]`
- probe_readiness_reasons: `[]`
- decision_rows_with_ab_record_id: `943`
- shadow_transport_rows_with_ab_record_id: `63`
- shadow_entry_rows_with_ab_record_id: `63`
- shadow_lifecycle_rows_with_ab_record_id: `177`
- onchain_lifecycle_rows_with_ab_record_id: `0`
- probe_transport_rows_with_ab_record_id: `37`
- probe_entry_rows_with_ab_record_id: `33`
- probe_transport_rows_with_probe_id: `37`
- probe_entry_rows_with_probe_id: `33`

## Artifact Coverage

| artifact | rows | candidate_id | ab_record_id | probe_id | pool_id | mint | v3_payload | feature_hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `decision` | 63 | 0 | 63 | 0 | 63 | 63 | 63 | 63 |
| `decision` | 440 | 0 | 440 | 0 | 440 | 440 | 440 | 440 |
| `decision` | 440 | 0 | 440 | 0 | 440 | 440 | 440 | 440 |
| `oracle_log` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `oracle_log` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `probe_entry` | 33 | 33 | 33 | 33 | 33 | 33 | 0 | 33 |
| `probe_lifecycle` | 64 | 64 | 64 | 64 | 64 | 64 | 0 | 64 |
| `probe_selection` | 42 | 42 | 42 | 42 | 42 | 42 | 0 | 42 |
| `probe_skip` | 399 | 399 | 399 | 399 | 399 | 399 | 0 | 399 |
| `probe_transport` | 37 | 37 | 37 | 37 | 37 | 37 | 0 | 37 |
| `shadow_entry` | 63 | 63 | 63 | 0 | 63 | 63 | 0 | 63 |
| `shadow_lifecycle` | 177 | 177 | 177 | 0 | 177 | 177 | 0 | 177 |
| `shadow_transport` | 63 | 63 | 63 | 0 | 0 | 63 | 0 | 63 |
| `system_log` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `system_log` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 6, "common_values": 63, "per_artifact_values": [63, 440, 440, 63, 63, 63]}`
- `probe_id`: `{"artifacts_with_rows": 6, "common_values": 0, "per_artifact_values": [0, 0, 0, 0, 0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 6, "common_values": 0, "per_artifact_values": [0, 0, 0, 63, 63, 63]}`
- `pool_id`: `{"artifacts_with_rows": 6, "common_values": 0, "per_artifact_values": [63, 440, 440, 63, 63, 0]}`
- `mint`: `{"artifacts_with_rows": 6, "common_values": 63, "per_artifact_values": [63, 440, 440, 63, 63, 63]}`

## Probe Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 4, "common_values": 32, "per_artifact_values": [33, 32, 42, 37]}`
- `probe_id`: `{"artifacts_with_rows": 4, "common_values": 32, "per_artifact_values": [33, 32, 42, 37]}`
- `candidate_id`: `{"artifacts_with_rows": 4, "common_values": 0, "per_artifact_values": [33, 32, 42, 37]}`
- `pool_id`: `{"artifacts_with_rows": 4, "common_values": 32, "per_artifact_values": [33, 32, 42, 37]}`
- `mint`: `{"artifacts_with_rows": 4, "common_values": 32, "per_artifact_values": [33, 32, 42, 37]}`

## Probe Decision Join

- decision_join_acceptance: `pass`
- required_exact_decision_v3_join_coverage: `1.0`
- `probe_selection`: `{"exact_decision_v3_join": 42, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 42, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 42, "joined_to_decision_with_v3_payload": 42, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 42}, "policy_hash_match": 42, "policy_hash_mismatch": 0, "rows": 42, "rows_with_ab_record_id": 42, "unmatched_rows": 0}`
- `probe_transport`: `{"exact_decision_v3_join": 37, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 37, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 37, "joined_to_decision_with_v3_payload": 37, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 37}, "policy_hash_match": 37, "policy_hash_mismatch": 0, "rows": 37, "rows_with_ab_record_id": 37, "unmatched_rows": 0}`
- `probe_entry`: `{"exact_decision_v3_join": 33, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 33, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 33, "joined_to_decision_with_v3_payload": 33, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 33}, "policy_hash_match": 33, "policy_hash_mismatch": 0, "rows": 33, "rows_with_ab_record_id": 33, "unmatched_rows": 0}`
- `probe_lifecycle`: `{"exact_decision_v3_join": 64, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 64, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 64, "joined_to_decision_with_v3_payload": 64, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 64}, "policy_hash_match": 64, "policy_hash_mismatch": 0, "rows": 64, "rows_with_ab_record_id": 64, "unmatched_rows": 0}`

## BCV2 Exact Watch Coverage

- bcv2_exact_watch_registered_rows: `3380`
- bcv2_exact_watch_in_subscribe_request_rows: `468`
- bcv2_exact_watch_subscribe_dropped_rows: `0`
- bcv2_resubscribe_sent_rows: `0`
- bcv2_rpc_hydration_ready_rows: `0`
- bcv2_rpc_hydration_missing_rows: `5092`
- bcv2_account_update_received_rows: `518`
- bcv2_account_state_seen_rows: `0`
- bcv2_account_state_owner_rows: `0`
- bcv2_account_state_data_len_rows: `0`

## BCV2 Working-Builder Pubkey Join

- marker_rows: `{"BCV2_ACCOUNT_UPDATE_RECEIVED": 518, "BCV2_EXACT_WATCH_REGISTERED": 3380, "BCV2_EXACT_WATCH_RESUBSCRIBE_SENT": 0, "BCV2_EXACT_WATCH_SUBSCRIBE_DROPPED": 0, "BCV2_EXACT_WATCH_SUBSCRIBE_INCLUDED": 468, "BCV2_RPC_HYDRATION_MISSING": 5092, "BCV2_RPC_HYDRATION_READY": 0}`
- marker_unique_pubkeys: `{"BCV2_ACCOUNT_UPDATE_RECEIVED": 60, "BCV2_EXACT_WATCH_REGISTERED": 323, "BCV2_EXACT_WATCH_RESUBSCRIBE_SENT": 0, "BCV2_EXACT_WATCH_SUBSCRIBE_DROPPED": 0, "BCV2_EXACT_WATCH_SUBSCRIBE_INCLUDED": 0, "BCV2_RPC_HYDRATION_MISSING": 323, "BCV2_RPC_HYDRATION_READY": 0}`
- watchdog_fatal_rows: `0`
- working_builder_bcv2_rows: `0`
- working_builder_bcv2_unique_pubkeys: `0`
- working_builder_bcv2_registered_unique_pubkeys: `0`
- working_builder_bcv2_included_unique_pubkeys: `0`
- working_builder_bcv2_resubscribe_sent_unique_pubkeys: `0`
- working_builder_bcv2_hydration_ready_unique_pubkeys: `0`
- working_builder_bcv2_hydration_missing_unique_pubkeys: `0`
- working_builder_bcv2_account_update_same_pubkey_unique_pubkeys: `0`
- global_bcv2_account_update_unique_pubkeys: `60`
- global_bcv2_account_update_other_pubkey_unique_pubkeys: `60`
- classification_rows: `{}`
- classification_unique_pubkeys: `{}`

| pubkey | rows | registered | included | resubscribe | update_same_pubkey | hydration_missing | classes |
| --- | ---: | --- | --- | --- | --- | --- | --- |

## X8D Unique BCV2 Pubkey Join

- schema: `x8d_pr1_unique_bcv2_pubkey_join_v1`
- unique_bcv2_pubkeys: `0`
- working_builder_rows: `0`
- primary_bucket_unique_pubkeys: `{}`
- primary_bucket_rows: `{}`
- audit_bucket_unique_pubkeys: `{}`
- audit_bucket_rows: `{}`
- capacity_summary: `{"drop_marker_rows": 0, "max_bcv2_dropped": null, "max_bcv2_sent": null, "max_exact_payload_cap": null, "max_tracked_bcv2": null}`

| pubkey | primary_bucket | audit_buckets | registered | included_inferred | dropped_over_cap | same_pubkey_update | hydration_missing | exec_ready_rows |
| --- | --- | --- | --- | --- | --- | --- | --- | ---: |

## Execution Feasibility

- decision_rows_total: `943`
- probe_selected_rows: `42`
- route_executable_rows: `39`
- route_non_executable_rows: `260`
- successful_entry_rows: `94`
- lifecycle_eligible_rows: `32`
- lifecycle_labeled_rows: `241`
- buy_quality_labeled_rows: `241`
- execution_feasibility_reject_rows: `8`
- active_buy_execution_infeasible_rows: `7`
- execution_feasibility_rate: `0.9285714285714286`
- entry_materialization_rate: `2.41025641025641`
- lifecycle_label_rate: `2.5638297872340425`
- probe_execution_feasibility_status_counts: `{"executable": 37, "not_executable_route": 1, "not_executable_route_identity": 252, "unknown": 146}`
- active_shadow_execution_feasibility_status_counts: `{"executable": 2, "not_executable_route": 7}`

## Active Shadow Dispatch Diagnostics

- active_shadow_transport_rows: `63`
- active_shadow_entry_rows: `63`
- active_shadow_lifecycle_rows: `177`
- active_shadow_dispatch_failure_rows: `9`
- active_shadow_precheck_failed_rows: `3`
- active_shadow_runtime_simulation_error_rows: `6`
- active_shadow_successful_entry_rows: `62`
- active_shadow_lifecycle_eligible_rows: `0`
- active_shadow_lifecycle_eligible_failure_rows: `0`
- active_shadow_simulation_required_account_not_in_precheck_count: `0`
- active_shadow_bonding_curve_v2_precheck_skipped_before_simulation_rows: `3`
- active_shadow_bonding_curve_v2_account_not_found_after_simulation_rows: `0`
- active_shadow_account_not_found_rows: `0`
- active_shadow_account_not_found_attributed_rows: `0`
- active_shadow_account_not_found_multi_candidate_rows: `0`
- active_shadow_account_not_found_unattributed_rows: `0`
- active_shadow_rpc_visibility_gap_rows: `0`
- active_shadow_account_not_found_role_counts: `{"bonding_curve_v2": 3}`
- active_shadow_simulation_error_category_counts: `{"active_shadow_precheck_failed": 3}`
- active_shadow_precheck_status_counts: `{"not_run_post_simulation_attribution": 2, "precheck_failed": 3, "precheck_recovered": 4}`
- active_shadow_lifecycle_eligibility_status_counts: `{"not_lifecycle_eligible": 9}`
- active_shadow_account_set_match_counts: `{"true": 9}`
- active_shadow_account_narrowing_status_counts: `{"exact_after_narrowing": 3}`
- active_shadow_account_candidate_raw_counts: `{"bonding_curve_v2": 3}`
- active_shadow_account_candidate_narrowed_counts: `{"bonding_curve_v2": 3}`
- active_shadow_bonding_curve_v2_authority_status_counts: `{"authoritative_observed_tx": 3, "meta_only_protocol_derived": 6}`
- active_shadow_bonding_curve_v2_identity_authority_status_counts: `{"authoritative_observed_tx": 3, "meta_only_protocol_derived": 6}`
- active_shadow_bonding_curve_v2_mismatch_reason_counts: `{"bonding_curve_v2_observed_meta_missing_on_rpc": 3}`
- active_shadow_bonding_curve_v2_source_counts: `{"observed_tx_account_meta": 3, "route_builder": 6}`
- active_shadow_bonding_curve_v2_rpc_load_status_counts: `{"BCV2_LOAD_NOT_REQUIRED": 6, "missing_on_rpc_precheck": 3}`
- active_shadow_bonding_curve_v2_rpc_load_ready_counts: `{"false": 3, "true": 6}`
- active_shadow_builder_required_curve_account_ready_counts: `{"false": 3, "true": 6}`
- active_shadow_builder_required_curve_account_ready_reason_counts: `{"BCV2_META_READY_BY_PROTOCOL_SCHEMA": 6, "bonding_curve_v2_observed_meta_missing_on_rpc": 3}`
- active_shadow_observed_bcv2_provenance_status_counts: `{"route_compatible": 3}`
- active_shadow_observed_bcv2_rows: `3`
- active_shadow_observed_bcv2_route_compatible_rows: `3`
- active_shadow_observed_bcv2_not_route_compatible_rows: `0`
- active_shadow_observed_bcv2_missing_provenance_rows: `0`
- active_shadow_observed_bcv2_authoritative_without_route_compatible_rows: `0`
- active_shadow_route_fallback_attempted_rows: `3`
- active_shadow_route_fallback_success_rows: `0`
- active_shadow_route_fallback_failed_rows: `3`
- active_shadow_working_builder_parity_rows: `0`
- active_shadow_working_builder_request_built_rows: `0`
- active_shadow_working_builder_buy_variant_counts: `{}`
- active_shadow_probe_working_builder_variant_drift_rows: `0`
- active_shadow_probe_working_builder_legacy_variant_rows: `0`
- active_shadow_probe_working_builder_selected_legacy_handoff_rows: `0`
- active_shadow_probe_working_builder_stale_route_diagnostics_rows: `0`
- active_shadow_legacy_fallback_attempted_rows: `3`
- active_shadow_selected_route_handoff_mismatch_rows: `0`
- active_shadow_working_builder_manifest_missing_required_rows: `0`
- active_shadow_working_builder_manifest_ready_rows: `0`
- active_shadow_working_builder_manifest_contains_bcv2_rows: `0`
- active_shadow_working_builder_bcv2_source_authority_counts: `{}`
- active_shadow_working_builder_bcv2_rpc_load_status_counts: `{}`
- active_shadow_working_builder_bcv2_reconciliation_class_counts: `{}`
- active_shadow_working_builder_bcv2_pubkey_consistency_status_counts: `{}`
- active_shadow_working_builder_bcv2_precheck_commitment_counts: `{}`
- active_shadow_working_builder_bcv2_rpc_error_class_counts: `{}`
- active_shadow_working_builder_bcv2_loaded_address_source_counts: `{}`
- active_shadow_working_builder_bcv2_precheck_age_bucket_counts: `{}`
- active_shadow_working_builder_bcv2_local_coverage_class_counts: `{}`
- active_shadow_working_builder_bcv2_materialization_class_counts: `{}`
- active_shadow_working_builder_bcv2_subscription_requested_counts: `{}`
- active_shadow_working_builder_bcv2_account_update_received_counts: `{}`
- active_shadow_working_builder_bcv2_account_update_mapped_counts: `{}`
- active_shadow_working_builder_bcv2_account_state_lookup_performed_counts: `{}`
- active_shadow_working_builder_bcv2_account_state_age_bucket_counts: `{}`
- active_shadow_working_builder_bcv2_mfs_seen_reason_counts: `{}`
- active_shadow_working_builder_bcv2_diag_seen_reason_counts: `{}`
- active_shadow_working_builder_bcv2_precheck_pubkey_rows: `0`
- active_shadow_working_builder_bcv2_builder_pubkey_rows: `0`
- active_shadow_working_builder_bcv2_observed_pubkey_rows: `0`
- active_shadow_working_builder_bcv2_observed_slot_rows: `0`
- active_shadow_working_builder_bcv2_observed_tx_signature_rows: `0`
- active_shadow_working_builder_bcv2_precheck_context_slot_rows: `0`
- active_shadow_working_builder_bcv2_precheck_attempt_count_rows: `0`
- active_shadow_working_builder_bcv2_precheck_latency_rows: `0`
- active_shadow_working_builder_bcv2_precheck_age_from_observed_slot_rows: `0`
- active_shadow_working_builder_bcv2_loaded_address_source_missing_rows: `0`
- active_shadow_working_builder_bcv2_account_state_lookup_performed_rows: `0`
- active_shadow_working_builder_bcv2_account_state_seen_rows: `0`
- active_shadow_working_builder_bcv2_account_state_seen_slot_rows: `0`
- active_shadow_working_builder_bcv2_account_state_age_slots_rows: `0`
- active_shadow_working_builder_bcv2_account_state_owner_rows: `0`
- active_shadow_working_builder_bcv2_account_state_data_len_rows: `0`
- active_shadow_working_builder_bcv2_subscription_requested_rows: `0`
- active_shadow_working_builder_bcv2_account_update_received_rows: `0`
- active_shadow_working_builder_bcv2_account_update_mapped_rows: `0`
- active_shadow_working_builder_bcv2_rpc_fetch_ready_rows: `0`
- active_shadow_working_builder_bcv2_rpc_fetch_missing_rows: `0`
- active_shadow_working_builder_bcv2_rpc_fetch_owner_rows: `0`
- active_shadow_working_builder_bcv2_rpc_fetch_data_len_rows: `0`
- active_shadow_working_builder_bcv2_account_state_materialized_rows: `0`
- active_shadow_working_builder_bcv2_mfs_materialized_rows: `0`
- active_shadow_working_builder_bcv2_diag_materialized_rows: `0`
- active_shadow_working_builder_bcv2_evidence_rows: `0`
- active_shadow_working_builder_bcv2_evidence_ready_rows: `0`
- active_shadow_working_builder_bcv2_evidence_conflict_rows: `0`
- active_shadow_working_builder_bcv2_evidence_owner_rows: `0`
- active_shadow_working_builder_bcv2_evidence_data_len_rows: `0`
- active_shadow_working_builder_bcv2_evidence_slot_rows: `0`
- active_shadow_working_builder_bcv2_evidence_context_slot_rows: `0`
- active_shadow_working_builder_bcv2_evidence_status_counts: `{}`
- active_shadow_working_builder_bcv2_evidence_source_counts: `{}`
- active_shadow_working_builder_bcv2_evidence_reason_counts: `{}`
- active_shadow_working_builder_bcv2_execution_evidence_ready_rows: `0`
- active_shadow_working_builder_bcv2_execution_evidence_conflict_rows: `0`
- active_shadow_working_builder_bcv2_execution_evidence_stale_rows: `0`
- active_shadow_working_builder_bcv2_execution_evidence_exact_pubkey_match_rows: `0`
- active_shadow_working_builder_bcv2_execution_evidence_status_counts: `{}`
- active_shadow_working_builder_bcv2_execution_evidence_source_counts: `{}`
- active_shadow_working_builder_bcv2_execution_evidence_reason_counts: `{}`
- active_shadow_bcv2_terminal_route_exclusion_rows: `0`
- active_shadow_bcv2_terminal_route_exclusion_unique_pubkeys: `0`
- active_shadow_execution_feasibility_reject_bcv2_not_persistent_rows: `0`
- active_shadow_buy_quality_denominator_excluded_bcv2_rows: `0`
- active_shadow_lifecycle_denominator_excluded_bcv2_rows: `0`
- active_shadow_working_builder_creator_vault_source_authority_counts: `{}`
- active_shadow_working_builder_creator_vault_rpc_load_status_counts: `{}`
- active_shadow_working_builder_bcv2_authoritative_and_load_ready_rows: `0`
- active_shadow_working_builder_bcv2_authoritative_but_missing_on_rpc_rows: `0`
- active_shadow_working_builder_bcv2_pubkey_mismatch_rows: `0`
- active_shadow_working_builder_bcv2_observed_tx_missing_on_rpc_rows: `0`
- active_shadow_working_builder_bcv2_account_state_missing_rows: `0`
- active_shadow_working_builder_creator_vault_authoritative_and_load_ready_rows: `0`
- active_shadow_working_builder_creator_vault_authoritative_but_missing_on_rpc_rows: `0`
- active_shadow_working_builder_creator_vault_source_mismatch_rows: `0`
- active_shadow_working_builder_manifest_ready_after_account_source_repair_rows: `0`
- active_shadow_working_builder_manifest_still_not_ready_after_account_source_repair_rows: `0`
- active_shadow_legacy_buy_route_attempted_rows: `5`
- active_shadow_legacy_buy_route_ready_rows: `2`
- active_shadow_legacy_buy_route_not_ready_rows: `3`
- active_shadow_legacy_buy_missing_core_curve_account_rows: `0`
- active_shadow_legacy_buy_missing_associated_bonding_curve_rows: `0`
- active_shadow_legacy_buy_authoritative_curve_rows: `5`
- active_shadow_legacy_buy_rpc_load_ready_rows: `5`
- active_shadow_legacy_buy_successful_entry_rows: `60`
- active_shadow_legacy_buy_account_set_status_counts: `{"not_ready": 3, "ready": 2}`
- active_shadow_legacy_buy_curve_source_counts: `{"account_state_core": 5}`
- active_shadow_legacy_buy_curve_authority_status_counts: `{"authoritative_account_state": 5}`
- active_shadow_legacy_buy_curve_rpc_load_status_counts: `{"rpc_load_ready": 5}`
- active_shadow_legacy_buy_curve_authority_readiness_status_counts: `{"authoritative_and_load_ready": 5}`
- active_shadow_legacy_buy_curve_authoritative_and_load_ready_rows: `5`
- active_shadow_legacy_buy_curve_load_ready_but_authority_unverified_rows: `0`
- active_shadow_legacy_buy_curve_authoritative_but_not_checked_rows: `0`
- active_shadow_legacy_buy_curve_derived_matches_account_state_rows: `0`
- active_shadow_legacy_buy_curve_derived_mismatch_account_state_rows: `0`
- active_shadow_legacy_buy_route_ready_after_reconciliation_rows: `2`
- active_shadow_legacy_buy_route_still_not_ready_after_reconciliation_rows: `3`
- active_shadow_legacy_buy_route_not_ready_reason_counts: `{"legacy_buy_simulation_load_not_ready": 3}`
- active_shadow_legacy_buy_primary_bcv2_leak_rows: `0`
- active_shadow_legacy_buy_missing_creatable_user_ata_rows: `3`
- active_shadow_legacy_buy_missing_creatable_user_volume_accumulator_rows: `0`
- active_shadow_legacy_buy_missing_ephemeral_payer_rows: `0`
- active_shadow_legacy_buy_blocking_missing_required_rows: `3`
- active_shadow_legacy_buy_non_blocking_missing_creatable_rows: `0`
- active_shadow_legacy_buy_non_blocking_ephemeral_payer_rows: `0`
- active_shadow_legacy_buy_fallback_account_set_ready_rows: `2`
- active_shadow_legacy_buy_route_ready_after_account_set_separation_rows: `2`
- active_shadow_legacy_buy_route_unsupported_builder_layout_rows: `0`
- active_shadow_legacy_buy_excluded_from_execution_route_universe_rows: `0`
- active_shadow_legacy_buy_removed_from_fallback_candidates_rows: `0`
- active_shadow_selected_fallback_route_ready_rows: `2`
- active_shadow_selected_fallback_route_handoff_applied_rows: `2`
- active_shadow_selected_fallback_route_handoff_mismatch_rows: `0`
- active_shadow_selected_fallback_route_handoff_not_applied_rows: `0`
- active_shadow_selected_fallback_route_blocked_by_primary_reason_rows: `0`
- active_shadow_legacy_buy_selected_but_primary_bcv2_terminal_rows: `0`
- active_shadow_selected_legacy_handoff_claimed_rows: `2`
- active_shadow_selected_legacy_handoff_validated_rows: `2`
- active_shadow_selected_legacy_handoff_mismatch_rows: `0`
- active_shadow_selected_legacy_final_manifest_contains_bcv2_rows: `2`
- active_shadow_selected_legacy_final_manifest_contains_primary_route_builder_rows: `2`
- active_shadow_selected_legacy_request_variant_not_legacy_rows: `0`
- active_shadow_selected_legacy_precheck_hash_mismatch_rows: `0`
- active_shadow_selected_legacy_simulation_hash_mismatch_rows: `0`
- active_shadow_no_executable_route_but_simulated_rows: `3`
- active_shadow_legacy_buy_selected_but_request_variant_not_legacy_rows: `0`
- active_shadow_legacy_buy_selected_but_primary_bcv2_in_selected_manifest_rows: `2`
- active_shadow_legacy_buy_selected_but_precheck_hash_mismatch_rows: `0`
- active_shadow_legacy_buy_selected_but_simulation_hash_mismatch_rows: `0`
- active_shadow_legacy_buy_selected_and_precheck_uses_legacy_account_set_rows: `2`
- active_shadow_legacy_buy_selected_and_simulation_uses_legacy_account_set_rows: `2`
- active_shadow_fallback_failure_class_counts: `{"fallback_missing_creator_vault": 3}`
- active_shadow_fallback_missing_role_counts: `{"creator_vault": 3, "user_ata": 3}`
- active_shadow_fallback_account_source_counts: `{"route_builder": 3, "user_ata": 3}`
- active_shadow_fallback_repairable: `False`
- active_shadow_recommended_next_path: `route_class_exclusion_from_execution_label_universe`
- active_shadow_executable_route_ready_rows: `2`
- active_shadow_route_executable_rows: `2`
- active_shadow_route_non_executable_rows: `7`
- active_shadow_execution_feasibility_reject_rows: `7`
- active_buy_execution_infeasible_rows: `7`
- active_shadow_execution_feasibility_status_counts: `{"executable": 2, "not_executable_route": 7}`
- active_shadow_execution_feasibility_reason_counts: `{"no_executable_route_account_set": 7, "primary_route_ready": 2}`
- active_shadow_lifecycle_label_eligibility_counts: `{"lifecycle_label_candidate": 2, "not_lifecycle_label_eligible": 7}`

## Probe Entry Materialization

- transport_rows: `37`
- entry_rows: `33`
- transport_without_entry_rows: `4`
- status_counts: `{"entry_materialized": 32, "simulation_error": 5}`
- reason_counts: `{"entry_row_present": 32, "simulation_account_layout_mismatch:custom_2006": 1, "simulation_instruction_error": 4}`
- buy_variant_counts: `{"legacy_buy": 37}`
- token_param_role_counts: `{"token_amount": 37}`
- creator_vault_authority_status_counts: `{"creator_vault_source_not_authoritative": 1}`
- creator_vault_mismatch_reason_counts: `{"actual_expected_mismatch": 1}`
- creator_identity_source_counts: `{"account_overrides.creator_pubkey": 1}`
- bonding_curve_v2_authority_status_counts: `{"meta_only_protocol_derived": 37}`
- bonding_curve_v2_identity_authority_status_counts: `{"meta_only_protocol_derived": 37}`
- bonding_curve_v2_mismatch_reason_counts: `{}`
- bonding_curve_v2_source_counts: `{"route_builder": 37}`
- bonding_curve_v2_rpc_load_status_counts: `{"BCV2_LOAD_NOT_REQUIRED": 37}`
- bonding_curve_v2_rpc_load_ready_counts: `{"true": 37}`
- builder_required_curve_account_ready_counts: `{"true": 37}`
- builder_required_curve_account_ready_reason_counts: `{"BCV2_META_READY_BY_PROTOCOL_SCHEMA": 37}`
- observed_bcv2_provenance_status_counts: `{}`
- observed_bcv2_rows: `0`
- observed_bcv2_route_compatible_rows: `0`
- observed_bcv2_not_route_compatible_rows: `0`
- observed_bcv2_missing_provenance_rows: `0`
- observed_bcv2_instruction_account_position_present_rows: `0`
- observed_bcv2_message_account_index_present_rows: `0`
- observed_bcv2_authoritative_without_route_compatible_rows: `0`
- amount_guard_status_counts: `{}`
- simulation_error_category_counts: `{"simulation_account_layout_mismatch": 1, "simulation_instruction_error": 4}`
- simulation_error_kind_counts: `{"simulation_error": 5}`
- simulation_error_account_role_counts: `{"creator_vault": 1}`
- simulation_error_account_source_counts: `{}`
- simulation_error_custom_code_counts: `{"custom_2006": 1}`
- account_set_match_counts: `{"true": 37}`
- account_set_mismatch_reason_counts: `{}`
- account_not_found_rows: `0`
- account_not_found_attributed_rows: `0`
- account_not_found_multi_candidate_rows: `0`
- account_not_found_unattributed_rows: `0`
- simulation_rpc_visibility_gap_rows: `0`
- simulation_required_account_not_in_precheck_rows: `0`
- simulation_account_meta_missing_on_rpc_rows: `0`
- bonding_curve_v2_precheck_skipped_before_simulation_rows: `0`
- bonding_curve_v2_account_not_found_after_simulation_rows: `0`
- precheck_simulation_account_set_mismatch_rows: `0`
- successful_probe_entry_rows: `32`
- simulation_error_entry_rows: `1`
- lifecycle_eligible_entry_rows: `32`
- route_fallback_attempted_rows: `1`
- route_fallback_success_rows: `0`
- route_fallback_failed_rows: `1`
- working_builder_parity_rows: `0`
- working_builder_request_built_rows: `0`
- working_builder_buy_variant_counts: `{}`
- probe_working_builder_variant_drift_rows: `0`
- probe_working_builder_legacy_variant_rows: `0`
- probe_working_builder_selected_legacy_handoff_rows: `0`
- probe_working_builder_stale_route_diagnostics_rows: `0`
- legacy_fallback_attempted_rows: `1`
- selected_route_handoff_mismatch_rows: `0`
- working_builder_manifest_missing_required_rows: `0`
- working_builder_manifest_ready_rows: `0`
- working_builder_manifest_contains_bcv2_rows: `0`
- working_builder_bcv2_source_authority_counts: `{}`
- working_builder_bcv2_rpc_load_status_counts: `{}`
- working_builder_bcv2_reconciliation_class_counts: `{}`
- working_builder_bcv2_pubkey_consistency_status_counts: `{}`
- working_builder_bcv2_precheck_commitment_counts: `{}`
- working_builder_bcv2_rpc_error_class_counts: `{}`
- working_builder_bcv2_loaded_address_source_counts: `{}`
- working_builder_bcv2_precheck_age_bucket_counts: `{}`
- working_builder_bcv2_local_coverage_class_counts: `{}`
- working_builder_bcv2_materialization_class_counts: `{}`
- working_builder_bcv2_subscription_requested_counts: `{}`
- working_builder_bcv2_account_update_received_counts: `{}`
- working_builder_bcv2_account_update_mapped_counts: `{}`
- working_builder_bcv2_account_state_lookup_performed_counts: `{}`
- working_builder_bcv2_account_state_age_bucket_counts: `{}`
- working_builder_bcv2_mfs_seen_reason_counts: `{}`
- working_builder_bcv2_diag_seen_reason_counts: `{}`
- working_builder_bcv2_precheck_pubkey_rows: `0`
- working_builder_bcv2_builder_pubkey_rows: `0`
- working_builder_bcv2_observed_pubkey_rows: `0`
- working_builder_bcv2_observed_slot_rows: `0`
- working_builder_bcv2_observed_tx_signature_rows: `0`
- working_builder_bcv2_precheck_context_slot_rows: `0`
- working_builder_bcv2_precheck_attempt_count_rows: `0`
- working_builder_bcv2_precheck_latency_rows: `0`
- working_builder_bcv2_precheck_age_from_observed_slot_rows: `0`
- working_builder_bcv2_loaded_address_source_missing_rows: `0`
- working_builder_bcv2_account_state_lookup_performed_rows: `0`
- working_builder_bcv2_account_state_seen_rows: `0`
- working_builder_bcv2_account_state_seen_slot_rows: `0`
- working_builder_bcv2_account_state_age_slots_rows: `0`
- working_builder_bcv2_account_state_owner_rows: `0`
- working_builder_bcv2_account_state_data_len_rows: `0`
- working_builder_bcv2_subscription_requested_rows: `0`
- working_builder_bcv2_account_update_received_rows: `0`
- working_builder_bcv2_account_update_mapped_rows: `0`
- working_builder_bcv2_rpc_fetch_ready_rows: `0`
- working_builder_bcv2_rpc_fetch_missing_rows: `0`
- working_builder_bcv2_rpc_fetch_owner_rows: `0`
- working_builder_bcv2_rpc_fetch_data_len_rows: `0`
- working_builder_bcv2_account_state_materialized_rows: `0`
- working_builder_bcv2_mfs_materialized_rows: `0`
- working_builder_bcv2_diag_materialized_rows: `0`
- working_builder_bcv2_evidence_rows: `0`
- working_builder_bcv2_evidence_ready_rows: `0`
- working_builder_bcv2_evidence_conflict_rows: `0`
- working_builder_bcv2_evidence_owner_rows: `0`
- working_builder_bcv2_evidence_data_len_rows: `0`
- working_builder_bcv2_evidence_slot_rows: `0`
- working_builder_bcv2_evidence_context_slot_rows: `0`
- working_builder_bcv2_evidence_status_counts: `{}`
- working_builder_bcv2_evidence_source_counts: `{}`
- working_builder_bcv2_evidence_reason_counts: `{}`
- working_builder_bcv2_execution_evidence_ready_rows: `0`
- working_builder_bcv2_execution_evidence_conflict_rows: `0`
- working_builder_bcv2_execution_evidence_stale_rows: `0`
- working_builder_bcv2_execution_evidence_exact_pubkey_match_rows: `0`
- working_builder_bcv2_execution_evidence_status_counts: `{}`
- working_builder_bcv2_execution_evidence_source_counts: `{}`
- working_builder_bcv2_execution_evidence_reason_counts: `{}`
- bcv2_terminal_route_exclusion_rows: `0`
- bcv2_terminal_route_exclusion_unique_pubkeys: `0`
- execution_feasibility_reject_bcv2_not_persistent_rows: `0`
- buy_quality_denominator_excluded_bcv2_rows: `0`
- lifecycle_denominator_excluded_bcv2_rows: `0`
- working_builder_creator_vault_source_authority_counts: `{}`
- working_builder_creator_vault_rpc_load_status_counts: `{}`
- working_builder_bcv2_authoritative_and_load_ready_rows: `0`
- working_builder_bcv2_authoritative_but_missing_on_rpc_rows: `0`
- working_builder_bcv2_pubkey_mismatch_rows: `0`
- working_builder_bcv2_observed_tx_missing_on_rpc_rows: `0`
- working_builder_bcv2_account_state_missing_rows: `0`
- working_builder_creator_vault_authoritative_and_load_ready_rows: `0`
- working_builder_creator_vault_authoritative_but_missing_on_rpc_rows: `0`
- working_builder_creator_vault_source_mismatch_rows: `0`
- working_builder_manifest_ready_after_account_source_repair_rows: `0`
- working_builder_manifest_still_not_ready_after_account_source_repair_rows: `0`
- legacy_buy_route_attempted_rows: `38`
- legacy_buy_route_ready_rows: `37`
- legacy_buy_route_not_ready_rows: `1`
- legacy_buy_missing_core_curve_account_rows: `0`
- legacy_buy_missing_associated_bonding_curve_rows: `0`
- legacy_buy_authoritative_curve_rows: `38`
- legacy_buy_rpc_load_ready_rows: `38`
- legacy_buy_successful_entry_rows: `32`
- legacy_buy_account_set_status_counts: `{"not_ready": 1, "ready": 37}`
- legacy_buy_curve_source_counts: `{"account_state_core": 38}`
- legacy_buy_curve_authority_status_counts: `{"authoritative_account_state": 38}`
- legacy_buy_curve_rpc_load_status_counts: `{"rpc_load_ready": 38}`
- legacy_buy_curve_authority_readiness_status_counts: `{"authoritative_and_load_ready": 38}`
- legacy_buy_curve_authoritative_and_load_ready_rows: `38`
- legacy_buy_curve_load_ready_but_authority_unverified_rows: `0`
- legacy_buy_curve_authoritative_but_not_checked_rows: `0`
- legacy_buy_curve_derived_matches_account_state_rows: `0`
- legacy_buy_curve_derived_mismatch_account_state_rows: `0`
- legacy_buy_route_ready_after_reconciliation_rows: `37`
- legacy_buy_route_still_not_ready_after_reconciliation_rows: `1`
- legacy_buy_route_not_ready_reason_counts: `{"legacy_buy_simulation_load_not_ready": 1}`
- legacy_buy_primary_bcv2_leak_rows: `0`
- legacy_buy_missing_creatable_user_ata_rows: `1`
- legacy_buy_missing_creatable_user_volume_accumulator_rows: `0`
- legacy_buy_missing_ephemeral_payer_rows: `0`
- legacy_buy_blocking_missing_required_rows: `1`
- legacy_buy_non_blocking_missing_creatable_rows: `0`
- legacy_buy_non_blocking_ephemeral_payer_rows: `0`
- legacy_buy_fallback_account_set_ready_rows: `37`
- legacy_buy_route_ready_after_account_set_separation_rows: `37`
- legacy_buy_route_unsupported_builder_layout_rows: `0`
- legacy_buy_excluded_from_execution_route_universe_rows: `0`
- legacy_buy_removed_from_fallback_candidates_rows: `0`
- selected_fallback_route_ready_rows: `37`
- selected_fallback_route_handoff_applied_rows: `37`
- selected_fallback_route_handoff_mismatch_rows: `0`
- selected_fallback_route_handoff_not_applied_rows: `0`
- selected_fallback_route_blocked_by_primary_reason_rows: `0`
- legacy_buy_selected_but_primary_bcv2_terminal_rows: `0`
- selected_legacy_handoff_claimed_rows: `37`
- selected_legacy_handoff_validated_rows: `37`
- selected_legacy_handoff_mismatch_rows: `0`
- selected_legacy_final_manifest_contains_bcv2_rows: `37`
- selected_legacy_final_manifest_contains_primary_route_builder_rows: `5`
- selected_legacy_request_variant_not_legacy_rows: `0`
- selected_legacy_precheck_hash_mismatch_rows: `0`
- selected_legacy_simulation_hash_mismatch_rows: `0`
- no_executable_route_but_simulated_rows: `0`
- legacy_buy_selected_but_request_variant_not_legacy_rows: `0`
- legacy_buy_selected_but_primary_bcv2_in_selected_manifest_rows: `37`
- legacy_buy_selected_but_precheck_hash_mismatch_rows: `0`
- legacy_buy_selected_but_simulation_hash_mismatch_rows: `0`
- legacy_buy_selected_and_precheck_uses_legacy_account_set_rows: `37`
- legacy_buy_selected_and_simulation_uses_legacy_account_set_rows: `37`
- fallback_failure_class_counts: `{"fallback_missing_creator_vault": 1}`
- fallback_missing_role_counts: `{"creator_vault": 1, "user_ata": 1}`
- fallback_account_source_counts: `{"route_builder": 1, "user_ata": 1}`
- fallback_repairable: `False`
- recommended_next_path: `route_class_exclusion_from_execution_label_universe`
- executable_route_ready_rows: `37`
- probe_selected_rows: `42`
- route_executable_rows: `37`
- route_non_executable_rows: `253`
- execution_feasibility_reject_rows: `1`
- execution_feasibility_status_counts: `{"executable": 37, "not_executable_route": 1, "not_executable_route_identity": 252, "unknown": 146}`
- execution_feasibility_reason_counts: `{"creator_vault_source_not_authoritative": 252, "no_executable_route_account_set": 1, "primary_route_ready": 37, "probe_execution_precheck_failed": 146}`
- lifecycle_label_eligibility_counts: `{"lifecycle_label_candidate": 37, "not_lifecycle_label_eligible": 1, "unknown": 398}`
- lifecycle_labeled_rows: `64`
- skip_reason_counts: `{"creator_vault_source_not_authoritative": 252, "no_executable_route_account_set": 1, "probe_execution_precheck_failed": 146}`
- skip_execution_account_readiness_role_counts: `{"bonding_curve_v2": 1, "creator_vault": 252}`
- skip_creator_vault_authority_status_counts: `{"creator_vault_source_not_authoritative": 252}`
- skip_creator_vault_mismatch_reason_counts: `{"creator_identity_source_not_authoritative": 252}`
- skip_creator_identity_source_counts: `{"detected_pool.creator": 252}`
- skip_bonding_curve_v2_authority_status_counts: `{}`
- skip_bonding_curve_v2_mismatch_reason_counts: `{}`
- skip_bonding_curve_v2_source_counts: `{}`

| probe_id | status | reason | buy_variant | token_param_role | entry_token_amount_raw | min_tokens_out |
| --- | --- | --- | --- | --- | ---: | ---: |
| `fafffe972039060e1ca87ad446840150aba5b335b4157f6d382a47ecf4f58aed` | `simulation_error` | `simulation_instruction_error` | `legacy_buy` | `token_amount` | `232262818800` | `185810255040` |
| `5a6876401c2c6179fe47274d307912f14694bb2cb3ef2f98744dcd482ee2cc6a` | `simulation_error` | `simulation_instruction_error` | `legacy_buy` | `token_amount` | `236635248748` | `189308198998` |
| `d86d25ce9506a3b13ce01980b6e26b0ba7b747343e1f9639a1db97c94d79cdd0` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `242689183907` | `194151347125` |
| `bb008103535eef92429b11dc3746a88c7beb5648e7a6598fdad0c2718b429636` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `222235494933` | `177788395946` |
| `d57c6eb3752cd9768ea8a6eb3c54cd7cf93631da80424a133ee9c008c37350fc` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `231526412969` | `185221130375` |
| `deb78450c18c681b254e456cb123e576d857370e79960e150df9e0c3163e59ac` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `246156313820` | `196925051056` |
| `e09d5bd15cc5437737b2c3a88d522132c134b0e9259cb8e1864be0eee5661318` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `238027418153` | `190421934522` |
| `ed31adcdd74eca17a81e5428767a39e445a4cddf32cb5459abc26c4e27b79364` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `247805756854` | `198244605483` |
| `b926dfebd61f5d1eec236f7746954e3eda793b45865a9ebfcc8ac576f0d4a5f0` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `242317037988` | `193853630390` |
| `aa7b8c6e062d6298a72c7a9d5776b81896618e651e97926fdd6e2ea8ba24a98a` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `247320795859` | `197856636687` |
| `2f3650efc4ace92b6a512596b5774dbd1376798fbdc18efe1bd83e5e72787e53` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `230980212521` | `184784170016` |
| `f6f097415cc4f349b8975b6d7554527f0d022912afaf63e3de40247ed5653691` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `243141434797` | `194513147837` |
| `98abb1a130ea91887a6b928716e4f7524a2c30e4377d4da2fd33a07e0c40d67f` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `237950982955` | `190360786364` |
| `587738fa8a619eca42fd228d8f138222d3e6166148a34e882d589deb23d09e9c` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `182711231263` | `146168985010` |
| `e3a170be3293a089dcdac328379e77fd9c3ec61e62facb96314e3f02d8652a51` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `legacy_buy` | `token_amount` | `247805756862` | `198244605489` |
| `a72159b8ad37a9416cfe1e5996e2ee814657137a3237aa5042b866b49eec2f59` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `244502821570` | `195602257256` |
| `26f781a74b365047a946f376ba5e3d0bed9b7fcbf824772ac396c27b12e6b7d5` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `179127735677` | `143302188541` |
| `e6e85f1de82e87715260af6208d7bd23d77774ea5d5ccec4779f75e4ac82b2bb` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `182711231269` | `146168985015` |
| `c023f72b0976baf25d7f9c884e3c5e298e00ba22729f3616914a76bdb04c9a85` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `227327837109` | `181862269687` |
| `73c2d5eeaa0e3d5a8f83784ae1d3173a85b41b375343072a0ed1e1b1b2856fd2` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `182711231295` | `146168985036` |
| `a14c6413c49f89710371de60e4333e3188b393320bedd1082e015868315e31a6` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `182711231300` | `146168985040` |
| `48f52d6e507d170d3a2110fedf0d2d2e35a7757a6520ecbc239ce32119d05419` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `247332316619` | `197865853295` |
| `4910889536a701a1a56cb46a7e9c6b7adeca2e32caf58685e0cde33a64b311ad` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `222235494905` | `177788395924` |
| `0251f2971854cb708e7cf9607ecfba5c260a5a945b13105c6121e261544dc87e` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `244641992877` | `195713594301` |
| `424b40bcb2f9c965d70a580e2526b1ab1a743c3f905d5d42881bce48bfc594c4` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `182711231300` | `146168985040` |
| `1de6068dba7745b9a34de2b384f0cec342eb32915eb121d7cd4a6f4f40950807` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `229766453723` | `183813162978` |
| `fc31bdb81cf6f41674ecb6a5d25974fad94fc2372e5f80576909ae28675fd777` | `simulation_error` | `simulation_instruction_error` | `legacy_buy` | `token_amount` | `245227013340` | `196181610672` |
| `af7e6e39b7f91307de822d2b407b5019f3988c6a9765663280e7b31da64127eb` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `245502016780` | `196401613424` |
| `898d2bc1e276bacf432df598e675119092baffcfd6d01c9664b5eb589220d8d2` | `simulation_error` | `simulation_instruction_error` | `legacy_buy` | `token_amount` | `247803369084` | `198242695267` |
| `06998b514595cf8795bf95cdb5a9aaddcc4996cace25d3f9feee6dd875918362` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `155263044356` | `124210435484` |
| `bed4ae85b6c6b06f4a53d899779e963646a29cef2768bf3a672f592f2d71e56e` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `247805756862` | `198244605489` |
| `c8252879130a95e0d6f184ef1d3da539c0d5bbdc90d21e14ac9ea852b5a14874` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `247805756862` | `198244605489` |
| `a16651dd0538f8ba63d746fd15ec26c8acbd1b86cfdaffdc7d42a5e0a3da1e16` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `182711231274` | `146168985019` |
| `14d33751562b0af8e3ad4a9f447886b9fc88c777589eac9919b04209958381a5` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `229402738232` | `183522190585` |
| `eae3e2fa7809d0358f781b965a52d65549ecb90dcfc7eef48a55461fa3b88961` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `227327837109` | `181862269687` |
| `df9bad4308a5054d19110d0dac34064caad7c1b552452ea84340919773cf89f4` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `243727184580` | `194981747664` |
| `cc2aeaff4e700c42887c13e40b14e34d3909336ebcb4f091efe83a96f808d735` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `242192853512` | `193754282809` |

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
