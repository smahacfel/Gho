# P3.7-I Diagnostic V2/V2.5 Feature Prototype - Buy Heavy Rerun

Diagnostic V2/V2.5 allowed: `true`
V3 selector prototype allowed: `false`
Runtime threshold recommendation allowed: `false`
Recommendation: `design_forward_v3_mfs_lifecycle_collection_run`
Reason: `Primary gatekeeper-context V2/V2.5 comparison has enough sample and at least moderate diagnostic separation.`

## Inputs

- `shadow_lifecycle_labels`: `logs/shadow_run/shadow-burnin-buy-heavy-rerun/p3_7_shadow_lifecycle_labels.jsonl`
- `feature_availability`: `logs/shadow_run/shadow-burnin-buy-heavy-rerun/p3_7_shadow_lifecycle_feature_availability.json`
- `decision_logs`: `['/root/Gho/logs/rollout/shadow-burnin-buy-heavy-rerun/v2.2/legacy_live/8c0371766e3bb9f8e001d5885967276a3ed1511560af47c424806920eac425fa/gatekeeper_v2_buys.jsonl', '/root/Gho/logs/rollout/shadow-burnin-buy-heavy-rerun/v2.2/legacy_live/8c0371766e3bb9f8e001d5885967276a3ed1511560af47c424806920eac425fa/gatekeeper_v2_decisions.jsonl', '/root/Gho/logs/rollout/shadow-burnin-buy-heavy-rerun/v2.5/v25_shadow/8c0371766e3bb9f8e001d5885967276a3ed1511560af47c424806920eac425fa/gatekeeper_v2_buys.jsonl', '/root/Gho/logs/rollout/shadow-burnin-buy-heavy-rerun/v2.5/v25_shadow/8c0371766e3bb9f8e001d5885967276a3ed1511560af47c424806920eac425fa/gatekeeper_v2_decisions.jsonl']`

## Joined Rows

- `labels_total`: `2386`
- `decision_rows_total`: `9062`
- `joined_feature_rows`: `738`
- `join_quality_counts`: `{"matched_by_pool_mint_time_window": 738, "unmatched": 1648}`
- `buy_quality_class_counts_joined`: `{"buy_quality_bad": 559, "buy_quality_dirty_good": 179}`
- `gatekeeper_context_counts_joined`: `{"gatekeeper_context_rows": 571, "no_gatekeeper_context_rows": 167}`
- `close_reason_counts_joined`: `{"StopLoss": 152, "Target": 154, "TimeStop": 432}`
- `truth_gap_counts_joined`: `{"truth_gap_clean": 262, "truth_gap_degraded_acceptable": 476}`

## Primary Comparison

- `n_good`: `154`
- `n_bad`: `417`
- `signal_level`: `moderate_diagnostic_signal`
- `top_auc_separation`: `0.194664`

### Top Numeric Features
- `flipper_presence_ratio`: auc=0.305336, sep=0.194664, rank_biserial=-0.389328, dir=bad_higher, overlap=0.490401, n_good=44, n_bad=161
- `max_tx_per_signer_observed`: auc=0.653828, sep=0.153828, rank_biserial=0.307655, dir=dirty_good_higher, overlap=0.724376, n_good=154, n_bad=417
- `max_single_tx_price_impact_pct_observed`: auc=0.647817, sep=0.147817, rank_biserial=0.295634, dir=dirty_good_higher, overlap=0.765331, n_good=154, n_bad=417
- `entry_drift_pct`: auc=0.623958, sep=0.123958, rank_biserial=0.247917, dir=dirty_good_higher, overlap=0.6775, n_good=100, n_bad=240
- `pdd_entry_drift_pct`: auc=0.623958, sep=0.123958, rank_biserial=0.247917, dir=dirty_good_higher, overlap=0.6775, n_good=100, n_bad=240
- `whale_reversal_ratio_top3`: auc=0.376078, sep=0.123922, rank_biserial=-0.247843, dir=bad_higher, overlap=0.78193, n_good=154, n_bad=417
- `jito_tip_intensity`: auc=0.619894, sep=0.119894, rank_biserial=0.239789, dir=dirty_good_higher, overlap=0.673544, n_good=150, n_bad=379
- `flip_ratio_10s`: auc=0.397357, sep=0.102643, rank_biserial=-0.205285, dir=bad_higher, overlap=0.704195, n_good=154, n_bad=417
- `early_slot_volume_dominance_buy`: auc=0.400067, sep=0.099933, rank_biserial=-0.199866, dir=bad_higher, overlap=0.698184, n_good=154, n_bad=417
- `sol_buy_ratio`: auc=0.59706, sep=0.09706, rank_biserial=0.19412, dir=dirty_good_higher, overlap=0.710766, n_good=154, n_bad=417

