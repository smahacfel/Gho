# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1.toml`
- readiness: `ready_for_lifecycle_feature_join`
- join_key_acceptance: `degraded`
- join_quality: `mint_only`
- probe_readiness: `ready_for_probe_transport_entry_join`
- probe_join_key_acceptance: `pass`
- probe_join_quality: `exact_probe_id_and_ab_record_id`
- probe_decision_join_acceptance: `pass`
- probe_required_exact_decision_v3_join_coverage: `1.0`
- probe_entry_materialization_status_counts: `{"entry_materialized": 42, "simulation_error": 8}`
- probe_entry_materialization_reason_counts: `{"entry_row_present": 42, "simulation_account_layout_mismatch:custom_2006": 8}`
- full_chain_ab_record_id_coverage: `1.0`
- probe_chain_ab_record_id_coverage: `1.0`
- probe_chain_probe_id_coverage: `1.0`
- readiness_reasons: `[]`
- probe_readiness_reasons: `[]`
- decision_rows_with_ab_record_id: `2344`
- shadow_transport_rows_with_ab_record_id: `2`
- shadow_entry_rows_with_ab_record_id: `2`
- shadow_lifecycle_rows_with_ab_record_id: `6`
- onchain_lifecycle_rows_with_ab_record_id: `0`
- probe_transport_rows_with_ab_record_id: `50`
- probe_entry_rows_with_ab_record_id: `50`
- probe_transport_rows_with_probe_id: `50`
- probe_entry_rows_with_probe_id: `50`

## Artifact Coverage

