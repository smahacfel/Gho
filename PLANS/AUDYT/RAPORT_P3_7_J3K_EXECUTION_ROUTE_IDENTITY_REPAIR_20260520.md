# RAPORT P3.7-J3K Execution Route Identity Repair

Status: CODE-LEVEL REPAIR PASS

## Context

R15-r8d after J3J stopped early with a clear structural blocker:

```text
probe_selection_rows = 25
probe_transport_rows = 0
probe_entry_rows = 0
wait_timeout = 21
missing_roles = bonding_curve_v2:19, creator_vault:2
```

The wait window did not produce execution-ready rows. That means the next
highest-value repair is not another longer smoke, but a stricter probe
eligibility gate around route identity before a shadow simulation request is
built.

## Change

J3K extends `p37_shadow_probe_execution_precheck` with route-identity checks:

```text
missing_execution_route_identity
missing_routed_associated_bonding_curve
missing_creator_pubkey
```

The new checks run before probe request construction. They prevent the probe
plane from constructing routed shadow simulations from incomplete
decision-time execution identity and later failing on derived accounts such as
`bonding_curve_v2` or `creator_vault`.

## Invariants

```text
active Gatekeeper verdicts: unchanged
IWIM: unchanged
live sender: unchanged
P2/live: disabled
thresholds: unchanged
strict required execution accounts: still strict
missing route identity: skip, not success
```

## Validation

Targeted Rust validation:

```text
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
  PASS: 30/30

cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
  PASS: 7/7
```

New targeted cases:

```text
p37_shadow_probe_execution_precheck_requires_route_identity
p37_shadow_probe_execution_precheck_requires_routed_account_identity
p37_shadow_probe_execution_precheck_accepts_complete_routed_identity
```

Additional validation:

```text
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py scripts/v3_p37_probe_execution_account_readiness_report.py
  PASS

python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py scripts/test_v3_p37_probe_execution_account_readiness_report.py -v
  PASS: 10/10

rustfmt --edition 2021 --check ghost-launcher/src/config.rs ghost-launcher/src/oracle_runtime.rs
  PASS

git diff --check
  PASS

cargo build --release -p ghost-launcher --bin ghost-launcher
  PASS
```

## Next Gate

Fresh smoke namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8e
```

R15-r8e must be stopped early once either:

```text
probe transport/entry rows appear
or route-identity/precheck skips clearly dominate
or a new structural blocker appears
```

## Decision

```text
J3K code-level repair: PASS
R15-r8e runtime smoke: NOT_READY_DIAGNOSED
Full/bounded collection: HOLD
Phase B: HOLD
P2/live/tuning: NO-GO
```
