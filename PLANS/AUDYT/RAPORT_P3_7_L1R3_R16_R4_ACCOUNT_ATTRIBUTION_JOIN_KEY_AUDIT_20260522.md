# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution.toml`
- readiness: `ready_for_lifecycle_feature_join`
- join_key_acceptance: `degraded`
- join_quality: `mint_only`
- probe_readiness: `ready_for_probe_transport_entry_join`
- probe_join_key_acceptance: `pass`
- probe_join_quality: `exact_probe_id_and_ab_record_id`
- probe_decision_join_acceptance: `pass`
- probe_required_exact_decision_v3_join_coverage: `1.0`
- probe_entry_materialization_status_counts: `{"simulation_error": 15}`
- probe_entry_materialization_reason_counts: `{"simulation_account_not_found_multi_candidate": 15}`
- full_chain_ab_record_id_coverage: `1.0`
- probe_chain_ab_record_id_coverage: `1.0`
- probe_chain_probe_id_coverage: `1.0`
- readiness_reasons: `[]`
- probe_readiness_reasons: `[]`
- decision_rows_with_ab_record_id: `427`
- shadow_transport_rows_with_ab_record_id: `4`
- shadow_entry_rows_with_ab_record_id: `4`
- shadow_lifecycle_rows_with_ab_record_id: `4`
- onchain_lifecycle_rows_with_ab_record_id: `0`
- probe_transport_rows_with_ab_record_id: `15`
- probe_entry_rows_with_ab_record_id: `15`
- probe_transport_rows_with_probe_id: `15`
- probe_entry_rows_with_probe_id: `15`

## Artifact Coverage

| artifact | rows | candidate_id | ab_record_id | probe_id | pool_id | mint | v3_payload | feature_hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `decision` | 4 | 0 | 4 | 0 | 4 | 4 | 4 | 4 |
| `decision` | 423 | 0 | 423 | 0 | 423 | 423 | 423 | 423 |
| `probe_entry` | 15 | 15 | 15 | 15 | 15 | 15 | 0 | 15 |
| `probe_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `probe_selection` | 22 | 22 | 22 | 22 | 22 | 22 | 0 | 22 |
| `probe_skip` | 408 | 408 | 408 | 408 | 408 | 408 | 0 | 408 |
| `probe_transport` | 15 | 15 | 15 | 15 | 15 | 15 | 0 | 15 |
| `shadow_entry` | 4 | 4 | 4 | 0 | 4 | 4 | 0 | 4 |
| `shadow_lifecycle` | 4 | 4 | 4 | 0 | 4 | 4 | 0 | 4 |
| `shadow_transport` | 4 | 4 | 4 | 0 | 0 | 4 | 0 | 4 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 5, "common_values": 4, "per_artifact_values": [4, 423, 4, 4, 4]}`
- `probe_id`: `{"artifacts_with_rows": 5, "common_values": 0, "per_artifact_values": [0, 0, 0, 0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 5, "common_values": 0, "per_artifact_values": [0, 0, 4, 4, 4]}`
- `pool_id`: `{"artifacts_with_rows": 5, "common_values": 0, "per_artifact_values": [4, 423, 4, 4, 0]}`
- `mint`: `{"artifacts_with_rows": 5, "common_values": 4, "per_artifact_values": [4, 423, 4, 4, 4]}`

## Probe Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 3, "common_values": 15, "per_artifact_values": [15, 22, 15]}`
- `probe_id`: `{"artifacts_with_rows": 3, "common_values": 15, "per_artifact_values": [15, 22, 15]}`
- `candidate_id`: `{"artifacts_with_rows": 3, "common_values": 0, "per_artifact_values": [15, 22, 15]}`
- `pool_id`: `{"artifacts_with_rows": 3, "common_values": 15, "per_artifact_values": [15, 22, 15]}`
- `mint`: `{"artifacts_with_rows": 3, "common_values": 15, "per_artifact_values": [15, 22, 15]}`

## Probe Decision Join

