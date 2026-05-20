# RAPORT P3.7-J3I R15-r7 Smoke Partial

Date: 2026-05-20

Status:

```text
R15-r7 runtime smoke: IN_FLIGHT_PARTIAL
J3I scan/dispatch split: PARTIALLY_OBSERVED
V3/MFS replay path: PASS on current snapshot
Probe transport/entry: NOT_READY on current snapshot
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

## Scope

This report captures an in-flight snapshot of
`shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7`.

The runtime process was still active when this report was written. Counts can
increase after this snapshot. The report is intentionally not a final smoke
verdict.

## Inputs

- config: `configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7.toml`
- runtime stdout: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7/r15_r7_runtime_stdout.log`
- decision root: `logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7/decisions`
- probe selection: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7/probe_selection.jsonl`
- probe skips: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7/probe_skips.jsonl`
- V3 shadow report snapshot: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7/v3_shadow_report_partial.json`
- strict replay snapshot: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7/v3_full_replay_report_strict_partial.json`
- join-key audit snapshot: `PLANS/AUDYT/RAPORT_P3_7_J3I_R15_R7_JOIN_KEY_AUDIT_PARTIAL_20260520.md`
- account readiness snapshot: `PLANS/AUDYT/RAPORT_P3_7_J3I_PROBE_EXECUTION_ACCOUNT_READINESS_PARTIAL_20260520.md`

## Snapshot Counts

At the snapshot point:

```text
v3_shadow_report.deduped_rows = 45
v3_shadow_report.v3_rows = 45
v3_shadow_report.bad_rows = 0
v3_shadow_report.replay_status = full

strict_replay.total_rows = 46
strict_replay.v3_rows = 46
strict_replay.bad_rows = 0
strict_replay.status = full_replay_ok

probe_selection_rows = 114
probe_skipped_rows = 221
probe_transport_rows = 0
probe_entry_rows = 0
probe_lifecycle_rows = 0
```

`probe_selection.jsonl` and `probe_skips.jsonl` parsed cleanly in the snapshot
check.

## Probe Skip Breakdown

Current `probe_skips.jsonl` breakdown:

```text
execution_account_not_ready = 114
probe_scan_concurrency_limit_exceeded = 77
verdict_type_not_in_sample_scope = 30
```

Execution-account readiness roles:

```text
bonding_curve_v2 = 108
creator_vault = 5
associated_bonding_curve = 1
none / not applicable = 107
```

Readiness status:

```text
not_ready = 114
pending_runtime_precheck = 77
none = 30
```

## J3I-Specific Interpretation

J3I intended to separate candidate scan from dispatch quota:

```text
candidate scan: may log selected / skipped rows
dispatch quota: should be consumed only after execution-account readiness
```

The snapshot supports the important negative condition:

```text
not-ready rows did not produce probe transport or entry rows
```

However, it also shows a new limiting behavior:

```text
probe_scan_concurrency_limit_exceeded = 77
```

This is not dispatch quota consumption, but it means the scan plane itself can
drop eligible candidate inspection while a runtime precheck is in flight. The
current snapshot therefore does not yet prove that there are no
execution-ready rows in the full candidate universe. It only proves that every
probe reaching strict readiness in the observed scan set was not ready.

## Join-Key Status

The partial join-key audit reports:

```text
probe_selection exact_decision_v3_join_coverage = 1.0
probe_transport_rows = 0
probe_entry_rows = 0
probe_decision_join_acceptance = fail
```

The fail status is expected for a snapshot with no transport/entry rows. There
is no observed hash mismatch in the selected probe rows.

## Current Verdict

```text
R15-r7 partial smoke: NOT_READY_IN_FLIGHT
V3/MFS replay: PASS on current snapshot
selection -> decision exact join: PASS on current snapshot
probe transport/entry: ABSENT
execution-account blocker: still bonding_curve_v2 / creator_vault / associated_bonding_curve
new scan-plane limiter: probe_scan_concurrency_limit_exceeded
collection: HOLD
```

## Next Decision Point

If the still-running R15-r7 later produces probe transport/entry rows, the final
smoke report should supersede this partial snapshot.

If the run completes without transport/entry rows, the next repair should focus
on one of these choices:

1. keep strict readiness and accept `NOT_READY_DIAGNOSED` if no ready rows exist
   in the scanned universe;
2. widen scan concurrency or make the readiness check faster so scan pressure
   does not mask the eligible universe;
3. add a decision-time-safe execution-account readiness filter before selection,
   if the required accounts can be known without post-hoc leakage.

Do not start collection from this snapshot.
