# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8k.toml`
- readiness: `not_ready`
- join_key_acceptance: `fail`
- join_quality: `pool_mint_time_window`
- probe_readiness: `ready_for_probe_transport_entry_join`
- probe_join_key_acceptance: `pass`
- probe_join_quality: `exact_probe_id_and_ab_record_id`
- probe_decision_join_acceptance: `pass`
- probe_required_exact_decision_v3_join_coverage: `1.0`
- full_chain_ab_record_id_coverage: `1.0`
- probe_chain_ab_record_id_coverage: `1.0`
- probe_chain_probe_id_coverage: `1.0`
- readiness_reasons: `["missing_shadow_transport_rows", "missing_shadow_entry_rows", "missing_shadow_lifecycle_rows"]`
- probe_readiness_reasons: `[]`
- decision_rows_with_ab_record_id: `33`
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
| `decision` | 23 | 0 | 23 | 0 | 23 | 23 | 23 | 23 |
| `decision` | 10 | 0 | 10 | 0 | 10 | 10 | 10 | 10 |
| `probe_entry` | 4 | 4 | 4 | 4 | 4 | 4 | 0 | 4 |
| `probe_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `probe_selection` | 23 | 23 | 23 | 23 | 23 | 23 | 0 | 23 |
| `probe_skip` | 17 | 17 | 17 | 17 | 17 | 17 | 0 | 17 |
| `probe_transport` | 4 | 4 | 4 | 4 | 4 | 4 | 0 | 4 |
| `shadow_entry` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_transport` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 2, "common_values": 10, "per_artifact_values": [23, 10]}`
- `probe_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `pool_id`: `{"artifacts_with_rows": 2, "common_values": 10, "per_artifact_values": [23, 10]}`
- `mint`: `{"artifacts_with_rows": 2, "common_values": 10, "per_artifact_values": [23, 10]}`

## Probe Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 3, "common_values": 4, "per_artifact_values": [4, 23, 4]}`
- `probe_id`: `{"artifacts_with_rows": 3, "common_values": 4, "per_artifact_values": [4, 23, 4]}`
- `candidate_id`: `{"artifacts_with_rows": 3, "common_values": 0, "per_artifact_values": [4, 23, 4]}`
- `pool_id`: `{"artifacts_with_rows": 3, "common_values": 4, "per_artifact_values": [4, 23, 4]}`
- `mint`: `{"artifacts_with_rows": 3, "common_values": 4, "per_artifact_values": [4, 23, 4]}`

## Probe Decision Join

- decision_join_acceptance: `pass`
- required_exact_decision_v3_join_coverage: `1.0`
- `probe_selection`: `{"exact_decision_v3_join": 23, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 23, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 23, "joined_to_decision_with_v3_payload": 23, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 10}, "policy_hash_match": 23, "policy_hash_mismatch": 0, "rows": 23, "rows_with_ab_record_id": 23, "unmatched_rows": 0}`
- `probe_transport`: `{"exact_decision_v3_join": 4, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 4, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 4, "joined_to_decision_with_v3_payload": 4, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 2}, "policy_hash_match": 4, "policy_hash_mismatch": 0, "rows": 4, "rows_with_ab_record_id": 4, "unmatched_rows": 0}`
- `probe_entry`: `{"exact_decision_v3_join": 4, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 4, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 4, "joined_to_decision_with_v3_payload": 4, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 2}, "policy_hash_match": 4, "policy_hash_mismatch": 0, "rows": 4, "rows_with_ab_record_id": 4, "unmatched_rows": 0}`
- `probe_lifecycle`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
