# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1.toml`
- readiness: `ready_for_lifecycle_feature_join`
- join_key_acceptance: `degraded`
- join_quality: `mint_only`
- probe_readiness: `ready_for_probe_transport_entry_join`
- probe_join_key_acceptance: `pass`
- probe_join_quality: `exact_probe_id_and_ab_record_id`
- probe_decision_join_acceptance: `pass`
- probe_required_exact_decision_v3_join_coverage: `1.0`
- probe_entry_materialization_status_counts: `{"entry_materialized": 39, "simulation_error": 11}`
- probe_entry_materialization_reason_counts: `{"entry_row_present": 39, "simulation_account_layout_mismatch:custom_2006": 11}`
- full_chain_ab_record_id_coverage: `0.833333`
- probe_chain_ab_record_id_coverage: `1.0`
- probe_chain_probe_id_coverage: `1.0`
- readiness_reasons: `[]`
- probe_readiness_reasons: `[]`
- decision_rows_with_ab_record_id: `1978`
- shadow_transport_rows_with_ab_record_id: `5`
- shadow_entry_rows_with_ab_record_id: `5`
- shadow_lifecycle_rows_with_ab_record_id: `13`
- onchain_lifecycle_rows_with_ab_record_id: `0`
- probe_transport_rows_with_ab_record_id: `50`
- probe_entry_rows_with_ab_record_id: `50`
- probe_transport_rows_with_probe_id: `50`
- probe_entry_rows_with_probe_id: `50`

## Artifact Coverage

