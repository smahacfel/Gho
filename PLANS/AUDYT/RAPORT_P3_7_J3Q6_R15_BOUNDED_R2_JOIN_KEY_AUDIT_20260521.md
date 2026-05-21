# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-q6-r2.toml`
- readiness: `not_ready`
- join_key_acceptance: `fail`
- join_quality: `pool_mint_time_window`
- probe_readiness: `ready_for_probe_transport_entry_join`
- probe_join_key_acceptance: `pass`
- probe_join_quality: `exact_probe_id_and_ab_record_id`
- probe_decision_join_acceptance: `pass`
- probe_required_exact_decision_v3_join_coverage: `1.0`
- probe_entry_materialization_status_counts: `{"entry_materialized": 4}`
- probe_entry_materialization_reason_counts: `{"entry_row_present": 4}`
- full_chain_ab_record_id_coverage: `1.0`
- probe_chain_ab_record_id_coverage: `1.0`
- probe_chain_probe_id_coverage: `1.0`
- readiness_reasons: `["missing_shadow_transport_rows", "missing_shadow_entry_rows", "missing_shadow_lifecycle_rows"]`
- probe_readiness_reasons: `[]`
- decision_rows_with_ab_record_id: `777`
- shadow_transport_rows_with_ab_record_id: `0`
- shadow_entry_rows_with_ab_record_id: `0`
- shadow_lifecycle_rows_with_ab_record_id: `0`
- onchain_lifecycle_rows_with_ab_record_id: `0`
- probe_transport_rows_with_ab_record_id: `4`
- probe_entry_rows_with_ab_record_id: `4`
- probe_transport_rows_with_probe_id: `4`
- probe_entry_rows_with_probe_id: `4`

## Artifact Coverage

| artifact | rows | candidate_id | ab_record_id | probe_id | pool_id | mint | v3_payload | feature_hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `decision` | 687 | 0 | 687 | 0 | 687 | 687 | 687 | 687 |
| `decision` | 90 | 0 | 90 | 0 | 90 | 90 | 90 | 90 |
| `probe_entry` | 4 | 4 | 4 | 4 | 4 | 4 | 0 | 4 |
| `probe_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `probe_selection` | 514 | 514 | 514 | 514 | 514 | 514 | 0 | 514 |
| `probe_skip` | 559 | 559 | 559 | 559 | 559 | 559 | 0 | 559 |
| `probe_transport` | 4 | 4 | 4 | 4 | 4 | 4 | 0 | 4 |
| `shadow_entry` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_transport` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 2, "common_values": 90, "per_artifact_values": [687, 90]}`
- `probe_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `pool_id`: `{"artifacts_with_rows": 2, "common_values": 90, "per_artifact_values": [687, 90]}`
- `mint`: `{"artifacts_with_rows": 2, "common_values": 90, "per_artifact_values": [687, 90]}`

## Probe Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 3, "common_values": 4, "per_artifact_values": [4, 514, 4]}`
- `probe_id`: `{"artifacts_with_rows": 3, "common_values": 4, "per_artifact_values": [4, 514, 4]}`
- `candidate_id`: `{"artifacts_with_rows": 3, "common_values": 0, "per_artifact_values": [4, 514, 4]}`
- `pool_id`: `{"artifacts_with_rows": 3, "common_values": 4, "per_artifact_values": [4, 514, 4]}`
- `mint`: `{"artifacts_with_rows": 3, "common_values": 4, "per_artifact_values": [4, 514, 4]}`

## Probe Decision Join

- decision_join_acceptance: `pass`
- required_exact_decision_v3_join_coverage: `1.0`
- `probe_selection`: `{"exact_decision_v3_join": 514, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 514, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 514, "joined_to_decision_with_v3_payload": 514, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 90}, "policy_hash_match": 514, "policy_hash_mismatch": 0, "rows": 514, "rows_with_ab_record_id": 514, "unmatched_rows": 0}`
- `probe_transport`: `{"exact_decision_v3_join": 4, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 4, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 4, "joined_to_decision_with_v3_payload": 4, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 2}, "policy_hash_match": 4, "policy_hash_mismatch": 0, "rows": 4, "rows_with_ab_record_id": 4, "unmatched_rows": 0}`
- `probe_entry`: `{"exact_decision_v3_join": 4, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 4, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 4, "joined_to_decision_with_v3_payload": 4, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 2}, "policy_hash_match": 4, "policy_hash_mismatch": 0, "rows": 4, "rows_with_ab_record_id": 4, "unmatched_rows": 0}`
- `probe_lifecycle`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`

## Probe Entry Materialization

- transport_rows: `4`
- entry_rows: `4`
- transport_without_entry_rows: `0`
- status_counts: `{"entry_materialized": 4}`
- reason_counts: `{"entry_row_present": 4}`
- buy_variant_counts: `{"legacy_buy": 4}`
- token_param_role_counts: `{"token_amount": 4}`
- skip_reason_counts: `{"probe_execution_precheck_failed": 396, "verdict_type_not_in_sample_scope": 163}`

| probe_id | status | reason | buy_variant | token_param_role | entry_token_amount_raw | min_tokens_out |
| --- | --- | --- | --- | --- | ---: | ---: |
| `b9fa05c2307e0350104018fe848e0aa62a2a5a10567099b291e24b86a9ce9b7b` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `103126203808` | `82500963046` |
| `d97162497b1bec56e12404ca1495c89d2b06e11479c1b391da48a115a386aeeb` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `120934617542` | `96747694033` |
| `8cd90ae06bfee142b17dd7563b83db534dfa074ad6f7118772f74881f1cebd89` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `247805756862` | `198244605489` |
| `fc8aa00628dd933b2f604a09fde557ee4424d58bf60bc63ae242da442343b61e` | `entry_materialized` | `entry_row_present` | `legacy_buy` | `token_amount` | `238100653255` | `190480522604` |

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
