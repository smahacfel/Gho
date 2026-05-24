# P3.7-L2E2 Decision Eval Snapshots Runtime Emission

## Verdict

- code_status: `pass`
- runtime_status: `not_started`
- r17_preflight_status: `pass`
- final_decision: `GO_R17_REPLAY_READY_DIAGNOSTIC_RUN`
- previous_blocker: `BLOCK_R17_TEMPORAL_SNAPSHOT_RUNTIME_GAP`
- blocker_status: `removed`
- recommended_next_path: `start_bounded_r17_replay_ready_diagnostic`

## Scope

L2E2 adds runtime emission for `decision_eval_snapshots` in the Gatekeeper V2 observation path.
The change is diagnostic/replay instrumentation only.

No threshold, live/P2, Phase B, policy tuning, route support, or collection behavior was changed.

## Runtime Contract Added

`GatekeeperAssessment` now carries ordered `decision_eval_snapshots` captured while `GatekeeperBuffer`
still owns the observation-session state. The DecisionLogger remains a serializer of captured payload,
not a post-hoc snapshot reconstructor.

Each snapshot emits:

- `eval_index`
- `eval_ts_ms`
- `elapsed_ms`
- `snapshot_target_ms`
- `snapshot_actual_elapsed_ms`
- `snapshot_drift_ms`
- `snapshot_source`
- `observation_stage`
- `trigger_source`
- `materialized_feature_snapshot_hash`
- `materialized_feature_snapshot_payload_or_compact_payload`
- `gatekeeper_mode`
- `gatekeeper_config_hash`
- `gatekeeper_gate_trace`
- `phase_pass_vector`
- `hard_reject_reason`
- `soft_points`
- `max_soft_points`
- `pdd_diagnostics`
- `prosperity_diagnostics`
- `hhi_diversity_diagnostics`
- `verdict_if_evaluated`
- `reason_code_if_evaluated`
- `reason_if_evaluated`

Configured R17 temporal targets are now represented as:

- early: `2000ms`
- normal: `5000ms`
- extended: `7000ms`
- terminal/deadline: `10000ms`

Snapshot source classification:

- `exact_tick`
- `nearest_eval`
- `terminal`

## Files Changed

- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/components/gatekeeper_policy.rs`
- `ghost-launcher/src/components/gatekeeper_adaptive_prosperity.rs`
- `ghost-launcher/src/oracle_runtime.rs`

## Preflight Evidence

Command:

```bash
python3 scripts/v3_p37_r17_replay_ready_preflight.py \
  --config configs/rollout/shadow-burnin-v3-p37-r17-replay-ready-diagnostic.toml \
  --json
```

Result:

- `preflight_status = pass`
- `final_decision = GO_R17_REPLAY_READY_DIAGNOSTIC_RUN`
- `gatekeeper_emits_temporal_snapshots = true`
- `gatekeeper_hardcodes_temporal_snapshots_none = false`
- `decision_logger_has_v22_fields = true`
- `no_threshold_tuning`
- `no_phase_b`
- `no_p2_live`
- `no_runtime_started_by_preflight`

## Test Evidence

Rust:

```bash
cargo test -p ghost-launcher --lib decision_eval_snapshots -- --nocapture
cargo test -p ghost-launcher --lib snapshot_contains_gatekeeper_v2_replay_fields -- --nocapture
cargo test -p ghost-launcher --lib terminal_snapshot_is_always_present -- --nocapture
cargo test -p ghost-launcher --lib snapshot_hash_is_stable_for_same_payload -- --nocapture
```

Python:

```bash
python3 -m py_compile \
  scripts/v3_p37_r17_replay_ready_preflight.py \
  scripts/test_v3_p37_r17_replay_ready_preflight.py

python3 -m unittest scripts/test_v3_p37_r17_replay_ready_preflight.py -v
```

Formatting/static checks:

```bash
rustfmt --edition 2021 --check \
  ghost-launcher/src/components/gatekeeper.rs \
  ghost-launcher/src/components/gatekeeper_policy.rs \
  ghost-launcher/src/components/gatekeeper_adaptive_prosperity.rs \
  ghost-launcher/src/oracle_runtime.rs

git diff --check
```

All listed checks passed.

## Interpretation

L2E2 removes the R17 preflight blocker for missing runtime temporal snapshots. R17 can now be started
as a bounded replay-ready diagnostic run, subject to the existing R17 safety contract.

This report does not claim runtime R17 evidence. It only establishes code-level emission support and
preflight readiness.

## Non-Goals Preserved

- no threshold tuning
- no policy-axis claim
- no runtime started by L2E2
- no Phase B
- no P2/live
- no route fallback or route support expansion
- no collection