| artifact | rows | candidate_id | ab_record_id | probe_id | pool_id | mint | v3_payload | feature_hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `decision` | 2 | 0 | 2 | 0 | 2 | 2 | 2 | 2 |
| `decision` | 2075 | 0 | 2075 | 0 | 2075 | 2075 | 2075 | 2075 |
| `decision` | 267 | 0 | 267 | 0 | 267 | 267 | 267 | 267 |
| `probe_entry` | 50 | 50 | 50 | 50 | 50 | 50 | 0 | 50 |
| `probe_lifecycle` | 84 | 84 | 84 | 84 | 84 | 84 | 0 | 84 |
| `probe_selection` | 151 | 151 | 151 | 151 | 151 | 151 | 0 | 151 |
| `probe_skip` | 2025 | 2025 | 2025 | 2025 | 2025 | 2025 | 0 | 2025 |
| `probe_transport` | 50 | 50 | 50 | 50 | 50 | 50 | 0 | 50 |
| `shadow_entry` | 2 | 2 | 2 | 0 | 2 | 2 | 0 | 2 |
| `shadow_lifecycle` | 6 | 6 | 6 | 0 | 6 | 6 | 0 | 6 |
| `shadow_transport` | 2 | 2 | 2 | 0 | 0 | 2 | 0 | 2 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 6, "common_values": 2, "per_artifact_values": [2, 2075, 267, 2, 2, 2]}`
- `probe_id`: `{"artifacts_with_rows": 6, "common_values": 0, "per_artifact_values": [0, 0, 0, 0, 0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 6, "common_values": 0, "per_artifact_values": [0, 0, 0, 2, 2, 2]}`
- `pool_id`: `{"artifacts_with_rows": 6, "common_values": 0, "per_artifact_values": [2, 2075, 267, 2, 2, 0]}`
- `mint`: `{"artifacts_with_rows": 6, "common_values": 2, "per_artifact_values": [2, 2075, 267, 2, 2, 2]}`

## Probe Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 4, "common_values": 42, "per_artifact_values": [50, 42, 151, 50]}`
- `probe_id`: `{"artifacts_with_rows": 4, "common_values": 42, "per_artifact_values": [50, 42, 151, 50]}`
- `candidate_id`: `{"artifacts_with_rows": 4, "common_values": 0, "per_artifact_values": [50, 42, 151, 50]}`
- `pool_id`: `{"artifacts_with_rows": 4, "common_values": 42, "per_artifact_values": [50, 42, 151, 50]}`
- `mint`: `{"artifacts_with_rows": 4, "common_values": 42, "per_artifact_values": [50, 42, 151, 50]}`

## Probe Decision Join

- decision_join_acceptance: `pass`
- required_exact_decision_v3_join_coverage: `1.0`
- `probe_selection`: `{"exact_decision_v3_join": 151, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 151, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 151, "joined_to_decision_with_v3_payload": 151, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 1}, "policy_hash_match": 151, "policy_hash_mismatch": 0, "rows": 151, "rows_with_ab_record_id": 151, "unmatched_rows": 0}`
- `probe_transport`: `{"exact_decision_v3_join": 50, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 50, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 50, "joined_to_decision_with_v3_payload": 50, "mismatch_reasons": {}, "policy_hash_match": 50, "policy_hash_mismatch": 0, "rows": 50, "rows_with_ab_record_id": 50, "unmatched_rows": 0}`
- `probe_entry`: `{"exact_decision_v3_join": 50, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 50, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 50, "joined_to_decision_with_v3_payload": 50, "mismatch_reasons": {}, "policy_hash_match": 50, "policy_hash_mismatch": 0, "rows": 50, "rows_with_ab_record_id": 50, "unmatched_rows": 0}`
- `probe_lifecycle`: `{"exact_decision_v3_join": 84, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 84, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 84, "joined_to_decision_with_v3_payload": 84, "mismatch_reasons": {}, "policy_hash_match": 84, "policy_hash_mismatch": 0, "rows": 84, "rows_with_ab_record_id": 84, "unmatched_rows": 0}`

## Probe Entry Materialization

- transport_rows: `50`
- entry_rows: `50`
- transport_without_entry_rows: `0`
- status_counts: `{"entry_materialized": 42, "simulation_error": 8}`
- reason_counts: `{"entry_row_present": 42, "simulation_account_layout_mismatch:custom_2006": 8}`
- buy_variant_counts: `{"routed_exact_sol_in": 50}`
- token_param_role_counts: `{"min_tokens_out": 50}`
- creator_vault_authority_status_counts: `{"creator_vault_source_not_authoritative": 8}`
- creator_vault_mismatch_reason_counts: `{"actual_expected_mismatch": 8}`
- creator_identity_source_counts: `{"account_overrides.creator_pubkey": 8}`
- amount_guard_status_counts: `{}`
- simulation_error_custom_code_counts: `{"custom_2006": 8}`
- skip_reason_counts: `{"active_buy_excluded": 2, "creator_vault_source_not_authoritative": 1198, "execution_account_not_ready": 7, "max_probes_per_run_exceeded": 69, "probe_concurrency_limit_exceeded": 6, "probe_execution_precheck_failed": 241, "probe_rate_limit_exceeded": 18, "verdict_type_not_in_sample_scope": 484}`
- skip_creator_vault_authority_status_counts: `{"creator_vault_source_not_authoritative": 1198}`
- skip_creator_vault_mismatch_reason_counts: `{"creator_identity_source_not_authoritative": 1198}`
- skip_creator_identity_source_counts: `{"detected_pool.creator": 1198}`

| probe_id | status | reason | buy_variant | token_param_role | entry_token_amount_raw | min_tokens_out |
| --- | --- | --- | --- | --- | ---: | ---: |
| `79142f54df8cdbfa2487972c0059aa4574e1effa9657488f04d1504a3b57487f` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `routed_exact_sol_in` | `min_tokens_out` | `247642691977` | `198114153581` |
| `779aa9875b5e67eaa8f4c127428dfffaccdf224af538bb4b32f08b99a99844b3` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `1046eb55f3899d02cf73691877a989466e0f917438459f33d63b0f6255cc23ed` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `239845545162` | `191876436129` |
| `31f1083a356545d77aefcf56a4d062056dac0a232ecf9750fb3f2e1a6932fc18` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `246182322887` | `196945858309` |
| `a24eccb51bb82a3ccc3af20a8d155bc6c16467a43be6478387a38436dce6ac0f` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247804125431` | `198243300344` |
| `e0bf4a317eaab545037c06f8e87266fa85a82e6b90af67a61e0b833fdcefced7` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `239747253939` | `191797803151` |
| `752c6e672a08ff441bfdbd78684e6fb77824da69cf7f694f54cad511a52a7c0a` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `194223230309` | `155378584247` |
| `db3c0378a1ab1c8878099690d4aa246b1f17b7d6526d5151d034d2337ff6a935` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247154461762` | `197723569409` |
| `58363f030a24a444901203c1d77a14e994bd6e4e8890aed271b8f4b7ab329259` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756854` | `198244605483` |
| `b88b9862da7708f6b1869c3866185a5046a295efc3c8dd341e97a3ee06784a4e` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `232262818800` | `185810255040` |
| `75b7d929cedd8153cf48be1ebff2717288732ff6388b74b1340d4ff81e170f4b` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `routed_exact_sol_in` | `min_tokens_out` | `211772991497` | `169418393197` |
| `858be973d42b92f2b9b0d14bbc70acfc0fc22ee9f8b8e7eb54ef8942d7cdc3c5` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247268252916` | `197814602332` |
| `ee54e0fb1a2e8b9d8f5dd4c6706fef58d4099fbd54f7fc1556a7ad4256e82cf7` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `199248802529` | `159399042023` |
| `0a42676c773467569834499436f3563d4c14d5993c19b04ec5911bf83d887023` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `246182322887` | `196945858309` |
| `b152a8042c9949dfd8517c317e85886d5d57929ff0d5679ac889bd6cecfc6963` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `245833450497` | `196666760397` |
| `0e58a37b97f334482283b0c850cd72ab94b26e7c267ab6c6d6c3edf415eb057f` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `routed_exact_sol_in` | `min_tokens_out` | `155379979754` | `124303983803` |
| `52f74eb961550fa855a02c74ebadb54bd15e759805b8adb15b708cee567b01d5` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `240348319397` | `192278655517` |
| `e41f69c7daa98bddcea5c9561535842c7ea4cb2d5e5d78f00e06064495a5a591` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `239747253939` | `191797803151` |
| `755b0e05f51e1558edcfa370ab8888f0077ec7c91fd16a88cdbd70b99e95006d` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `172804284096` | `138243427276` |
| `e806b9d83605e5b9f9761274d740c0e86241d731e64f81f53be3993e3fcc5e80` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `235856947166` | `188685557732` |
| `c25f52bfd4116cde4f1457bfd6017dc8f09acd0e75e3c3a2885808438e14316c` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `239845545177` | `191876436141` |
| `9b3c25af163e2d0aec6f10fc00c6bdfb76afeeebd159cd658a1cd32a8fe953cb` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756854` | `198244605483` |
| `f09a84ca7cff654de672e9cd4f83a368b74f18daedbf3adb44a33308af6d7741` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756854` | `198244605483` |
| `2bbb3afb928fa95b01072ea9940c1dfc7b6b1ec3a1fd44c5732028247a38b595` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `246237887893` | `196990310314` |
| `df89ccb449aa75dc7f1837e25169246e11bf3329f808986c81ac59e24902d787` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `236633562594` | `189306850075` |
| `cdc806f9518f06586c881915d26d4402e61653e8c8d81a029c9f086368ae2792` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247642691977` | `198114153581` |
| `452983c9e1f356bf5682eaca5de77de6476eb5bffcec1b78ee18778a267162c3` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756846` | `198244605476` |
| `f41e4562e29133c5dc589735890106b3250c21b8160bace421c8ccd3f3246ca8` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `239845545185` | `191876436148` |
| `bb56466f13175a0f2e998c34901321ff7166d09f41ed64be0496572fccb2c8e1` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247724204302` | `198179363441` |
| `77f1a22e15d77c841ca8e2ec37f2623a36d459d192f431b1d5f42fdab33687b0` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `239845545185` | `191876436148` |
| `56e8cf47d56775553ac0409717d5e6b714bdc3d7c4b667f6aa18ecc03f30b63e` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `cca2b011202668b0c83f92a04d5d66448a5e0c0a75301ff5796897540d448513` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `239845545185` | `191876436148` |
| `934cf22e4faaa95ee18f6bdbb0457a1b06d3bd8b3ea0ee31110c9792ae147668` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `239845545185` | `191876436148` |
| `9eb0c7e908e51c0cbfd127712beade4734d501d687457c5be59f2c04b9ff510a` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247642691977` | `198114153581` |
| `dd1887977d25fe983554ffb1f06b7f6bf1602e0abebaf3faa573df16bae3e621` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247642691977` | `198114153581` |
| `5a32819f2d471e8cf21650e0da62a0fafb2fb8a1605d108383ef4a03573d9f97` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `92a49431fefbeb65b15ee49834e488214b7a0057a5230c86fde15f0b53e82e65` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `26f22c876935afd817bcb6049046488ade6987fe1330582cfa24b51c7271f239` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `b06531448c4c15dfd25a8dd361e5e76ee1a95f2acbf40759f8a18874fadc38eb` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `routed_exact_sol_in` | `min_tokens_out` | `247642691977` | `198114153581` |
| `3de5c3c79cc7a4efdd54a9747e84da1a66de10774c5da8e5c8e5cefb8a359ff4` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `8154f47454bf0da102d4f833a9e5d20738e8ba1e255c145b2116c0cdbdd31a6f` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `240000968540` | `192000774832` |
| `f2ac95eefc3e1b5094bbecbd7a64ee63a8884547cab5bf12991738797d749cc5` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `228497584303` | `182798067442` |
| `ac65b8d963f67abc9b3a7d3acd27de4836dd7e93bbaf5d34deea527857019b1c` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `0aa9156242681b1bb0e0a30a8ab8ab86e4a5bdff76bb73e3b0718cda84d1ca37` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `242982951314` | `194386361051` |
| `3b9f5619756726b27c6fff8c4f15e08b53c8b3c2a7d4ea2b0a5be6cf17387aa3` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `c689c71d0fc1a7bf037b337672b9e1d8a66b8dca654124e829d1ea1083503a70` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `239845545185` | `191876436148` |
| `7b87cf9251a93baa973a58c2d26b8946d540a8cf696ee7cd91d2867755960fc9` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805756862` | `198244605489` |
| `48a5aeb1a591137e6015ff6242031fdd12560026285fc651cbddbb5e1af8da61` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `239845545185` | `191876436148` |
| `5737d1b503c81f0df3a99a33af0d4b31fa3c1708c10715cd2552c7d02c67864a` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `239845545177` | `191876436141` |
| `628b68fc76745dbcd3d2837859e092d8f71fd14c9d7ae080915cf1a7f7a09440` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `routed_exact_sol_in` | `min_tokens_out` | `203571505820` | `162857204656` |

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
