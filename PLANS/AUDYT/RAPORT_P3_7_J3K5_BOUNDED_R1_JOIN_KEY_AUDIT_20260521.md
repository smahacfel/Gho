# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k5-r1.toml`
- readiness: `not_ready`
- join_key_acceptance: `fail`
- join_quality: `pool_mint_time_window`
- probe_readiness: `ready_for_probe_transport_entry_join`
- probe_join_key_acceptance: `pass`
- probe_join_quality: `exact_probe_id_and_ab_record_id`
- probe_decision_join_acceptance: `pass`
- probe_required_exact_decision_v3_join_coverage: `1.0`
- probe_entry_materialization_status_counts: `{"entry_materialized": 8, "simulation_error": 1, "transport_only_missing_token_quantity": 1}`
- probe_entry_materialization_reason_counts: `{"entry_row_present": 8, "routed_exact_sol_in_entry_token_amount_raw_null": 1, "simulation_slippage_or_price_mismatch:custom_6002": 1}`
- full_chain_ab_record_id_coverage: `1.0`
- probe_chain_ab_record_id_coverage: `1.0`
- probe_chain_probe_id_coverage: `1.0`
- readiness_reasons: `["missing_shadow_transport_rows", "missing_shadow_entry_rows", "missing_shadow_lifecycle_rows"]`
- probe_readiness_reasons: `[]`
- decision_rows_with_ab_record_id: `14`
- shadow_transport_rows_with_ab_record_id: `0`
- shadow_entry_rows_with_ab_record_id: `0`
- shadow_lifecycle_rows_with_ab_record_id: `0`
- onchain_lifecycle_rows_with_ab_record_id: `0`
- probe_transport_rows_with_ab_record_id: `10`
- probe_entry_rows_with_ab_record_id: `9`
- probe_transport_rows_with_probe_id: `10`
- probe_entry_rows_with_probe_id: `9`

## Artifact Coverage

| artifact | rows | candidate_id | ab_record_id | probe_id | pool_id | mint | v3_payload | feature_hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `decision` | 13 | 0 | 13 | 0 | 13 | 13 | 13 | 13 |
| `decision` | 1 | 0 | 1 | 0 | 1 | 1 | 1 | 1 |
| `probe_entry` | 9 | 9 | 9 | 9 | 9 | 9 | 0 | 9 |
| `probe_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `probe_selection` | 13 | 13 | 13 | 13 | 13 | 13 | 0 | 13 |
| `probe_skip` | 3 | 3 | 3 | 3 | 3 | 3 | 0 | 3 |
| `probe_transport` | 10 | 10 | 10 | 10 | 10 | 10 | 0 | 10 |
| `shadow_entry` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_transport` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 2, "common_values": 1, "per_artifact_values": [13, 1]}`
- `probe_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `pool_id`: `{"artifacts_with_rows": 2, "common_values": 1, "per_artifact_values": [13, 1]}`
- `mint`: `{"artifacts_with_rows": 2, "common_values": 1, "per_artifact_values": [13, 1]}`

## Probe Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 3, "common_values": 9, "per_artifact_values": [9, 13, 10]}`
- `probe_id`: `{"artifacts_with_rows": 3, "common_values": 9, "per_artifact_values": [9, 13, 10]}`
- `candidate_id`: `{"artifacts_with_rows": 3, "common_values": 0, "per_artifact_values": [9, 13, 10]}`
- `pool_id`: `{"artifacts_with_rows": 3, "common_values": 9, "per_artifact_values": [9, 13, 10]}`
- `mint`: `{"artifacts_with_rows": 3, "common_values": 9, "per_artifact_values": [9, 13, 10]}`

## Probe Decision Join

