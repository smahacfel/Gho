# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k3-r1.toml`
- readiness: `not_ready`
- join_key_acceptance: `fail`
- join_quality: `pool_mint_time_window`
- probe_readiness: `ready_for_probe_transport_entry_join`
- probe_join_key_acceptance: `pass`
- probe_join_quality: `exact_probe_id_and_ab_record_id`
- probe_decision_join_acceptance: `pass`
- probe_required_exact_decision_v3_join_coverage: `1.0`
- probe_entry_materialization_status_counts: `{"entry_materialized": 13, "simulation_error": 3, "transport_only_missing_token_quantity": 1}`
- probe_entry_materialization_reason_counts: `{"entry_row_present": 13, "routed_exact_sol_in_entry_token_amount_raw_null": 1, "simulation_account_layout_mismatch:custom_2006": 2, "simulation_slippage_or_price_mismatch:custom_6002": 1}`
- full_chain_ab_record_id_coverage: `1.0`
- probe_chain_ab_record_id_coverage: `1.0`
- probe_chain_probe_id_coverage: `1.0`
- readiness_reasons: `["missing_shadow_transport_rows", "missing_shadow_entry_rows", "missing_shadow_lifecycle_rows"]`
- probe_readiness_reasons: `[]`
- decision_rows_with_ab_record_id: `50`
- shadow_transport_rows_with_ab_record_id: `0`
- shadow_entry_rows_with_ab_record_id: `0`
- shadow_lifecycle_rows_with_ab_record_id: `0`
- onchain_lifecycle_rows_with_ab_record_id: `0`
- probe_transport_rows_with_ab_record_id: `17`
- probe_entry_rows_with_ab_record_id: `16`
- probe_transport_rows_with_probe_id: `17`
- probe_entry_rows_with_probe_id: `16`

## Artifact Coverage

| artifact | rows | candidate_id | ab_record_id | probe_id | pool_id | mint | v3_payload | feature_hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `decision` | 45 | 0 | 45 | 0 | 45 | 45 | 45 | 45 |
| `decision` | 5 | 0 | 5 | 0 | 5 | 5 | 5 | 5 |
| `probe_entry` | 16 | 16 | 16 | 16 | 16 | 16 | 0 | 16 |
| `probe_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `probe_selection` | 26 | 26 | 26 | 26 | 26 | 26 | 0 | 26 |
| `probe_skip` | 28 | 28 | 28 | 28 | 28 | 28 | 0 | 28 |
| `probe_transport` | 17 | 17 | 17 | 17 | 17 | 17 | 0 | 17 |
| `shadow_entry` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_transport` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 2, "common_values": 5, "per_artifact_values": [45, 5]}`
- `probe_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `pool_id`: `{"artifacts_with_rows": 2, "common_values": 5, "per_artifact_values": [45, 5]}`
- `mint`: `{"artifacts_with_rows": 2, "common_values": 5, "per_artifact_values": [45, 5]}`

## Probe Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 3, "common_values": 16, "per_artifact_values": [16, 26, 17]}`
- `probe_id`: `{"artifacts_with_rows": 3, "common_values": 16, "per_artifact_values": [16, 26, 17]}`
- `candidate_id`: `{"artifacts_with_rows": 3, "common_values": 0, "per_artifact_values": [16, 26, 17]}`
- `pool_id`: `{"artifacts_with_rows": 3, "common_values": 16, "per_artifact_values": [16, 26, 17]}`
- `mint`: `{"artifacts_with_rows": 3, "common_values": 16, "per_artifact_values": [16, 26, 17]}`

## Probe Decision Join

