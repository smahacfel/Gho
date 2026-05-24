# P3.7-R17 Replay-Ready Diagnostic Run Plan

## Status

Planned, not started.

R17 is the first future diagnostic run intended to produce Gatekeeper V2 axis
replay input. It must not be treated as another R16 smoke. Its purpose is to
collect rows that are replay-ready for causal L2 analysis.

## Context

L1R21 locked the historical executable subset:

- J4C denominator: 42 rows, 0 dirty_good
- R16-r1 denominator: 43 rows, 4 dirty_good
- combined denominator: 85 rows, 4 dirty_good

L2A confirmed a directional policy signal. L2B, L2C, and L2D1 correctly blocked
causal axis ablation because the historical rows do not carry enough Gatekeeper
V2 replay input. L2E added the future-run replay-input contract fields, but
historical rows still remain non-replayable.

The critical distinction is:

```text
full V3 replay payload != sufficient Gatekeeper V2 axis replay input
```

R17 exists to collect the missing Gatekeeper V2 replay input at decision time.

## Non-Goals

- no threshold tuning
- no policy promotion
- no Phase B
- no P2/live
- no full R16 universe claim
- no causal axis claim before replay-readiness and baseline parity pass
- no runtime start before R17 preflight passes

## Run Profile

Tracked config:

```text
configs/rollout/shadow-burnin-v3-p37-r17-replay-ready-diagnostic.toml
```

Namespace:

```text
shadow-burnin-v3-p37-r17-replay-ready-diagnostic
```

Safety profile:

- `[execution].execution_mode = "shadow"`
- `[trigger].entry_mode = "shadow_only"`
- `[trigger.shadow_run].payer_strategy = "ephemeral"`
- no live/P2 behavior
- bounded probe cap
- `max_concurrent = 1`
- append disabled
- unique namespace required

Policy profile:

- use the existing R16/L1 standard-softPDD Ghost brain config
- do not change PDD thresholds
- do not change HHI/prosperity/IWIM/Gatekeeper thresholds
- do not alter probe amount or slippage

## Required Replay Input

Every terminal decision row must carry the additive Gatekeeper V2 replay input
contract:

- `gatekeeper_v2_replay_input_schema_version`
- `gatekeeper_v2_replay_ready_non_temporal`
- `gatekeeper_v2_replay_ready_temporal`
- `gatekeeper_v2_replay_missing_fields`
- `gatekeeper_gate_trace`
- `gatekeeper_first_kill_gate`
- `gatekeeper_terminal_gate`
- `gatekeeper_v2_phase_pass_vector`
- `hard_reject_reason`
- `soft_points`
- `max_soft_points`
- `pdd_hard_fail`
- `pdd_soft_penalty_points`
- `pdd_entry_drift_pct`
- `pdd_entry_drift_effective_max_pct`
- `pdd_entry_drift_threshold_source`
- `prosperity_enabled`
- `prosperity_pass`
- `prosperity_reject_trigger`
- `hhi`
- `hard_fail_hhi_threshold`
- `max_hhi`
- `phase3_passed`
- `observed_mode`
- `observed_window_ms`
- `observed_stage`

## Temporal Snapshot Contract

The `standard_mode_shorter_window` axis remains unsupported unless R17 emits
decision evaluation snapshots.

Minimum snapshot checkpoints:

- 2s
- 5s
- 7s
- terminal/deadline

Each snapshot should include:

- `eval_ts_ms`
- `elapsed_ms`
- `stage`
- `materialized_feature_snapshot_hash`
- compact feature payload or full MFS payload
- `gate_trace`
- `phase_pass_vector`
- PDD diagnostics
- `verdict_if_evaluated`
- `reason_if_evaluated`

If `decision_eval_snapshots` are not emitted by runtime, R17 preflight must
block the run. A config marker alone is not enough.

## Preflight

Preflight script:

```text
scripts/v3_p37_r17_replay_ready_preflight.py
```

Required command before any R17 runtime:

```bash
python3 scripts/v3_p37_r17_replay_ready_preflight.py \
  --config configs/rollout/shadow-burnin-v3-p37-r17-replay-ready-diagnostic.toml \
  --json
```

Preflight must fail closed if:

- execution mode is not shadow
- trigger entry mode is not shadow_only
- shadow payer is not ephemeral
- P37 probe is disabled
- V3 replay payload is disabled
- required P37 hash/identity contracts are disabled
- R17 replay contract marker is missing
- non-temporal replay fields are not emitted by current runtime code
- temporal `decision_eval_snapshots` are required but not emitted by current runtime code
- namespace does not match the R17 namespace
- output paths mix another namespace

## Current Expected Gate

At the time this plan is added, the current L2E runtime code emits the v22
non-temporal replay contract, but `decision_eval_snapshots` are still emitted as
`None`.

Therefore the expected preflight result is:

```text
BLOCK_R17_TEMPORAL_SNAPSHOT_RUNTIME_GAP
```

This is intentional. R17 must not start until temporal snapshot emission is
implemented or the run is explicitly scoped as non-temporal-only.

## Reports After A Passing Run

Only after preflight passes and R17 completes:

```bash
python3 scripts/v3_shadow_report.py --config <r17-config> --json
python3 scripts/v3_full_replay_report.py --config <r17-config> --strict --json
python3 scripts/v3_p37_mfs_lifecycle_join_key_audit.py --config <r17-config> --json
python3 scripts/v3_p37_l1_reject_diagnostics.py --config <r17-config> --json
python3 scripts/v3_p37_l2e_gatekeeper_v2_replay_readiness.py --json
```

If lifecycle rows exist, also run the lifecycle/on-chain/feature availability
reports.

## Acceptance

R17 is acceptable as replay-ready input only if:

- strict replay passes
- diagnostic quality does not regress
- identity/hash contract passes
- execution feasibility is reported separately from decision quality
- non-executable rows remain out of buy-quality denominator
- `gatekeeper_v2_replay_ready_non_temporal = true` for replay candidate rows
- `gatekeeper_v2_replay_ready_temporal = true` only when decision snapshots are present
- baseline parity can be evaluated by L2D2

## Next Decision

If R17 produces replay-ready rows with lifecycle labels:

```text
GO_L2D2_MANIFEST_LOCKED_AXIS_REPLAY
```

If R17 remains blocked by temporal snapshot emission:

```text
IMPLEMENT_TEMPORAL_DECISION_EVAL_SNAPSHOTS
```

If R17 has replay-ready diagnostics but no executable lifecycle denominator:

```text
BLOCK_L2D2_ROUTE_SUPPORT_REQUIRED
```
