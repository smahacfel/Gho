# P3.7 Shadow Lifecycle Feature Availability - Buy Heavy Rerun

Feature availability status: `v2_features_available`
Phase B possible: `true`
Phase B scope: `diagnostic_v2_v25_feature_prototype_only`
V3 selector prototype possible: `false`
Reason: `V2/V2.5, tx-intel, and PDD/checkpoint features are available for gatekeeper-context dirty_good and bad rows; V3 selector remains blocked without MFS`

## Inputs

- labels: `logs/shadow_run/shadow-burnin-buy-heavy-rerun/p3_7_shadow_lifecycle_labels.jsonl`
- shadow_onchain_lifecycle: `logs/shadow_run/shadow-burnin-buy-heavy-rerun/shadow_onchain_lifecycle_report_all.jsonl`
- config: `configs/rollout/shadow-burnin-buy-heavy-rerun-report-only.toml`
- decision_logs: `["/root/Gho/logs/rollout/shadow-burnin-buy-heavy-rerun/v2.2/legacy_live/8c0371766e3bb9f8e001d5885967276a3ed1511560af47c424806920eac425fa/gatekeeper_v2_buys.jsonl", "/root/Gho/logs/rollout/shadow-burnin-buy-heavy-rerun/v2.2/legacy_live/8c0371766e3bb9f8e001d5885967276a3ed1511560af47c424806920eac425fa/gatekeeper_v2_decisions.jsonl", "/root/Gho/logs/rollout/shadow-burnin-buy-heavy-rerun/v2.5/v25_shadow/8c0371766e3bb9f8e001d5885967276a3ed1511560af47c424806920eac425fa/gatekeeper_v2_buys.jsonl", "/root/Gho/logs/rollout/shadow-burnin-buy-heavy-rerun/v2.5/v25_shadow/8c0371766e3bb9f8e001d5885967276a3ed1511560af47c424806920eac425fa/gatekeeper_v2_decisions.jsonl"]`

## Label Counts

- `rows_total`: `2386`
- `raw_shadow_onchain_rows_total`: `2386`
- `buy_quality_class_counts`: `{"buy_quality_bad": 1807, "buy_quality_dirty_good": 579}`
- `market_outcome_class_counts`: `{"market_bad_clean": 1807, "market_good_clean": 579}`
- `gatekeeper_context_split`: `{"gatekeeper_context_rows": 571, "no_gatekeeper_context_rows": 1815}`
- `close_reason_counts`: `{"StopLoss": 497, "Target": 500, "TimeStop": 1389}`
- `join_quality_counts`: `{"ambiguous_matches": 0, "matched_by_candidate_id": 0, "matched_by_pool_mint": 0, "matched_by_pool_mint_time_window": 738, "matched_by_position_id": 0, "matched_by_time_window": 738, "unmatched": 1648}`
- `rows_with_any_decision_time_features`: `{"all_rows": 738, "buy_quality_bad": 559, "buy_quality_dirty_good": 179, "close_reason_StopLoss": 152, "close_reason_Target": 154, "close_reason_TimeStop": 432, "gatekeeper_context_buy_quality_bad": 417, "gatekeeper_context_buy_quality_dirty_good": 154, "gatekeeper_context_rows": 571, "no_gatekeeper_context_rows": 167, "truth_gap_clean": 262, "truth_gap_degraded_acceptable": 476}`
- `decision_rows_total`: `9062`
- `decision_log_row_counts`: `{"/root/Gho/logs/rollout/shadow-burnin-buy-heavy-rerun/v2.2/legacy_live/8c0371766e3bb9f8e001d5885967276a3ed1511560af47c424806920eac425fa/gatekeeper_v2_buys.jsonl": 1922, "/root/Gho/logs/rollout/shadow-burnin-buy-heavy-rerun/v2.2/legacy_live/8c0371766e3bb9f8e001d5885967276a3ed1511560af47c424806920eac425fa/gatekeeper_v2_decisions.jsonl": 2826, "/root/Gho/logs/rollout/shadow-burnin-buy-heavy-rerun/v2.5/v25_shadow/8c0371766e3bb9f8e001d5885967276a3ed1511560af47c424806920eac425fa/gatekeeper_v2_buys.jsonl": 1488, "/root/Gho/logs/rollout/shadow-burnin-buy-heavy-rerun/v2.5/v25_shadow/8c0371766e3bb9f8e001d5885967276a3ed1511560af47c424806920eac425fa/gatekeeper_v2_decisions.jsonl": 2826}`
- `matched_decision_identifier_presence`: `{"ab_record_id": 738, "config_hash": 738, "join_key": 738}`
- `matched_config_hash_counts`: `{"8c0371766e3bb9f8e001d5885967276a3ed1511560af47c424806920eac425fa": 738}`
- `matched_decision_plane_counts`: `{"v25_shadow": 738}`
- `matched_gatekeeper_version_counts`: `{"v2.5": 738}`
- `matched_log_kind_counts`: `{"buy": 571, "decision": 167}`
- `diagnostic_minimums`: `{"bad_with_features": 559, "dirty_good_with_features": 179, "gatekeeper_context_bad_with_features": 417, "gatekeeper_context_dirty_good_with_features": 154, "lifecycle_leakage_fields_in_decision_logs": 0, "min_feature_label_rows": 100, "min_temporal_split_class_rows": 20, "temporal_split_possible": true}`

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
| `tx_intel_fields` | 738 | 179 | 559 | 154 | 417 |
| `checkpoint_features` | 738 | 179 | 559 | 154 | 417 |
| `account_features` | 738 | 179 | 559 | 154 | 417 |
| `curve_readiness` | 738 | 179 | 559 | 154 | 417 |
| `sybil_resistance` | 738 | 179 | 559 | 154 | 417 |
| `alpha_fingerprint` | 738 | 179 | 559 | 154 | 417 |
| `pdd_fields` | 738 | 179 | 559 | 154 | 417 |
| `tas_fields` | 738 | 179 | 559 | 154 | 417 |
| `dow_stage_fields` | 738 | 179 | 559 | 154 | 417 |
| `gatekeeper_v2_v25_phase_fields` | 738 | 179 | 559 | 154 | 417 |
| `v25_shadow_fields` | 738 | 179 | 559 | 154 | 417 |
| `legacy_decision_fields` | 738 | 179 | 559 | 154 | 417 |

## Temporal Split

- `temporal_split`: `{"possible": true, "reason": "ok", "splits": {"early": {"buy_quality_class_counts": {"buy_quality_bad": 270, "buy_quality_dirty_good": 99}, "first_decision_ts_ms": 1778104340181, "last_decision_ts_ms": 1778107543971, "rows": 369}, "late": {"buy_quality_class_counts": {"buy_quality_bad": 289, "buy_quality_dirty_good": 80}, "first_decision_ts_ms": 1778107551521, "last_decision_ts_ms": 1778118240423, "rows": 369}}}`
- `match_time_delta_ms`: `{"count": 738, "max": 449.0, "mean": 73.75880758807588, "min": 55.0, "p50": 66.0, "p90": 86.0, "p99": 264.3199999999997}`

## Decision

- GO: diagnostic V2/V2.5 feature analysis on gatekeeper-context rows.
- NO-GO: V3 selector prototype until V3/MFS payload coverage exists.
- Lifecycle labels are target labels only, not decision-time features.
- No P2/live/threshold/IWIM/live-sender change is authorized by this audit.
