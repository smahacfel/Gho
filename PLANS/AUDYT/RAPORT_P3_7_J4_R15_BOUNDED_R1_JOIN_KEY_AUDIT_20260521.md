# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4-r1.toml`
- readiness: `ready_for_lifecycle_feature_join`
- join_key_acceptance: `degraded`
- join_quality: `mint_only`
- probe_readiness: `ready_for_probe_transport_entry_join`
- probe_join_key_acceptance: `pass`
- probe_join_quality: `exact_probe_id_and_ab_record_id`
- probe_decision_join_acceptance: `pass`
- probe_required_exact_decision_v3_join_coverage: `1.0`
- probe_entry_materialization_status_counts: `{"entry_materialized": 25}`
- probe_entry_materialization_reason_counts: `{"entry_row_present": 25}`
- full_chain_ab_record_id_coverage: `1.0`
- probe_chain_ab_record_id_coverage: `1.0`
- probe_chain_probe_id_coverage: `1.0`
- readiness_reasons: `[]`
- probe_readiness_reasons: `[]`
- decision_rows_with_ab_record_id: `1987`
- shadow_transport_rows_with_ab_record_id: `2`
- shadow_entry_rows_with_ab_record_id: `2`
- shadow_lifecycle_rows_with_ab_record_id: `6`
- onchain_lifecycle_rows_with_ab_record_id: `0`
- probe_transport_rows_with_ab_record_id: `25`
- probe_entry_rows_with_ab_record_id: `25`
- probe_transport_rows_with_probe_id: `25`
- probe_entry_rows_with_probe_id: `25`

## Artifact Coverage

