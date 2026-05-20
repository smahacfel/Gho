# RAPORT P3.7-J3Q3 R15-r8m Counterfactual Probe Smoke

Date: 2026-05-20

## Verdict

```text
R15-r8m smoke: MINIMAL PASS
V3/MFS strict replay: PASS
probe transport/entry: PASS
probe lifecycle close: NOT_OBSERVED
full/bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

R15-r8m was stopped early after the gate signal appeared. The purpose was not to
wait for timeout, but to verify whether the optional `bonding_curve_v2` precheck
repair could reach real counterfactual probe transport and entry rows.

## Inputs

```text
config = configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8m.toml
namespace = shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8m
```

## Runtime Counts

```text
probe_selection_rows = 11
probe_skips_rows = 15
probe_transport_rows = 4
probe_shadow_entries_rows = 4
shadow_lifecycle_rows = 0
buys_jsonl = missing
```

Skip reasons:

```text
verdict_type_not_in_sample_scope = 7
probe_rate_limit_exceeded = 6
probe_concurrency_limit_exceeded = 1
probe_execution_precheck_failed = 1
```

The only precheck failure was:

```text
precheck_failure_reason = missing_execution_route_identity
```

Transport/entry outcomes:

```text
counterfactual_shadow_probe_simulated = 3
counterfactual_shadow_probe_simulation_error = 1
```

The simulation error was classified, not silent:

```text
err = InstructionError(3, Custom(2006))
error_class = simulation_mismatch
simulation_error_kind = simulation_error
```

## Reports

Generated artifacts:

```text
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8m/v3_shadow_report.json
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8m/v3_full_replay_report.json
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8m/p3_7_join_key_audit.json
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8m/p3_7_probe_execution_account_readiness.json
PLANS/AUDYT/RAPORT_P3_7_J3Q3_R15_R8M_JOIN_KEY_AUDIT_20260520.md
PLANS/AUDYT/RAPORT_P3_7_J3Q3_PROBE_EXECUTION_ACCOUNT_READINESS_20260520.md
```

Join-key audit:

```text
probe_readiness.status = ready_for_probe_transport_entry_join
decision_join_acceptance = pass
join_key_acceptance = pass
join_quality = exact_probe_id_and_ab_record_id
probe_selection_rows = 11
probe_transport_rows = 4
probe_entry_rows = 4
probe_lifecycle_rows = 0
```

Readiness audit:

```text
status = PASS
collection_gate = HOLD
selected_probe_rows = 11
exact_decision_v3_join_rows = 12
pre_scan_precheck_skip_rows = 1
missing_account_roles.none = 12
```

## Decision

R15-r8m proves the immediate blocker was narrowed correctly:

- counterfactual probe transport rows now exist;
- counterfactual probe entry rows now exist;
- exact AB/probe join remains accepted;
- active BUY output was not produced;
- lifecycle close was not observed and is not claimed.

This permits a next bounded probe step only after reviewing the remaining
`simulation_mismatch` class. It does not permit Phase B, P2, live, active policy
changes or threshold tuning.
