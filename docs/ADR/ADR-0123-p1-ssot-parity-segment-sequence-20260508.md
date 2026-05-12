# ADR-0123: P1 SSOT parity + segment_sequence — implementation 2026-05-08

**Date:** 2026-05-08
**Status:** Accepted
**Author:** Ghost Father

## Task goal

Implement P1 from `PLAN_NAPRAWCZY_GATEKEEPER_V25_SHADOW_BURNIN_20260507.md`:
give Path B (feature-driven, materialized) access to `TxSegmentSequence` so it can
compute TAS, spike, ramping, and flash crash signals — achieving parity with
Path A (buffer-driven) without synthetic backfill (N14).

## Summary of work

1. **New SSOT types** — `TrajectorySegmentSnapshot` and `TxSegmentSequence` in
   `ghost-core/src/checkpoint/types.rs`. Added as `Option<TxSegmentSequence>` to
   `MaterializedFeatureSet` with `#[serde(default, skip_serializing_if = ...)]`.
   Contract N1 preserved: additive only, backward-compatible.

2. **`GatekeeperBuffer::current_segment_sequence()`** — Builds raw per-segment
   trajectory snapshots (T0/T1/T2) from buffered transactions using the same
   3-segment division as `materialize_trajectory`. Returns `TxSegmentSequence`
   with per-segment metrics including `same_size_streak` (ramping heuristic)
   and `max_price_impact_pct`.

3. **Materialization in session** — `PoolObservationSession::materialize_features()`
   populates `materialized.tx_segment_sequence` from the buffer, alongside the
   existing `trajectory_assessment`.

4. **`gatekeeper_pdd_sequence.rs`** — New module with three detection functions:
   - `detect_spike_from_segments()` — T2 volume rate vs (T0+T1)/2
   - `detect_ramping_from_segments()` — `same_size_streak` in T1/T2
   - `detect_flash_crash_from_segments()` — max price impact in T2 vs T0/T1
   
   Each returns `(detected: bool, reason: Option<&str>)`. When `min_tx_per_segment_satisfied`
   is false, returns `(false, None)` — honest unavailability (N14).

5. **Path B wiring** — `build_assessment_from_features()`:
   - Computes TAS from `tx_segment_sequence` when Path A trajectory is unavailable
   - Enriches PDD diagnostics with spike/ramping/flash from segment sequence
   - Sequence-based signals only override PDD hard_fail if no earlier signal
     (drift/whale/reserve) already vetoed

6. **Honest `tas_unavailable_reason`** — When `tx_segment_sequence` is `None` or
   `min_tx_per_segment_satisfied` is false, the reason string is specific:
   `"materialized_features_missing_segment_sequence"` (generic → appropriate when
   Path B truly has no segment data).

## Decisions made

1. **No synthetic backfill (N14 preserved)** — When segment data is insufficient,
   we mark signals as unavailable with explicit reasons rather than guessing.
   Path B never fabricates trajectory data.

2. **PDD signal priority** — Drift, whale, and reserve (available in Path A from
   account/tx features) take priority over sequence-based signals. Sequence
   signals only trigger hard_fail when earlier checks passed.

3. **Segment reconstruction** — `build_assessment_from_features` reconstructs
   `TrajectorySegment` structs from `TrajectorySegmentSnapshot` values and passes
   them to the existing `score_trajectory()` function. This ensures Path A and
   Path B use the same scoring algorithm.

4. **`same_size_streak` computation** — Computed inline in `current_segment_sequence()`
   using ±15% size tolerance across consecutive buys. Simple heuristic, consistent
   with the existing `detect_ramping()` in `gatekeeper_pdd.rs`.

## Files changed

| File | Change |
|------|--------|
| `ghost-core/src/checkpoint/types.rs` | Added `TrajectorySegmentSnapshot`, `TxSegmentSequence`, `Option<TxSegmentSequence>` field on `MaterializedFeatureSet` |
| `ghost-core/src/checkpoint/mod.rs` | Re-exported new types |
| `ghost-core/src/checkpoint/feature_builder.rs` | Added `tx_segment_sequence: None` to `MaterializedFeatureSet` constructor |
| `ghost-launcher/src/components/gatekeeper.rs` | Added `current_segment_sequence()`, `current_segment_sequence_from_config()`, inline `same_size_streak` computation |
| `ghost-launcher/src/components/gatekeeper_pdd_sequence.rs` | **New** — spike/ramping/flash-from-segments detection |
| `ghost-launcher/src/components/gatekeeper_policy.rs` | Path B: TAS from sequence, PDD enrichment with sequence signals |
| `ghost-launcher/src/components/mod.rs` | Added `pub mod gatekeeper_pdd_sequence` |
| `ghost-launcher/src/session/observation.rs` | Materialize `tx_segment_sequence` from buffer |
| `ghost-launcher/src/components/gatekeeper.rs` | Fixed `v25_confidence_breakdown` (availability guard), `pdd_sequence_signals_available` (checks `tx_segment_sequence`), `tas_availability` (specific taxonomy for materialized path) |
| `ghost-launcher/tests/gatekeeper_v25_regression.rs` | 3 P1 contract tests + 1 assertion update |

## Test results

- **13/13** `gatekeeper_v25_regression` tests pass (10 P0 + 3 P1)
- **186/186** gatekeeper lib tests pass (**0 failures** — P1-blocker resolved)
- **5/5** `gatekeeper_pdd_sequence` unit tests pass
- **`ghost-core` and `ghost-launcher` compile without errors**

## Contract blockers resolved (post-review)

1. **`v25_confidence` availability guard** — `v25_confidence_breakdown` now returns
   `None` when TAS is required but unavailable, or when PDD sequence signals
   are required but unavailable. Implements the plan's `v25_confidence_inputs_available`
   contract.

2. **`pdd_sequence_signals_available` semantic fix** — Now checks
   `feature_snapshot.tx_segment_sequence` directly instead of blindly returning
   `false` for all materialized paths. Returns `Some(true)` for buffer path,
   `Some(seq.min_tx_per_segment_satisfied)` when sequence exists,
   `Some(false)` when materialized without sequence.

3. **`tas_availability` taxonomy** — For materialized path, distinguishes
   `insufficient_tx_per_segment`, `insufficient_duration`,
   `materialized_features_missing_segment_sequence`, and
   `materialized_sequence_present_but_trajectory_not_computed`.

## DoD P1 checklist

- [x] `MaterializedFeatureSet` ma `Option<TxSegmentSequence>` z `#[serde(default)]` (N1)
- [x] Path B liczy TAS gdy sequence dostępna; `tas_unavailable_reason` z konkretną taksonomią
- [x] Path B liczy spike/ramping/flash z sequence; jawny `unavailable_reason` gdy brak
- [x] `v25_confidence` availability-aware — `None` gdy wejścia niekompletne
- [x] `pdd_sequence_signals_available` sprawdza `tx_segment_sequence` zamiast ścieżki
- [x] Test: `path_b_marks_unavailable_instead_of_guessing_sequence_features`
- [x] Test: `path_a_and_path_b_compute_same_tas_when_segment_sequence_present`
- [x] Test: `materialized_feature_set_carries_optional_segment_sequence`
- [x] Test: `build_assessment_from_features_keeps_v25_payload_availability_aware` (186/186)
- [x] N14 zachowany: brak syntetycznego backfillu, honest `unavailable_reason`
