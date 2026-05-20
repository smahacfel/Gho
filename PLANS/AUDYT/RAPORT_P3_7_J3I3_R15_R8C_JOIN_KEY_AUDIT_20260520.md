# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8c.toml`
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
- decision_rows_with_ab_record_id: `56`
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
| `decision` | 48 | 0 | 48 | 0 | 48 | 48 | 48 | 48 |
| `decision` | 8 | 0 | 8 | 0 | 8 | 8 | 8 | 8 |
| `probe_entry` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `probe_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `probe_selection` | 33 | 33 | 33 | 33 | 33 | 33 | 0 | 33 |
| `probe_skip` | 46 | 46 | 46 | 46 | 46 | 46 | 0 | 46 |
| `probe_transport` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_entry` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_transport` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 2, "common_values": 8, "per_artifact_values": [48, 8]}`
- `probe_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `pool_id`: `{"artifacts_with_rows": 2, "common_values": 8, "per_artifact_values": [48, 8]}`
- `mint`: `{"artifacts_with_rows": 2, "common_values": 8, "per_artifact_values": [48, 8]}`

## Probe Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 1, "common_values": 33, "per_artifact_values": [33]}`
- `probe_id`: `{"artifacts_with_rows": 1, "common_values": 33, "per_artifact_values": [33]}`
- `candidate_id`: `{"artifacts_with_rows": 1, "common_values": 33, "per_artifact_values": [33]}`
- `pool_id`: `{"artifacts_with_rows": 1, "common_values": 33, "per_artifact_values": [33]}`
- `mint`: `{"artifacts_with_rows": 1, "common_values": 33, "per_artifact_values": [33]}`

## Probe Decision Join

- decision_join_acceptance: `fail`
- required_exact_decision_v3_join_coverage: `1.0`
- `probe_selection`: `{"exact_decision_v3_join": 33, "exact_decision_v3_join_coverage": 1.0, "feature_hash_match": 33, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 33, "joined_to_decision_with_v3_payload": 33, "mismatch_reasons": {"multiple_decision_rows_for_ab_record_id": 8}, "policy_hash_match": 33, "policy_hash_mismatch": 0, "rows": 33, "rows_with_ab_record_id": 33, "unmatched_rows": 0}`
- `probe_transport`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`
- `probe_entry`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`
- `probe_lifecycle`: `{"exact_decision_v3_join": 0, "exact_decision_v3_join_coverage": 0.0, "feature_hash_match": 0, "feature_hash_mismatch": 0, "joined_to_decision_by_ab_record_id": 0, "joined_to_decision_with_v3_payload": 0, "mismatch_reasons": {}, "policy_hash_match": 0, "policy_hash_mismatch": 0, "rows": 0, "rows_with_ab_record_id": 0, "unmatched_rows": 0}`

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
