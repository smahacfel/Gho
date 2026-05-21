# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-q6.toml`
- readiness: `ready_for_lifecycle_feature_join`
- join_key_acceptance: `degraded`
- join_quality: `mint_only`
- probe_readiness: `ready_for_probe_transport_entry_join`
- probe_join_key_acceptance: `pass`
- probe_join_quality: `exact_probe_id_and_ab_record_id`
- probe_decision_join_acceptance: `pass`
- probe_required_exact_decision_v3_join_coverage: `1.0`
- probe_entry_materialization_status_counts: `{"simulation_error": 25}`
- probe_entry_materialization_reason_counts: `{"simulation_mismatch": 25}`
- full_chain_ab_record_id_coverage: `1.0`
- probe_chain_ab_record_id_coverage: `1.0`
- probe_chain_probe_id_coverage: `1.0`
- readiness_reasons: `[]`
- probe_readiness_reasons: `[]`
- decision_rows_with_ab_record_id: `2167`
- shadow_transport_rows_with_ab_record_id: `1`
- shadow_entry_rows_with_ab_record_id: `1`
- shadow_lifecycle_rows_with_ab_record_id: `3`
- onchain_lifecycle_rows_with_ab_record_id: `0`
- probe_transport_rows_with_ab_record_id: `25`
- probe_entry_rows_with_ab_record_id: `25`
- probe_transport_rows_with_probe_id: `25`
- probe_entry_rows_with_probe_id: `25`

## Artifact Coverage

| artifact | rows | candidate_id | ab_record_id | probe_id | pool_id | mint | v3_payload | feature_hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `decision` | 1 | 0 | 1 | 0 | 1 | 1 | 1 | 1 |
| `decision` | 1857 | 0 | 1857 | 0 | 1857 | 1857 | 1857 | 1857 |
| `decision` | 309 | 0 | 309 | 0 | 309 | 309 | 309 | 309 |
| `probe_entry` | 25 | 25 | 25 | 25 | 25 | 25 | 0 | 25 |
| `probe_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `probe_selection` | 1630 | 1630 | 1630 | 1630 | 1630 | 1630 | 0 | 1630 |
| `probe_skip` | 1794 | 1794 | 1794 | 1794 | 1794 | 1794 | 0 | 1794 |
| `probe_transport` | 25 | 25 | 25 | 25 | 25 | 25 | 0 | 25 |
| `shadow_entry` | 1 | 1 | 1 | 0 | 1 | 1 | 0 | 1 |
| `shadow_lifecycle` | 3 | 3 | 3 | 0 | 3 | 3 | 0 | 3 |
| `shadow_transport` | 1 | 1 | 1 | 0 | 0 | 1 | 0 | 1 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 6, "common_values": 1, "per_artifact_values": [1, 1857, 309, 1, 1, 1]}`
- `probe_id`: `{"artifacts_with_rows": 6, "common_values": 0, "per_artifact_values": [0, 0, 0, 0, 0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 6, "common_values": 0, "per_artifact_values": [0, 0, 0, 1, 1, 1]}`
- `pool_id`: `{"artifacts_with_rows": 6, "common_values": 0, "per_artifact_values": [1, 1857, 309, 1, 1, 0]}`
- `mint`: `{"artifacts_with_rows": 6, "common_values": 1, "per_artifact_values": [1, 1857, 309, 1, 1, 1]}`

## Probe Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 3, "common_values": 25, "per_artifact_values": [25, 1630, 25]}`
- `probe_id`: `{"artifacts_with_rows": 3, "common_values": 25, "per_artifact_values": [25, 1630, 25]}`
- `candidate_id`: `{"artifacts_with_rows": 3, "common_values": 0, "per_artifact_values": [25, 1630, 25]}`
- `pool_id`: `{"artifacts_with_rows": 3, "common_values": 25, "per_artifact_values": [25, 1630, 25]}`
- `mint`: `{"artifacts_with_rows": 3, "common_values": 25, "per_artifact_values": [25, 1630, 25]}`

## Probe Decision Join

