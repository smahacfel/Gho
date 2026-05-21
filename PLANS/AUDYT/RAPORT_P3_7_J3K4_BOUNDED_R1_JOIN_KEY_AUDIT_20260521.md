# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k4-r1.toml`
- readiness: `not_ready`
- join_key_acceptance: `fail`
- join_quality: `pool_mint_time_window`
- probe_readiness: `ready_for_probe_transport_entry_join`
- probe_join_key_acceptance: `pass`
- probe_join_quality: `exact_probe_id_and_ab_record_id`
- probe_decision_join_acceptance: `pass`
- probe_required_exact_decision_v3_join_coverage: `1.0`
- probe_entry_materialization_status_counts: `{"entry_materialized": 8, "simulation_error": 2}`
- probe_entry_materialization_reason_counts: `{"entry_row_present": 8, "simulation_account_layout_mismatch:custom_2006": 1, "simulation_slippage_or_price_mismatch:custom_6002": 1}`
- full_chain_ab_record_id_coverage: `1.0`
- probe_chain_ab_record_id_coverage: `1.0`
- probe_chain_probe_id_coverage: `1.0`
- readiness_reasons: `["missing_shadow_transport_rows", "missing_shadow_entry_rows", "missing_shadow_lifecycle_rows"]`
- probe_readiness_reasons: `[]`
- decision_rows_with_ab_record_id: `13`
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
| `decision` | 12 | 0 | 12 | 0 | 12 | 12 | 12 | 12 |
| `decision` | 1 | 0 | 1 | 0 | 1 | 1 | 1 | 1 |
| `probe_entry` | 10 | 10 | 10 | 10 | 10 | 10 | 0 | 10 |
| `probe_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `probe_selection` | 12 | 12 | 12 | 12 | 12 | 12 | 0 | 12 |
| `probe_skip` | 2 | 2 | 2 | 2 | 2 | 2 | 0 | 2 |
| `probe_transport` | 10 | 10 | 10 | 10 | 10 | 10 | 0 | 10 |
| `shadow_entry` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_transport` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 2, "common_values": 1, "per_artifact_values": [12, 1]}`
- `probe_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `pool_id`: `{"artifacts_with_rows": 2, "common_values": 1, "per_artifact_values": [12, 1]}`
- `mint`: `{"artifacts_with_rows": 2, "common_values": 1, "per_artifact_values": [12, 1]}`

## Probe Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 3, "common_values": 10, "per_artifact_values": [10, 12, 10]}`
- `probe_id`: `{"artifacts_with_rows": 3, "common_values": 10, "per_artifact_values": [10, 12, 10]}`
- `candidate_id`: `{"artifacts_with_rows": 3, "common_values": 0, "per_artifact_values": [10, 12, 10]}`
- `pool_id`: `{"artifacts_with_rows": 3, "common_values": 10, "per_artifact_values": [10, 12, 10]}`
- `mint`: `{"artifacts_with_rows": 3, "common_values": 10, "per_artifact_values": [10, 12, 10]}`

## Probe Decision Join

- decision_join_acceptance: `pass`
- required_exact_decision_v3_join_coverage: `1.0`
- `probe_selection`: `{"exact_decision_v3_join": 12, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 12, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 12, "joined_to_decision_with_v3_payload": 12, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 1}, "policy_hash_match": 12, "policy_hash_mismatch": 0, "rows": 12, "rows_with_ab_record_id": 12, "unmatched_rows": 0}`
- `probe_transport`: `{"exact_decision_v3_join": 10, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 10, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 10, "joined_to_decision_with_v3_payload": 10, "mismatch_reasons": {}, "policy_hash_match": 10, "policy_hash_mismatch": 0, "rows": 10, "rows_with_ab_record_id": 10, "unmatched_rows": 0}`
- `probe_entry`: `{"exact_decision_v3_join": 10, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 10, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 10, "joined_to_decision_with_v3_payload": 10, "mismatch_reasons": {}, "policy_hash_match": 10, "policy_hash_mismatch": 0, "rows": 10, "rows_with_ab_record_id": 10, "unmatched_rows": 0}`
- `probe_lifecycle`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`

## Probe Entry Materialization

- transport_rows: `10`
- entry_rows: `10`
- transport_without_entry_rows: `0`
- status_counts: `{"entry_materialized": 8, "simulation_error": 2}`
- reason_counts: `{"entry_row_present": 8, "simulation_account_layout_mismatch:custom_2006": 1, "simulation_slippage_or_price_mismatch:custom_6002": 1}`
- buy_variant_counts: `{"legacy_buy": 10}`
- token_param_role_counts: `{"token_amount": 10}`
- skip_reason_counts: `{"execution_account_not_ready": 1, "probe_concurrency_limit_exceeded": 1}`

| probe_id | status | reason | buy_variant | token_param_role | entry_token_amount_raw | min_tokens_out |
| --- | --- | --- | --- | --- | ---: | ---: |
| `3df75ea2065db8f7bb7f008bb226926a48ef9faf33914e1fddffaf6a8547c1b1` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `243357438542` | `194685950833` |
| `f6dcb2954648917cfdffa3f4541c1143317eda8a13c7649c0a7a17fa990a48c5` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `172525691750` | `138020553400` |
| `094eef08e495a9ad482eb049db1013b970a6ae93fb3a0e8414a09357a13c4dfb` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `243986460159` | `195189168127` |
| `35e3d843c581ebaf2e746254286160ebd821d28e11f20cee5fbbfbeb2f044b70` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `576473251912` | `461178601529` |
| `36bb623ccb42b4775c8919f4c47714e5e790f62062c23ea485b861b2d1175a92` | `simulation_error` | `simulation_slippage_or_price_mismatch:custom_6002` | `legacy_buy` | `token_amount` | `124803725982` | `99842980785` |
| `f2135132389f164c4d77b0d72ffa3b484dd189102c6b394702d86c102e7ae141` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `488517273591` | `390813818872` |
| `253b24c16093c01d1dfb2a941934bbe9a46b893b77878bae4ac90d67f6cc8f71` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `81347418527` | `65077934821` |
| `9a5ff91aa7de1d2b83fe96d4a0afaac046aa25688c758b0d6108f552e4bbf130` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `857359634961` | `685887707968` |
| `232c9a168b63333e3203b0a1e71f3b8d169e2190e7b87532521802d075b50783` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `234449566349` | `187559653079` |
| `29b72dd8892cb8d6cf38bb5e4c4fb5aba3190c0d79e5238faa5ee9add10b2cec` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `legacy_buy` | `token_amount` | `409726855955` | `327781484764` |

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
