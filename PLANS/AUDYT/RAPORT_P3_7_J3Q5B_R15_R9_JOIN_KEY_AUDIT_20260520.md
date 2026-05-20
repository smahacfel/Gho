# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r9-q5b.toml`
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
- decision_rows_with_ab_record_id: `68`
- shadow_transport_rows_with_ab_record_id: `0`
- shadow_entry_rows_with_ab_record_id: `0`
- shadow_lifecycle_rows_with_ab_record_id: `0`
- onchain_lifecycle_rows_with_ab_record_id: `0`
- probe_transport_rows_with_ab_record_id: `5`
- probe_entry_rows_with_ab_record_id: `5`
- probe_transport_rows_with_probe_id: `5`
- probe_entry_rows_with_probe_id: `5`

## Artifact Coverage

| artifact | rows | candidate_id | ab_record_id | probe_id | pool_id | mint | v3_payload | feature_hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `decision` | 54 | 0 | 54 | 0 | 54 | 54 | 54 | 54 |
| `decision` | 14 | 0 | 14 | 0 | 14 | 14 | 14 | 14 |
| `probe_entry` | 5 | 5 | 5 | 5 | 5 | 5 | 0 | 5 |
| `probe_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `probe_selection` | 50 | 50 | 50 | 50 | 50 | 50 | 0 | 50 |
| `probe_skip` | 49 | 49 | 49 | 49 | 49 | 49 | 0 | 49 |
| `probe_transport` | 5 | 5 | 5 | 5 | 5 | 5 | 0 | 5 |
| `shadow_entry` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_transport` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 2, "common_values": 14, "per_artifact_values": [54, 14]}`
- `probe_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `pool_id`: `{"artifacts_with_rows": 2, "common_values": 14, "per_artifact_values": [54, 14]}`
- `mint`: `{"artifacts_with_rows": 2, "common_values": 14, "per_artifact_values": [54, 14]}`

## Probe Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 3, "common_values": 5, "per_artifact_values": [5, 50, 5]}`
- `probe_id`: `{"artifacts_with_rows": 3, "common_values": 5, "per_artifact_values": [5, 50, 5]}`
- `candidate_id`: `{"artifacts_with_rows": 3, "common_values": 0, "per_artifact_values": [5, 50, 5]}`
- `pool_id`: `{"artifacts_with_rows": 3, "common_values": 5, "per_artifact_values": [5, 50, 5]}`
- `mint`: `{"artifacts_with_rows": 3, "common_values": 5, "per_artifact_values": [5, 50, 5]}`

## Probe Decision Join

- decision_join_acceptance: `pass`
- required_exact_decision_v3_join_coverage: `1.0`
- `probe_selection`: `{"exact_decision_v3_join": 50, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 50, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 50, "joined_to_decision_with_v3_payload": 50, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 14}, "policy_hash_match": 50, "policy_hash_mismatch": 0, "rows": 50, "rows_with_ab_record_id": 50, "unmatched_rows": 0}`
- `probe_transport`: `{"exact_decision_v3_join": 5, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 5, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 5, "joined_to_decision_with_v3_payload": 5, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 1}, "policy_hash_match": 5, "policy_hash_mismatch": 0, "rows": 5, "rows_with_ab_record_id": 5, "unmatched_rows": 0}`
- `probe_entry`: `{"exact_decision_v3_join": 5, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 5, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 5, "joined_to_decision_with_v3_payload": 5, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 1}, "policy_hash_match": 5, "policy_hash_mismatch": 0, "rows": 5, "rows_with_ab_record_id": 5, "unmatched_rows": 0}`
- `probe_lifecycle`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
