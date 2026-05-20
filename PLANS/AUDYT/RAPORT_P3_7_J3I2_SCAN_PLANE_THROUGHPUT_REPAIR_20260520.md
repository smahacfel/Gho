# RAPORT P3.7-J3I2 Scan-Plane Throughput Repair

Date: 2026-05-20

Status:

```text
P3.7-J3I2 code-level repair: PASS
R15-r7 final smoke: NOT_READY_DIAGNOSED
R15-r8 runtime smoke: NEXT GATE / NOT RUN BY THIS REPAIR
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

## Trigger

Final R15-r7 artifacts showed that J3I correctly prevented
execution-account-not-ready rows from consuming dispatch quota, but scan-plane
pressure was still material:

```text
strict_replay.status = full_replay_ok
strict_replay.v3_rows = 199
probe_selection_rows = 548
probe_skipped_rows = 1079
probe_transport_rows = 0
probe_entry_rows = 0
execution_account_not_ready = 543
probe_scan_concurrency_limit_exceeded = 283
```

That means R15-r7 did not prove absence of execution-ready candidates. It proved
that checked candidates were not ready, while many candidate scans were skipped
because the scan semaphore was saturated.

## Implementation

J3I2 separates scan throughput from dispatch concurrency:

- added `p37_shadow_probe.max_scan_concurrent`;
- added `p37_shadow_probe.max_probe_candidates_scanned_per_run`;
- changed `P37ShadowProbeRuntimeState` to hold independent scan and dispatch
  semaphores;
- added a scan counter with
  `max_probe_candidates_scanned_per_run_exceeded`;
- preserved dispatch-only limits:
  - `max_probes_per_run`;
  - `max_probes_per_minute`;
  - `max_concurrent`;
- kept dedupe at scan reservation;
- kept strict execution-account readiness before dispatch reservation.

The runtime still does not dispatch, simulate, or write transport/entry until
execution-account readiness is `ready`.

## R15-r8 Profile

Added:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8.toml
```

Relevant probe limits:

```text
max_probes_per_run = 5
max_probes_per_minute = 5
max_concurrent = 1
max_scan_concurrent = 8
max_probe_candidates_scanned_per_run = 1000
```

This keeps dispatch bounded while allowing the runtime to inspect substantially
more candidate readiness states.

## Validation

Executed:

```text
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
  PASS: 27/27

cargo test -p ghost-launcher --lib p37_shadow_probe_runtime_state -- --nocapture
  PASS: 6/6

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

## Decision

```text
J3I2 code-level repair: PASS
R15-r8 bounded smoke: GO as next runtime gate
Collection: HOLD until R15-r8 proves either entry/transport or a finite
            no-ready-candidate diagnosis without scan-pressure masking
```

R15-r8 must not be interpreted as collection. Its job is only to determine
whether a wider scan plane finds execution-ready candidates while dispatch quota
remains fixed and strict execution-account precheck remains intact.

