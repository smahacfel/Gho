# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r9-q5c.toml`
- readiness: `not_ready`
- join_key_acceptance: `fail`
- join_quality: `pool_mint_time_window`
- probe_readiness: `ready_for_probe_transport_entry_join`
- probe_join_key_acceptance: `pass`
- probe_join_quality: `exact_probe_id_and_ab_record_id`
- probe_decision_join_acceptance: `pass`
- probe_required_exact_decision_v3_join_coverage: `1.0`
- probe_entry_materialization_status_counts: `{"entry_materialized": 4, "transport_only_missing_token_quantity": 1}`
- probe_entry_materialization_reason_counts: `{"entry_row_present": 4, "routed_exact_sol_in_entry_token_amount_raw_null": 1}`
- full_chain_ab_record_id_coverage: `1.0`
- probe_chain_ab_record_id_coverage: `1.0`
- probe_chain_probe_id_coverage: `1.0`
- readiness_reasons: `["missing_shadow_transport_rows", "missing_shadow_entry_rows", "missing_shadow_lifecycle_rows"]`
- probe_readiness_reasons: `[]`
- decision_rows_with_ab_record_id: `17`
- shadow_transport_rows_with_ab_record_id: `0`
- shadow_entry_rows_with_ab_record_id: `0`
- shadow_lifecycle_rows_with_ab_record_id: `0`
- onchain_lifecycle_rows_with_ab_record_id: `0`
- probe_transport_rows_with_ab_record_id: `5`
- probe_entry_rows_with_ab_record_id: `4`
- probe_transport_rows_with_probe_id: `5`
- probe_entry_rows_with_probe_id: `4`

## Artifact Coverage

| artifact | rows | candidate_id | ab_record_id | probe_id | pool_id | mint | v3_payload | feature_hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `decision` | 16 | 0 | 16 | 0 | 16 | 16 | 16 | 16 |
| `decision` | 1 | 0 | 1 | 0 | 1 | 1 | 1 | 1 |
| `probe_entry` | 4 | 4 | 4 | 4 | 4 | 4 | 0 | 4 |
| `probe_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `probe_selection` | 16 | 16 | 16 | 16 | 16 | 16 | 0 | 16 |
| `probe_skip` | 10 | 10 | 10 | 10 | 10 | 10 | 0 | 10 |
| `probe_transport` | 5 | 5 | 5 | 5 | 5 | 5 | 0 | 5 |
| `shadow_entry` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_transport` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 2, "common_values": 1, "per_artifact_values": [16, 1]}`
- `probe_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `pool_id`: `{"artifacts_with_rows": 2, "common_values": 1, "per_artifact_values": [16, 1]}`
- `mint`: `{"artifacts_with_rows": 2, "common_values": 1, "per_artifact_values": [16, 1]}`

## Probe Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 3, "common_values": 4, "per_artifact_values": [4, 16, 5]}`
- `probe_id`: `{"artifacts_with_rows": 3, "common_values": 4, "per_artifact_values": [4, 16, 5]}`
- `candidate_id`: `{"artifacts_with_rows": 3, "common_values": 0, "per_artifact_values": [4, 16, 5]}`
- `pool_id`: `{"artifacts_with_rows": 3, "common_values": 4, "per_artifact_values": [4, 16, 5]}`
- `mint`: `{"artifacts_with_rows": 3, "common_values": 4, "per_artifact_values": [4, 16, 5]}`

## Probe Decision Join

- decision_join_acceptance: `pass`
- required_exact_decision_v3_join_coverage: `1.0`
- `probe_selection`: `{"exact_decision_v3_join": 16, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 16, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 16, "joined_to_decision_with_v3_payload": 16, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 1}, "policy_hash_match": 16, "policy_hash_mismatch": 0, "rows": 16, "rows_with_ab_record_id": 16, "unmatched_rows": 0}`
- `probe_transport`: `{"exact_decision_v3_join": 5, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 5, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 5, "joined_to_decision_with_v3_payload": 5, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 1}, "policy_hash_match": 5, "policy_hash_mismatch": 0, "rows": 5, "rows_with_ab_record_id": 5, "unmatched_rows": 0}`
- `probe_entry`: `{"exact_decision_v3_join": 4, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 4, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 4, "joined_to_decision_with_v3_payload": 4, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 1}, "policy_hash_match": 4, "policy_hash_mismatch": 0, "rows": 4, "rows_with_ab_record_id": 4, "unmatched_rows": 0}`
- `probe_lifecycle`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`

## Probe Entry Materialization

- transport_rows: `5`
- entry_rows: `4`
- transport_without_entry_rows: `1`
- status_counts: `{"entry_materialized": 4, "transport_only_missing_token_quantity": 1}`
- reason_counts: `{"entry_row_present": 4, "routed_exact_sol_in_entry_token_amount_raw_null": 1}`
- buy_variant_counts: `{"legacy_buy": 4, "routed_exact_sol_in": 1}`
- token_param_role_counts: `{"min_tokens_out": 1, "token_amount": 4}`
- skip_reason_counts: `{"execution_account_not_ready": 1, "probe_rate_limit_exceeded": 9}`

| probe_id | status | reason | buy_variant | token_param_role | entry_token_amount_raw | min_tokens_out |
| --- | --- | --- | --- | --- | ---: | ---: |
| `4db137602108df3f8d3384d628f8c3fa0c42aedc1da3d829207ec0c62716a6a4` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `231417155142` | `185133724113` |
| `be2a849b1948506ea48f35c60e80733e98a1f9168b5d4051b49803c601bd2023` | `transport_only_missing_token_quantity` | `routed_exact_sol_in_entry_token_amount_raw_null` | `routed_exact_sol_in` | `min_tokens_out` | `None` | `1` |
| `a0d936d5f1b0b44f6151b32b572588b7030e7021e6fca07ca2c610c91933423c` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `206090264378` | `164872211502` |
| `e32bcb31a45bddd18d13ff2808fafb2698759248776034dee61a46ab057343ee` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `278690623255` | `222952498604` |
| `86d92583f5d459c9647c656e448ae596e3b0654600d2cf9a38a44f081205dad0` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `245376581822` | `196301265457` |

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
