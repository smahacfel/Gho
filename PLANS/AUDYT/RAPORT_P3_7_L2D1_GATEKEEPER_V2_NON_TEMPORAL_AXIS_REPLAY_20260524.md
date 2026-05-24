# P3.7-L2D1 Gatekeeper V2 Non-Temporal Axis Replay

## Verdict

- analysis_status: `pass`
- final_decision: `BLOCK_L2D1_GATEKEEPER_V2_AXIS_REPLAY_INPUT_GAP`
- manifest_path: `PLANS/AUDYT/MANIFEST_P3_7_L1R21_L2_INPUT_DATASET_CONTRACT_20260524.json`
- recommended_next_path: `emit_gatekeeper_v2_replay_contract_fields_for_baseline_rows`

## Locked Denominator

- rows: `85`
- quality_counts: `{"buy_quality_bad": 81, "buy_quality_dirty_good": 4}`
- namespace_counts: `{"shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1": 42, "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1": 43}`
- dirty_good_rate: `0.0471`

## Input Support

- gate_trace_counts: `{"gate_trace_missing": 42, "gate_trace_present": 43}`
- baseline_parity_counts: `{"baseline_parity_gap": 44, "baseline_parity_ok": 41}`
- namespace_parity_counts: `{"shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1|parity_gap": 42, "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1|parity_gap": 2, "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1|parity_ok": 41}`
- j4c_missing_gate_trace_rows: `42`
- r16_replayable_rows: `41`

## Axis Results

| axis | axis_status | axis_evaluable_rows | variant_buy_rows | accepted_dirty_good | accepted_bad |
| --- | --- | ---: | ---: | ---: | ---: |
| `soft_pdd_instead_of_hard_pdd` | `blocked_no_causal_replay_rows` | 0 | 0 | 0 | 0 |
| `prosperity_filter_disabled` | `blocked_no_causal_replay_rows` | 0 | 0 | 0 | 0 |
| `hhi_hard_fail_relaxed` | `blocked_no_causal_replay_rows` | 0 | 0 | 0 | 0 |
| `elapsed_aware_entry_drift` | `blocked_no_causal_replay_rows` | 0 | 0 | 0 | 0 |

## Row Status Counts Per Axis

- `soft_pdd_instead_of_hard_pdd`: `{"unsupported_axis_already_applied_in_source_run": 43, "unsupported_missing_fields:gatekeeper_gate_trace,pdd_soft_penalty_points": 42}`
- `prosperity_filter_disabled`: `{"unsupported_axis_already_applied_in_source_run": 43, "unsupported_missing_fields:gatekeeper_gate_trace": 42}`
- `hhi_hard_fail_relaxed`: `{"unsupported_axis_already_applied_in_source_run": 43, "unsupported_missing_fields:gatekeeper_gate_trace": 31, "unsupported_missing_fields:gatekeeper_gate_trace,hhi,top3_volume_pct,same_ms_tx_ratio": 11}`
- `elapsed_aware_entry_drift`: `{"unsupported_axis_already_applied_in_source_run": 43, "unsupported_missing_fields:gatekeeper_gate_trace,pdd_entry_drift_effective_max_pct,pdd_entry_drift_threshold_source": 18, "unsupported_missing_fields:gatekeeper_gate_trace,pdd_entry_drift_pct,pdd_entry_drift_effective_max_pct,pdd_entry_drift_threshold_source": 24}`

## Interpretation

- Causal forward axis replay requires baseline-source rows, complete V2 trace evidence, and baseline parity.
- R16-r1 rows are useful for trace coverage diagnostics, but they already contain the tested R16 bundle axes and cannot prove forward J4C-to-R16 causality by themselves.
- J4C rows are the needed baseline source for forward axis replay, but the current manifest rows do not carry Gatekeeper V2 gate traces.
- `standard_mode_shorter_window` remains unsupported until temporal snapshots are available.
- No diagnostic flag was promoted to a causal ablation result.

## Non-Goals

- `no_runtime`
- `no_new_runs`
- `no_threshold_tuning`
- `no_phase_b`
- `no_p2_live`
- `no_full_r16_route_universe`
