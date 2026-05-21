# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k5-r2.toml`
- readiness: `not_ready`
- join_key_acceptance: `fail`
- join_quality: `pool_mint_time_window`
- probe_readiness: `ready_for_probe_transport_entry_join`
- probe_join_key_acceptance: `pass`
- probe_join_quality: `exact_probe_id_and_ab_record_id`
- probe_decision_join_acceptance: `pass`
- probe_required_exact_decision_v3_join_coverage: `1.0`
- probe_entry_materialization_status_counts: `{"entry_materialized": 7, "simulation_error": 2, "transport_only_missing_token_quantity": 1}`
- probe_entry_materialization_reason_counts: `{"entry_row_present": 7, "routed_exact_sol_in_entry_token_amount_raw_null": 1, "simulation_account_layout_mismatch:custom_2006": 2}`
- full_chain_ab_record_id_coverage: `1.0`
- probe_chain_ab_record_id_coverage: `1.0`
- probe_chain_probe_id_coverage: `1.0`
- readiness_reasons: `["missing_shadow_transport_rows", "missing_shadow_entry_rows", "missing_shadow_lifecycle_rows"]`
- probe_readiness_reasons: `[]`
- decision_rows_with_ab_record_id: `23`
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
| `decision` | 19 | 0 | 19 | 0 | 19 | 19 | 19 | 19 |
| `decision` | 4 | 0 | 4 | 0 | 4 | 4 | 4 | 4 |
| `probe_entry` | 9 | 9 | 9 | 9 | 9 | 9 | 0 | 9 |
| `probe_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `probe_selection` | 19 | 19 | 19 | 19 | 19 | 19 | 0 | 19 |
| `probe_skip` | 7 | 7 | 7 | 7 | 7 | 7 | 0 | 7 |
| `probe_transport` | 10 | 10 | 10 | 10 | 10 | 10 | 0 | 10 |
| `shadow_entry` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_transport` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 2, "common_values": 4, "per_artifact_values": [19, 4]}`
- `probe_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `pool_id`: `{"artifacts_with_rows": 2, "common_values": 4, "per_artifact_values": [19, 4]}`
- `mint`: `{"artifacts_with_rows": 2, "common_values": 4, "per_artifact_values": [19, 4]}`

## Probe Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 3, "common_values": 9, "per_artifact_values": [9, 19, 10]}`
- `probe_id`: `{"artifacts_with_rows": 3, "common_values": 9, "per_artifact_values": [9, 19, 10]}`
- `candidate_id`: `{"artifacts_with_rows": 3, "common_values": 0, "per_artifact_values": [9, 19, 10]}`
- `pool_id`: `{"artifacts_with_rows": 3, "common_values": 9, "per_artifact_values": [9, 19, 10]}`
- `mint`: `{"artifacts_with_rows": 3, "common_values": 9, "per_artifact_values": [9, 19, 10]}`

## Probe Decision Join

- decision_join_acceptance: `pass`
- required_exact_decision_v3_join_coverage: `1.0`
- `probe_selection`: `{"exact_decision_v3_join": 19, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 19, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 19, "joined_to_decision_with_v3_payload": 19, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 4}, "policy_hash_match": 19, "policy_hash_mismatch": 0, "rows": 19, "rows_with_ab_record_id": 19, "unmatched_rows": 0}`
- `probe_transport`: `{"exact_decision_v3_join": 10, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 10, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 10, "joined_to_decision_with_v3_payload": 10, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 3}, "policy_hash_match": 10, "policy_hash_mismatch": 0, "rows": 10, "rows_with_ab_record_id": 10, "unmatched_rows": 0}`
- `probe_entry`: `{"exact_decision_v3_join": 9, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 9, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 9, "joined_to_decision_with_v3_payload": 9, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 3}, "policy_hash_match": 9, "policy_hash_mismatch": 0, "rows": 9, "rows_with_ab_record_id": 9, "unmatched_rows": 0}`
- `probe_lifecycle`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`

## Probe Entry Materialization

- transport_rows: `10`
- entry_rows: `9`
- transport_without_entry_rows: `1`
- status_counts: `{"entry_materialized": 7, "simulation_error": 2, "transport_only_missing_token_quantity": 1}`
- reason_counts: `{"entry_row_present": 7, "routed_exact_sol_in_entry_token_amount_raw_null": 1, "simulation_account_layout_mismatch:custom_2006": 2}`
- buy_variant_counts: `{"legacy_buy": 9, "routed_exact_sol_in": 1}`
- token_param_role_counts: `{"min_tokens_out": 1, "token_amount": 9}`
- creator_vault_authority_status_counts: `{"creator_vault_source_not_authoritative": 2}`
- creator_vault_mismatch_reason_counts: `{"actual_expected_mismatch": 2}`
- creator_identity_source_counts: `{"account_overrides.creator_pubkey": 2}`
- amount_guard_status_counts: `{}`
- simulation_error_custom_code_counts: `{"custom_2006": 2}`
- skip_reason_counts: `{"execution_account_not_ready": 2, "probe_concurrency_limit_exceeded": 2, "probe_execution_precheck_failed": 2, "probe_rate_limit_exceeded": 1}`

| probe_id | status | reason | buy_variant | token_param_role | entry_token_amount_raw | min_tokens_out |
| --- | --- | --- | --- | --- | ---: | ---: |
| `4f87cf9531dd1ec463d205cc70aa489b8d03549829f6bd560f51952acda7b8a9` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `legacy_buy` | `token_amount` | `247805756763` | `198244605410` |
| `f1c51c367501d6e3fc2744a923fc8aa31ea68172cf7e15742cb900f03bd6247d` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `99125137135` | `79300109708` |
| `16b95e3d79328eff48432e0d6c0456226fb159287d96b264791b11c5e008faec` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `23744924284` | `18995939427` |
| `51bad8a2ed81f3713cced64f37814ec24f1be905b3012932c5fe5b96d7524c5d` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `115834744063` | `92667795250` |
| `c542d42a1ccee95758f9df1a9077ca79aac3b62f420d04642b0771fa509deeec` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `247805756862` | `198244605489` |
| `cc0d01d341bf38c780c5f2c677e602d0496d8e9c9ae1968ceee85e06ab20e795` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `887117205071` | `709693764056` |
| `4caf3a08e620978c161a089fbb52abf270bab8ec81bb6400be7f3c60bb633f30` | `transport_only_missing_token_quantity` | `routed_exact_sol_in_entry_token_amount_raw_null` | `routed_exact_sol_in` | `min_tokens_out` | `None` | `1` |
| `7d69563dcdbfb10ef542e287a8b4f2b8bacecb85f57d607cb2eae94e8b92d41b` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `247642691968` | `198114153574` |
| `25326d3cb521e58709425fe3789468a25db589ed93b897cc675f249231760809` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `245112855487` | `196090284389` |
| `cfdc28467c720fcf979f0685726a1654aa24821a443b701346ef9435a5e2b6c7` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `legacy_buy` | `token_amount` | `125445605403` | `100356484322` |

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
