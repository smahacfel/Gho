# RAPORT P3.7-J3E Probe Payer / Account Resolution

Date: 2026-05-20

Status:

```text
P3.7-J3E code-level repair: PASS
R15-r4 runtime smoke: NOT_READY_DIAGNOSED
Full / bounded collection: HOLD
Phase B V3/MFS lifecycle feature prototype: HOLD
P2 / live / tuning: NO-GO
```

## Scope

J3E addressed the R15-r3 blocker where every selected counterfactual
probe was stopped by:

```text
missing_required_account:payer_pubkey:HvLVQMA4...
```

The goal was not to disable required-account checks. The goal was to
separate probe payer semantics from true required execution accounts.

## Findings

The R15 probe profile uses:

```toml
[trigger.shadow_run]
payer_strategy = "ephemeral"
```

In this mode the payer is created by the shadow runtime and cached as an
ephemeral shadow payer. It is not an operator-funded live payer account and
must not be treated as a chain-visible prerequisite in the counterfactual
probe execution precheck.

The precheck therefore now treats a missing payer as acceptable only when:

```text
request.payer_provenance == "ephemeral"
and missing_pubkey == request.payer_pubkey
```

Configured payer mode remains strict. True required execution accounts remain
strict. AccountNotFound remains a failure and is not promoted to success.

## Code Changes

Updated:

```text
ghost-launcher/src/components/trigger/component.rs
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r4.toml
PLANS/PLAN_P3_7_J3_COUNTERFACTUAL_SHADOW_PROBE_PLANE_20260519.md
```

Key implementation points:

- added ephemeral-payer exception for counterfactual probe required-account
  precheck;
- preserved configured-payer strictness;
- preserved idempotently creatable user ATA exception;
- added a targeted test proving ephemeral payer is skipped while mint,
  token program and user ATA remain required;
- added fresh bounded R15-r4 smoke namespace.

## Validation

Passed:

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
p37_counterfactual_probe: 6/6 PASS
```

## R15-r4 Smoke

Config:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r4.toml
```

Namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r4
```

Run was stopped by operator after the sentinel condition was met: five probe
selections were recorded. SIGTERM was required after SIGINT did not stop the
runtime promptly.

Post-run reports:

```text
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r4/v3_shadow_report.json
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r4/v3_full_replay_report_strict.json
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r4/p3_7_j3e_r15_r4_join_key_audit.json
PLANS/AUDYT/RAPORT_P3_7_J3E_R15_R4_JOIN_KEY_AUDIT_20260520.md
```

Runtime result:

```text
v3_rows = 11
strict replay status = full_replay_ok
strict replay bad_rows = 0
stale_against_config = false

probe_selection_rows = 5
probe_skip_rows = 23
probe_transport_rows = 0
probe_entry_rows = 0
probe_lifecycle_rows = 0
active shadow transport rows = 0
active shadow entry rows = 0
active shadow lifecycle rows = 0
```

Probe skip reasons:

```text
probe_execution_precheck_failed = 5
probe_rate_limit_exceeded = 13
max_probes_per_run_exceeded = 4
probe_concurrency_limit_exceeded = 1
```

Execution precheck failures:

```text
missing_required_account:transaction_account:4NpkpkjPC9DYD2nSLsmLWKBsLXEPgZSkXySpwUoMgiLL = 5
```

Important: no selected probe was blocked by `missing_required_account:payer_pubkey`.
The original J3E payer blocker is removed.

## Join-Key Result

The post-run join-key audit remains `not_ready` because no probe transport or
entry rows were produced.

For probe selection rows:

```text
exact decision/V3 join coverage = 5/5
feature hash mismatch = 0
policy hash mismatch = 0
```

The audit still reports probe readiness as not ready because the downstream
transport and entry artifacts are absent.

## Decision

J3E is accepted as a code-level and payer-semantics repair.

R15-r4 is not a runtime PASS because no probe transport or entry rows were
generated. It is a diagnosed not-ready result: the blocker moved from missing
ephemeral payer to a true required transaction account.

Next recommended stage:

```text
P3.7-J3F Probe Required Transaction Account Resolution
```

J3F should identify why the prepared probe request requires:

```text
transaction_account:4NpkpkjPC9DYD2nSLsmLWKBsLXEPgZSkXySpwUoMgiLL
```

and decide whether that account is:

- a true execution account that must be present before probe simulation,
- an account that should be supplied through overrides,
- a protocol/program/sysvar account with special handling,
- or evidence that the sampled row is not execution-ready.

Until J3F or equivalent account-resolution work passes:

```text
Full / bounded collection: HOLD
Phase B: HOLD
P2 / live / tuning: NO-GO
```