- decision_join_acceptance: `pass`
- required_exact_decision_v3_join_coverage: `1.0`
- `probe_selection`: `{"exact_decision_v3_join": 13, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 13, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 13, "joined_to_decision_with_v3_payload": 13, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 1}, "policy_hash_match": 13, "policy_hash_mismatch": 0, "rows": 13, "rows_with_ab_record_id": 13, "unmatched_rows": 0}`
- `probe_transport`: `{"exact_decision_v3_join": 10, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 10, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 10, "joined_to_decision_with_v3_payload": 10, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 1}, "policy_hash_match": 10, "policy_hash_mismatch": 0, "rows": 10, "rows_with_ab_record_id": 10, "unmatched_rows": 0}`
- `probe_entry`: `{"exact_decision_v3_join": 9, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 9, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 9, "joined_to_decision_with_v3_payload": 9, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 1}, "policy_hash_match": 9, "policy_hash_mismatch": 0, "rows": 9, "rows_with_ab_record_id": 9, "unmatched_rows": 0}`
- `probe_lifecycle`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`

## Probe Entry Materialization

- transport_rows: `10`
- entry_rows: `9`
- transport_without_entry_rows: `1`
- status_counts: `{"entry_materialized": 8, "simulation_error": 1, "transport_only_missing_token_quantity": 1}`
- reason_counts: `{"entry_row_present": 8, "routed_exact_sol_in_entry_token_amount_raw_null": 1, "simulation_slippage_or_price_mismatch:custom_6002": 1}`
- buy_variant_counts: `{"legacy_buy": 9, "routed_exact_sol_in": 1}`
- token_param_role_counts: `{"min_tokens_out": 1, "token_amount": 9}`
- creator_vault_authority_status_counts: `{}`
- creator_vault_mismatch_reason_counts: `{}`
- creator_identity_source_counts: `{}`
- amount_guard_status_counts: `{"amount_guard_values_unavailable": 1}`
- simulation_error_custom_code_counts: `{"custom_6002": 1}`
- skip_reason_counts: `{"probe_rate_limit_exceeded": 3}`

| probe_id | status | reason | buy_variant | token_param_role | entry_token_amount_raw | min_tokens_out |
| --- | --- | --- | --- | --- | ---: | ---: |
| `59f4c9a61d3ab44796c7a1ad73712099c48c2ec1365728bb6b41297f4e7744ed` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `204349995546` | `163479996436` |
| `ead1006d7e210d7fd6c8644f9cf6b058bcdb8ee27ddc882ba2cc51ca8eb7569a` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `244243823179` | `195395058543` |
| `afad5e02812e605c5cc6e424cd4a674d605c9653d5c06bedde467d24e6024587` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `132147639001` | `105718111200` |
| `305959c748fe8f6b7aea447b6a6340873664a84a13c05a9a1688e1c5204d9d1c` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `294237309612` | `235389847689` |
| `66766d2a73e1e71c7256afa2fe941bc7d07485e2560b7b56e111671d1fc00c14` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `327990340928` | `262392272742` |
| `fa5da314c49a34e24343f541021e475f7768f080b9b48e804f8691ef941da565` | `simulation_error` | `simulation_slippage_or_price_mismatch:custom_6002` | `legacy_buy` | `token_amount` | `462355797064` | `369884637651` |
| `c9448c14be931dabf75a1eb1f3373880f9f1fa72fcd7aa139d68d6601fc25003` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `476995551727` | `381596441381` |
| `2d5e13121add8b7aa984c35648c4166e144e42b8ac4906a39cd9b6e9b55ec121` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `456883898188` | `365507118550` |
| `87af4f0daa7e1c00c65f875c99b5dd1a4435fa14d5709ae397da34d9fb05f56e` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `198302696618` | `158642157294` |
| `e2924034189019d110e4195b98531798fd89226eebe2dcd97a6fa66b0a482f47` | `transport_only_missing_token_quantity` | `routed_exact_sol_in_entry_token_amount_raw_null` | `routed_exact_sol_in` | `min_tokens_out` | `None` | `1` |

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
