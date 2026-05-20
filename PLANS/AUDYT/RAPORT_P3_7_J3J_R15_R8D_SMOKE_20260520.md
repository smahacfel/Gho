# RAPORT P3.7-J3J R15-r8d Smoke

Status: NOT_READY_DIAGNOSED

## Context

R15-r8d was run after the J3J bounded execution-account wait repair. The run
was intentionally stopped early once the structural blocker was visible. It was
not allowed to burn the full timeout after probe artifacts showed that the wait
window did not produce execution-ready rows.

Config:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8d.toml
```

Runtime semantics:

```text
probe_wait_for_execution_accounts_ms = 1500
max_probes_per_run = 5
max_concurrent = 1
max_scan_concurrent = 8
max_probe_candidates_scanned_per_run = 1000
```

## V3 Replay

```text
v3_shadow_report.status = ok
v3_rows = 5
bad_rows = 0
strict_full_replay = full_replay_ok
full_replay_bad_rows = 0
stale_against_config = false
```

## Probe Artifacts

```text
probe_selection_rows = 25
probe_skipped_rows = 30
probe_transport_rows = 0
probe_entry_rows = 0
probe_lifecycle_rows = 0
active_buy_rows = 0
```

Probe skip reasons:

```text
verdict_type_not_in_sample_scope = 9
execution_account_not_ready = 21
```

Wait diagnostics:

```text
wait_timeout = 21
wait_result_none = 9
```

Missing strict execution-account roles after the bounded wait:

```text
bonding_curve_v2 = 19
creator_vault = 2
```

## Join-Key Audit

```text
probe_selection exact decision/V3 join = 25/25
probe_selection feature_hash_mismatch = 0
probe_selection policy_hash_mismatch = 0
probe_transport_rows = 0
probe_entry_rows = 0
probe_readiness = not_ready
```

The audit cannot pass transport/entry continuity because no probe transport or
entry rows were produced.

## Interpretation

J3J confirmed that a short decision-time-safe wait does not resolve the current
strict execution-account blocker. The probe plane still reaches selected rows
with exact V3 decision joins, but `bonding_curve_v2` and route-specific
`creator_vault` remain unavailable after the 1500 ms wait window.

This is no longer a scan-concurrency problem and no longer a hash-continuity
problem. It is an execution-account source/coverage problem.

## Decision

```text
J3J code-level repair: PASS
R15-r8d smoke: NOT_READY_DIAGNOSED
Full/bounded collection: HOLD
Phase B: HOLD
P2/live/tuning: NO-GO
```

Next repair path:

```text
P3.7-J3K Execution Account Readiness Source Coverage
```

J3K should determine whether strict account identities such as
`bonding_curve_v2` and route-aware `creator_vault` can be sourced from
decision-time-safe account coverage or explicitly materialized for probe
eligibility. Do not weaken strict precheck and do not increase probe dispatch
limits.
