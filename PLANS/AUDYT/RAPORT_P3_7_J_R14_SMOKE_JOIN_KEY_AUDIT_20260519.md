# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke.toml`
- readiness: `degraded`
- readiness_reasons: `["no_common_candidate_id_across_nonempty_artifacts"]`

## Artifact Coverage

| artifact | rows | candidate_id | ab_record_id | pool_id | mint | v3_payload | feature_hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `decision` | 1 | 0 | 1 | 1 | 1 | 1 | 1 |
| `decision` | 416 | 0 | 416 | 416 | 416 | 416 | 416 |
| `decision` | 150 | 0 | 150 | 150 | 150 | 150 | 150 |
| `shadow_entry` | 1 | 1 | 0 | 1 | 1 | 0 | 0 |
| `shadow_lifecycle` | 1 | 1 | 0 | 1 | 1 | 0 | 0 |
| `shadow_transport` | 1 | 1 | 0 | 0 | 1 | 0 | 0 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 6, "common_values": 0, "per_artifact_values": [1, 416, 150, 0, 0, 0]}`
- `candidate_id`: `{"artifacts_with_rows": 6, "common_values": 0, "per_artifact_values": [0, 0, 0, 1, 1, 1]}`
- `pool_id`: `{"artifacts_with_rows": 6, "common_values": 0, "per_artifact_values": [1, 416, 150, 1, 1, 0]}`
- `mint`: `{"artifacts_with_rows": 6, "common_values": 1, "per_artifact_values": [1, 416, 150, 1, 1, 1]}`

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
