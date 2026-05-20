# RAPORT P3.7-J3J Execution Account Wait Repair

Status: CODE-LEVEL REPAIR IN PROGRESS

## Context

R15-r8c after J3I3 removed the scan-plane concurrency blocker but still produced
no probe transport or entry rows. The dominant blocker was strict execution
account readiness:

```text
probe_selection_rows = 33
probe_transport_rows = 0
probe_entry_rows = 0
probe_scan_concurrency_limit_exceeded = 0
execution_account_not_ready = 31
```

This confirmed that the next problem is not candidate admission or decision/V3
hash continuity. The probe plane is reaching selected rows, but required
execution accounts are still unavailable at immediate dispatch time.

## Change

J3J adds a bounded wait before dispatch quota is consumed:

- `p37_shadow_probe.probe_wait_for_execution_accounts_ms`
- default `0`, preserving existing fail-fast behavior
- wait runs only in the counterfactual probe background path
- wait is bounded and does not block the active decision hot path
- strict execution accounts remain strict
- active Gatekeeper, IWIM, live sender, thresholds and P2 remain unchanged

The probe rows now carry:

```text
probe_execution_account_wait_ms
probe_execution_account_wait_result
```

Valid wait result classes:

```text
ready_without_wait
wait_disabled
ready_after_wait
wait_timeout
```

Rows that still lack a strict required account after the wait remain explicit
`execution_account_not_ready:<role>:<pubkey>` skips.

## Validation

Initial targeted validation:

```text
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
  PASS: 27/27

cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
  PASS: 7/7

python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py scripts/v3_p37_probe_execution_account_readiness_report.py
  PASS

python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py scripts/test_v3_p37_probe_execution_account_readiness_report.py -v
  PASS: 10/10

rustfmt --edition 2021 --check ghost-launcher/src/config.rs ghost-launcher/src/oracle_runtime.rs
  PASS

git diff --check
  PASS
```

Runtime smoke validation:

```text
R15-r8d smoke: NOT_READY_DIAGNOSED
v3_rows = 5
strict_full_replay = full_replay_ok
probe_selection_rows = 25
probe_transport_rows = 0
probe_entry_rows = 0
execution_account_not_ready = 21
wait_timeout = 21
missing_roles = bonding_curve_v2:19, creator_vault:2
```

The run was stopped early after the decisive blocker was visible.

## Runtime Gate

Next smoke namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8d
```

The smoke must not wait blindly for timeout. Stop early when:

- probe transport/entry rows appear,
- `wait_timeout` plus `execution_account_not_ready` clearly dominates,
- or any new structural blocker appears.

## Decision

```text
J3J code-level repair: PASS
R15-r8d smoke: NOT_READY_DIAGNOSED
Full/bounded collection: HOLD
Phase B: HOLD
P2/live/tuning: NO-GO
```
