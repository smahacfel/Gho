# P3.7 Shadow Lifecycle Feature Availability - Buy Heavy Rerun

Feature availability status: `lifecycle_only`
Phase B possible: `false`
Phase B scope: `none`
V3 selector prototype possible: `false`
Reason: `no decision-time feature rows joined to lifecycle labels`

## Inputs

- labels: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/p3_7_shadow_lifecycle_labels.jsonl`
- shadow_onchain_lifecycle: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/shadow_onchain_lifecycle_report.jsonl`
- config: `configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke.toml`
- decision_logs: `["/root/Gho/logs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/decisions/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/v2.2/legacy_live/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_buys.jsonl", "/root/Gho/logs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/decisions/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/v2.2/legacy_live/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl", "/root/Gho/logs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/decisions/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl"]`

## Label Counts

- `rows_total`: `0`
- `raw_shadow_onchain_rows_total`: `0`
- `buy_quality_class_counts`: `{}`
- `market_outcome_class_counts`: `{}`
- `gatekeeper_context_split`: `{}`
- `close_reason_counts`: `{}`
- `join_quality_counts`: `{"ambiguous_matches": 0, "matched_by_candidate_id": 0, "matched_by_pool_mint": 0, "matched_by_pool_mint_time_window": 0, "matched_by_position_id": 0, "matched_by_time_window": 0, "unmatched": 0}`
- `rows_with_any_decision_time_features`: `{}`
- `decision_rows_total`: `567`
- `decision_log_row_counts`: `{"/root/Gho/logs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/decisions/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/v2.2/legacy_live/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_buys.jsonl": 1, "/root/Gho/logs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/decisions/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/v2.2/legacy_live/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl": 416, "/root/Gho/logs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/decisions/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl": 150}`
- `matched_decision_identifier_presence`: `{}`
- `matched_config_hash_counts`: `{}`
- `matched_decision_plane_counts`: `{}`
- `matched_gatekeeper_version_counts`: `{}`
- `matched_log_kind_counts`: `{}`
- `diagnostic_minimums`: `{"bad_with_features": 0, "dirty_good_with_features": 0, "gatekeeper_context_bad_with_features": 0, "gatekeeper_context_dirty_good_with_features": 0, "lifecycle_leakage_fields_in_decision_logs": 0, "min_feature_label_rows": 100, "min_temporal_split_class_rows": 20, "temporal_split_possible": false}`

## Feature Matrix

| feature_group | all rows | dirty_good | bad | gatekeeper_context_dirty_good | gatekeeper_context_bad |
| --- | ---: | ---: | ---: | ---: | ---: |
| `v3_materialized_feature_snapshot` | 0 | 0 | 0 | 0 | 0 |
| `v3_evidence_status` | 0 | 0 | 0 | 0 | 0 |
| `v3_organic_broadening` | 0 | 0 | 0 | 0 | 0 |
| `v3_manipulation_contradictions` | 0 | 0 | 0 | 0 | 0 |
| `v3_component_scores` | 0 | 0 | 0 | 0 | 0 |
| `v3_shadow_reason_code` | 0 | 0 | 0 | 0 | 0 |
| `v3_shadow_verdict` | 0 | 0 | 0 | 0 | 0 |
| `tx_intel_fields` | 0 | 0 | 0 | 0 | 0 |
| `checkpoint_features` | 0 | 0 | 0 | 0 | 0 |
| `account_features` | 0 | 0 | 0 | 0 | 0 |
| `curve_readiness` | 0 | 0 | 0 | 0 | 0 |
| `sybil_resistance` | 0 | 0 | 0 | 0 | 0 |
| `alpha_fingerprint` | 0 | 0 | 0 | 0 | 0 |
| `pdd_fields` | 0 | 0 | 0 | 0 | 0 |
| `tas_fields` | 0 | 0 | 0 | 0 | 0 |
| `dow_stage_fields` | 0 | 0 | 0 | 0 | 0 |
| `gatekeeper_v2_v25_phase_fields` | 0 | 0 | 0 | 0 | 0 |
| `v25_shadow_fields` | 0 | 0 | 0 | 0 | 0 |
| `legacy_decision_fields` | 0 | 0 | 0 | 0 | 0 |

## Temporal Split

- `temporal_split`: `{"possible": false, "reason": "not_enough_feature_rows", "splits": {}}`
- `match_time_delta_ms`: `{"count": 0, "max": null, "mean": null, "min": null, "p50": null, "p90": null, "p99": null}`

## Decision

- NO-GO: Phase B feature prototype with the current recovered dataset.
- Lifecycle labels are target labels only, not decision-time features.
- No P2/live/threshold/IWIM/live-sender change is authorized by this audit.
