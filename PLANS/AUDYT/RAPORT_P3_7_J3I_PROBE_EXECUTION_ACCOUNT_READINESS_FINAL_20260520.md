# RAPORT P3.7-J3I Probe Execution-Account Readiness Final

Date: 2026-05-20

Status:

```text
P3.7-J3I account readiness final: NOT_READY_DIAGNOSED
R15-r7 runtime smoke: NOT_READY_DIAGNOSED
Strict execution account precheck: PRESERVED
Probe dispatch quota: not consumed by not-ready rows
Probe transport/entry: ABSENT
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

## Scope

This report summarizes the final R15-r7 account-readiness outcome after the
operator stopped the runtime before natural timeout. It intentionally uses an
aggregate JSONL pass over final artifacts instead of a full per-probe markdown
table, because the full row-level readiness report is large and not needed for
the gate decision.

## Inputs

- config: `configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7.toml`
- probe selection: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7/probe_selection.jsonl`
- probe skips: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7/probe_skips.jsonl`
- final join-key audit: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7/p3_7_j3i_r15_r7_join_key_audit_final.json`

## Final Account-Readiness Counts

```text
probe_selection_rows = 548
probe_skipped_rows = 1079
probe_transport_rows = 0
probe_entry_rows = 0
probe_lifecycle_rows = 0
```

Readiness-related skip reasons:

```text
execution_account_not_ready = 543
probe_execution_precheck_failed = 4
probe_scan_concurrency_limit_exceeded = 283
```

The remaining skip class was outside sample scope:

```text
verdict_type_not_in_sample_scope = 249
```

## Missing Required Execution Roles

Strict execution-account readiness stopped the checked candidates on true
execution accounts:

```text
bonding_curve_v2 = 519
creator_vault = 22
associated_bonding_curve = 1
mint = 1
```

Rows without a readiness role were expected for non-readiness skips:

```text
none / not applicable = 536
```

## J3I Contract Check

J3I intended to separate candidate scan from dispatch quota. Final R15-r7
confirms the critical safety side of that contract:

```text
not-ready rows did not produce probe transport rows
not-ready rows did not produce probe entry rows
not-ready rows did not bypass strict precheck
not-ready rows did not mutate active BUY
```

The final run still does not prove absence of ready candidates, because scan
pressure remained visible:

```text
probe_scan_concurrency_limit_exceeded = 283
```

Those rows were not fully checked for execution-account readiness.

## Decision

The final account-readiness state is:

```text
NOT_READY_DIAGNOSED
```

This is not a reason to weaken the execution-account precheck. It is a reason to
repair scan-plane throughput so the runtime can inspect enough candidate rows
without consuming dispatch quota and without dropping candidate scans too early.

Next repair:

```text
P3.7-J3I2 Probe Scan-Plane Throughput Repair
```

