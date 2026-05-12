# ADR-0122: P0 DOW timing reliability — implementation 2026-05-08

**Date:** 2026-05-08
**Status:** Accepted
**Author:** Ghost Father

## Task goal

Implement P0 from `PLAN_NAPRAWCZY_GATEKEEPER_V25_SHADOW_BURNIN_20260507.md`:
time-guaranteed DOW (Dynamic Observation Window) shadow checkpoints that fire
independently of TX traffic, with single-owner serialization per stage.

## Summary of work

1. **Dedicated per-pool DOW timer** — `gatekeeper_dow_timer.rs` module providing
   a `tokio::time::interval` with `MissedTickBehavior::Skip`. Integrated as a
   third branch in `pool_observation_task`'s `tokio::select!`, alongside
   `rx.recv()` and the main deadline timer. No separate task — same task,
   serialized buffer access.

2. **Unified entry point** — `GatekeeperBuffer::maybe_fire_shadow_checkpoint(now_wall_ms)`
   called by both the timer and TX ingestion path. Uses event time (`now_ms`) in
   the TX path and wall-clock time (`current_time_ms()`) in the timer path.

3. **Extended stage as full checkpoint** — `try_shadow_evaluate(Extended)` no
   longer hits `unreachable!()`. Produces typed verdicts:
   `RejectPumpAndDump`, `RejectLowTrajectory`, `NormalBuyCandidate`, `ShadowReject`.

4. **Single-owner invariant** — `*_shadow_fired` flags set on the first checkpoint
   per stage, including `InsufficientData`. Exactly one checkpoint per stage, period.
   No duplicates from timer, TX path, or deadline fallback.

5. **Closed window semantics**:
   - Early: `[early_entry_min_ms, early_entry_max_ms]` (2-5s)
   - Normal: `[normal_window_ms, extended_window_ms)` (7-10s)
   - Extended: `[extended_window_ms, deadline)` (>=10s)

6. **Deadline fallback** — `check_long_deadline` checks `extended_shadow_fired`.
   If timer already fired Extended → skip. If not → fire with
   `EXTENDED_SHADOW_DEADLINE_FALLBACK_*` reason.

7. **Telemetry** — `GATEKEEPER_DOW_TIMER_FIRED_TOTAL{stage}` Prometheus counter,
   incremented via `record_dow_timer_fired()` helper (matching existing pattern).

8. **Assessment carries shadow decisions** — After `check_long_deadline` produces
   the Extended verdict, `assessment.v25_shadow_decisions` is cloned from the buffer
   so downstream consumers (tests, JSONL) see the complete shadow plane.

## Decisions made

1. **InsufficientData sets `*_shadow_fired`** — restores single-owner invariant.
   Trade-off: an early-stage checkpoint that fires with insufficient data "locks"
   that stage. In practice, the 250ms tick rate and continuous TX flow mean checkpoints
   fire near the window boundary with maximum accumulated data.

2. **TX path uses event time, timer uses wall clock** — TX path passes `now_ms`
   (TX event timestamp) to `maybe_fire_shadow_checkpoint`. Timer uses `current_time_ms()`.
   Both converge in production where TX timestamps ≈ wall clock.

3. **`MissedTickBehavior::Skip`** — follows existing repo pattern in
   `oracle_runtime.rs`, `main.rs`, and `gatekeeper_commit_loop.rs`. Prevents
   burst ticks after stalls.

4. **Bounded Normal window** — `elapsed < extended_window_ms` prevents Normal
   from firing after Extended window opens, restoring the intended DOW window
   semantics.

## Files changed

| File | Change |
|------|--------|
| `ghost-brain/src/config/gatekeeper_v25_config.rs` | Added `tick_interval_ms: u64` (default 250) |
| `ghost-brain/ghost_brain_config.toml` | Added `tick_interval_ms = 250` under `[gatekeeper_v2.dow]` |
| `ghost-launcher/src/components/gatekeeper.rs` | 8 edits: `extended_shadow_fired` field, `maybe_fire_shadow_checkpoint()`, Extended branch in `try_shadow_evaluate`, TX path refactor (event time), `check_long_deadline` timer-aware + assessment clone, `v25_shadow_decisions()` accessor |
| `ghost-launcher/src/components/gatekeeper_dow_timer.rs` | **New** — DOW interval helper with `MissedTickBehavior::Skip` |
| `ghost-launcher/src/components/mod.rs` | Added `pub mod gatekeeper_dow_timer` |
| `ghost-launcher/src/oracle_runtime.rs` | DOW interval as third `tokio::select!` branch in `pool_observation_task` |
| `ghost-launcher/src/oracle_metrics.rs` | Added `GATEKEEPER_DOW_TIMER_FIRED_TOTAL` counter + `record_dow_timer_fired()` helper |
| `ghost-launcher/tests/gatekeeper_v25_regression.rs` | 4 P0 integration tests + 2 regression test fixes |

## Test results

- **10/10** `gatekeeper_v25_regression` tests pass (including 4 new P0 tests)
- **180/181** gatekeeper lib tests pass (1 pre-existing failure in `gatekeeper_policy.rs` — outside P0 scope)
- **3/3** `gatekeeper_dow_timer` unit tests pass
- **`ghost-launcher` and `ghost-brain` compile without errors**

## DoD P0 checklist

- [x] Early/Normal/Extended fire na timerze, niezależnie od ruchu TX
- [x] Extended ma własną gałąź w `try_shadow_evaluate`, bez `unreachable!`
- [x] Timer kończy się przy `Approved`/`Rejected`/`deadline`
- [x] Single-owner invariant: timer, ingest i deadline fallback nie generują zdublowanych checkpointów
- [x] Telemetry: `GATEKEEPER_DOW_TIMER_FIRED_TOTAL{stage="Early|Normal|Extended"}`
- [x] Test integracyjny: pool z 0 TX → każdy stage odpala raz z `InsufficientData`
- [x] Test integracyjny: pool z TX w oknach → poprawny verdict per stage
- [x] Test race-safety: tick + nowy TX + deadline fallback → jeden checkpoint per stage
- [x] Okna Normal i Extended jako zamknięte przedziały czasowe
- [x] `MissedTickBehavior::Skip` dla DOW timera
- [x] Metryka Prometheus używa `with_label_values().inc()` (zgodnie z resztą oracle_metrics)
- [x] `check_long_deadline` klonuje shadow decisions do assessmentu
