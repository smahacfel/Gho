# P3.7-L2C Gatekeeper V2 Axis Replay Backend

## Verdict

- analysis_status: `pass`
- final_decision: `BLOCK_L2D_GATEKEEPER_V2_AXIS_REPLAY_INPUT_GAP`
- manifest_path: `PLANS/AUDYT/MANIFEST_P3_7_L1R21_L2_INPUT_DATASET_CONTRACT_20260524.json`
- recommended_next_path: `add_authoritative_gatekeeper_v2_axis_replay_payload_or_backend`

## Locked Denominator

- rows: `85`
- quality_counts: `{"buy_quality_bad": 81, "buy_quality_dirty_good": 4}`
- namespace_counts: `{"shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1": 42, "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1": 43}`
- dirty_good_rate: `0.0471`

## Replay Support

- v3_payload_counts: `{"full_v3_replay_payload": 85}`
- gate_trace_counts: `{"gate_trace_missing": 42, "gate_trace_present": 43}`
- baseline_parity_counts: `{"baseline_parity_gap": 44, "baseline_parity_ok": 41}`
- namespace_trace_counts: `{"shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1|no_trace": 42, "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1|trace": 43}`

## Axis Results

| axis | axis_status | evaluated_rows | unsupported_rows | accepted_dirty_good | accepted_bad |
| --- | --- | ---: | ---: | ---: | ---: |
| `soft_pdd_instead_of_hard_pdd` | `unsupported_missing_fields` | 0 | 85 | n/a | n/a |
| `prosperity_filter_disabled` | `unsupported_missing_fields` | 0 | 85 | n/a | n/a |
| `hhi_hard_fail_relaxed` | `unsupported_missing_fields` | 0 | 85 | n/a | n/a |
| `elapsed_aware_entry_drift` | `unsupported_missing_fields` | 0 | 85 | n/a | n/a |
| `standard_mode_shorter_window` | `unsupported_temporal_replay_required` | 0 | 85 | n/a | n/a |

## Row Status Counts Per Axis

- `soft_pdd_instead_of_hard_pdd`: `{"unsupported_axis_already_applied_in_source_run": 38, "unsupported_baseline_parity_gap": 2, "unsupported_missing_fields:gatekeeper_gate_trace": 42, "unsupported_missing_fields:pdd_hard_fail": 3}`
- `prosperity_filter_disabled`: `{"unsupported_axis_already_applied_in_source_run": 41, "unsupported_baseline_parity_gap": 2, "unsupported_missing_fields:gatekeeper_gate_trace": 42}`
- `hhi_hard_fail_relaxed`: `{"unsupported_axis_already_applied_in_source_run": 36, "unsupported_baseline_parity_gap": 2, "unsupported_missing_fields:gatekeeper_gate_trace": 31, "unsupported_missing_fields:gatekeeper_gate_trace,hhi,top3_volume_pct,same_ms_tx_ratio": 11, "unsupported_missing_fields:hhi,top3_volume_pct,same_ms_tx_ratio": 5}`
- `elapsed_aware_entry_drift`: `{"unsupported_axis_already_applied_in_source_run": 20, "unsupported_baseline_parity_gap": 2, "unsupported_missing_fields:gatekeeper_gate_trace,pdd_entry_drift_effective_max_pct,pdd_entry_drift_threshold_source": 18, "unsupported_missing_fields:gatekeeper_gate_trace,pdd_entry_drift_pct,pdd_entry_drift_effective_max_pct,pdd_entry_drift_threshold_source": 24, "unsupported_missing_fields:pdd_entry_drift_pct": 21}`
- `standard_mode_shorter_window`: `{"unsupported_temporal_replay_required": 85}`

## Baseline Parity Gap Examples

