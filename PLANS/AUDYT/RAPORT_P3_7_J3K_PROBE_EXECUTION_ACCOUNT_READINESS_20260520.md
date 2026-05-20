# RAPORT P3.7-J3K Probe Execution Account Readiness

Date: 2026-05-20

Status: NOT_READY_DIAGNOSED

## Inputs

```text
config = configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8e.toml
probe_selection = logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8e/probe_selection.jsonl
probe_skips = logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8e/probe_skips.jsonl
decision_root = logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8e/decisions
```

The source JSON summary is stored locally at:

```text
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8e/reports/probe_execution_account_readiness.json
```

## Summary

```text
selected_probe_rows = 58
diagnosed_selected_probe_rows = 31
exact_decision_v3_join_rows = 58
missing_account_roles = bonding_curve_v2:28, creator_vault:3, none:27
classifications = execution_account_not_ready:31, unknown:27
```

The `unknown:9` rows are not unclassified runtime errors. They are rows that
failed the new J3K route-identity precheck before a concrete execution account
role was available.

Runtime skip counts from the r8e artifacts:

```text
probe_selection_rows = 58
probe_skipped_rows = 64
probe_transport_rows = 0
probe_entry_rows = 0
probe_lifecycle_rows = 0

probe_execution_precheck_failed = 25
execution_account_not_ready = 31
verdict_type_not_in_sample_scope = 8

missing_execution_route_identity = 25
wait_timeout = 31
bonding_curve_v2 = 28
creator_vault = 3
```

## Interpretation

J3K changed the failure mode in the intended direction:

- rows without decision-time execution route identity now fail early as
  `missing_execution_route_identity`;
- rows with enough route identity to build the request still fail on strict
  execution-account readiness, mainly `bonding_curve_v2`;
- no row reached probe transport or probe entry;
- active BUY output stayed empty.

This proves the next blocker is not scan throughput, dispatch quota, payer
semantics, hash continuity, or short account-readiness wait. The blocker is
decision-time route/account identity coverage for counterfactual probes.

## Decision

```text
J3K route-identity gate: PASS
R15-r8e readiness: NOT_READY_DIAGNOSED
Full/bounded collection: HOLD
Phase B: HOLD
P2/live/tuning: NO-GO
```

The next repair should not increase dispatch limits and should not bypass
strict precheck. It should decide how probe eligibility gets decision-time-safe
execution route/account identity:

```text
P3.7-J3L Execution Route Identity Materialization / Eligibility Source
```
