# RAPORT P3.7 R17A Replay-Readiness Failure Breakdown - 2026-05-24

## Verdict

Final decision: **SNAPSHOT_SIDE_PASS_GO_E1**

Recommended next path: `start_p3_7_e1_pumpfun_executable_route_support_matrix`

This audit is offline only. It does not change thresholds, policy, route support, runtime behavior, P2/live, or Phase B.

## Inputs

- decision log: `/root/Gho-r17-clean/logs/rollout/shadow-burnin-v3-p37-r17-replay-ready-diagnostic/decisions/shadow-burnin-v3-p37-r17-replay-ready-diagnostic/v2.2/legacy_live/1ddc1a9e03e0010ddc17c51d129bb890c1c79d4a967b9fbb8b31d7bf4dc13b6c/gatekeeper_v2_decisions.jsonl`
- config: `configs/rollout/shadow-burnin-v3-p37-r17-replay-ready-diagnostic.toml`
- required temporal targets: `[2000, 5000, 7000]`
- terminal target: `10000`
- max drift threshold: `2000 ms`

## Summary

- total rows: `44`
- temporal ready rows: `22`
- temporal not-ready rows: `22`
- runtime emission bug rows: `0`
- natural not-applicable rows: `22`
- contract too strict rows: `0`
- payload gap rows: `0`
- unknown rows: `0`
- missing 2000 snapshot rows: `8`
- missing 5000 snapshot rows: `17`
- missing 7000 snapshot rows: `7`
- missing terminal snapshot rows: `0`
- missing gate trace rows: `0`
- missing phase vector rows: `0`
- missing PDD diagnostics rows: `0`
- missing prosperity diagnostics rows: `0`
- missing HHI diagnostics rows: `0`
- early close before target rows: `7`
- timeout/no-data before target rows: `4`
- insufficient sample before target rows: `11`
- terminal-only rows: `0`
- unknown temporal readiness gap rows: `0`
- rows with snapshot drift above threshold: `44`

## Reason Counts

Counts below are for `gatekeeper_v2_replay_ready_temporal=false` rows.

| Reason | Rows |
| --- | --- |
| early_close_before_targets | 7 |
| insufficient_sample_before_target | 11 |
| timeout_no_data_before_targets | 4 |

## Root Cause Classes

Counts below are for `gatekeeper_v2_replay_ready_temporal=false` rows.

| Root cause class | Rows |
| --- | --- |
| natural_not_applicable | 22 |

## Snapshot Target Coverage

| Target ms | Rows with snapshot |
| --- | --- |
| 2000 | 36 |
| 5000 | 27 |
| 7000 | 37 |

Snapshot count stats:

```json
{
  "count": 44,
  "max": 4,
  "mean": 3.27,
  "min": 2,
  "p50": 4,
  "p90": 4
}
```

Snapshot drift stats:

```json
{
  "count": 100,
  "max": 5799,
  "mean": 1809.78,
  "min": 6,
  "p50": 1746,
  "p90": 3000
}
```

## Verdict Counts

| Verdict | Rows |
| --- | --- |
| BUY | 1 |
| REJECT_CORE_FAIL | 1 |
| REJECT_HARD_FAIL | 7 |
| TIMEOUT_PHASE1_INSUFFICIENT | 31 |
| TIMEOUT_PHASE1_NO_DATA | 4 |

## Interpretation

`temporal_ready=false` is not automatically a runtime bug. R17A separates:

- missing terminal/payload fields as runtime or payload problems;
- early terminal and no-data rows as natural non-applicability;
- sparse/no-data checkpoint rows as natural non-applicability unless payload or terminal data is missing.

L2D2 remains blocked unless a future manifest has both replay-ready inputs and an executable lifecycle-labeled denominator.
