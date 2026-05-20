# RAPORT P3.7-J3I3 Scan Backlog Admission Repair

Date: 2026-05-20

Status:

```text
P3.7-J3I3 code-level repair: PASS
R15-r8 / R15-r8b runtime smoke: stopped early after useful blocker signal
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

## Trigger

R15-r8 was stopped early after it showed no probe transport/entry rows and a
dominant execution-account readiness blocker. A follow-up R15-r8b attempt was
also stopped early when `probe_scan_concurrency_limit_exceeded` appeared again
as a recurring skip reason before the candidate scan budget was exhausted.

Waiting for the full timeout would not have added useful evidence. The next
blocking class was already visible.

## Repair

J3I3 changes the scan-plane contract from "try-acquire scan semaphore or skip"
to "reserve a bounded candidate scan slot, then await scan concurrency in the
background task".

Implemented behavior:

```text
candidate admission:
  dedupe_by_probe_id
  max_probe_candidates_scanned_per_run

background readiness work:
  await max_scan_concurrent semaphore

dispatch:
  dispatch quota/rate/concurrency consumed only after readiness passes
```

This preserves decision hot-path behavior: the decision path does not wait on
the scan semaphore. The background backlog remains bounded by
`max_probe_candidates_scanned_per_run`.

## Files Changed

- `ghost-launcher/src/oracle_runtime.rs`
- `PLANS/PLAN_P3_7_J3_COUNTERFACTUAL_SHADOW_PROBE_PLANE_20260519.md`
- `PLANS/AUDYT/RAPORT_P3_7_J3I3_SCAN_BACKLOG_REPAIR_20260520.md`

## Validation

Commands run:

```bash
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py scripts/v3_p37_probe_execution_account_readiness_report.py
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py scripts/test_v3_p37_probe_execution_account_readiness_report.py -v
rustfmt --edition 2021 --check ghost-launcher/src/oracle_runtime.rs
git diff --check
```

Result:

```text
ghost-launcher p37_shadow_probe tests: PASS
ghost-launcher p37_counterfactual_probe tests: PASS
Python audit/readiness tests: PASS
rustfmt check: PASS
git diff --check: PASS
```

Targeted new/updated behavior covered:

- scan concurrency waits in the background instead of dropping a candidate;
- scan concurrency remains independent from dispatch concurrency;
- scan-count budget still bounds the number of admitted candidate scans;
- not-ready rows still do not consume dispatch quota.

## Governance

This repair does not change active Gatekeeper verdicts, IWIM, P2, live sender,
thresholds, or selector policy. It does not relax strict execution-account
readiness. `bonding_curve_v2` remains a strict core execution account and
`creator_vault` remains route-aware.

## Next Gate

Run the next bounded smoke as an early-failure detector:

```text
if probe_scan_concurrency_limit_exceeded appears early:
  stop and repair scan-plane again
if execution_account_not_ready dominates and no transport/entry rows appear:
  stop and move to account-readiness/source coverage repair
if probe transport/entry appears with exact AB/probe/V3 continuity:
  generate reports and consider a small bounded collection
```

Full/bounded collection remains blocked until probe transport/entry rows pass
with exact join-key continuity.
