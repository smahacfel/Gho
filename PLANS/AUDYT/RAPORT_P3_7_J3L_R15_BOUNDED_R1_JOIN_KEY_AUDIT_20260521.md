# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3l-r1.toml`
- readiness: `not_ready`
- join_key_acceptance: `fail`
- join_quality: `pool_mint_time_window`
- probe_readiness: `ready_for_probe_transport_entry_join`
- probe_join_key_acceptance: `pass`
- probe_join_quality: `exact_probe_id_and_ab_record_id`
- probe_decision_join_acceptance: `pass`
- probe_required_exact_decision_v3_join_coverage: `1.0`
- probe_entry_materialization_status_counts: `{"entry_materialized": 23, "simulation_error": 2}`
- probe_entry_materialization_reason_counts: `{"entry_row_present": 23, "simulation_account_layout_mismatch:custom_2006": 2}`
- full_chain_ab_record_id_coverage: `1.0`
- probe_chain_ab_record_id_coverage: `1.0`
- probe_chain_probe_id_coverage: `1.0`
- readiness_reasons: `["missing_shadow_transport_rows", "missing_shadow_entry_rows", "missing_shadow_lifecycle_rows"]`
- probe_readiness_reasons: `[]`
- decision_rows_with_ab_record_id: `3272`
- shadow_transport_rows_with_ab_record_id: `0`
- shadow_entry_rows_with_ab_record_id: `0`
- shadow_lifecycle_rows_with_ab_record_id: `0`
- onchain_lifecycle_rows_with_ab_record_id: `0`
- probe_transport_rows_with_ab_record_id: `25`
- probe_entry_rows_with_ab_record_id: `25`
- probe_transport_rows_with_probe_id: `25`
- probe_entry_rows_with_probe_id: `25`

## Artifact Coverage

| artifact | rows | candidate_id | ab_record_id | probe_id | pool_id | mint | v3_payload | feature_hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `decision` | 3025 | 0 | 3025 | 0 | 3025 | 3025 | 3025 | 3025 |
| `decision` | 247 | 0 | 247 | 0 | 247 | 247 | 247 | 247 |
| `probe_entry` | 25 | 25 | 25 | 25 | 25 | 25 | 0 | 25 |
| `probe_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `probe_selection` | 96 | 96 | 96 | 96 | 96 | 96 | 0 | 96 |
| `probe_skip` | 2997 | 2997 | 2997 | 2997 | 2997 | 2997 | 0 | 2997 |
| `probe_transport` | 25 | 25 | 25 | 25 | 25 | 25 | 0 | 25 |
| `shadow_entry` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_transport` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 2, "common_values": 247, "per_artifact_values": [3025, 247]}`
- `probe_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `pool_id`: `{"artifacts_with_rows": 2, "common_values": 247, "per_artifact_values": [3025, 247]}`
- `mint`: `{"artifacts_with_rows": 2, "common_values": 247, "per_artifact_values": [3025, 247]}`

## Probe Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 3, "common_values": 25, "per_artifact_values": [25, 96, 25]}`
- `probe_id`: `{"artifacts_with_rows": 3, "common_values": 25, "per_artifact_values": [25, 96, 25]}`
- `candidate_id`: `{"artifacts_with_rows": 3, "common_values": 0, "per_artifact_values": [25, 96, 25]}`
- `pool_id`: `{"artifacts_with_rows": 3, "common_values": 25, "per_artifact_values": [25, 96, 25]}`
- `mint`: `{"artifacts_with_rows": 3, "common_values": 25, "per_artifact_values": [25, 96, 25]}`

## Probe Decision Join

