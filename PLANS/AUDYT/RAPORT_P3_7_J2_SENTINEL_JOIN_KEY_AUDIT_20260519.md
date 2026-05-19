# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel.toml`
- readiness: `not_ready`
- join_quality: `exact_ab_record_id`
- full_chain_ab_record_id_coverage: `1.0`
- readiness_reasons: `["missing_shadow_transport_rows", "missing_shadow_entry_rows", "missing_shadow_lifecycle_rows"]`
- decision_rows_with_ab_record_id: `2880`
- shadow_transport_rows_with_ab_record_id: `0`
- shadow_entry_rows_with_ab_record_id: `0`
- shadow_lifecycle_rows_with_ab_record_id: `0`
- onchain_lifecycle_rows_with_ab_record_id: `0`

## Artifact Coverage

| artifact | rows | candidate_id | ab_record_id | pool_id | mint | v3_payload | feature_hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `decision` | 2375 | 0 | 2375 | 2375 | 2375 | 2375 | 2375 |
| `decision` | 505 | 0 | 505 | 505 | 505 | 505 | 505 |
| `shadow_entry` | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_transport` | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 2, "common_values": 505, "per_artifact_values": [2375, 505]}`
- `candidate_id`: `{"artifacts_with_rows": 2, "common_values": 0, "per_artifact_values": [0, 0]}`
- `pool_id`: `{"artifacts_with_rows": 2, "common_values": 505, "per_artifact_values": [2375, 505]}`
- `mint`: `{"artifacts_with_rows": 2, "common_values": 505, "per_artifact_values": [2375, 505]}`

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
