# RAPORT P3.7-J3L Route-Identity Pre-Scan Repair

Status: CODE-LEVEL PASS

Date: 2026-05-20

## Decision

J3L changes the probe plane so cheap execution route-identity failures are
classified before scan admission. Rows without usable route identity are now
written as `probe_skipped` immediately and do not reserve scan budget, spawn a
background readiness task, wait for strict accounts, or touch dispatch quota.

This preserves the J3K fail-closed semantics:

```text
missing_execution_route_identity
missing_routed_associated_bonding_curve
missing_creator_pubkey
```

Strict execution accounts remain strict after route identity passes:

```text
bonding_curve_v2
creator_vault
associated_bonding_curve
```

## Implementation

Code changes:

- added a shared helper to derive probe account overrides from buffered
  transaction evidence plus pool metadata;
- reused that helper in both the foreground probe admission path and the
  background dispatch path;
- moved `p37_shadow_probe_execution_precheck` before
  `try_reserve_scan_slot` in `maybe_handle_p37_shadow_probe_decision`;
- kept the background precheck as a defensive guard before request building;
- extended `scripts/v3_p37_probe_execution_account_readiness_report.py` so
  pre-scan `probe_execution_precheck_failed` rows from `probe_skips.jsonl` are
  included in readiness diagnostics even when there is no paired
  `probe_selected` row;
- added explicit classification for route-identity precheck failures.

## Runtime Semantics

Before J3L, a selected row with missing route identity could consume scan-plane
capacity before being skipped. After J3L:

```text
route identity missing -> probe_skipped -> no scan slot -> no background task
```

This is not a bypass and not a collection gate. It is an admission repair for
the counterfactual probe plane only.

## Validation

Passed:

```bash
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
python3 -m py_compile scripts/v3_p37_probe_execution_account_readiness_report.py scripts/v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts/test_v3_p37_probe_execution_account_readiness_report.py scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
rustfmt --edition 2021 --check ghost-launcher/src/config.rs ghost-launcher/src/oracle_runtime.rs
git diff --check
```

Observed test counts:

```text
p37_shadow_probe: 30/30 PASS
p37_counterfactual_probe: 7/7 PASS
python readiness/join-key tests: 12/12 PASS
```

## Next Gate

Run R15-r8f in a clean namespace and stop as soon as a structural blocker is
visible.

Expected interpretations:

```text
probe_transport/entry appears:
  stop after short grace period, generate reports, evaluate minimal smoke PASS

route/precheck skips dominate:
  stop early, generate reports, repair the next blocker

execution_account_not_ready dominates:
  stop early, keep collection HOLD, move to account source/materialization fix
```

## Holds

```text
Full/bounded collection: HOLD
Phase B V3/MFS lifecycle feature prototype: HOLD
P2/live/tuning: NO-GO
active policy/IWIM/live sender changes: NO-GO
```
