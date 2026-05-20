# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r6.toml`
- readiness: `not_ready`
- join_key_acceptance: `fail`
- join_quality: `pool_mint_time_window`
- probe_readiness: `not_ready`
- probe_join_key_acceptance: `fail`
- probe_join_quality: `exact_probe_id`
- probe_decision_join_acceptance: `fail`
- probe_required_exact_decision_v3_join_coverage: `1.0`
- full_chain_ab_record_id_coverage: `1.0`
- probe_chain_ab_record_id_coverage: `1.0`
- probe_chain_probe_id_coverage: `1.0`
- readiness_reasons: `["missing_shadow_transport_rows", "missing_shadow_entry_rows", "missing_shadow_lifecycle_rows"]`
- probe_readiness_reasons: `["missing_probe_transport_rows", "missing_probe_entry_rows", "probe_rows_missing_exact_decision_v3_join"]`
- decision_rows_with_ab_record_id: `1312`
- shadow_transport_rows_with_ab_record_id: `0`
- shadow_entry_rows_with_ab_record_id: `0`
- shadow_lifecycle_rows_with_ab_record_id: `0`
- onchain_lifecycle_rows_with_ab_record_id: `0`
- probe_transport_rows_with_ab_record_id: `0`
- probe_entry_rows_with_ab_record_id: `0`
- probe_transport_rows_with_probe_id: `0`
- probe_entry_rows_with_probe_id: `0`

## Artifact Coverage

| artifact | rows | candidate_id | ab_record_id | probe_id | pool_id | mint | v3_payload | feature_hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `decision` | 1062 | 0 | 1062 | 0 | 1062 | 1062 | 1062 | 1062 |
| `decision` | 250 | 0 | 250 | 0 | 250 | 250 | 250 | 250 |
| `probe_entry` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `probe_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `probe_selection` | 5 | 5 | 5 | 5 | 5 | 5 | 0 | 5 |
| `probe_skip` | 1062 | 1062 | 1062 | 1062 | 1062 | 1062 | 0 | 1062 |
| `probe_transport` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_entry` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_transport` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 2, "common_values": 250, "per_artifact_values": [1062, 250]}`
- `probe_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `pool_id`: `{"artifacts_with_rows": 2, "common_values": 250, "per_artifact_values": [1062, 250]}`
- `mint`: `{"artifacts_with_rows": 2, "common_values": 250, "per_artifact_values": [1062, 250]}`

## Probe Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 1, "common_values": 5, "per_artifact_values": [5]}`
- `probe_id`: `{"artifacts_with_rows": 1, "common_values": 5, "per_artifact_values": [5]}`
- `candidate_id`: `{"artifacts_with_rows": 1, "common_values": 5, "per_artifact_values": [5]}`
- `pool_id`: `{"artifacts_with_rows": 1, "common_values": 5, "per_artifact_values": [5]}`
- `mint`: `{"artifacts_with_rows": 1, "common_values": 5, "per_artifact_values": [5]}`

## Probe Decision Join

- decision_join_acceptance: `fail`
- required_exact_decision_v3_join_coverage: `1.0`
- `probe_selection`: `{"exact_decision_v3_join": 5, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 5, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 5, "joined_to_decision_with_v3_payload": 5, "mismatch_reasons": {}, "policy_hash_match": 5, "policy_hash_mismatch": 0, "rows": 5, "rows_with_ab_record_id": 5, "unmatched_rows": 0}`
- `probe_transport`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`
- `probe_entry`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`
- `probe_lifecycle`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