- decision_join_acceptance: `pass`
- required_exact_decision_v3_join_coverage: `1.0`
- `probe_selection`: `{"exact_decision_v3_join": 96, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 96, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 96, "joined_to_decision_with_v3_payload": 96, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 1}, "policy_hash_match": 96, "policy_hash_mismatch": 0, "rows": 96, "rows_with_ab_record_id": 96, "unmatched_rows": 0}`
- `probe_transport`: `{"exact_decision_v3_join": 25, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 25, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 25, "joined_to_decision_with_v3_payload": 25, "mismatch_reasons": {}, "policy_hash_match": 25, "policy_hash_mismatch": 0, "rows": 25, "rows_with_ab_record_id": 25, "unmatched_rows": 0}`
- `probe_entry`: `{"exact_decision_v3_join": 25, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 25, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 25, "joined_to_decision_with_v3_payload": 25, "mismatch_reasons": {}, "policy_hash_match": 25, "policy_hash_mismatch": 0, "rows": 25, "rows_with_ab_record_id": 25, "unmatched_rows": 0}`
- `probe_lifecycle`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`

## Probe Entry Materialization

- transport_rows: `25`
- entry_rows: `25`
- transport_without_entry_rows: `0`
- status_counts: `{"entry_materialized": 23, "simulation_error": 2}`
- reason_counts: `{"entry_row_present": 23, "simulation_account_layout_mismatch:custom_2006": 2}`
- buy_variant_counts: `{"routed_exact_sol_in": 25}`
- token_param_role_counts: `{"min_tokens_out": 25}`
- creator_vault_authority_status_counts: `{"creator_vault_source_not_authoritative": 2}`
- creator_vault_mismatch_reason_counts: `{"actual_expected_mismatch": 2}`
- creator_identity_source_counts: `{"account_overrides.creator_pubkey": 2}`
- amount_guard_status_counts: `{}`
- simulation_error_custom_code_counts: `{"custom_2006": 2}`
- skip_reason_counts: `{"creator_vault_source_not_authoritative": 1661, "execution_account_not_ready": 3, "max_probes_per_run_exceeded": 64, "probe_execution_precheck_failed": 275, "verdict_type_not_in_sample_scope": 994}`
- skip_creator_vault_authority_status_counts: `{"creator_vault_source_not_authoritative": 1661}`
- skip_creator_vault_mismatch_reason_counts: `{"creator_identity_source_not_authoritative": 1661}`
- skip_creator_identity_source_counts: `{"detected_pool.creator": 1661}`

| probe_id | status | reason | buy_variant | token_param_role | entry_token_amount_raw | min_tokens_out |
| --- | --- | --- | --- | --- | ---: | ---: |
| `b140911cd7843824f24221f99261abc5d05f7f107e464098eac6f506532acd95` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `33d82368a7bcb48f52af5318ffee26b8f126d1eeafc7516edc85b61619664a7d` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `450174500198` | `360139600158` |
| `8b9f7af80e59661a5df808f8e78175ac26a56d46f8a1f4665266c764feb00332` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `344432373137` | `275545898509` |
| `bf6d6694db4be2d222ed524259a76161a34117dc2c2329e4caa2a4f9408e3e89` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `154447046452` | `123557637161` |
| `f0cf5f264b434c22796b038ea014bf9ed4a450504f8b19333e8b13daa18a00bd` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `232114850084` | `185691880067` |
| `113f2f62f251a030c7a7e2d564a7d05a1cfa000dce1a47608407d09eb94123bb` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `246182322887` | `196945858309` |
| `de8d09f9e6dd64656e2ec0f6109b8b535dcf8d27806c1fca4d0423708ab47f10` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `246990415793` | `197592332634` |
| `f701978cd67dd5bd0a790de347ca70726bee048ba1a54c178b69e78174a0d867` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `245000930992` | `196000744793` |
| `ea3af5c6f8f9e9566e0c77de8148f183f530804d9d505347c4d4f7c2cb5b670a` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `routed_exact_sol_in` | `min_tokens_out` | `218137638167` | `174510110533` |
| `bda858adc2eff51053aed0574de38fa825eee383588d2b59afad53a5760dec6d` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `492932024037` | `394345619229` |
| `ed6a1f32cb34e0d600a80907f014864705aa33a14dfee88c563cd19653b77528` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `430937636359` | `344750109087` |
| `87a3d524de796ff79a3bb264a08a0b623f4856dfb628f73c3216a9568572a10d` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `routed_exact_sol_in` | `min_tokens_out` | `205262886394` | `164210309115` |
| `29f832c3f5306b65051aac5068ebbbcd0539cd97388fb73a3e16e27b630cfa17` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `477362617191` | `381890093752` |
| `208422041f0c0c178aa76a6e319c95fbd24f5eb1a42773d3a7d82e82e5ee3781` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `710579259740` | `568463407792` |
| `47c8cdeea6dccb1648211815b8b9313f7e8aa58bae3ccc3d6a342623229731be` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247642691977` | `198114153581` |
| `4b78a5ac7e9936b89f4cee6ec7016c590394333d56bbb3a738b56047f6c8aa47` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247642691977` | `198114153581` |
| `55b86b1f9cdc0ea364cc190dfc8d6405570d92ffc241a7ba0d6f11185b526be6` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `246019574654` | `196815659723` |
| `3bd9860ba8cb10ff71885b2357874974ddc57907b2056ed7dd8d8c6c289fdf07` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `238115486414` | `190492389131` |
| `50e46111dd9b91b2e3a016ba362a69558159d4ea0ec4ce95be466eaef8fefec9` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `232322045944` | `185857636755` |
| `83db6e39b3ddd8b2f7f77971838f6ce3c48b50fd76c8c6db7458b11ea8b68ae3` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `246818246601` | `197454597280` |
| `f1ca6098142fff7314782d13afe8419b3a47246af3c9f5a1bc767fe24a340357` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `246981764625` | `197585411700` |
| `288547d788b040f0d5a7d6632dda0ba6f72bd1c30ed012b001dde3e1940089ee` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `564452989327` | `451562391461` |
| `35e68ff64230539bd05ccb448b14af38dd9d5325489274dff127f66f7e18daae` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `232114850099` | `185691880079` |
| `2a663588f28d755e7a2db3c8f687e3cac8a8357347d6e7061834418def7e3cf3` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `197679438485` | `158143550788` |
| `5f95f2369ec6f2e4b4fc89cc684db4bcef087ca631624740f2a97bc2965b1724` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `225251602153` | `180201281722` |

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