- decision_join_acceptance: `pass`
- required_exact_decision_v3_join_coverage: `1.0`
- `probe_selection`: `{"exact_decision_v3_join": 22, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 22, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 22, "joined_to_decision_with_v3_payload": 22, "mismatch_reasons": {}, "policy_hash_match": 22, "policy_hash_mismatch": 0, "rows": 22, "rows_with_ab_record_id": 22, "unmatched_rows": 0}`
- `probe_transport`: `{"exact_decision_v3_join": 15, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 15, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 15, "joined_to_decision_with_v3_payload": 15, "mismatch_reasons": {}, "policy_hash_match": 15, "policy_hash_mismatch": 0, "rows": 15, "rows_with_ab_record_id": 15, "unmatched_rows": 0}`
- `probe_entry`: `{"exact_decision_v3_join": 15, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 15, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 15, "joined_to_decision_with_v3_payload": 15, "mismatch_reasons": {}, "policy_hash_match": 15, "policy_hash_mismatch": 0, "rows": 15, "rows_with_ab_record_id": 15, "unmatched_rows": 0}`
- `probe_lifecycle`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`

## Probe Entry Materialization

- transport_rows: `15`
- entry_rows: `15`
- transport_without_entry_rows: `0`
- status_counts: `{"simulation_error": 15}`
- reason_counts: `{"simulation_account_not_found_multi_candidate": 15}`
- buy_variant_counts: `{"routed_exact_sol_in": 15}`
- token_param_role_counts: `{"min_tokens_out": 15}`
- creator_vault_authority_status_counts: `{}`
- creator_vault_mismatch_reason_counts: `{}`
- creator_identity_source_counts: `{}`
- amount_guard_status_counts: `{}`
- simulation_error_category_counts: `{"simulation_account_not_found_multi_candidate": 15}`
- simulation_error_kind_counts: `{"AccountNotFound": 15}`
- simulation_error_account_role_counts: `{}`
- simulation_error_account_source_counts: `{}`
- simulation_error_custom_code_counts: `{}`
- account_set_match_counts: `{"true": 15}`
- account_set_mismatch_reason_counts: `{}`
- account_not_found_rows: `15`
- account_not_found_attributed_rows: `0`
- account_not_found_multi_candidate_rows: `15`
- account_not_found_unattributed_rows: `0`
- simulation_rpc_visibility_gap_rows: `0`
- precheck_simulation_account_set_mismatch_rows: `0`
- successful_probe_entry_rows: `0`
- simulation_error_entry_rows: `15`
- lifecycle_eligible_entry_rows: `0`
- skip_reason_counts: `{"creator_vault_source_not_authoritative": 299, "execution_account_not_ready": 1, "max_probes_per_run_exceeded": 6, "probe_execution_precheck_failed": 47, "verdict_type_not_in_sample_scope": 55}`
- skip_creator_vault_authority_status_counts: `{"creator_vault_source_not_authoritative": 299}`
- skip_creator_vault_mismatch_reason_counts: `{"creator_identity_source_not_authoritative": 299}`
- skip_creator_identity_source_counts: `{"detected_pool.creator": 299}`

| probe_id | status | reason | buy_variant | token_param_role | entry_token_amount_raw | min_tokens_out |
| --- | --- | --- | --- | --- | ---: | ---: |
| `c700a3af4c8e65e9dc79f5a2b67efdf3a17283ddb4816de3eb6aebc864a4c699` | `simulation_error` | `simulation_account_not_found_multi_candidate` | `routed_exact_sol_in` | `min_tokens_out` | `504466285360` | `403573028288` |
| `b7c92ab9c5cc0344deb36011de16de9962c39ad744979e748d9b6910721523cb` | `simulation_error` | `simulation_account_not_found_multi_candidate` | `routed_exact_sol_in` | `min_tokens_out` | `395091120809` | `316072896647` |
| `3c10fc7cca2d4b7e16dc85e0a3cfc6cae0e13f176685ee8dc9e7f7944c5001ab` | `simulation_error` | `simulation_account_not_found_multi_candidate` | `routed_exact_sol_in` | `min_tokens_out` | `126707644648` | `101366115718` |
| `906f75112f459ea1772343e4c8e53246adceeb8b5978d61f9957ad7945d34ee3` | `simulation_error` | `simulation_account_not_found_multi_candidate` | `routed_exact_sol_in` | `min_tokens_out` | `227109179723` | `181687343778` |
| `c691cf0f4bc068b8b19cdc4c69afe8c729fff9fd23b8db51422dbf49feb95c55` | `simulation_error` | `simulation_account_not_found_multi_candidate` | `routed_exact_sol_in` | `min_tokens_out` | `275761231319` | `220608985055` |
| `7dd8756dbbec61d4657fb706324d3c669c3add00741e429e867ffdbdedf60e20` | `simulation_error` | `simulation_account_not_found_multi_candidate` | `routed_exact_sol_in` | `min_tokens_out` | `783831099994` | `627064879995` |
| `dd7e786aa5949262a8b9c5c00eea8ddf1a071e3e16d6158bcea10eea10a7738c` | `simulation_error` | `simulation_account_not_found_multi_candidate` | `routed_exact_sol_in` | `min_tokens_out` | `837856553264` | `670285242611` |
| `cb5358c8fd29ec919f512cb4914ac41747119ecc163dba41048bc7eca47f74b9` | `simulation_error` | `simulation_account_not_found_multi_candidate` | `routed_exact_sol_in` | `min_tokens_out` | `712132365898` | `569705892718` |
| `71caac9eb1c4de44466f7c4de2df1a1bebd1b83282480b61041ccb867826774a` | `simulation_error` | `simulation_account_not_found_multi_candidate` | `routed_exact_sol_in` | `min_tokens_out` | `246980497717` | `197584398173` |
| `7184aed9e1503e669d35997e5b1cfd2ec314c192bd5bd5e1c437a01327453437` | `simulation_error` | `simulation_account_not_found_multi_candidate` | `routed_exact_sol_in` | `min_tokens_out` | `573371516678` | `458697213342` |
| `755a82a5dc80eb525e9a7973d74d9f9062df878a35d88bee6b428380f7f428f9` | `simulation_error` | `simulation_account_not_found_multi_candidate` | `routed_exact_sol_in` | `min_tokens_out` | `334122419606` | `267297935684` |
| `eb0b5268acaf714dd34fade1d56a5e0d5472d842787b2a29a07073925dacb5cd` | `simulation_error` | `simulation_account_not_found_multi_candidate` | `routed_exact_sol_in` | `min_tokens_out` | `209283304123` | `167426643298` |
| `c80d0fc54bd6a3ed5223225ce9b4aa5f78427e334631d7a9ec3e47f3a189b75e` | `simulation_error` | `simulation_account_not_found_multi_candidate` | `routed_exact_sol_in` | `min_tokens_out` | `280949890005` | `224759912004` |
| `5ae2f8b3158978c68dee4c914e6bbdf506beeb55993dd472f388cca6babd2894` | `simulation_error` | `simulation_account_not_found_multi_candidate` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `e3f0ca857b9c5d8e9953e9d4474617c614d2d4d61b488ea0b7729246ec01a107` | `simulation_error` | `simulation_account_not_found_multi_candidate` | `routed_exact_sol_in` | `min_tokens_out` | `243776921714` | `195021537371` |

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
