# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k6-r1.toml`
- readiness: `not_ready`
- join_key_acceptance: `fail`
- join_quality: `pool_mint_time_window`
- probe_readiness: `not_ready`
- probe_join_key_acceptance: `fail`
- probe_join_quality: `exact_probe_id_and_ab_record_id`
- probe_decision_join_acceptance: `fail`
- probe_required_exact_decision_v3_join_coverage: `1.0`
- probe_entry_materialization_status_counts: `{"transport_only_missing_token_quantity": 10}`
- probe_entry_materialization_reason_counts: `{"routed_exact_sol_in_entry_token_amount_raw_null": 10}`
- full_chain_ab_record_id_coverage: `1.0`
- probe_chain_ab_record_id_coverage: `1.0`
- probe_chain_probe_id_coverage: `1.0`
- readiness_reasons: `["missing_shadow_transport_rows", "missing_shadow_entry_rows", "missing_shadow_lifecycle_rows"]`
- probe_readiness_reasons: `["missing_probe_entry_rows", "probe_rows_missing_exact_decision_v3_join"]`
- decision_rows_with_ab_record_id: `751`
- shadow_transport_rows_with_ab_record_id: `0`
- shadow_entry_rows_with_ab_record_id: `0`
- shadow_lifecycle_rows_with_ab_record_id: `0`
- onchain_lifecycle_rows_with_ab_record_id: `0`
- probe_transport_rows_with_ab_record_id: `10`
- probe_entry_rows_with_ab_record_id: `0`
- probe_transport_rows_with_probe_id: `10`
- probe_entry_rows_with_probe_id: `0`

## Artifact Coverage

| artifact | rows | candidate_id | ab_record_id | probe_id | pool_id | mint | v3_payload | feature_hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `decision` | 624 | 0 | 624 | 0 | 624 | 624 | 624 | 624 |
| `decision` | 127 | 0 | 127 | 0 | 127 | 127 | 127 | 127 |
| `probe_entry` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `probe_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `probe_selection` | 18 | 18 | 18 | 18 | 18 | 18 | 0 | 18 |
| `probe_skip` | 612 | 612 | 612 | 612 | 612 | 612 | 0 | 612 |
| `probe_transport` | 10 | 10 | 10 | 10 | 10 | 10 | 0 | 10 |
| `shadow_entry` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_transport` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 2, "common_values": 127, "per_artifact_values": [624, 127]}`
- `probe_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `pool_id`: `{"artifacts_with_rows": 2, "common_values": 127, "per_artifact_values": [624, 127]}`
- `mint`: `{"artifacts_with_rows": 2, "common_values": 127, "per_artifact_values": [624, 127]}`

## Probe Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 2, "common_values": 10, "per_artifact_values": [18, 10]}`
- `probe_id`: `{"artifacts_with_rows": 2, "common_values": 10, "per_artifact_values": [18, 10]}`
- `candidate_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [18, 10]}`
- `pool_id`: `{"artifacts_with_rows": 2, "common_values": 10, "per_artifact_values": [18, 10]}`
- `mint`: `{"artifacts_with_rows": 2, "common_values": 10, "per_artifact_values": [18, 10]}`

## Probe Decision Join

- decision_join_acceptance: `fail`
- required_exact_decision_v3_join_coverage: `1.0`
- `probe_selection`: `{"exact_decision_v3_join": 18, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 18, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 18, "joined_to_decision_with_v3_payload": 18, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 1}, "policy_hash_match": 18, "policy_hash_mismatch": 0, "rows": 18, "rows_with_ab_record_id": 18, "unmatched_rows": 0}`
- `probe_transport`: `{"exact_decision_v3_join": 10, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 10, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 10, "joined_to_decision_with_v3_payload": 10, "mismatch_reasons": {}, "policy_hash_match": 10, "policy_hash_mismatch": 0, "rows": 10, "rows_with_ab_record_id": 10, "unmatched_rows": 0}`
- `probe_entry`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`
- `probe_lifecycle`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`

## Probe Entry Materialization

