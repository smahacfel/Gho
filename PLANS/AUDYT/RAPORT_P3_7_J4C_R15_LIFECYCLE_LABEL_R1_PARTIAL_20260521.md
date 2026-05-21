# P3.7 J4C R15 lifecycle-label r1 partial snapshot

Date: 2026-05-21

Namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1
```

Status: `IN_FLIGHT_PARTIAL`

This report is a partial runtime snapshot, not a final collection report. The
tmux run was still active when this snapshot was taken.

## Counts

```text
V3/V2.5 decision rows: 51
probe_selection_rows: 16
probe_skip_rows: 408
probe_transport_rows: 14
probe_shadow_entry_rows: 14
probe_shadow_lifecycle_rows: 22
closed_probe_positions: 11
active_buy_rows: 0
```

Lifecycle rows are paired records:

```text
exit_filled: 11
position_closed: 11
```

## Replay / Join State

Strict replay remained clean at the snapshot point:

```text
full_replay_ok: 51
bad_rows: 0
```

Probe transport/entry/lifecycle rows preserved the P3.7 join chain in the
current artifact set. Full post-run reports still need to be generated after
the bounded run exits.

## Skip Distribution

The dominant probe-plane skip classes were:

```text
creator_vault_source_not_authoritative: 244
verdict_type_not_in_sample_scope: 104
probe_execution_precheck_failed: 58
probe_concurrency_limit_exceeded: 1
active_buy_excluded: 1
```

The dominant precheck failure was:

```text
missing_execution_route_identity: 57
```

## Interpretation

The run was intentionally bounded and conservative:

```text
max_probes_per_run = 50
max_probes_per_minute = 10
max_concurrent = 1
max_scan_concurrent = 8
max_probe_candidates_scanned_per_run = 20000
```

The low number of lifecycle labels is not evidence of active BUY scarcity.
Counterfactual probe selection is being limited mostly by route/account
authority checks, especially `creator_vault_source_not_authoritative`, and by
strict execution prechecks.

Active BUY remained untouched:

```text
active_buy_rows = 0
live/P2 = not enabled
```

## Decision

This snapshot is useful as evidence that the lifecycle-label collection path is
producing closed probe positions, but it is not a final report.

Current operational state:

```text
J4C lifecycle-label collection: IN_FLIGHT_PARTIAL
probe lifecycle close path: OBSERVED
label/onchain report: pending post-run
full collection / Phase B / P2 / live: HOLD / NO-GO
```
