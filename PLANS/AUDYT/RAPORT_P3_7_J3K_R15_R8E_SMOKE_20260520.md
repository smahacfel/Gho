# RAPORT P3.7-J3K R15-r8e Smoke

Status: NOT_READY_DIAGNOSED

## Context

R15-r8e was run after the J3K execution route-identity gate. The run was not
allowed to wait for the full timeout. It ended quickly and artifacts already
showed a decisive blocker: no probe transport or entry rows were produced, and
selected probes were stopped by explicit route/account readiness classes.

Config:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8e.toml
```

## V3 Replay

```text
v3_shadow_report.status = ok
v3_shadow_report.replay_status = full
v3_shadow_report.deduped_v3_rows = 20
v3_shadow_report.bad_rows = 0

strict_full_replay.status = ok
strict_full_replay.replay_status = full_replay_ok
strict_full_replay.v3_rows = 20
strict_full_replay.bad_rows = 0
```

## Probe Artifacts

```text
probe_selection_rows = 58
probe_skipped_rows = 64
probe_transport_rows = 0
probe_entry_rows = 0
probe_lifecycle_rows = 0

active_buy_rows = 0
shadow_entries_rows = 0
shadow_lifecycle_rows = 0
```

Skip reason distribution:

```text
probe_execution_precheck_failed = 25
execution_account_not_ready = 31
verdict_type_not_in_sample_scope = 8
```

Precheck distribution:

```text
missing_execution_route_identity = 25
execution_account_not_ready:bonding_curve_v2 = 28
execution_account_not_ready:creator_vault = 3
```

Wait diagnostics:

```text
wait_timeout = 31
wait_result_none = 33
```

## Join-Key Audit

```text
probe_selection exact decision/V3 join = 58/58
probe_selection feature_hash_mismatch = 0
probe_selection policy_hash_mismatch = 0
probe_transport_rows = 0
probe_entry_rows = 0
probe_readiness = not_ready
```

The audit remains `not_ready` because transport and entry rows are absent. The
selection side is clean and exact-joins to V3 decision rows.

## Interpretation

J3K successfully moved one class of late failure earlier:

```text
missing_execution_route_identity
```

That is the correct behavior for rows where route identity is not available at
decision time. Rows that do have enough route identity still hit strict
execution-account readiness, mainly `bonding_curve_v2`, after the bounded wait.

This is a useful smoke result and should not be stretched into a longer run.
The next step is not collection. The next step is a route/account identity
source decision.

## Decision

```text
J3K code-level repair: PASS
R15-r8e smoke: NOT_READY_DIAGNOSED
Full/bounded collection: HOLD
Phase B: HOLD
P2/live/tuning: NO-GO
```

Recommended next stage:

```text
P3.7-J3L Execution Route Identity Materialization / Eligibility Source
```

J3L should determine whether `buy_variant`, routed associated bonding curve,
creator identity, and strict execution account readiness can be materialized or
proved decision-time-safe for probe eligibility. It must not guess accounts
post-hoc and must not weaken strict precheck.
