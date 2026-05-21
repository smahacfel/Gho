# RAPORT P3.7-J3K5 Creator-Vault / Amount Guard R2

## Status

```text
J3K5-r2 creator-vault authority runtime check: PASS / DIAGNOSED
J3K5-r2 amount guard runtime check: NOT OBSERVED
Collection / Phase B / P2 / live / tuning: HOLD / NO-GO
```

## Runtime Evidence

Source smoke:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k5-r2.toml
```

Artifacts:

```text
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k5-r2/probe_transport.jsonl
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k5-r2/probe_shadow_entries.jsonl
PLANS/AUDYT/RAPORT_P3_7_J3K5_BOUNDED_R2_JOIN_KEY_AUDIT_20260521.md
```

Counts:

```text
probe_selection_rows = 19
probe_transport_rows = 10
probe_shadow_entry_rows = 9
probe_required_exact_decision_v3_join_coverage = 1.0
active_buys_rows = 0
```

## Creator-Vault Authority

Observed:

```text
simulation_error_custom_code_counts = {"custom_2006": 2}
creator_vault_authority_status_counts = {"creator_vault_source_not_authoritative": 2}
creator_vault_mismatch_reason_counts = {"actual_expected_mismatch": 2}
creator_identity_source_counts = {"account_overrides.creator_pubkey": 2}
```

Decision:

```text
creator-vault authority diagnostics are runtime validated.
```

The two rows are correctly treated as simulation/account-layout mismatches, not
as successful entries and not as post-hoc repair sources.

## Amount Guard

Observed:

```text
custom_6002 rows = 0
amount_guard_status_counts = {}
```

Decision:

```text
amount guard parser remains code/test validated, but r2 did not re-observe the
TooMuchSolRequired class after the inline Left/Right parser fix.
```

## Next Gate

```text
Do not start collection yet.
Next repair path: creator-vault source authority / route identity.
```

The known blocker after r2 is no longer amount shortfall. It is the
creator-vault authority mismatch for `custom_2006` rows.