- decision_join_acceptance: `pass`
- required_exact_decision_v3_join_coverage: `1.0`
- `probe_selection`: `{"exact_decision_v3_join": 1630, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 1630, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 1630, "joined_to_decision_with_v3_payload": 1630, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 308}, "policy_hash_match": 1630, "policy_hash_mismatch": 0, "rows": 1630, "rows_with_ab_record_id": 1630, "unmatched_rows": 0}`
- `probe_transport`: `{"exact_decision_v3_join": 25, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 25, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 25, "joined_to_decision_with_v3_payload": 25, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 6}, "policy_hash_match": 25, "policy_hash_mismatch": 0, "rows": 25, "rows_with_ab_record_id": 25, "unmatched_rows": 0}`
- `probe_entry`: `{"exact_decision_v3_join": 25, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 25, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 25, "joined_to_decision_with_v3_payload": 25, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 6}, "policy_hash_match": 25, "policy_hash_mismatch": 0, "rows": 25, "rows_with_ab_record_id": 25, "unmatched_rows": 0}`
- `probe_lifecycle`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`

## Probe Entry Materialization

- transport_rows: `25`
- entry_rows: `25`
- transport_without_entry_rows: `0`
- status_counts: `{"simulation_error": 25}`
- reason_counts: `{"simulation_mismatch": 25}`
- buy_variant_counts: `{}`
- token_param_role_counts: `{}`
- skip_reason_counts: `{"active_buy_excluded": 1, "execution_account_not_ready": 206, "max_probes_per_run_exceeded": 787, "probe_concurrency_limit_exceeded": 4, "probe_execution_precheck_failed": 209, "probe_rate_limit_exceeded": 375, "verdict_type_not_in_sample_scope": 212}`

| probe_id | status | reason | buy_variant | token_param_role | entry_token_amount_raw | min_tokens_out |
| --- | --- | --- | --- | --- | ---: | ---: |
| `77272dc2c492effe360510756b86bdf80366486e52c8507ef0e4089a92cd9174` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `c83cdd9397a194eb94a33282cf0b2a2b87b306d07755eb0ee25b75cfc7eaf540` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `905771cd8e88365eefbf4b213da4cf160738f99afdfb4071328ee5c218630f62` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `412a751dfa6e360ef7fac07add9a147832df863666651caabe19af6cc083eb6d` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `cdfe6b3a3c9c9b7d3c12b3de7cb776e3b064b76614f5234731d3d3eb05b4694a` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `84ecd2dc6f9e21dd5030933ee43772893192295d2f5e083a1f25f1b010a814e8` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `e913d794a578170546b63145861734a0590896b4e93efa46899e8cefc8b5fba6` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `9cd07935d5a0360bc482488d777d3b7329c8a11a33038fa9f1709bf0c648bc12` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `3203111893207a52bc0dd15443659f316f7ab25989318a650a3ed4c73a14865b` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `623aaf4de20ef121552facb23891d746829496095e43ce5a39bd3581c7660c00` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `c6a678394ff691321fda2bd13993abdef2a1c6e38d5bcb6ac07b7afadf76c4ab` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `d2e3d2d52c7fdc3b8a67a0a0c5c4207a8b901573457ca86aef4c073f484cca02` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `546067cfbecd0d81117457a74c7c52f5891f36de8fdda75a22050fbbc9d1a18f` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `d9f861eda0cd58d66b6b67e93fa58f6d02fbe2780691f2b7237e1f7e977f6a96` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `25cabc962ce58832210ac63ff4bd5c36974e87ac224fc4635a6ffa4fdd2fbb8f` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `3009d76dea7fa91757ae47d0b9c1a2d52ef6f60529fef07ebcc6a473e4f1c2ba` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `a4992936b59903c8764dc539a5e2c4c90657605f2e40499013f04f8a81da2890` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `35f71e9bc437b9c3937c75c40e71385f8c2ccf5bd150861d5ca51bec3f8617e0` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `0b1687878749bbdce840be3c2f59ffca5fac45477bfa87cc5224c2c67bfe5042` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `fb58a9fe1f43ae6e3b30102cf97827ff5694787edaaf84d7acaabc678cfde663` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `c713ec21dbf204f9e8da26b753ef312e456c4ed04309b9869d9822fa454ef088` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `a9ade395cac8923d8e9f885440e692d8896a61eea49e86d9572975b2a7d1c18e` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `938ad8e9846aafa3aadd19f2711c7ddf5faa71ffd29b18605bba706ab584b645` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `811dffc41a24f1a6cf7fd97f1e1b9394d5710be437cce02c37b7431322f8f9a6` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |
| `f57af24b5335425f6cdc264ad17fa29dcfc4fe230e0392bfa5d44b4ad88d6b6e` | `simulation_error` | `simulation_mismatch` | `None` | `None` | `None` | `None` |

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
