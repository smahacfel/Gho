# RAPORT P3.7-J3F Probe Required Transaction Account Resolution

Date: 2026-05-20

Status:

```text
P3.7-J3F code-level repair: PASS
R15-r5 runtime smoke: NOT_READY_DIAGNOSED
Full / bounded collection: HOLD
Phase B V3/MFS lifecycle feature prototype: HOLD
P2 / live / tuning: NO-GO
```

## Scope

J3F addressed the R15-r4 blocker where every selected counterfactual probe was
stopped by:

```text
missing_required_account:transaction_account:4NpkpkjPC9DYD2nSLsmLWKBsLXEPgZSkXySpwUoMgiLL
```

The goal was not to disable required-account precheck. The goal was to identify
the account role and decide whether it should be strict for counterfactual
shadow probes.

## Finding

The missing account was not a generic unknown transaction account. It was part
of the routed pump.fun `buy_exact_sol_in` account layout built by
`DirectBuyBuilder::build_buy_ix_with_accounts(...)`.

The relevant routed account positions are:

```text
account[12] = global_volume_accumulator
account[13] = user_volume_accumulator
account[14] = fee_config
account[15] = fee_program
account[16] = bonding_curve_v2
account[17] = buyback_fee_recipient
```

The R15-r4 missing account maps to:

```text
user_volume_accumulator
```

It was stable across selected probes because it is derived from the ephemeral
shadow payer, not from the mint.

## Precheck Semantics

For counterfactual shadow probes:

- missing routed `user_volume_accumulator` is allowed through precheck;
- this exception applies only to routed probe requests where the missing pubkey
  matches the account at buy instruction index 13;
- missing `user_volume_accumulator` is not success;
- if simulation later returns `AccountNotFound`, that remains a simulation/data
  problem;
- true execution accounts remain strict, including mint, bonding curve,
  associated bonding curve, creator vault, global volume accumulator,
  `bonding_curve_v2`, fee config, fee program and buyback fee recipient.

This preserves the boundary between a probe-specific account-resolution rule
and strict execution readiness.

## Code Changes

Updated:

```text
ghost-launcher/src/components/trigger/component.rs
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r5.toml
PLANS/PLAN_P3_7_J3_COUNTERFACTUAL_SHADOW_PROBE_PLANE_20260519.md
```

Key implementation points:

- map routed `DirectBuyBuilder` buy instruction account indices to explicit
  roles;
- replace generic `transaction_account` for known routed positions with
  specific role names;
- allow missing `user_volume_accumulator` through counterfactual probe
  precheck only for routed probe requests;
- preserve strict precheck for true required execution accounts;
- add a targeted test proving `user_volume_accumulator` is skipped while
  strict accounts remain required;
- add a fresh bounded R15-r5 smoke profile.

## Validation

Passed before R15-r5 smoke:

```bash
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
rustfmt --edition 2021 --check ghost-launcher/src/components/trigger/component.rs ghost-launcher/src/oracle_runtime.rs ghost-launcher/src/config.rs
git diff --check
```

Observed targeted Rust coverage:

```text
p37_shadow_probe: 21/21 PASS
p37_counterfactual_probe: 7/7 PASS
```

## R15-r5 Smoke Summary

Config:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r5.toml
```

Namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r5
```

Post-run reports:

```text
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r5/v3_shadow_report.json
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r5/v3_full_replay_report_strict.json
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r5/p3_7_j3f_r15_r5_join_key_audit.json
PLANS/AUDYT/RAPORT_P3_7_J3F_R15_R5_JOIN_KEY_AUDIT_20260520.md
```

Runtime result:

```text
v3_rows = 169
strict replay status = full_replay_ok
strict replay bad_rows = 0
stale_against_config = false

probe_selection_rows = 5
probe_skip_rows = 576
probe_transport_rows = 0
probe_entry_rows = 0
probe_lifecycle_rows = 0
active shadow transport rows = 0
active shadow entry rows = 0
active shadow lifecycle rows = 0
```

Probe skip reasons:

```text
probe_rate_limit_exceeded = 350
max_probes_per_run_exceeded = 172
verdict_type_not_in_sample_scope = 48
probe_execution_precheck_failed = 5
probe_concurrency_limit_exceeded = 1
```

Execution precheck failures for selected probes:

```text
missing_required_account:bonding_curve_v2 = 4
missing_required_account:creator_vault = 1
```

Important: no selected probe was blocked by `payer_pubkey`,
`user_volume_accumulator`, or generic `transaction_account` for the known
routed user-volume PDA.

## Join-Key Result

The post-run join-key audit remains `not_ready` because no probe transport or
entry rows were produced.

For probe selection rows:

```text
exact decision/V3 join coverage = 5/5
feature hash mismatch = 0
policy hash mismatch = 0
```

The audit still reports probe readiness as not ready because downstream probe
transport and entry artifacts are absent.

## Decision

J3F is accepted as a code-level account-role and precheck-semantics repair.

R15-r5 is not a runtime PASS because no probe transport or entry rows were
generated. It is a diagnosed not-ready result: the blocker moved from the
routed user-volume PDA to true required routed execution accounts.

Next recommended stage:

```text
P3.7-J3G Probe Strict Execution Account Readiness
```

J3G should answer how counterfactual probes obtain, wait for, or classify
missing strict execution accounts such as:

```text
bonding_curve_v2
creator_vault
```

without weakening global precheck, using post-hoc lifecycle data, changing
active policy, or treating AccountNotFound as success.

Until J3G or equivalent execution-account readiness work passes:

```text
Full / bounded collection: HOLD
Phase B: HOLD
P2 / live / tuning: NO-GO
```
