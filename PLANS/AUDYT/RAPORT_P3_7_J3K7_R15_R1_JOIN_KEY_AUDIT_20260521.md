# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k7-r1.toml`
- readiness: `not_ready`
- join_key_acceptance: `fail`
- join_quality: `pool_mint_time_window`
- probe_readiness: `ready_for_probe_transport_entry_join`
- probe_join_key_acceptance: `pass`
- probe_join_quality: `exact_probe_id_and_ab_record_id`
- probe_decision_join_acceptance: `pass`
- probe_required_exact_decision_v3_join_coverage: `1.0`
- probe_entry_materialization_status_counts: `{"entry_materialized": 10}`
- probe_entry_materialization_reason_counts: `{"entry_row_present": 10}`
- full_chain_ab_record_id_coverage: `1.0`
- probe_chain_ab_record_id_coverage: `1.0`
- probe_chain_probe_id_coverage: `1.0`
- readiness_reasons: `["missing_shadow_transport_rows", "missing_shadow_entry_rows", "missing_shadow_lifecycle_rows"]`
- probe_readiness_reasons: `[]`
- decision_rows_with_ab_record_id: `582`
- shadow_transport_rows_with_ab_record_id: `0`
- shadow_entry_rows_with_ab_record_id: `0`
- shadow_lifecycle_rows_with_ab_record_id: `0`
- onchain_lifecycle_rows_with_ab_record_id: `0`
- probe_transport_rows_with_ab_record_id: `10`
- probe_entry_rows_with_ab_record_id: `10`
- probe_transport_rows_with_probe_id: `10`
- probe_entry_rows_with_probe_id: `10`

## Artifact Coverage

| artifact | rows | candidate_id | ab_record_id | probe_id | pool_id | mint | v3_payload | feature_hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `decision` | 508 | 0 | 508 | 0 | 508 | 508 | 508 | 508 |
| `decision` | 74 | 0 | 74 | 0 | 74 | 74 | 74 | 74 |
| `probe_entry` | 10 | 10 | 10 | 10 | 10 | 10 | 0 | 10 |
| `probe_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `probe_selection` | 43 | 43 | 43 | 43 | 43 | 43 | 0 | 43 |
| `probe_skip` | 495 | 495 | 495 | 495 | 495 | 495 | 0 | 495 |
| `probe_transport` | 10 | 10 | 10 | 10 | 10 | 10 | 0 | 10 |
| `shadow_entry` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_transport` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 2, "common_values": 74, "per_artifact_values": [508, 74]}`
- `probe_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `pool_id`: `{"artifacts_with_rows": 2, "common_values": 74, "per_artifact_values": [508, 74]}`
- `mint`: `{"artifacts_with_rows": 2, "common_values": 74, "per_artifact_values": [508, 74]}`

## Probe Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 3, "common_values": 10, "per_artifact_values": [10, 43, 10]}`
- `probe_id`: `{"artifacts_with_rows": 3, "common_values": 10, "per_artifact_values": [10, 43, 10]}`
- `candidate_id`: `{"artifacts_with_rows": 3, "common_values": 0, "per_artifact_values": [10, 43, 10]}`
- `pool_id`: `{"artifacts_with_rows": 3, "common_values": 10, "per_artifact_values": [10, 43, 10]}`
- `mint`: `{"artifacts_with_rows": 3, "common_values": 10, "per_artifact_values": [10, 43, 10]}`

## Probe Decision Join

- decision_join_acceptance: `pass`
- required_exact_decision_v3_join_coverage: `1.0`
- `probe_selection`: `{"exact_decision_v3_join": 43, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 43, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 43, "joined_to_decision_with_v3_payload": 43, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 2}, "policy_hash_match": 43, "policy_hash_mismatch": 0, "rows": 43, "rows_with_ab_record_id": 43, "unmatched_rows": 0}`
- `probe_transport`: `{"exact_decision_v3_join": 10, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 10, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 10, "joined_to_decision_with_v3_payload": 10, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 1}, "policy_hash_match": 10, "policy_hash_mismatch": 0, "rows": 10, "rows_with_ab_record_id": 10, "unmatched_rows": 0}`
- `probe_entry`: `{"exact_decision_v3_join": 10, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 10, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 10, "joined_to_decision_with_v3_payload": 10, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 1}, "policy_hash_match": 10, "policy_hash_mismatch": 0, "rows": 10, "rows_with_ab_record_id": 10, "unmatched_rows": 0}`
- `probe_lifecycle`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`

## Probe Entry Materialization

- transport_rows: `10`
- entry_rows: `10`
- transport_without_entry_rows: `0`
- status_counts: `{"entry_materialized": 10}`
- reason_counts: `{"entry_row_present": 10}`
- buy_variant_counts: `{"routed_exact_sol_in": 10}`
- token_param_role_counts: `{"min_tokens_out": 10}`
- creator_vault_authority_status_counts: `{}`
- creator_vault_mismatch_reason_counts: `{}`
- creator_identity_source_counts: `{}`
- amount_guard_status_counts: `{}`
- simulation_error_custom_code_counts: `{}`
- skip_reason_counts: `{"creator_vault_source_not_authoritative": 287, "execution_account_not_ready": 8, "max_probes_per_run_exceeded": 20, "probe_execution_precheck_failed": 12, "probe_rate_limit_exceeded": 2, "verdict_type_not_in_sample_scope": 166}`
- skip_creator_vault_authority_status_counts: `{"creator_vault_source_not_authoritative": 287}`
- skip_creator_vault_mismatch_reason_counts: `{"creator_identity_source_not_authoritative": 287}`
- skip_creator_identity_source_counts: `{"detected_pool.creator": 287}`

| probe_id | status | reason | buy_variant | token_param_role | entry_token_amount_raw | min_tokens_out |
| --- | --- | --- | --- | --- | ---: | ---: |
| `af5c449c0723d64d8bad85aae2130c7b99b02dbe9a115db6e9358ee44118ee9b` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756813` | `198244605450` |
| `0bb8e458ce3d38186ba90e28ac594bdfdc230253d98e4955c92f8a5a2f24bc76` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `246905340415` | `197524272332` |
| `0fccd92fb5749fbcf74b9a9c7c0b9df0ed90beb81720a35a9cd07e4de837ff76` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `217334808042` | `173867846433` |
| `7e1341e137e38cddc2a4531a5c8cd80df25da15dc08b70286363a21622f0e007` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247797599811` | `198238079848` |
| `846f6aff4bed8f3ce5b5bf5cf22743bfcc466c3e4ccddbe0179e1fc7e9cc23f4` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `217099358837` | `173679487069` |
| `08c88f0da198acb66bcdeae4160fcd005f2a867e5ce082f91be6cfdf02a3835a` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `214688123133` | `171750498506` |
| `a59c2a5b963d43f37ab753d07858caa452d38de12420d2bae8bccdd36a60d7e2` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `232114850055` | `185691880044` |
| `3fd3d669b6b18ef68239b22d314d176fc22ea535c5d2908c4194274c70d1065f` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `205262886387` | `164210309109` |
| `2c2f949241766becfb7b86cb2fdd6579b9bff46ab79ec8d9ba1b0bfa83875677` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `246940403183` | `197552322546` |
| `06f8627000c9181c0ae5761316ed5c1ac39fc5a0b28534d69193eb4ae3990270` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `245038627854` | `196030902283` |

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
