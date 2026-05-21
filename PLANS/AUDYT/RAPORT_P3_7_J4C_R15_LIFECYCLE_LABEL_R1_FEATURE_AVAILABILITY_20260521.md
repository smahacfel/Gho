# P3.7 Shadow Lifecycle Feature Availability

Feature availability status: `insufficient_for_selector`
Phase B possible: `false`
Phase B scope: `none`
V3 selector prototype possible: `false`
Reason: `feature coverage or class balance is below configured minimums`

## Inputs

- labels: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/p3_7_probe_shadow_lifecycle_labels.jsonl`
- shadow_onchain_lifecycle: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/probe_shadow_onchain_lifecycle_report.jsonl`
- config: `configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1.toml`
- decision_logs: `["/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/decisions/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/v2.2/legacy_live/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_buys.jsonl", "/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/decisions/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/v2.2/legacy_live/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl", "/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/decisions/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl"]`

## Label Counts

- `rows_total`: `42`
- `raw_shadow_onchain_rows_total`: `42`
- `buy_quality_class_counts`: `{"buy_quality_bad": 42}`
- `market_outcome_class_counts`: `{"market_bad_clean": 42}`
- `gatekeeper_context_split`: `{"no_gatekeeper_context_rows": 42}`
- `close_reason_counts`: `{"TimeStop": 42}`
- `join_quality_counts`: `{"ambiguous_matches": 0, "matched_by_ab_record_id": 42, "matched_by_candidate_id": 0, "matched_by_pool_mint": 0, "matched_by_pool_mint_time_window": 0, "matched_by_position_id": 0, "matched_by_time_window": 0, "unmatched": 0}`
- `rows_with_any_decision_time_features`: `{"all_rows": 42, "buy_quality_bad": 42, "close_reason_TimeStop": 42, "no_gatekeeper_context_rows": 42, "truth_gap_degraded_acceptable": 32, "truth_gap_too_large": 10}`
- `decision_rows_total`: `2344`
- `decision_log_row_counts`: `{"/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/decisions/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/v2.2/legacy_live/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_buys.jsonl": 2, "/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/decisions/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/v2.2/legacy_live/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl": 2075, "/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/decisions/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl": 267}`
- `matched_decision_identifier_presence`: `{"ab_record_id": 42, "config_hash": 42, "join_key": 42}`
- `matched_config_hash_counts`: `{"eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581": 42}`
- `matched_decision_plane_counts`: `{"legacy_live": 42}`
- `matched_gatekeeper_version_counts`: `{"v2.2": 42}`
- `matched_log_kind_counts`: `{"decision": 42}`
- `diagnostic_minimums`: `{"bad_with_features": 42, "dirty_good_with_features": 0, "gatekeeper_context_bad_with_features": 0, "gatekeeper_context_dirty_good_with_features": 0, "lifecycle_leakage_fields_in_decision_logs": 0, "min_feature_label_rows": 100, "min_temporal_split_class_rows": 20, "temporal_split_possible": false}`

## Feature Matrix

| feature_group | all rows | dirty_good | bad | gatekeeper_context_dirty_good | gatekeeper_context_bad |
| --- | ---: | ---: | ---: | ---: | ---: |
| `v3_materialized_feature_snapshot` | 42 | 0 | 42 | 0 | 0 |
| `v3_evidence_status` | 42 | 0 | 42 | 0 | 0 |
| `v3_organic_broadening` | 42 | 0 | 42 | 0 | 0 |
| `v3_manipulation_contradictions` | 42 | 0 | 42 | 0 | 0 |
| `v3_component_scores` | 42 | 0 | 42 | 0 | 0 |
| `v3_shadow_reason_code` | 42 | 0 | 42 | 0 | 0 |
| `v3_shadow_verdict` | 42 | 0 | 42 | 0 | 0 |
| `tx_intel_fields` | 42 | 0 | 42 | 0 | 0 |
| `checkpoint_features` | 42 | 0 | 42 | 0 | 0 |
| `account_features` | 42 | 0 | 42 | 0 | 0 |
| `curve_readiness` | 42 | 0 | 42 | 0 | 0 |
| `sybil_resistance` | 42 | 0 | 42 | 0 | 0 |
| `alpha_fingerprint` | 42 | 0 | 42 | 0 | 0 |
| `pdd_fields` | 42 | 0 | 42 | 0 | 0 |
| `tas_fields` | 42 | 0 | 42 | 0 | 0 |
| `dow_stage_fields` | 42 | 0 | 42 | 0 | 0 |
| `gatekeeper_v2_v25_phase_fields` | 42 | 0 | 42 | 0 | 0 |
| `v25_shadow_fields` | 42 | 0 | 42 | 0 | 0 |
| `legacy_decision_fields` | 42 | 0 | 42 | 0 | 0 |

## Temporal Split

- `temporal_split`: `{"possible": false, "reason": "insufficient_good_or_bad_in_temporal_half", "splits": {"early": {"buy_quality_class_counts": {"buy_quality_bad": 21}, "first_decision_ts_ms": 1779399862657, "last_decision_ts_ms": 1779401693875, "rows": 21}, "late": {"buy_quality_class_counts": {"buy_quality_bad": 21}, "first_decision_ts_ms": 1779401710932, "last_decision_ts_ms": 1779402479909, "rows": 21}}}`
- `match_time_delta_ms`: `{"count": 42, "max": 0.0, "mean": 0.0, "min": 0.0, "p50": 0.0, "p90": 0.0, "p99": 0.0}`

## Decision

- NO-GO: Phase B feature prototype with the current recovered dataset.
- Lifecycle labels are target labels only, not decision-time features.
- No P2/live/threshold/IWIM/live-sender change is authorized by this audit.
