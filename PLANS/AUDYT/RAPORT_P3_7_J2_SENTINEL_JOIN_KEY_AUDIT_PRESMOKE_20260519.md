# P3.7-J MFS Lifecycle Join-Key Audit

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel.toml`
- readiness: `not_ready`
- join_quality: `unmatched`
- full_chain_ab_record_id_coverage: `0.0`
- readiness_reasons: `["missing_decision_rows", "missing_v3_replay_payload_rows", "missing_shadow_transport_rows", "missing_shadow_entry_rows", "missing_shadow_lifecycle_rows", "no_common_ab_record_id_across_nonempty_artifacts", "no_common_candidate_id_across_nonempty_artifacts"]`
- decision_rows_with_ab_record_id: `0`
- shadow_transport_rows_with_ab_record_id: `0`
- shadow_entry_rows_with_ab_record_id: `0`
- shadow_lifecycle_rows_with_ab_record_id: `0`
- onchain_lifecycle_rows_with_ab_record_id: `0`

## Artifact Coverage

| artifact | rows | candidate_id | ab_record_id | pool_id | mint | v3_payload | feature_hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `shadow_entry` | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_lifecycle` | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `shadow_transport` | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

## Cross-Artifact Intersections

- `ab_record_id`: `{"artifacts_with_rows": 0, "common_values": 0}`
- `candidate_id`: `{"artifacts_with_rows": 0, "common_values": 0}`
- `pool_id`: `{"artifacts_with_rows": 0, "common_values": 0}`
- `mint`: `{"artifacts_with_rows": 0, "common_values": 0}`

## Governance

- This audit measures join-key coverage only.
- It does not infer lifecycle truth, strategy edge, or live inclusion.
