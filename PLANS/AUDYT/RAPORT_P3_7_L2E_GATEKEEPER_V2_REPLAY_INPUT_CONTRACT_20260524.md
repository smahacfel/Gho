# P3.7-L2E Gatekeeper V2 Replay Input Contract

## Verdict

- analysis_status: `pass`
- final_decision: `BLOCK_L2E_HISTORICAL_ROWS_MISSING_V22_REPLAY_CONTRACT`
- manifest_path: `PLANS/AUDYT/MANIFEST_P3_7_L1R21_L2_INPUT_DATASET_CONTRACT_20260524.json`
- recommended_next_path: `run_r17_replay_ready_diagnostic_after_l2e_instrumentation`

## Locked Denominator

- rows: `85`
- quality_counts: `{"buy_quality_bad": 81, "buy_quality_dirty_good": 4}`
- namespace_counts: `{"shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1": 42, "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1": 43}`
- dirty_good_rate: `0.0471`

## Replay Input Support

- schema_status_counts: `{"missing_v2_replay_contract": 85}`
- non_temporal_ready_counts: `{"not_ready": 85}`
- temporal_ready_counts: `{"not_ready": 85}`
- missing_contract_field_counts: `{"gatekeeper_v2_phase_pass_vector": 85, "gatekeeper_v2_replay_input_schema_version": 85, "gatekeeper_v2_replay_missing_fields": 85, "gatekeeper_v2_replay_ready_non_temporal": 85, "gatekeeper_v2_replay_ready_temporal": 85, "observed_mode": 85, "observed_stage": 85, "observed_window_ms": 85}`
- explicit_missing_field_counts: `{}`

## Axis Readiness

| axis | kind | status | ready_rows | blocked_rows |
| --- | --- | --- | ---: | ---: |
| `soft_pdd_instead_of_hard_pdd` | `non_temporal` | `unsupported_v2_replay_input_gap` | 0 | 85 |
| `prosperity_filter_disabled` | `non_temporal` | `unsupported_v2_replay_input_gap` | 0 | 85 |
| `hhi_hard_fail_relaxed` | `non_temporal` | `unsupported_v2_replay_input_gap` | 0 | 85 |
| `elapsed_aware_entry_drift` | `non_temporal` | `unsupported_v2_replay_input_gap` | 0 | 85 |
| `standard_mode_shorter_window` | `temporal` | `unsupported_temporal_snapshots_missing` | 0 | 85 |

## Contract

- runtime_schema_version: `1`
- non_temporal_axes: `["soft_pdd_instead_of_hard_pdd", "prosperity_filter_disabled", "hhi_hard_fail_relaxed", "elapsed_aware_entry_drift"]`
- temporal_axes: `["standard_mode_shorter_window"]`
- `standard_mode_shorter_window` remains blocked until `decision_eval_snapshots` are emitted.

## Interpretation

- L2E adds the future-run replay-input contract; it does not claim that historical L1R21 rows satisfy it.
- Full V3 replay payload is not sufficient for Gatekeeper V2 causal axis replay.
- Non-temporal axes require explicit V2 trace/phase/soft-budget/PDD/prosperity/HHI diagnostics at terminal decision time.
- Temporal standard-window replay requires explicit decision-evaluation snapshots, not final MFS snapshots.

## Non-Goals

- `no_runtime_started_by_this_script`
- `no_threshold_tuning`
- `no_phase_b`
- `no_p2_live`
- `no_new_runs_added_to_manifest`
- `no_causal_axis_claim`