### Top Categorical Features
- `fingerprint_reason`: BUYER_PRE_BALANCE_CV_UNAVAILABLE,STATIC_FEE_MIN_BUYS,FLIPPER_MIN_WALLETS good=51 bad=91 or=1.775534; BUYER_PRE_BALANCE_CV_UNAVAILABLE,CU_CLUSTER_MIN_TX,STATIC_FEE_MIN_BUYS,FIXED_SIZE_MIN_BUYS,FLIPPER_MIN_WALLETS good=25 bad=75 or=0.893273; BUYER_PRE_BALANCE_CV_UNAVAILABLE good=8 bad=51 or=0.412903
- `dev_has_sold`: false good=114 bad=233 or=2.233881; true good=40 bad=184 or=0.447652
- `shadow_normal_verdict`: BUY good=108 bad=224 or=2.011136
- `v25_shadow_observation_stage`: Normal good=108 bad=224 or=2.011136; Early good=46 bad=193 or=0.497231
- `shadow_early_verdict`: BUY good=141 bad=382 or=0.972791; REJECT_PUMP_AND_DUMP good=12 bad=18 or=1.894263
- `dev_sold_within_3s`: false good=138 bad=341 or=1.880341; true good=16 bad=76 or=0.531819
- `pdd_soft_flags`: whale good=123 bad=366 or=0.550921
- `sybil_metric_degraded_reasons`: FSC_FUNDING_STREAM_UNAVAILABLE good=43 bad=164 or=0.60121; FSC_FUNDING_STREAM_UNAVAILABLE,SFD_INSUFFICIENT_BUYS,SFD_POSTBALANCE_UNAVAILABLE good=33 bad=57 or=1.728646; CPV_INSUFFICIENT_SIGNERS,DBIA_INSUFFICIENT_BUYERS,FSC_FUNDING_STREAM_UNAVAILABLE,SFD_INSUFFICIENT_BUYS,SFD_POSTBALANCE_UNAVAILABLE good=31 bad=59 or=1.536794

## Comparison Summary

| comparison | n_good | n_bad | signal | top_auc_sep |
| --- | ---: | ---: | --- | ---: |
| `gatekeeper_context_dirty_good_vs_bad` | 154 | 417 | `moderate_diagnostic_signal` | 0.194664 |
| `all_dirty_good_vs_bad` | 179 | 559 | `moderate_diagnostic_signal` | 0.189509 |
| `target_dirty_good_vs_stoploss_bad` | 154 | 152 | `moderate_diagnostic_signal` | 0.122992 |
| `timestop_dirty_good_vs_stoploss_bad` | 25 | 152 | `no_numeric_signal` | None |
| `truth_gap_clean_dirty_good_vs_bad` | 128 | 134 | `moderate_diagnostic_signal` | 0.114734 |
| `truth_gap_degraded_dirty_good_vs_bad` | 51 | 425 | `moderate_diagnostic_signal` | 0.158665 |
| `gatekeeper_context_truth_gap_clean_dirty_good_vs_bad` | 112 | 116 | `moderate_diagnostic_signal` | 0.114734 |
| `gatekeeper_context_truth_gap_degraded_dirty_good_vs_bad` | 42 | 301 | `moderate_diagnostic_signal` | 0.152291 |

## Feature Family Summary

| family | available_features | top_auc_sep | top_feature |
| --- | ---: | ---: | --- |
| `alpha_manipulation` | 9 | 0.194664 | `flipper_presence_ratio` |
| `concentration_sybil` | 9 | 0.153828 | `max_tx_per_signer_observed` |
| `dev` | 11 | 0.089367 | `dev_tx_ratio` |
| `gatekeeper_reason` | 9 | 0.0 | `None` |
| `market_curve` | 8 | 0.059983 | `price_change_ratio` |
| `other` | 73 | 0.147817 | `max_single_tx_price_impact_pct_observed` |
| `pdd` | 9 | 0.123958 | `pdd_entry_drift_pct` |
| `phase_fields` | 14 | 0.0 | `None` |
| `tas` | 2 | 0.0 | `None` |
| `timing` | 4 | 0.035915 | `shadow_early_elapsed_ms` |
| `tx_intel` | 12 | 0.09706 | `sol_buy_ratio` |

## Governance

- This is diagnostic V2/V2.5 feature analysis only.
- It is not a V3 selector prototype because recovered rows have `0` V3/MFS coverage.
- `close_reason`, PnL, truth gap, and curve finality are labels/stratifiers, not predictive features.
- No P2/live/runtime threshold/tuning/MFS extension is authorized by this report.
