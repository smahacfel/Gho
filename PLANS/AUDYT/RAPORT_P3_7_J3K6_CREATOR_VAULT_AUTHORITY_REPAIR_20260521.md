# RAPORT P3.7-J3K6 Creator Vault Authority / Route Identity Repair

## Status

```text
J3K6 code-level repair: PASS
R15 J3K6 runtime smoke: NEXT GATE / NOT RUN IN THIS REPORT
Collection / Phase B / P2 / live / tuning: HOLD / NO-GO
```

## Context

R15 J3K5-r2 produced a useful bounded probe result:

```text
probe_transport_rows = 10
probe_shadow_entry_rows = 9
probe_required_exact_decision_v3_join_coverage = 1.0
active_buys_rows = 0
simulation_error_custom_code_counts = {"custom_2006": 2}
creator_vault_authority_status_counts = {"creator_vault_source_not_authoritative": 2}
creator_identity_source_counts = {"account_overrides.creator_pubkey": 2}
```

The two `custom_2006` rows were Pump.fun Anchor `ConstraintSeeds` failures on
`creator_vault`. That means the counterfactual probe reached the program with a
creator-vault PDA that did not match the program's expected creator-vault seeds.

## Repair

J3K6 makes this class fail closed before simulation when the route requires a
creator vault but the creator identity source is not authoritative.

Implemented behavior:

```text
route requires creator_vault
+ creator identity source is non-authoritative
= probe_skipped
  probe_skip_reason = creator_vault_source_not_authoritative
  execution_account_readiness_role = creator_vault
```

The change is probe-only. It does not change active policy, active BUY behavior,
IWIM, live sender, P2, thresholds, or selector semantics.

## Code Changes

Runtime:

```text
ghost-launcher/src/oracle_runtime.rs
```

Main additions:

- creator-vault authority fields on probe selection/skip records;
- `P37ShadowProbeAccountOverrideContext`;
- route-aware creator-vault precheck;
- pre-dispatch skip for non-authoritative creator identity;
- targeted unit coverage for LegacyBuy non-authoritative creator skip,
  RoutedExactSolIn authoritative creator acceptance, and skip-row schema.

Audit:

```text
scripts/v3_p37_mfs_lifecycle_join_key_audit.py
scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py
```

Main additions:

- skip-side creator-vault authority counts;
- fixture coverage for `creator_vault_source_not_authoritative` skip rows.

Smoke config:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k6-r1.toml
```

## Validation

Commands run:

```bash
rustfmt --edition 2021 --check ghost-launcher/src/oracle_runtime.rs
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py scripts/v3_p37_probe_account_reconciliation_report.py scripts/v3_p37_probe_execution_account_readiness_report.py
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py scripts/test_v3_p37_probe_account_reconciliation_report.py scripts/test_v3_p37_probe_execution_account_readiness_report.py -v
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
git diff --check
```

Result:

```text
python unittest: 26/26 PASS
p37_shadow_probe: 44/44 PASS
p37_counterfactual_probe: 8/8 PASS
git diff --check: PASS
```

The Rust test runs emitted existing repository warnings; no J3K6 test failure
was observed.

Pre-run join-key audit against the fresh J3K6 namespace was also executed with
temporary `/tmp` outputs. It returned `not_ready` with zero rows, which is
expected before the runtime smoke and confirms that the namespace starts empty.

## Runtime Gate

J3K6 is not collection-ready by itself. The next gate is a clean bounded smoke:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k6-r1.toml
```

Smoke PASS requires:

```text
strict V3 replay OK
probe exact decision/V3 join = 100%
custom_2006 = 0 OR explicitly new/diagnosed creator-vault class
creator_vault_source_not_authoritative rows are skipped before simulation
active_buys_rows = 0
no live/P2 path touched
```

If entry yield collapses because LegacyBuy creator identity is now skipped as
non-authoritative, the next repair is to add or identify an authoritative
LegacyBuy creator source. It is not to bypass the creator-vault authority
precheck.
