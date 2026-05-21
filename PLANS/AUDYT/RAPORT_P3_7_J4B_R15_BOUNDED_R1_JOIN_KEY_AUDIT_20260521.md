# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1.toml`
- readiness: `ready_for_lifecycle_feature_join`
- join_key_acceptance: `degraded`
- join_quality: `mint_only`
- probe_readiness: `ready_for_probe_transport_entry_join`
- probe_join_key_acceptance: `pass`
- probe_join_quality: `exact_probe_id_and_ab_record_id`
- probe_decision_join_acceptance: `pass`
- probe_required_exact_decision_v3_join_coverage: `1.0`
- probe_entry_materialization_status_counts: `{"entry_materialized": 24, "simulation_error": 1}`
- probe_entry_materialization_reason_counts: `{"entry_row_present": 24, "simulation_instruction_error:custom_6042": 1}`
- full_chain_ab_record_id_coverage: `1.0`
- probe_chain_ab_record_id_coverage: `1.0`
- probe_chain_probe_id_coverage: `1.0`
- readiness_reasons: `[]`
- probe_readiness_reasons: `[]`
- decision_rows_with_ab_record_id: `1975`
- shadow_transport_rows_with_ab_record_id: `2`
- shadow_entry_rows_with_ab_record_id: `2`
- shadow_lifecycle_rows_with_ab_record_id: `4`
- onchain_lifecycle_rows_with_ab_record_id: `0`
- probe_transport_rows_with_ab_record_id: `25`
- probe_entry_rows_with_ab_record_id: `25`
- probe_transport_rows_with_probe_id: `25`
- probe_entry_rows_with_probe_id: `25`

## Artifact Coverage

