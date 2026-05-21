# RAPORT P3.7-J3K4 Simulation Error Diagnostics

Date: 2026-05-21

Status:

```text
J3K4 code-level diagnostic repair = PASS
runtime smoke with new fields = PASS / DIAGNOSED
bounded collection = HOLD
Phase B / P2 / live / tuning = NO-GO
```

## Context

`r15-bounded-j3k3-r1` was stopped early after producing probe transport and
entry rows plus three classified simulation errors:

```text
simulation_account_layout_mismatch:custom_2006 = 2
simulation_slippage_or_price_mismatch:custom_6002 = 1
```

Manual inspection showed that `custom_2006` was a Pump.fun Anchor
`ConstraintSeeds` failure on `creator_vault`. The simulation logs had the
critical detail, but the transport schema did not expose it as structured
fields.

## Change

J3K4 adds additive parser logic for Anchor constraint logs:

```text
AnchorError caused by account: <role>
Program log: Left:
Program log: <actual_pubkey>
Program log: Right:
Program log: <expected_pubkey>
```

New/strengthened transport fields:

```text
simulation_error_account_role
simulation_error_account_pubkey
simulation_error_actual_account_pubkey
simulation_error_expected_account_pubkey
```

For creator-vault seed mismatches, future probe transport rows should now show:

```text
simulation_error_account_role = creator_vault
simulation_error_actual_account_pubkey = <provided creator_vault>
simulation_error_expected_account_pubkey = <program expected creator_vault>
```

This is diagnostic-only. It does not change active decisions, live sender,
IWIM, thresholds, P2, or probe dispatch policy.

## Validation

Commands:

```bash
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
python3 -m py_compile scripts/v3_p37_probe_account_reconciliation_report.py scripts/v3_p37_probe_execution_account_readiness_report.py scripts/v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts/test_v3_p37_probe_account_reconciliation_report.py scripts/test_v3_p37_probe_execution_account_readiness_report.py scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
rustfmt --edition 2021 --check ghost-launcher/src/oracle_runtime.rs
```

Results:

```text
p37_shadow_probe = 38/38 PASS
p37_counterfactual_probe = 8/8 PASS
python unit tests = 25/25 PASS
py_compile = PASS
rustfmt_check = PASS
```

## Remaining Gate

The clean bounded namespace was executed:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k4-r1
```

Result:

```text
probe_selection_rows = 12
probe_transport_rows = 10
probe_shadow_entry_rows = 10
probe_lifecycle_rows = 0
probe_required_exact_decision_v3_join_coverage = 1.0
probe_entry_materialized = 8
simulation_error_rows = 2
active_buy_rows = 0
```

The J3K4 fields were populated for the Pump.fun Anchor `ConstraintSeeds`
failure:

```text
simulation_error_account_role = creator_vault
simulation_error_account_pubkey = 4D8hkwjsgvn5hrQgJULqxuh5hSX3UEUEe2U9nWpTiyTP
simulation_error_actual_account_pubkey = 4D8hkwjsgvn5hrQgJULqxuh5hSX3UEUEe2U9nWpTiyTP
simulation_error_expected_account_pubkey = GdZspP3tLaQQ5jrFixZ2xPmWjshMWEX6K9ynkx2BiXLM
```

This confirms that `custom_2006` is a route/account identity mismatch for
`creator_vault`, not an anonymous simulation failure and not an AccountNotFound
class.

The next runtime check should not be treated as a lifecycle/selector run. Stop
early if:

```text
simulation_error_rate spikes
creator_vault actual/expected mismatch dominates
custom_6002 amount/slippage errors dominate
join/hash continuity regresses
active BUY rows appear
live/P2 path is touched
```

If creator-vault mismatches remain rare and fully classified, a bounded probe
collection can continue under stop-loss gates. If they dominate, the next repair
must materialize route-correct creator-vault identity or narrow eligibility
before dispatch.
