# P3.7-L2B Manifest-Locked Offline Axis Ablation

## Verdict

- status: `blocked`
- final_decision: `BLOCK_L2B_AXIS_REPLAY_UNSUPPORTED`
- manifest_path: `PLANS/AUDYT/MANIFEST_P3_7_L1R21_L2_INPUT_DATASET_CONTRACT_20260524.json`
- recommended_next_path: `implement_manifest_locked_gatekeeper_v2_axis_replay_backend`
- blocker: `BLOCK_L2B_AXIS_REPLAY_UNSUPPORTED`

## Locked Denominator

- rows: `85`
- quality_counts: `{"buy_quality_bad": 81, "buy_quality_dirty_good": 4}`
- namespace_counts: `{"shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1": 42, "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1": 43}`
- small_sample_directional_only: `True`

## Replay Payload Gate

- ablation_evaluable_rows: `85`
- ablation_not_evaluable_rows: `0`
- status_counts: `{"full_payload_available": 85}`

## Counterfactual Axis Support

- requested_axes: `["standard_mode_shorter_window", "soft_pdd_instead_of_hard_pdd", "prosperity_filter_disabled", "hhi_hard_fail_relaxed", "elapsed_aware_entry_drift"]`
- supported_axes: `[]`
- unsupported_axes: `["standard_mode_shorter_window", "soft_pdd_instead_of_hard_pdd", "prosperity_filter_disabled", "hhi_hard_fail_relaxed", "elapsed_aware_entry_drift"]`
- reason: Existing replay payloads are full, but no deterministic Gatekeeper V2 axis replay backend exists for the requested L2B axes.

## Variant Results

| variant | mode/status | accepted_dirty_good | accepted_bad | dirty_good_precision |
| --- | --- | ---: | ---: | ---: |
| `A0_j4c_baseline` | `observed_anchor_not_counterfactual` | 0 | 42 | 0.0000 |
| `Afull_r16_r1_bundle` | `observed_anchor_not_counterfactual` | 4 | 39 | 0.0930 |
| `combined_allowed_subset` | `observed_dataset_summary_not_counterfactual` | 4 | 81 | 0.0471 |
| `A1_standard_mode_shorter_window_only` | `not_evaluated_axis_replay_unsupported` | n/a | n/a | n/a |
| `A2_soft_pdd_instead_of_hard_pdd_only` | `not_evaluated_axis_replay_unsupported` | n/a | n/a | n/a |
| `A3_prosperity_filter_disabled_only` | `not_evaluated_axis_replay_unsupported` | n/a | n/a | n/a |
| `A4_hhi_hard_fail_relaxed_only` | `not_evaluated_axis_replay_unsupported` | n/a | n/a | n/a |
| `A5_elapsed_aware_entry_drift_only` | `not_evaluated_axis_replay_unsupported` | n/a | n/a | n/a |

## Diagnostic Axis Matrix

- `standard_mode_shorter_window`: `{"bad_flagged_rows": 0, "bad_unflagged_rows": 0, "dirty_good_flagged_rows": 0, "dirty_good_unflagged_rows": 0, "mode": "diagnostic_flag_not_counterfactual", "unknown_rows": 85}`
- `soft_pdd_instead_of_hard_pdd`: `{"bad_flagged_rows": 79, "bad_unflagged_rows": 2, "dirty_good_flagged_rows": 3, "dirty_good_unflagged_rows": 1, "mode": "diagnostic_flag_not_counterfactual", "unknown_rows": 0}`
- `prosperity_filter_disabled`: `{"bad_flagged_rows": 77, "bad_unflagged_rows": 4, "dirty_good_flagged_rows": 1, "dirty_good_unflagged_rows": 3, "mode": "diagnostic_flag_not_counterfactual", "unknown_rows": 0}`
- `hhi_hard_fail_relaxed`: `{"bad_flagged_rows": 1, "bad_unflagged_rows": 80, "dirty_good_flagged_rows": 0, "dirty_good_unflagged_rows": 4, "mode": "diagnostic_flag_not_counterfactual", "unknown_rows": 0}`
- `elapsed_aware_entry_drift`: `{"bad_flagged_rows": 4, "bad_unflagged_rows": 77, "dirty_good_flagged_rows": 1, "dirty_good_unflagged_rows": 3, "mode": "diagnostic_flag_not_counterfactual", "unknown_rows": 0}`

## Interpretation

- L2B did not produce causal axis ablation because the requested Gatekeeper V2 policy axes are not supported by a deterministic replay backend.
- Observed anchors remain useful: J4C has 0 dirty_good and R16-r1 has 4 dirty_good on the locked executable subset.
- The diagnostic matrix is only directional evidence and must not be used for threshold tuning.

## Non-Goals

- `no_runtime`
- `no_new_runs`
- `no_threshold_tuning`
- `no_phase_b`
- `no_p2_live`
- `no_full_r16_route_universe`
