# ADR-0099: Test Fixture Event-Time Provenance Backfill

**Date:** 2026-04-15
**Status:** Accepted
**Author:** Ghost Father

## Context
`ghost-brain` test fixtures lagged behind the event provenance migration that introduced required `event_time` fields on `TxEvent`, `DetectedPool`, and `CandidatePool`. The original reported failure surfaced in `ghost-brain` tests as `missing field 'event_time' in initializer of 'TxEvent'`, and subsequent compilation exposed the same schema drift in other `ghost-brain` fixture types.

## Decision
Backfill the required provenance fields in affected `ghost-brain` tests and benches by explicitly populating:
- `event_time: ghost_core::EventTimeMetadata::default()` for fixtures that rely on legacy timestamp compatibility.
- Existing semantic and event timestamp fields where newer fixture shapes require them.

The change is limited to test and bench fixture construction and does not alter production logic.

## Architectural Impact
This keeps the test surface aligned with the current SSOT for event provenance. Any future schema evolution on cross-pipeline event envelopes now requires synchronized fixture updates across `ghost-brain` test modules that instantiate canonical event structs directly.

## Risk Assessment
Low. The patch only updates synthetic test data to satisfy the current struct contracts. Production behavior, serialization logic, and runtime decision paths remain unchanged.

## Consequences
Fixture compilation is restored for `ghost-brain` test targets, and the original `TxEvent.event_time` regression is removed. The trade-off is that direct struct literals remain sensitive to future required-field additions, so similar drift can recur unless helpers/builders are introduced later.

## Alternatives Considered
Populate fully derived provenance values from each fixture timestamp and block time. Rejected for this fix because the affected tests do not assert provenance semantics, and `EventTimeMetadata::default()` preserves existing legacy-compat behavior with the smallest blast radius.

## Validation Steps
1. Compile `ghost-brain` test targets with `cargo test -p ghost-brain --tests --no-run --quiet`.
2. Re-run `cargo test --workspace --no-run --quiet` to confirm the original `ghost-brain` `event_time` failure is gone and identify any remaining unrelated baseline errors.
