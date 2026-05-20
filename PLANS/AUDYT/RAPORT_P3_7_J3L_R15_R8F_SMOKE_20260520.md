# RAPORT P3.7-J3L R15-r8f Smoke

Date: 2026-05-20

Status:

```text
R15-r8f smoke: NOT_READY_DIAGNOSED
V3/MFS strict replay: PASS
Probe transport/entry: ABSENT
Active shadow BUY: FAILED_WITH_ACCOUNT_NOT_FOUND
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
Next: P3.7-J3M route-source compatibility repair
```

## Runtime Handling

The run was intentionally stopped early after the blocker was visible in the
artifacts. It was not allowed to idle until the full timeout because
`probe_transport.jsonl` and probe entries were absent while structured probe
skips had already converged on route/account readiness failures.

## Inputs

- config: `configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8f.toml`
- namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8f`
- shadow root: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8f`
- decision root: `logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8f/decisions`

## Counts

```text
v3_rows = 15
strict_full_replay_ok = 15
strict_bad_rows = 0

probe_selection_rows = 30
probe_skip_rows = 76
probe_transport_rows = 0
probe_entry_rows = 0
probe_lifecycle_rows = 0

active_shadow_buy_rows = 1
active_shadow_entry_rows = 1
active_shadow_lifecycle_rows = 1
```

## Probe Skip Distribution

```text
probe_execution_precheck_failed = 26
execution_account_not_ready = 30
verdict_type_not_in_sample_scope = 19
active_buy_excluded = 1
```

Dominant concrete reasons:

```text
missing_execution_route_identity = 26
execution_account_not_ready:bonding_curve_v2 = 29
execution_account_not_ready:creator_vault = 1
```

All selected probe rows exact-joined back to persisted V3 decision rows by
`ab_record_id` and V3 hashes in the join-key audit. The blocker is not hash
continuity.

## Active Shadow Finding

One non-probe active shadow BUY was produced during the smoke, but it failed:

```text
execution_outcome = shadow_data_problem
err = AccountNotFound
dispatch_status = failed
simulation_outcome = failed
```

This matters because the failure is not confined to counterfactual probe
plumbing. The active shadow path also hit account resolution / route compatibility
failure for the same runtime profile.

## Interpretation

J3L successfully moved missing route identity out of the expensive scan path, but
R15-r8f still did not produce any probe transport or entry rows. The remaining
problem is upstream route/account source compatibility:

- many probe candidates have no execution route identity before scan;
- selected probe candidates with route identity still fail strict execution
  account readiness on `bonding_curve_v2` or route-specific `creator_vault`;
- the active shadow BUY path also failed with `AccountNotFound`.

The next repair should focus on source transaction route enrichment and the
metadata handed to trigger account overrides. Do not bypass required-account
precheck and do not start collection.

## Decision

```text
R15-r8f: NOT_READY_DIAGNOSED
J3L route pre-scan repair: useful but insufficient
Next: P3.7-J3M route-source compatibility repair
Collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```