| artifact | rows | candidate_id | ab_record_id | probe_id | pool_id | mint | v3_payload | feature_hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `decision` | 6 | 0 | 6 | 0 | 6 | 6 | 6 | 6 |
| `decision` | 1972 | 0 | 1972 | 0 | 1972 | 1972 | 1972 | 1972 |
| `probe_entry` | 50 | 50 | 50 | 50 | 50 | 50 | 0 | 50 |
| `probe_lifecycle` | 78 | 78 | 78 | 78 | 78 | 78 | 0 | 78 |
| `probe_selection` | 163 | 163 | 163 | 163 | 163 | 163 | 0 | 163 |
| `probe_skip` | 1922 | 1922 | 1922 | 1922 | 1922 | 1922 | 0 | 1922 |
| `probe_transport` | 50 | 50 | 50 | 50 | 50 | 50 | 0 | 50 |
| `shadow_entry` | 5 | 5 | 5 | 0 | 5 | 5 | 0 | 5 |
| `shadow_lifecycle` | 14 | 14 | 13 | 0 | 14 | 14 | 0 | 13 |
| `shadow_transport` | 6 | 6 | 5 | 0 | 0 | 6 | 0 | 5 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 5, "common_values": 5, "per_artifact_values": [6, 1972, 5, 5, 5]}`
- `probe_id`: `{"artifacts_with_rows": 5, "common_values": 0, "per_artifact_values": [0, 0, 0, 0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 5, "common_values": 0, "per_artifact_values": [0, 0, 5, 6, 6]}`
- `pool_id`: `{"artifacts_with_rows": 5, "common_values": 0, "per_artifact_values": [6, 1972, 5, 6, 0]}`
- `mint`: `{"artifacts_with_rows": 5, "common_values": 5, "per_artifact_values": [6, 1972, 5, 6, 6]}`

## Probe Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 4, "common_values": 39, "per_artifact_values": [50, 39, 163, 50]}`
- `probe_id`: `{"artifacts_with_rows": 4, "common_values": 39, "per_artifact_values": [50, 39, 163, 50]}`
- `candidate_id`: `{"artifacts_with_rows": 4, "common_values": 0, "per_artifact_values": [50, 39, 163, 50]}`
- `pool_id`: `{"artifacts_with_rows": 4, "common_values": 39, "per_artifact_values": [50, 39, 163, 50]}`
- `mint`: `{"artifacts_with_rows": 4, "common_values": 39, "per_artifact_values": [50, 39, 163, 50]}`

## Probe Decision Join

- decision_join_acceptance: `pass`
- required_exact_decision_v3_join_coverage: `1.0`
- `probe_selection`: `{"exact_decision_v3_join": 163, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 163, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 163, "joined_to_decision_with_v3_payload": 163, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 3}, "policy_hash_match": 163, "policy_hash_mismatch": 0, "rows": 163, "rows_with_ab_record_id": 163, "unmatched_rows": 0}`
- `probe_transport`: `{"exact_decision_v3_join": 50, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 50, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 50, "joined_to_decision_with_v3_payload": 50, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 2}, "policy_hash_match": 50, "policy_hash_mismatch": 0, "rows": 50, "rows_with_ab_record_id": 50, "unmatched_rows": 0}`
- `probe_entry`: `{"exact_decision_v3_join": 50, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 50, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 50, "joined_to_decision_with_v3_payload": 50, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 2}, "policy_hash_match": 50, "policy_hash_mismatch": 0, "rows": 50, "rows_with_ab_record_id": 50, "unmatched_rows": 0}`
- `probe_lifecycle`: `{"exact_decision_v3_join": 78, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 78, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 78, "joined_to_decision_with_v3_payload": 78, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 2}, "policy_hash_match": 78, "policy_hash_mismatch": 0, "rows": 78, "rows_with_ab_record_id": 78, "unmatched_rows": 0}`

## Probe Entry Materialization

- transport_rows: `50`
- entry_rows: `50`
- transport_without_entry_rows: `0`
- status_counts: `{"entry_materialized": 39, "simulation_error": 11}`
- reason_counts: `{"entry_row_present": 39, "simulation_account_layout_mismatch:custom_2006": 11}`
- buy_variant_counts: `{"routed_exact_sol_in": 50}`
- token_param_role_counts: `{"min_tokens_out": 50}`
- creator_vault_authority_status_counts: `{"creator_vault_source_not_authoritative": 11}`
- creator_vault_mismatch_reason_counts: `{"actual_expected_mismatch": 11}`
- creator_identity_source_counts: `{"account_overrides.creator_pubkey": 11}`
- amount_guard_status_counts: `{}`
- simulation_error_custom_code_counts: `{"custom_2006": 11}`
- skip_reason_counts: `{"creator_vault_source_not_authoritative": 1074, "execution_account_not_ready": 1, "max_probes_per_run_exceeded": 64, "probe_concurrency_limit_exceeded": 42, "probe_execution_precheck_failed": 226, "probe_rate_limit_exceeded": 6, "verdict_type_not_in_sample_scope": 509}`
- skip_creator_vault_authority_status_counts: `{"creator_vault_source_not_authoritative": 1074}`
- skip_creator_vault_mismatch_reason_counts: `{"creator_identity_source_not_authoritative": 1074}`
- skip_creator_identity_source_counts: `{"detected_pool.creator": 1074}`

| probe_id | status | reason | buy_variant | token_param_role | entry_token_amount_raw | min_tokens_out |
| --- | --- | --- | --- | --- | ---: | ---: |
| `2904ea7fbcba4740b087ac856e8e7ae200a85bbf777b7c6e2496e5f2ab403aaf` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `routed_exact_sol_in` | `min_tokens_out` | `155379979754` | `124303983803` |
| `e9320c5011e3b2ae933313e68ce2751573800ace06cff49c5a4fa9f468821229` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `routed_exact_sol_in` | `min_tokens_out` | `209972273198` | `167977818558` |
| `8e406ad7de99608bc10ccd6b34ccd0dc19f647e28272d9209f2f82042353bf27` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `74a7fff2ca96199e1c6cdc7b3c1578f69b0d54988935df56c1d2030ca8a691c8` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `a98a6b58e07a9695be6f3209f94aae58ca82fc565207d94ad146c094f819aea9` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `8c96c60bf8f5811b3047d2b25b5dcd76ae72171b65e89923b4d993ea9843e968` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247642691977` | `198114153581` |
| `738f4f7ce9b3c27c964c7ba315bf4c646f01ad14ec02dbf492765a429f7ca0f8` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `ddc6f1185c7fd0a31eb272d878e6d08dd5088f94648c72129ca3ffb75a7ff3f9` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `243249854509` | `194599883607` |
| `0323c191e4a4c2a5ca8abe993692cae6028dfc6effc2c41247d191efec22a5f3` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `routed_exact_sol_in` | `min_tokens_out` | `182711231300` | `146168985040` |
| `58282c8f047f8e22799877d55f5866ddfcc8a1f83a70598286b5b3c1bdea844c` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `244095597579` | `195276478063` |
| `98a7dc589603565757c8ad78c26d0c8119182ca0918270d6b7cf3f80f41951fd` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `182711228516` | `146168982812` |
| `202f9def6c5dc65125b08bafb61662de8a68335b969ab976a6bd11580db43c73` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `94389807034` | `75511845627` |
| `616bdf27caa47d7174dfdfa3ed1128d3c35975ea4cd0994de727cefbf7cae534` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `routed_exact_sol_in` | `min_tokens_out` | `209972270222` | `167977816177` |
| `49047d9eb2693b32238dff19a86ab3b4a36a591f76db62bb0fa6f37c8ab3fa94` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `routed_exact_sol_in` | `min_tokens_out` | `198056999461` | `158445599568` |
| `4fd3cbd302ec9b2ce2e993740653fbdd93813c7b7a862252f897af2c29102471` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `182711231289` | `146168985031` |
| `f9efe15e2a65fc3517424eec6e653eeeb1f6e09a549539c0a6c91f0760ad8b13` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `239277312752` | `191876436148` |
| `088b82ae127ba8ba5552d2b24ebc00a1c82c4d5139d577cd6f667c13c6fdef6b` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `155379979754` | `124303983803` |
| `bf393d30154cb7581b0c874ea326a83548dc0d6deca1b6254166b5ef389ac3b4` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `218137638167` | `174510110533` |
| `65db52941b8a6cb9b4154b670b18316c17c3aed63d0d4b64d98a53a9b197d133` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `133658896491` | `106927117192` |
| `6035e74adb706f97ceac76ab5ebf1684d920527f3dff284328ce1db6e0ee8572` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `routed_exact_sol_in` | `min_tokens_out` | `123857796236` | `99086236988` |
| `01eca25fc91d7a9aee9ccfd224fdd91c32b832c347d7c1d0606d5067e4188a18` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `fa0369355ed3d0c786058c7396320af125f4f4e9336dc3545c88d72c0cee307e` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `181867193869` | `145493755095` |
| `4c42d9bee449e0c72da514ab6d30b5c21f13b2218343df099ff71ffbf2e9d4f4` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `440bc68767b1d6ab80a9db28200168a87fa3f1aceaab02e4f7046b56cfe7de95` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `routed_exact_sol_in` | `min_tokens_out` | `247394755072` | `197915804057` |
| `d32a12f56c70f78cb423051e5d0c631b126c0f4c96f3bc3c9f72b658f7b61e2b` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `4ddd1777b79bb119639e2015da219a55eee3fdcf2a9be66a93167f7a6ef540e8` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `128706993591` | `102965594872` |
| `92f6d8f0fab6c63a8e12babfbb8ba64dd03428ce38a0078c10fc915a3e51f69c` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `2d727bfdad2e8a1e3faa9d14f6369178b7dc4d047f852de4e43a68a2b24932d5` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `8afbc9524ca274542447c1be49db57400717be0f8adfc1a3468b1d7e2c8cd224` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `routed_exact_sol_in` | `min_tokens_out` | `182711231289` | `146168985031` |
| `8f1417cc1712655138022ec6250943b6149610da5413919caed8cf54eafecbf9` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `fe6dcf2ae9f8e35810a92a284059e5800f8d78af0eec9875bb230f5496407e6f` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `91a8661b6a2eb4854fc0ca16bf49fe07ac790c590c0a58ba2e94fe27b170f8ec` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `154601690778` | `123681352622` |
| `372fb79dfb8bc22886cdc37b263bcd86cc6ba870c39a91de5dd4927b5bc1ef8e` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `216782581847` | `173426065477` |
| `ae686fd7d38fe5a26d2707228a189b26a4b117ddb66cf2e67016623565dd1c1e` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `c7e84ff7bbf0760dbf6a3a2340e2f549f55f06e630001970dab9ab5ffe3a8e1e` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `routed_exact_sol_in` | `min_tokens_out` | `172804284096` | `138243427276` |
| `6d76abb07dcbca10e1b41a341b6580bee785d5e9953eb6181dee1f757e8fd6e9` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `102568621519` | `82054897215` |
| `1e213aae90ca4400f4c9197229bec9c7b6a85c605760b3ad1ee6c8e52c043cb0` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `f038a7c7131a3bf0bb77c78a8f5badbe9bdf55367d9b73d144058d12faeea39e` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `778d4a40795e639c271271f555358f30bea0c60496876770b5cb34a240c8f788` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247346558290` | `197877246632` |
| `b057df0d39df37be83e32f81f00a63ceda73e31f5f3711ea8d80858cbcf852ef` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `227109179723` | `181687343778` |
| `ff48b047f6376d6294de7361f4c52c56774c1a91f849718741236d0877bdfd61` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `68123141358` | `54498513086` |
| `185bea890d2e280ff97ad3c6943a1e1d76a73664e7b184368857437a05498af2` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `138528634527` | `110822907621` |
| `dd6eb8e6b2442b827798ff6fb5a928e8f6f99391f107cb22adec683cd3d224f2` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `155379979754` | `124303983803` |
| `1a01c35eb2388cd4fa2e7338817f8618e59bf05cb215d4ff51181982a9c60e38` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `155379979754` | `124303983803` |
| `3df8a525d259958ca5ef3a6a7be793ef9ea570601461fe3c5a946c53a2058f6e` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `182711231289` | `146168985031` |
| `6a83c58325f61dc572cfb0a2374729d7ef206277200b03e2ce767dc4f1db4130` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `239277312752` | `191876436148` |
| `74b97b132e6fd3feb419ef48a3b2810dc4251642db5350b5dda21f81854f0fbb` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `133658896491` | `106927117192` |
| `4fca74a7f077936dd163e7d7741e685719e39b15b529755b4918207f2ecd51a0` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `227109179723` | `181687343778` |
| `ea0e09b8b8d6dc3baadfcb853733b761a9234685e2eb850c696d9538647ceb67` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `239277312752` | `191876436148` |
| `b9b7a59600ffbd7cb014a83d8065412dd4bac1e72424728cf4dfe991b4d6afe2` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
