# P3.7 Shadow Lifecycle Feature Availability

Feature availability status: `insufficient_for_selector`
Phase B possible: `false`
Phase B scope: `none`
V3 selector prototype possible: `false`
Reason: `feature coverage or class balance is below configured minimums`

## Inputs

- labels: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/p3_7_shadow_lifecycle_labels.jsonl`
- shadow_onchain_lifecycle: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/probe_shadow_onchain_lifecycle_report.jsonl`
- config: `configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1.toml`
- decision_logs: `["/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/decisions/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/v2.2/legacy_live/00b3d576e6ddfaefe5f738ef016d91e644fe3c67269a7cb058b29e4c75a2087d/gatekeeper_v2_buys.jsonl", "/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/decisions/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/v2.2/legacy_live/00b3d576e6ddfaefe5f738ef016d91e644fe3c67269a7cb058b29e4c75a2087d/gatekeeper_v2_decisions.jsonl"]`

## Label Counts

- `rows_total`: `39`
- `raw_shadow_onchain_rows_total`: `39`
- `buy_quality_class_counts`: `{"buy_quality_bad": 37, "buy_quality_dirty_good": 2}`
- `market_outcome_class_counts`: `{"market_bad_clean": 37, "market_good_clean": 2}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 1, "no_gatekeeper_context_rows": 38}`
- `close_reason_counts`: `{"StopLoss": 2, "Target": 2, "TimeStop": 35}`
- `join_quality_counts`: `{"ambiguous_matches": 1, "matched_by_ab_record_id": 39, "matched_by_candidate_id": 0, "matched_by_pool_mint": 0, "matched_by_pool_mint_time_window": 0, "matched_by_position_id": 0, "matched_by_time_window": 0, "unmatched": 0}`
- `rows_with_any_decision_time_features`: `{"all_rows": 39, "buy_quality_bad": 37, "buy_quality_dirty_good": 2, "close_reason_StopLoss": 2, "close_reason_Target": 2, "close_reason_TimeStop": 35, "gatekeeper_context_buy_quality_bad": 1, "gatekeeper_context_rows": 1, "no_gatekeeper_context_rows": 38, "truth_gap_clean": 3, "truth_gap_degraded_acceptable": 36}`
- `decision_rows_total`: `1978`
- `decision_log_row_counts`: `{"/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/decisions/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/v2.2/legacy_live/00b3d576e6ddfaefe5f738ef016d91e644fe3c67269a7cb058b29e4c75a2087d/gatekeeper_v2_buys.jsonl": 6, "/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/decisions/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/v2.2/legacy_live/00b3d576e6ddfaefe5f738ef016d91e644fe3c67269a7cb058b29e4c75a2087d/gatekeeper_v2_decisions.jsonl": 1972}`
- `matched_decision_identifier_presence`: `{"ab_record_id": 39, "config_hash": 39, "join_key": 39}`
- `matched_config_hash_counts`: `{"00b3d576e6ddfaefe5f738ef016d91e644fe3c67269a7cb058b29e4c75a2087d": 39}`
- `matched_decision_plane_counts`: `{"legacy_live": 39}`
- `matched_gatekeeper_version_counts`: `{"v2.2": 39}`
- `matched_log_kind_counts`: `{"buy": 1, "decision": 38}`
- `diagnostic_minimums`: `{"bad_with_features": 37, "dirty_good_with_features": 2, "gatekeeper_context_bad_with_features": 1, "gatekeeper_context_dirty_good_with_features": 0, "lifecycle_leakage_fields_in_decision_logs": 0, "min_feature_label_rows": 100, "min_temporal_split_class_rows": 20, "temporal_split_possible": false}`

## Feature Matrix

| feature_group | all rows | dirty_good | bad | gatekeeper_context_dirty_good | gatekeeper_context_bad |
| --- | ---: | ---: | ---: | ---: | ---: |
| `v3_materialized_feature_snapshot` | 39 | 2 | 37 | 0 | 1 |
| `v3_evidence_status` | 39 | 2 | 37 | 0 | 1 |
| `v3_organic_broadening` | 39 | 2 | 37 | 0 | 1 |
| `v3_manipulation_contradictions` | 39 | 2 | 37 | 0 | 1 |
| `v3_component_scores` | 39 | 2 | 37 | 0 | 1 |
| `v3_shadow_reason_code` | 39 | 2 | 37 | 0 | 1 |
| `v3_shadow_verdict` | 39 | 2 | 37 | 0 | 1 |
| `tx_intel_fields` | 39 | 2 | 37 | 0 | 1 |
| `checkpoint_features` | 39 | 2 | 37 | 0 | 1 |
| `account_features` | 39 | 2 | 37 | 0 | 1 |
| `curve_readiness` | 39 | 2 | 37 | 0 | 1 |
| `sybil_resistance` | 39 | 2 | 37 | 0 | 1 |
| `alpha_fingerprint` | 39 | 2 | 37 | 0 | 1 |
| `pdd_fields` | 39 | 2 | 37 | 0 | 1 |
| `tas_fields` | 39 | 2 | 37 | 0 | 1 |
| `dow_stage_fields` | 39 | 2 | 37 | 0 | 1 |
| `gatekeeper_v2_v25_phase_fields` | 39 | 2 | 37 | 0 | 1 |
| `v25_shadow_fields` | 39 | 2 | 37 | 0 | 1 |
| `legacy_decision_fields` | 39 | 2 | 37 | 0 | 1 |

## Temporal Split

- `temporal_split`: `{"possible": false, "reason": "insufficient_good_or_bad_in_temporal_half", "splits": {"early": {"buy_quality_class_counts": {"buy_quality_bad": 17, "buy_quality_dirty_good": 2}, "first_decision_ts_ms": 1779413340727, "last_decision_ts_ms": 1779415381334, "rows": 19}, "late": {"buy_quality_class_counts": {"buy_quality_bad": 20}, "first_decision_ts_ms": 1779415404433, "last_decision_ts_ms": 1779416906516, "rows": 20}}}`
- `match_time_delta_ms`: `{"count": 39, "max": 0.0, "mean": 0.0, "min": 0.0, "p50": 0.0, "p90": 0.0, "p99": 0.0}`

## Decision

- NO-GO: Phase B feature prototype with the current recovered dataset.
- Lifecycle labels are target labels only, not decision-time features.
- No P2/live/threshold/IWIM/live-sender change is authorized by this audit.
