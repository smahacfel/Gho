# RAPORT P3.7-J4 Probe Lifecycle Handoff / Post-Buy Monitor Repair

Date: 2026-05-21

## Verdict

```text
P3.7-J4 code-level repair: PASS
J3L-r1 finding: ENTRY-LEVEL PASS / LIFECYCLE NOT VALIDATED
Next gate: fresh bounded J4 runtime smoke
Full collection: HOLD
Phase B / P2 / live / threshold tuning: NO-GO
```

## Trigger

J3L-r1 produced controlled probe transport and entry rows but no lifecycle rows:

```text
probe_transport_rows = 25
probe_shadow_entry_rows = 25
probe_lifecycle_rows = 0
active_buys_rows = 0
exact decision/V3 join = 100%
```

This ruled out decision thresholds as the current blocker. The failing boundary
was probe entry -> post-buy lifecycle handoff.

## Root Cause

The counterfactual probe runtime path stopped after writing:

```text
p37_shadow_probe.transport_log_path
p37_shadow_probe.entry_log_path
```

It did not send a `PostBuySubmitted` handoff to `PostBuyRuntime`, so
`MonitoringEngine::register_position_with_context(...)` was never called for
probe entries. The serialized `probe_position_id` on the entry row was metadata
only; it did not create monitored lifecycle state.

## Repair

Implemented a probe-only lifecycle handoff:

- successful counterfactual probe simulation and entry materialization now send
  a direct-only post-buy handoff with `lane="probe"`;
- `PostBuyRuntime` starts a separate probe `MonitoringEngine` when
  `p37_shadow_probe.lifecycle_log_path` is configured;
- the probe monitor uses an isolated `ShadowPositionBook`;
- probe lifecycle writes to `p37_shadow_probe.lifecycle_log_path`;
- canonical shadow lifecycle writes remain on `execution.shadow.lifecycle_log_path`;
- probe handoff does not reserve active position slots;
- probe handoff does not use the live sender;
- probe handoff preserves `ab_record_id`, `probe_id`, V3 feature hash and V3
  policy hash through `PositionJoinMetadata`.

Touched files:

```text
ghost-launcher/src/components/post_buy_runtime.rs
ghost-launcher/src/oracle_runtime.rs
ghost-launcher/src/events.rs
ghost-launcher/src/main.rs
PLANS/PLAN_P3_7_J3_COUNTERFACTUAL_SHADOW_PROBE_PLANE_20260519.md
```

## Tests

```text
cargo test -p ghost-launcher --lib probe_handoff_uses_isolated_probe_monitor_and_lifecycle_path -- --nocapture
PASS: 1/1

cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
PASS: 47/47

cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
PASS: 8/8
```

Warnings are pre-existing workspace warnings and were not introduced by this
repair.

## Runtime Status

No fresh J4 runtime smoke has been claimed by this report. J4 is a code-level
handoff repair. A clean bounded runtime namespace is still required to validate:

```text
probe_lifecycle_monitor_started > 0
probe lifecycle rows appear if positions close
active BUY remains 0
exact decision/V3 join remains 100%
```

## Decision

```text
GO: fresh bounded J4 runtime smoke
HOLD: bounded/full collection
HOLD: lifecycle labels until close/on-chain proof exists
NO-GO: P2/live/threshold tuning/IWIM/active policy changes
```
