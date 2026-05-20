# RAPORT P3.7-J3E R15-r4 Smoke

Date: 2026-05-20

Status:

```text
R15-r4 smoke: NOT_READY_DIAGNOSED
V3/MFS replay path: PASS
probe payer blocker: REMOVED
probe transport / entry: ABSENT
collection: HOLD
```

## Run

Config:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r4.toml
```

Namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r4
```

Runtime command:

```bash
timeout 45m env RUST_LOG=info \
cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r4.toml
```

The run was stopped after five probe selections were recorded. The process
required SIGTERM after SIGINT did not stop it promptly.

## V3 Replay

```text
v3_shadow_report counts.raw_rows = 11
v3_shadow_report counts.v3_rows = 11
v3_shadow_report counts.bad_rows = 0
stale_against_config = false
strict replay status = full_replay_ok
strict replay total_rows = 11
strict replay v3_rows = 11
strict replay bad_rows = 0
```

## Probe Artifacts

```text
probe_selection_rows = 5
probe_skip_rows = 23
probe_transport_rows = 0
probe_entry_rows = 0
probe_lifecycle_rows = 0
active_shadow_transport_rows = 0
active_shadow_entry_rows = 0
active_shadow_lifecycle_rows = 0
```

## Skip Reasons

```text
probe_execution_precheck_failed = 5
probe_rate_limit_exceeded = 13
max_probes_per_run_exceeded = 4
probe_concurrency_limit_exceeded = 1
```

Execution precheck:

```text
missing_required_account:transaction_account:4NpkpkjPC9DYD2nSLsmLWKBsLXEPgZSkXySpwUoMgiLL = 5
```

No selected probe was stopped by `missing_required_account:payer_pubkey`.

## Join-Key Audit

Post-run audit:

```text
PLANS/AUDYT/RAPORT_P3_7_J3E_R15_R4_JOIN_KEY_AUDIT_20260520.md
```

Summary:

```text
probe_selection exact decision/V3 join coverage = 1.0
probe_selection feature_hash_mismatch = 0
probe_selection policy_hash_mismatch = 0
probe_transport rows = 0
probe_entry rows = 0
probe readiness = not_ready
```

The audit correctly remains not-ready because selection did not advance to
transport or entry.

## Decision

R15-r4 does not pass the P0R runtime smoke gate. It does, however, prove the
J3E payer-semantics repair:

```text
missing payer_pubkey no longer blocks selected probes
hash continuity for selected probes remains 100%
AccountNotFound is not treated as success
```

Next gate:

```text
P3.7-J3F Probe Required Transaction Account Resolution
```

Collection remains blocked until probe transport and entry rows are produced
with exact join-key continuity.