- `{"buy_quality_class": "buy_quality_bad", "hard_gate_fails": [], "namespace": "shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1", "observed_verdict": "TIMEOUT", "row_key": "8QPW9Z7w7mSJ9kusi7uLGehVbvuABur113HkHd5McXLq:1779399860657:1779399862657:TIMEOUT", "trace_baseline_verdict": "UNSUPPORTED"}`
- `{"buy_quality_class": "buy_quality_bad", "hard_gate_fails": [], "namespace": "shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1", "observed_verdict": "TIMEOUT", "row_key": "8AsQ6dCyexu72e8asSipURuLdshxex2NFXkTtcDmUJLt:1779400004120:1779400006120:TIMEOUT", "trace_baseline_verdict": "UNSUPPORTED"}`
- `{"buy_quality_class": "buy_quality_bad", "hard_gate_fails": [], "namespace": "shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1", "observed_verdict": "TIMEOUT", "row_key": "HXuy5ixqt5CQLw8yeS3xvvHZtNjqHh6PKaioubd6poJ5:1779400038390:1779400040390:TIMEOUT", "trace_baseline_verdict": "UNSUPPORTED"}`
- `{"buy_quality_class": "buy_quality_bad", "hard_gate_fails": [], "namespace": "shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1", "observed_verdict": "TIMEOUT", "row_key": "GMMw6ZXLG7qSzj5Wfe7c2aDiivN2BzQGKu53G3hwwoNK:1779400126032:1779400128032:TIMEOUT", "trace_baseline_verdict": "UNSUPPORTED"}`
- `{"buy_quality_class": "buy_quality_bad", "hard_gate_fails": [], "namespace": "shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1", "observed_verdict": "TIMEOUT", "row_key": "Fw7eBLkeyhxDanhiHxBbcMuWNDNi6iZw2v3kpU64z2UC:1779400204673:1779400206673:TIMEOUT", "trace_baseline_verdict": "UNSUPPORTED"}`
- `{"buy_quality_class": "buy_quality_bad", "hard_gate_fails": [], "namespace": "shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1", "observed_verdict": "TIMEOUT", "row_key": "BiroFXMmqbPqQiAnuetJdB15zq7iA38vwynX9NBy6W8r:1779400269531:1779400271531:TIMEOUT", "trace_baseline_verdict": "UNSUPPORTED"}`
- `{"buy_quality_class": "buy_quality_bad", "hard_gate_fails": [], "namespace": "shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1", "observed_verdict": "TIMEOUT", "row_key": "9pxM2wF18xMTwoKGNb5kZ5QWoPmPXnK4SKuWrYouhJ8b:1779400291988:1779400293988:TIMEOUT", "trace_baseline_verdict": "UNSUPPORTED"}`
- `{"buy_quality_class": "buy_quality_bad", "hard_gate_fails": [], "namespace": "shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1", "observed_verdict": "TIMEOUT", "row_key": "CLJRssuw2HW7dsFXNLNyiv8gLUrtzzbgBoPW2YNAqLnU:1779400326616:1779400328616:TIMEOUT", "trace_baseline_verdict": "UNSUPPORTED"}`
- `{"buy_quality_class": "buy_quality_bad", "hard_gate_fails": [], "namespace": "shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1", "observed_verdict": "TIMEOUT", "row_key": "ARkwFE4JAbVbgzoBAXrwBwsqMhXfU5oMBBuoD3yJ5eYZ:1779400616334:1779400618334:TIMEOUT", "trace_baseline_verdict": "UNSUPPORTED"}`
- `{"buy_quality_class": "buy_quality_bad", "hard_gate_fails": [], "namespace": "shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1", "observed_verdict": "TIMEOUT", "row_key": "GfmsdL6nD4o5Bmcpq5eJJ622voSZdU59a1Ab7BR1Eem7:1779400717920:1779400719920:TIMEOUT", "trace_baseline_verdict": "UNSUPPORTED"}`

## Interpretation

- All 85 denominator rows have full V3 replay payload, but that payload is not an authoritative Gatekeeper V2 axis replay contract.
- J4C denominator rows do not carry gatekeeper_gate_trace, so single-axis replay cannot prove the non-axis gates.
- Some R16-r1 rows have diagnostic gate traces that do not baseline-replay to the observed V2 verdict, so those traces cannot be used as the sole verdict engine.
- No diagnostic flag was promoted to a causal ablation result.

## Non-Goals

- `no_runtime`
- `no_new_runs`
- `no_threshold_tuning`
- `no_phase_b`
- `no_p2_live`
- `no_full_r16_route_universe`