- transport_rows: `10`
- entry_rows: `0`
- transport_without_entry_rows: `10`
- status_counts: `{"transport_only_missing_token_quantity": 10}`
- reason_counts: `{"routed_exact_sol_in_entry_token_amount_raw_null": 10}`
- buy_variant_counts: `{"routed_exact_sol_in": 10}`
- token_param_role_counts: `{"min_tokens_out": 10}`
- creator_vault_authority_status_counts: `{}`
- creator_vault_mismatch_reason_counts: `{}`
- creator_identity_source_counts: `{}`
- amount_guard_status_counts: `{}`
- simulation_error_custom_code_counts: `{}`
- skip_reason_counts: `{"creator_vault_source_not_authoritative": 545, "execution_account_not_ready": 2, "max_probes_per_run_exceeded": 3, "probe_execution_precheck_failed": 26, "verdict_type_not_in_sample_scope": 36}`
- skip_creator_vault_authority_status_counts: `{"creator_vault_source_not_authoritative": 545}`
- skip_creator_vault_mismatch_reason_counts: `{"creator_identity_source_not_authoritative": 545}`
- skip_creator_identity_source_counts: `{"detected_pool.creator": 545}`

| probe_id | status | reason | buy_variant | token_param_role | entry_token_amount_raw | min_tokens_out |
| --- | --- | --- | --- | --- | ---: | ---: |
| `ef60a12e0bdc916b1abea3fb0c6ab4c258243d9959b4f3a8e6339d3482fb9259` | `transport_only_missing_token_quantity` | `routed_exact_sol_in_entry_token_amount_raw_null` | `routed_exact_sol_in` | `min_tokens_out` | `None` | `1` |
| `46b0f6e054deb9785ccaed0b13accabbb49539f932b1417adeffb94e802a64ed` | `transport_only_missing_token_quantity` | `routed_exact_sol_in_entry_token_amount_raw_null` | `routed_exact_sol_in` | `min_tokens_out` | `None` | `1` |
| `4385455b31f42754d15ed5c0234439aed77bce46b1af59f5bb821df3cdac964c` | `transport_only_missing_token_quantity` | `routed_exact_sol_in_entry_token_amount_raw_null` | `routed_exact_sol_in` | `min_tokens_out` | `None` | `1` |
| `d5b9493d57d35412d99b2dc50d362feff6a4da7f840ad640781cf5e1ca98c0e6` | `transport_only_missing_token_quantity` | `routed_exact_sol_in_entry_token_amount_raw_null` | `routed_exact_sol_in` | `min_tokens_out` | `None` | `1` |
| `78a200cf85c076a283bd1db6cf8e4bd355c2dfc852db89a51a19bf866d99c416` | `transport_only_missing_token_quantity` | `routed_exact_sol_in_entry_token_amount_raw_null` | `routed_exact_sol_in` | `min_tokens_out` | `None` | `1` |
| `ebba2fd71030614feb99afffa52bc78cf4b999b3de381017d89f2843c87a3135` | `transport_only_missing_token_quantity` | `routed_exact_sol_in_entry_token_amount_raw_null` | `routed_exact_sol_in` | `min_tokens_out` | `None` | `1` |
| `8d13293df026c0fc35fbdc7090169d1376d4937cfc00899afb16248da5f11f12` | `transport_only_missing_token_quantity` | `routed_exact_sol_in_entry_token_amount_raw_null` | `routed_exact_sol_in` | `min_tokens_out` | `None` | `1` |
| `e6b7248e4da3fdc396d382cf4adc1b2de6f254329ac129db2abb2c0a728b6f90` | `transport_only_missing_token_quantity` | `routed_exact_sol_in_entry_token_amount_raw_null` | `routed_exact_sol_in` | `min_tokens_out` | `None` | `1` |
| `ee794388b331b924ff5f99f1a57343e1505c1a458f2eae26bc549eb77b868ab9` | `transport_only_missing_token_quantity` | `routed_exact_sol_in_entry_token_amount_raw_null` | `routed_exact_sol_in` | `min_tokens_out` | `None` | `1` |
| `463bdb0f026ce3e4d14297ff315fd90ccaa39559477da1adabf7279e4ea8e084` | `transport_only_missing_token_quantity` | `routed_exact_sol_in_entry_token_amount_raw_null` | `routed_exact_sol_in` | `min_tokens_out` | `None` | `1` |

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