| artifact | rows | candidate_id | ab_record_id | probe_id | pool_id | mint | v3_payload | feature_hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `decision` | 2 | 0 | 2 | 0 | 2 | 2 | 2 | 2 |
| `decision` | 1822 | 0 | 1822 | 0 | 1822 | 1822 | 1822 | 1822 |
| `decision` | 151 | 0 | 151 | 0 | 151 | 151 | 151 | 151 |
| `probe_entry` | 25 | 25 | 25 | 25 | 25 | 25 | 0 | 25 |
| `probe_lifecycle` | 48 | 48 | 48 | 48 | 48 | 48 | 0 | 48 |
| `probe_selection` | 102 | 102 | 102 | 102 | 102 | 102 | 0 | 102 |
| `probe_skip` | 1797 | 1797 | 1797 | 1797 | 1797 | 1797 | 0 | 1797 |
| `probe_transport` | 25 | 25 | 25 | 25 | 25 | 25 | 0 | 25 |
| `shadow_entry` | 2 | 2 | 2 | 0 | 2 | 2 | 0 | 2 |
| `shadow_lifecycle` | 4 | 4 | 4 | 0 | 4 | 4 | 0 | 4 |
| `shadow_transport` | 2 | 2 | 2 | 0 | 0 | 2 | 0 | 2 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 6, "common_values": 2, "per_artifact_values": [2, 1822, 151, 2, 2, 2]}`
- `probe_id`: `{"artifacts_with_rows": 6, "common_values": 0, "per_artifact_values": [0, 0, 0, 0, 0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 6, "common_values": 0, "per_artifact_values": [0, 0, 0, 2, 2, 2]}`
- `pool_id`: `{"artifacts_with_rows": 6, "common_values": 0, "per_artifact_values": [2, 1822, 151, 2, 2, 0]}`
- `mint`: `{"artifacts_with_rows": 6, "common_values": 2, "per_artifact_values": [2, 1822, 151, 2, 2, 2]}`

## Probe Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 4, "common_values": 24, "per_artifact_values": [25, 24, 102, 25]}`
- `probe_id`: `{"artifacts_with_rows": 4, "common_values": 24, "per_artifact_values": [25, 24, 102, 25]}`
- `candidate_id`: `{"artifacts_with_rows": 4, "common_values": 0, "per_artifact_values": [25, 24, 102, 25]}`
- `pool_id`: `{"artifacts_with_rows": 4, "common_values": 24, "per_artifact_values": [25, 24, 102, 25]}`
- `mint`: `{"artifacts_with_rows": 4, "common_values": 24, "per_artifact_values": [25, 24, 102, 25]}`

## Probe Decision Join

- decision_join_acceptance: `pass`
- required_exact_decision_v3_join_coverage: `1.0`
- `probe_selection`: `{"exact_decision_v3_join": 102, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 102, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 102, "joined_to_decision_with_v3_payload": 102, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 1}, "policy_hash_match": 102, "policy_hash_mismatch": 0, "rows": 102, "rows_with_ab_record_id": 102, "unmatched_rows": 0}`
- `probe_transport`: `{"exact_decision_v3_join": 25, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 25, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 25, "joined_to_decision_with_v3_payload": 25, "mismatch_reasons": {}, "policy_hash_match": 25, "policy_hash_mismatch": 0, "rows": 25, "rows_with_ab_record_id": 25, "unmatched_rows": 0}`
- `probe_entry`: `{"exact_decision_v3_join": 25, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 25, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 25, "joined_to_decision_with_v3_payload": 25, "mismatch_reasons": {}, "policy_hash_match": 25, "policy_hash_mismatch": 0, "rows": 25, "rows_with_ab_record_id": 25, "unmatched_rows": 0}`
- `probe_lifecycle`: `{"exact_decision_v3_join": 48, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 48, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 48, "joined_to_decision_with_v3_payload": 48, "mismatch_reasons": {}, "policy_hash_match": 48, "policy_hash_mismatch": 0, "rows": 48, "rows_with_ab_record_id": 48, "unmatched_rows": 0}`

## Probe Entry Materialization

- transport_rows: `25`
- entry_rows: `25`
- transport_without_entry_rows: `0`
- status_counts: `{"entry_materialized": 24, "simulation_error": 1}`
- reason_counts: `{"entry_row_present": 24, "simulation_instruction_error:custom_6042": 1}`
- buy_variant_counts: `{"routed_exact_sol_in": 25}`
- token_param_role_counts: `{"min_tokens_out": 25}`
- creator_vault_authority_status_counts: `{}`
- creator_vault_mismatch_reason_counts: `{}`
- creator_identity_source_counts: `{}`
- amount_guard_status_counts: `{}`
- simulation_error_custom_code_counts: `{"custom_6042": 1}`
- skip_reason_counts: `{"active_buy_excluded": 2, "creator_vault_source_not_authoritative": 1083, "execution_account_not_ready": 1, "max_probes_per_run_exceeded": 76, "probe_execution_precheck_failed": 156, "verdict_type_not_in_sample_scope": 479}`
- skip_creator_vault_authority_status_counts: `{"creator_vault_source_not_authoritative": 1083}`
- skip_creator_vault_mismatch_reason_counts: `{"creator_identity_source_not_authoritative": 1083}`
- skip_creator_identity_source_counts: `{"detected_pool.creator": 1083}`

| probe_id | status | reason | buy_variant | token_param_role | entry_token_amount_raw | min_tokens_out |
| --- | --- | --- | --- | --- | ---: | ---: |
| `8e7c87b347a52f832cbac8e94d2928f3c47e36d8713dc694746643b2ed2ba3da` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `391825756698` | `313460605358` |
| `e514dc8ca25b1b53bca2ff7d043aff54802f0a9aa93a79f7b821a2e41cac5fa0` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `464479025661` | `371583220528` |
| `f97cecc36c7dfd382f66b3fd8cdee133cbdcd5b9dd8b957fe869f432ea3719dc` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `324989515371` | `259991612296` |
| `d36b1249d7c9f7f05d261ce771f8088584d582f6965f3645c83e469ffc938e36` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `538463743818` | `430770995054` |
| `5f0c4958071a6529404d154328fc30b6f01c1484ae295821f9ccf548f34d350c` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `407135953647` | `325708762917` |
| `f8f5aac71415ef8bec39ee23e9fbcd70a3bc447930396810b85b753d4779ed86` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `441856490966` | `353485192772` |
| `959cf73063468f322083c8b7573bad489cbb1595f0e903a6aec5d859ed88fe69` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `350354969212` | `280283975369` |
| `13f59652c9bc9468bf5c88326c08cffb729f517421368ae7d710deb9b5f172a6` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `383356468491` | `306685174792` |
| `3547bb0ef49f42696be4c92345002e5b8c9ada0503136af5b8b66b94e8a6f3c3` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247805591669` | `198244473335` |
| `fe8d29b6cefc8cd2e3860f2849d57faf0855f7e8c6157da71483018248675418` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `488437712343` | `390750169874` |
| `048a12feff8700cc17bcc0616bdeb9120e5931f609794c80d4941d045d206faf` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `246667673936` | `197334139148` |
| `6fea86fcbc3b11727344cfcc32bcdcea7303be420c1966ee9cbfad29f38afe3e` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `352718835277` | `282175068221` |
| `0b54d9ef2018a0030ef8e1bb0ee10ddab070a8b69161551ded81850fe5da5050` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `634089294645` | `507271435716` |
| `b77a996b5a1eaf6ecabe56c05018251a1d3039a92b155accd8a1f333bbb9d00f` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `295075266686` | `236060213348` |
| `acc69616a7539773b5e2dda739a930098e1afa081775d26dbacc2464de75636f` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `194320716437` | `155456573149` |
| `832af640e6e62ead908fb50916f014858ccaa17e99a1d8a7029182a25f7384e3` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `232262818807` | `185810255045` |
| `ca78c5321e49ad7bd8e3e6082b852538da631b52888fc3f6f1386868e2ac4ba1` | `simulation_error` | `simulation_instruction_error:custom_6042` | `routed_exact_sol_in` | `min_tokens_out` | `416350899526` | `333080719620` |
| `bd0b86934fce01ab734b8311c278fd913f64abee81354f806d527aa5d5512172` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `480604431009` | `384483544807` |
| `a994d15fa8b42d398f6f0eadfec704f86fac98d5b264e92aef7c15027be79ca0` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `225382333641` | `180305866912` |
| `953f5fe6537bb1c101f03c82fa770632187e77e9c674419d606f67786bd5be86` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `458926764708` | `367141411766` |
| `36b23abb956a2144886a8094c5480b2daf57f4f7402ec83720078e958c5cd3f1` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `444452298128` | `355561838502` |
| `48f007d0070e1f3dff176e8adb2a488d7810e230978397cd3e9b98c218a9d205` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `822327248980` | `657861799184` |
| `e5809f64f2648807a724565f70545d94951aa38b042bcc467f36829fac3b5186` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `508107292360` | `406485833888` |
| `28fd605ff230666a2a047e494d97ee6967d466225cdf40c7a7018bff41b12a20` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `450881023290` | `360704818632` |
| `9d2025171289b2d8051ec8d9c3dc041afa8e7126e963efec6abbb2d65bb2e3fe` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `499512292957` | `399609834365` |

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