- decision_join_acceptance: `pass`
- required_exact_decision_v3_join_coverage: `1.0`
- `probe_selection`: `{"exact_decision_v3_join": 26, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 26, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 26, "joined_to_decision_with_v3_payload": 26, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 5}, "policy_hash_match": 26, "policy_hash_mismatch": 0, "rows": 26, "rows_with_ab_record_id": 26, "unmatched_rows": 0}`
- `probe_transport`: `{"exact_decision_v3_join": 17, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 17, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 17, "joined_to_decision_with_v3_payload": 17, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 4}, "policy_hash_match": 17, "policy_hash_mismatch": 0, "rows": 17, "rows_with_ab_record_id": 17, "unmatched_rows": 0}`
- `probe_entry`: `{"exact_decision_v3_join": 16, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 16, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 16, "joined_to_decision_with_v3_payload": 16, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 4}, "policy_hash_match": 16, "policy_hash_mismatch": 0, "rows": 16, "rows_with_ab_record_id": 16, "unmatched_rows": 0}`
- `probe_lifecycle`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`

## Probe Entry Materialization

- transport_rows: `17`
- entry_rows: `16`
- transport_without_entry_rows: `1`
- status_counts: `{"entry_materialized": 13, "simulation_error": 3, "transport_only_missing_token_quantity": 1}`
- reason_counts: `{"entry_row_present": 13, "routed_exact_sol_in_entry_token_amount_raw_null": 1, "simulation_account_layout_mismatch:custom_2006": 2, "simulation_slippage_or_price_mismatch:custom_6002": 1}`
- buy_variant_counts: `{"legacy_buy": 16, "routed_exact_sol_in": 1}`
- token_param_role_counts: `{"min_tokens_out": 1, "token_amount": 16}`
- skip_reason_counts: `{"execution_account_not_ready": 6, "probe_concurrency_limit_exceeded": 1, "probe_rate_limit_exceeded": 2, "verdict_type_not_in_sample_scope": 19}`

| probe_id | status | reason | buy_variant | token_param_role | entry_token_amount_raw | min_tokens_out |
| --- | --- | --- | --- | --- | ---: | ---: |
| `83b82d0dc8263a9b6c7b86067f2010392734d1e1a6742e52922af1cc5c9ce909` | `simulation_error` | `simulation_slippage_or_price_mismatch:custom_6002` | `legacy_buy` | `token_amount` | `419271976603` | `335417581282` |
| `794e5fa7e9fc65f0313b3aa02b61075a860cd61b65f1943321dc11db0a7da80d` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `219549057255` | `175639245804` |
| `10d66b7fedd42f13b0327b9183cbfcd096c60c49e3bc0c9125d26a012c01ba9b` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `legacy_buy` | `token_amount` | `238455358956` | `190764287164` |
| `7c04d49a4be93ba7650a100a3e5b2f1810ebb71fbdc88a305f5cdfa6e2b41714` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `720530719212` | `576424575369` |
| `90ddc06103faf568263d55a57919b47c2b42ae58e125b69338239eab77d28a1f` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `318336518325` | `254669214660` |
| `44f622c693dd50112b1a58078957732de8ceb427a3234dc3c813835b39787b06` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `855139210530` | `684111368424` |
| `a5ec8cb0dd7f838714d15ecf92256e682760782ca0a79cee5e5ec2f9498fa72c` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `238870984707` | `191096787765` |
| `3a3b3c6e826309116dc3591d3fffa65fcf761f0fd50a0f399fb8a9062ac852fa` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `244769112303` | `195815289842` |
| `c2097e177a95d3efefef6d8c16dc750b28c00b3d42e0476181e22fa36dd7e51b` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `116661549682` | `93329239745` |
| `d2061296f09e38c98b0ee3d2888cff6d1679f63f8a0e5460014517afc6423ef1` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `115451198764` | `92360959011` |
| `d0af35acbe740e658de5d312fb2a7664935e0a6a8775521941c7e84ae413d7c4` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `157222512167` | `125778009733` |
| `614568877e65c51bbe24634d7487e6160188c337a12b5fbcda81a0dac08a99bb` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `182199623052` | `145759698441` |
| `c65165a90b21c62548de40dc011b829e715e82e37955584b76bf9a32aa4abcf1` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `143096576876` | `114477261500` |
| `4b28b203ad61b78286ebfd044947c1a6853ca5192efb292d4e21f41eb515a7c1` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `405795869167` | `324636695333` |
| `91c38ff679e581159550f855a60d1a6911ef515d4d398e8c7b17d7cbd1f62c45` | `simulation_error` | `simulation_account_layout_mismatch:custom_2006` | `legacy_buy` | `token_amount` | `141634726076` | `113307780860` |
| `8821990a2d290b5338afeedff631c137e496f05caf44c8c8ed63956836df66e8` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `246161875442` | `196929500353` |
| `029ba2ce1ebc7b7c3f1dd7d127f17c73f0c02e513e5cdc55b259e89c6f95bf18` | `transport_only_missing_token_quantity` | `routed_exact_sol_in_entry_token_amount_raw_null` | `routed_exact_sol_in` | `min_tokens_out` | `None` | `1` |

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
