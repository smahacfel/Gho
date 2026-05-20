# RAPORT P3.7-J3I3 R15-r8c Smoke

Date: 2026-05-20

Status:

```text
R15-r8c smoke: NOT_READY_DIAGNOSED
J3I3 scan backlog repair: runtime signal confirmed
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

## Purpose

R15-r8c was a short diagnostic smoke after J3I3. It was not a collection run.
The run was stopped early once the next blocker was visible.

## Runtime Result

```text
v3_full_replay_status = ok
v3_rows = 8
bad_rows = 0
probe_selection_rows = 33
probe_skip_rows = 46
probe_transport_rows = 0
probe_entry_rows = 0
probe_lifecycle_rows = 0
active_shadow_transport_rows = 0
active_shadow_entry_rows = 0
active_shadow_lifecycle_rows = 0
```

Skip reasons:

```text
execution_account_not_ready = 31
verdict_type_not_in_sample_scope = 15
probe_scan_concurrency_limit_exceeded = 0
```

## Interpretation

J3I3 did what it was supposed to do: scan concurrency is no longer discarding
candidate rows before readiness can be evaluated. The smoke moved the blocker
back to strict execution-account readiness.

The run did not generate counterfactual probe transport/entry rows, so it does
not unlock collection.

## Decision

```text
J3I3 code/runtime smoke signal: PASS for scan backlog repair
R15-r8c collection readiness: NOT_READY_DIAGNOSED
Next: P3.7-J3J Execution Account Readiness Source / Wait Strategy
```

Do not increase dispatch limits. Do not bypass `bonding_curve_v2` or
route-aware `creator_vault` checks. The next repair must address how probe
discovers or waits for decision-time-safe execution-account readiness.
