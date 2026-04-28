# ADR-0094: ADR-0093 Residual Ring Buffer Reaudit

**Date:** 2026-04-12
**Status:** Accepted
**Author:** Ghost Father

## Context
ADR-0093 left a final residual risk in `ghost-brain` where decision-path transactions were normalized onto the decision event axis in `engine.rs`, but `snapshot_engine.rs` still trimmed the backing transaction buffer by raw stored `tx.timestamp_ms`. Because stored records can still carry `LegacyCompat`, `Arrival`, or `Wallclock` timestamps, storage-only clocks could indirectly evict earlier decision-eligible records before the engine filtered them.

The current audit rechecked the claimed fix in current code only, with focus on `ghost-brain/src/oracle/snapshot_engine.rs`, `ghost-brain/src/oracle/engine.rs`, and the directly relevant tests.

## Decision
The local `RingSnapshots` transaction buffer fix is real:
- `push_transaction()` now runs both capacity trimming and decision-time trimming.
- decision-time trimming keys off the latest `decision_event_ts_ms()`, not raw `timestamp_ms`.
- capacity trimming preferentially removes storage-only records first.
- `engine.rs` still filters all decision-path inputs through `decision_axis_transactions()` / `decision_window_transactions()`.

However, the residual is not globally closed because the pre-activation inactive transaction queue still uses `resolve_tx_event_timestamp()` for TTL trimming, dedup key construction, replay sorting, and FIFO overflow eviction. That means storage-only clocks can still affect whether inactive transactions survive long enough to be replayed into the decision path.

## Architectural Impact
The active per-pool ring-buffer path is now aligned with the ADR-0093 event-axis contract.

The inactive pre-activation buffering path remains a separate survivability surface outside the repaired ring-buffer logic. Any pool that accumulates buffered trades before activation still permits storage-only timestamps to influence retention and replay ordering.

## Risk Assessment
**Rate:** Medium

The originally reported active-buffer residual is closed, but the broader “storage-only clocks cannot influence decision-path survivability” claim is still too strong. The remaining exposure is bounded to inactive/pending transaction buffering before pool activation.

## Consequences
The main live transaction buffer no longer allows late storage-only timestamps to evict decision-eligible records indirectly.

What remains harder is asserting full closure of Brain 5b unless inactive buffering is either:
- explicitly excluded from scope, or
- migrated onto the same decision-clock retention policy.

## Alternatives Considered
1. **Accept full closure based only on `RingSnapshots`**
   - Rejected because inactive buffering is in the same module and still affects survivability before replay.

2. **Mark the fix as fully incomplete**
   - Rejected because the specific active-buffer defect reported in the prior review is demonstrably fixed.

## Validation Steps
1. Confirm `RingSnapshots::push_transaction()` trims by `decision_event_ts_ms()` and prefers dropping storage-only records first.
2. Confirm `PredictionSession` still filters decision inputs through decision-axis normalization before sorting/windowing.
3. Confirm targeted unit tests exist and pass for:
   - age trim ignoring storage-only clocks,
   - capacity preferring storage-only eviction,
   - engine decision-axis filtering,
   - engine decision-window filtering.
4. Reaudit inactive pre-activation buffering separately if Brain 5b closure is meant to cover all pre-engine survivability paths.
