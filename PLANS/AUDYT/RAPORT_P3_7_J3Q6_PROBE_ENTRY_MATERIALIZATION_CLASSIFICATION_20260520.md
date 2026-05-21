# RAPORT P3.7-J3Q6 Probe Entry Materialization Classification

## Status

J3Q6 classification guard: PASS
R15-r9-q5c runtime smoke: unchanged, re-audited only
Next gate: very small bounded counterfactual probe collection
Full collection / Phase B / P2 / live / tuning: HOLD / NO-GO

## Scope

J3Q6 adds an audit-only classification for counterfactual probe transport rows that do not materialize into probe entry rows.

This is not a runtime policy change. It does not change active Gatekeeper verdicts, IWIM, live sender, P2, thresholds, or counterfactual probe dispatch behavior.

## Implemented Changes

- Extended `scripts/v3_p37_mfs_lifecycle_join_key_audit.py` schema to version 4.
- Added probe token/entry materialization counters:
  - `buy_variant`
  - `token_param_role`
  - `entry_token_amount_raw`
  - `min_tokens_out`
  - `execution_outcome`
  - `error_class`
  - `simulation_error_category`
  - `execution_account_readiness_status`
- Added `probe_entry_materialization` report section.
- Added transport-to-entry status classes:
  - `entry_materialized`
  - `transport_only_missing_token_quantity`
  - `simulation_error`
  - `execution_account_not_ready`
  - `unknown`
- Added explicit reason for the known Q5c edge case:
  - `routed_exact_sol_in_entry_token_amount_raw_null`

## Q5c Re-Audit Result

Input:

- `configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r9-q5c.toml`

Output:

- `PLANS/AUDYT/RAPORT_P3_7_J3Q6_R15_R9_ENTRY_MATERIALIZATION_AUDIT_20260520.md`
- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r9-q5c/p3_7_join_key_audit_q6.json`

Materialization counts:

```text
probe_transport_rows = 5
probe_entry_rows = 4
entry_materialized = 4
transport_only_missing_token_quantity = 1
simulation_error = 0
execution_account_not_ready_transport_rows = 0
unknown = 0
```

The single transport-only row is classified as:

```text
buy_variant = routed_exact_sol_in
token_param_role = min_tokens_out
entry_token_amount_raw = null
min_tokens_out = 1
probe_entry_materialization_reason = routed_exact_sol_in_entry_token_amount_raw_null
```

## Validation

Commands:

```bash
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
```

Result:

```text
py_compile: PASS
unittest: 8/8 PASS
```

## Decision

Q6 closes the reporting gap for transport-only probe rows. The probe plane now separates:

- transport rows,
- entry-materialized rows,
- transport-only missing token quantity rows,
- simulation-error rows,
- execution-account-not-ready rows.

This is sufficient to run a very small bounded counterfactual probe collection while keeping Phase B, P2, live, and runtime threshold work blocked.
