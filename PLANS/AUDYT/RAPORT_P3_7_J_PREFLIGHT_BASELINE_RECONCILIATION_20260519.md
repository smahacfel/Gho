# P3.7-J Preflight Baseline Reconciliation

Date: 2026-05-19

Status: accepted for R14 smoke with documented alternate preflight.

## Scope

This report reconciles the formal production preflight wrapper with the direct
runtime preflight for the P3.7-J R14 smoke profile.

Smoke profile:

```text
configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke.toml
```

Current HEAD:

```text
17fd583c6e3e561df15faeadd9e4894f4229fffc
```

Current baseline stamp:

```text
256efc4419bf900c223016d44e1fc73f37471ac4
```

## Wrapper Result

Command:

```bash
bash scripts/ghost_production_preflight.sh \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke.toml
```

Result:

```text
[fail] baseline.accepted_revision: expected 17fd583c6e3e561df15faeadd9e4894f4229fffc but stamp contains 256efc4419bf900c223016d44e1fc73f37471ac4
```

Interpretation:

- The wrapper stops before structural and runtime preflight checks.
- The observed failure is a local governance baseline mismatch.
- This is not evidence that the R14 smoke config is invalid.
- The baseline stamp was not modified by Codex.

## Checks Run Separately

Workspace compile/no-run:

```bash
cargo test --workspace --no-run
```

Result:

```text
ok
```

Structural acceptance gate:

```bash
python3 scripts/refactor_phase0_guardrails.py structural-check --repo-root /root/Gho
```

Result:

```text
ok
```

Launcher profile load test:

```bash
cargo test -p ghost-launcher \
  config::tests::test_p37_mfs_lifecycle_rollout_profiles_load_shadow_only_primary_only \
  -- --nocapture
```

Result:

```text
ok
```

Brain config load test:

```bash
cargo test -p ghost-brain --test ghost_brain_config_load_test \
  gatekeeper_v3_p37_mfs_lifecycle_collection_descopes_fsc_forward_only \
  -- --nocapture
```

Result:

```text
ok
```

Direct runtime preflight:

```bash
cargo run --quiet -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke.toml \
  --preflight
```

Result:

```text
[ok] preflight: all runtime checks passed for /root/Gho/configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke.toml
```

The direct runtime preflight verified:

- `execution_mode=Shadow`
- `entry_mode=shadow_only`
- Gatekeeper brain config load
- single-stream gRPC transport config
- writable snapshot/log/event/decision/shadow paths
- trigger keypair availability
- RPC `getVersion`
- Yellowstone gRPC app probe
- shadow trigger balance reserve
- metrics port availability

## Join-Key Pre-Smoke Baseline

Command:

```bash
python3 scripts/v3_p37_mfs_lifecycle_join_key_audit.py \
  --config configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke.toml \
  --output-json logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/p3_7_mfs_lifecycle_join_key_audit_presmoke.json \
  --output-md PLANS/AUDYT/RAPORT_P3_7_J_R14_SMOKE_JOIN_KEY_AUDIT_PRESMOKE_20260519.md
```

Result:

```text
readiness = not_ready
reasons = missing_decision_rows, missing_v3_replay_payload_rows,
          missing_shadow_transport_rows, missing_shadow_entry_rows,
          missing_shadow_lifecycle_rows
```

Interpretation:

- This is expected before smoke runtime starts.
- It confirms that the join-key audit itself runs and reports empty-state
  readiness fail-closed.

## Decision

Do not update `.ghost/baseline_accepted_revision` in this task.

R14 smoke may proceed using the documented alternate preflight bundle:

1. `cargo test --workspace --no-run`
2. `python3 scripts/refactor_phase0_guardrails.py structural-check --repo-root /root/Gho`
3. targeted R14 config load tests
4. direct runtime `--preflight`
5. pre-smoke join-key audit

The formal wrapper remains blocked until an operator explicitly accepts
`17fd583c6e3e561df15faeadd9e4894f4229fffc` as the new local baseline stamp.

## Non-Goals

This reconciliation does not authorize:

- full R14 primary-only collection,
- updating the baseline stamp,
- P2 or live execution,
- runtime threshold tuning,
- active V2/V2.5 policy changes,
- IWIM changes,
- live sender changes,
- treating lifecycle labels as decision-time features.