| artifact | rows | candidate_id | ab_record_id | probe_id | pool_id | mint | v3_payload | feature_hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `decision` | 2 | 0 | 2 | 0 | 2 | 2 | 2 | 2 |
| `decision` | 1749 | 0 | 1749 | 0 | 1749 | 1749 | 1749 | 1749 |
| `decision` | 236 | 0 | 236 | 0 | 236 | 236 | 236 | 236 |
| `probe_entry` | 25 | 25 | 25 | 25 | 25 | 25 | 0 | 25 |
| `probe_lifecycle` | 50 | 50 | 50 | 50 | 50 | 50 | 0 | 50 |
| `probe_selection` | 92 | 92 | 92 | 92 | 92 | 92 | 0 | 92 |
| `probe_skip` | 1724 | 1724 | 1724 | 1724 | 1724 | 1724 | 0 | 1724 |
| `probe_transport` | 25 | 25 | 25 | 25 | 25 | 25 | 0 | 25 |
| `shadow_entry` | 2 | 2 | 2 | 0 | 2 | 2 | 0 | 2 |
| `shadow_lifecycle` | 6 | 6 | 6 | 0 | 6 | 6 | 0 | 6 |
| `shadow_transport` | 2 | 2 | 2 | 0 | 0 | 2 | 0 | 2 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 6, "common_values": 2, "per_artifact_values": [2, 1749, 236, 2, 2, 2]}`
- `probe_id`: `{"artifacts_with_rows": 6, "common_values": 0, "per_artifact_values": [0, 0, 0, 0, 0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 6, "common_values": 0, "per_artifact_values": [0, 0, 0, 2, 2, 2]}`
- `pool_id`: `{"artifacts_with_rows": 6, "common_values": 0, "per_artifact_values": [2, 1749, 236, 2, 2, 0]}`
- `mint`: `{"artifacts_with_rows": 6, "common_values": 2, "per_artifact_values": [2, 1749, 236, 2, 2, 2]}`

## Probe Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 4, "common_values": 25, "per_artifact_values": [25, 25, 92, 25]}`
- `probe_id`: `{"artifacts_with_rows": 4, "common_values": 25, "per_artifact_values": [25, 25, 92, 25]}`
- `candidate_id`: `{"artifacts_with_rows": 4, "common_values": 0, "per_artifact_values": [25, 25, 92, 25]}`
- `pool_id`: `{"artifacts_with_rows": 4, "common_values": 25, "per_artifact_values": [25, 25, 92, 25]}`
- `mint`: `{"artifacts_with_rows": 4, "common_values": 25, "per_artifact_values": [25, 25, 92, 25]}`

## Probe Decision Join

- decision_join_acceptance: `pass`
- required_exact_decision_v3_join_coverage: `1.0`
- `probe_selection`: `{"exact_decision_v3_join": 92, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 92, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 92, "joined_to_decision_with_v3_payload": 92, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 4}, "policy_hash_match": 92, "policy_hash_mismatch": 0, "rows": 92, "rows_with_ab_record_id": 92, "unmatched_rows": 0}`
- `probe_transport`: `{"exact_decision_v3_join": 25, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 25, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 25, "joined_to_decision_with_v3_payload": 25, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 1}, "policy_hash_match": 25, "policy_hash_mismatch": 0, "rows": 25, "rows_with_ab_record_id": 25, "unmatched_rows": 0}`
- `probe_entry`: `{"exact_decision_v3_join": 25, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 25, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 25, "joined_to_decision_with_v3_payload": 25, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 1}, "policy_hash_match": 25, "policy_hash_mismatch": 0, "rows": 25, "rows_with_ab_record_id": 25, "unmatched_rows": 0}`
- `probe_lifecycle`: `{"exact_decision_v3_join": 50, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 50, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 50, "joined_to_decision_with_v3_payload": 50, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 2}, "policy_hash_match": 50, "policy_hash_mismatch": 0, "rows": 50, "rows_with_ab_record_id": 50, "unmatched_rows": 0}`

## Probe Entry Materialization

- transport_rows: `25`
- entry_rows: `25`
- transport_without_entry_rows: `0`
- status_counts: `{"entry_materialized": 25}`
- reason_counts: `{"entry_row_present": 25}`
- buy_variant_counts: `{"routed_exact_sol_in": 25}`
- token_param_role_counts: `{"min_tokens_out": 25}`
- creator_vault_authority_status_counts: `{}`
- creator_vault_mismatch_reason_counts: `{}`
- creator_identity_source_counts: `{}`
- amount_guard_status_counts: `{}`
- simulation_error_custom_code_counts: `{}`
- skip_reason_counts: `{"active_buy_excluded": 2, "creator_vault_source_not_authoritative": 1169, "execution_account_not_ready": 3, "max_probes_per_run_exceeded": 63, "probe_execution_precheck_failed": 140, "verdict_type_not_in_sample_scope": 347}`
- skip_creator_vault_authority_status_counts: `{"creator_vault_source_not_authoritative": 1169}`
- skip_creator_vault_mismatch_reason_counts: `{"creator_identity_source_not_authoritative": 1169}`
- skip_creator_identity_source_counts: `{"detected_pool.creator": 1169}`

| probe_id | status | reason | buy_variant | token_param_role | entry_token_amount_raw | min_tokens_out |
| --- | --- | --- | --- | --- | ---: | ---: |
| `b5c8f3a08121356d454af6a48534f15d3e7a0678598e5913c993f1f19f1869fd` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `885301324693` | `708241059754` |
| `c4a83bcfd14715b4448b52e399f9c81b82f83be5df3107353b56a46fd9f14e49` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `260050200844` | `208040160675` |
| `1084bf9d32981a8c1adcbb9ac9d8b94bc0d394cbd0f3856a4703fdb03d339f9d` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `343297425660` | `274637940528` |
| `819f0ff74de7057ebe0866e81de7dcec4c4c2b8a3f5a5b7e2fa664d1d5a3fbd8` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `245365153454` | `196292122763` |
| `6ddba6d2622c3db8aec5823fa887bd82e55976f79f949fd3bce39d1e77bf4d17` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247144711908` | `197715769526` |
| `0ec2bc604d429419effd212fd83f9d4126922c0bd1e0653ff35d8f86a6581af6` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `246182322912` | `196945858329` |
| `5d0d0ffe8d7ffdcc67b356dc8b04ce49dc86825eb3807b1d779163bdd9dfd361` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `232114850084` | `185691880067` |
| `e95fcb07dcacf85409a5bad96b0e50c5a5749d82308f435e805f71b28ff56ba3` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `357310565709` | `285848452567` |
| `5bcafc985d093b3c32f36969c19911fdfe01656fbef7109e813d20e7abae6482` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `615933236380` | `492746589104` |
| `61e34c16e0fe9a3e4139ac7d60b79b96dd1d228da593efd2c9d1ab97815e1f85` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `242849266524` | `194279413219` |
| `cb1e48796abfcb0ed65b2316115f7d6cfd9ec16a21c10fdaff9aea9745c97292` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247494948654` | `197995958923` |
| `7c834a87df2d56a350e359174e6de5b9f5a076ff9a16a230001c4e77fe175d34` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `236768514543` | `189414811634` |
| `5df064cd163eefd3bf8497b43a783be34d4293a4c37fd2dc2df670e8842d3656` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247218667607` | `198244605489` |
| `78acb422ead975e438b68e8688bec09530d6a0a0e671c28dd566ec439f27acc8` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247538812755` | `198031050204` |
| `db5f63e8031a5cb786f6c71f45e68d6541e088c6a156630279a7471990e55a4f` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `215794433318` | `172635546654` |
| `25c61c0d1d6a32899310ac522792d760124f3798ba692ebce50827fa0a980cdc` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `469682124025` | `375745699220` |
| `ea534df414f0dd33fabe08accbc00e49c9b5c91018ec9c98275a35e5fd670dfe` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `218227733545` | `174582186836` |
| `cef0e7d8bc754ef4aed4db8032300e9792dd2e7c0f9beccc8a656641f7aedeb5` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247804105023` | `198243284018` |
| `2c8ea53f35e7b3769ecd8fd0f9db4d35f9adf6d0c6e032d0a7a12eaeb2d2d2c3` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `171272126943` | `137017701554` |
| `18ac3e86ad02f3f2b8b48d853f4b4fb85e1fddf2b5d7766f02d2d3cadf1c7754` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `507546667257` | `406037333805` |
| `c045b1d5c2c5b982e8e00e4aacfbd4f5531f3674f1c02425ccdfd7a804d46ff9` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `232114850099` | `185691880079` |
| `74363bb5cc6468cd0b729bf38f7316a541471e3d135e26ae768b9d5325636f2e` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `297526446259` | `238021157007` |
| `e1f9185826ae8ca103a057396fecf74762e3ea64db7ece4458499055cd15ac1f` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `247536936687` | `198029549349` |
| `bc1e3b5f85e0b151d11bbdba4937e7c4e4d2a4c52148e7adfba94d2742e1c834` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `559378733591` | `447502986872` |
| `85e27ae45b0af3cec692d9bf0a7d02632013d7dc38465cd6672bc186c6802eb5` | `entry_materialized` | `entry_row_present` | `routed_exact_sol_in` | `min_tokens_out` | `371700067284` | `297360053827` |

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
