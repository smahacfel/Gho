# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8f.toml`
- readiness: `ready_for_lifecycle_feature_join`
- join_key_acceptance: `degraded`
- join_quality: `mint_only`
- probe_readiness: `not_ready`
- probe_join_key_acceptance: `fail`
- probe_join_quality: `exact_probe_id`
- probe_decision_join_acceptance: `fail`
- probe_required_exact_decision_v3_join_coverage: `1.0`
- full_chain_ab_record_id_coverage: `1.0`
- probe_chain_ab_record_id_coverage: `1.0`
- probe_chain_probe_id_coverage: `1.0`
- readiness_reasons: `[]`
- probe_readiness_reasons: `["missing_probe_transport_rows", "missing_probe_entry_rows", "probe_rows_missing_exact_decision_v3_join"]`
- decision_rows_with_ab_record_id: `92`
- shadow_transport_rows_with_ab_record_id: `1`
- shadow_entry_rows_with_ab_record_id: `1`
- shadow_lifecycle_rows_with_ab_record_id: `1`
- onchain_lifecycle_rows_with_ab_record_id: `0`
- probe_transport_rows_with_ab_record_id: `0`
- probe_entry_rows_with_ab_record_id: `0`
- probe_transport_rows_with_probe_id: `0`
- probe_entry_rows_with_probe_id: `0`

## Artifact Coverage

| artifact | rows | candidate_id | ab_record_id | probe_id | pool_id | mint | v3_payload | feature_hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `decision` | 1 | 0 | 1 | 0 | 1 | 1 | 1 | 1 |
| `decision` | 76 | 0 | 76 | 0 | 76 | 76 | 76 | 76 |
| `decision` | 15 | 0 | 15 | 0 | 15 | 15 | 15 | 15 |
| `probe_entry` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `probe_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `probe_selection` | 30 | 30 | 30 | 30 | 30 | 30 | 0 | 30 |
| `probe_skip` | 76 | 76 | 76 | 76 | 76 | 76 | 0 | 76 |
| `probe_transport` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_entry` | 1 | 1 | 1 | 0 | 1 | 1 | 0 | 1 |
| `shadow_lifecycle` | 1 | 1 | 1 | 0 | 1 | 1 | 0 | 1 |
| `shadow_transport` | 1 | 1 | 1 | 0 | 0 | 1 | 0 | 1 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 6, "common_values": 1, "per_artifact_values": [1, 76, 15, 1, 1, 1]}`
- `probe_id`: `{"artifacts_with_rows": 6, "common_values": 0, "per_artifact_values": [0, 0, 0, 0, 0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 6, "common_values": 0, "per_artifact_values": [0, 0, 0, 1, 1, 1]}`
- `pool_id`: `{"artifacts_with_rows": 6, "common_values": 0, "per_artifact_values": [1, 76, 15, 1, 1, 0]}`
- `mint`: `{"artifacts_with_rows": 6, "common_values": 1, "per_artifact_values": [1, 76, 15, 1, 1, 1]}`

## Probe Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 1, "common_values": 30, "per_artifact_values": [30]}`
- `probe_id`: `{"artifacts_with_rows": 1, "common_values": 30, "per_artifact_values": [30]}`
- `candidate_id`: `{"artifacts_with_rows": 1, "common_values": 30, "per_artifact_values": [30]}`
- `pool_id`: `{"artifacts_with_rows": 1, "common_values": 30, "per_artifact_values": [30]}`
- `mint`: `{"artifacts_with_rows": 1, "common_values": 30, "per_artifact_values": [30]}`

## Probe Decision Join

- decision_join_acceptance: `fail`
- required_exact_decision_v3_join_coverage: `1.0`
- `probe_selection`: `{"exact_decision_v3_join": 30, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 30, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 30, "joined_to_decision_with_v3_payload": 30, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 13}, "policy_hash_match": 30, "policy_hash_mismatch": 0, "rows": 30, "rows_with_ab_record_id": 30, "unmatched_rows": 0}`
- `probe_transport`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`
- `probe_entry`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`
- `probe_lifecycle`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
