# ADR-0039: Paper burn-in post-fix run still NO-GO due to early closeout and negative PnL

**Date:** 2026-03-27
**Status:** Accepted
**Author:** Ghost Father

## Context

After implementing the `PostBuyRuntime` shutdown-drain fix, a new paper burn-in run was executed and then closed for formal validation.

The formal report for the latest session window (`session_start_ms=1774646609959`) produced:

- `trace_correlation = passed`
- `paper_lifecycle_complete = failed`
- `economics_not_fatal = failed`
- overall verdict: `NO-GO`

The new run therefore had to be analyzed to determine whether a fresh runtime bug remained or whether the remaining blockers were operational/economic.

## Decision

The analysis establishes three facts:

1. **The trace-correlation fix worked.**
   - Formal report now shows `decision_without_shadow=0` and `shadow_without_paper=0`.
   - The prior orphaned shadow-success → paper-lifecycle handoff blocker is closed.

2. **The remaining inflight paper position was caused by early closeout, not a missing lifecycle trace.**
   - Candidate:
     `AsnJT1Gb798YGanqzPtZNtiXuDLhao6G4NRpMHAepump_5juXDV7CnQjfEi9VACtKjVeCAbwdioyaorc55T2NR1UU_1774647162647`
   - Shadow simulation succeeded at `1774647163140`.
   - Paper lifecycle emitted:
     - `Candidate`
     - `EntrySubmitted`
     - `EntryFilled`
     - `PositionOpened`
     - then continuous `AemTick` / `ManagementDecision`
   - No `PositionClosed` was emitted before session end.
   - Runtime wiring in `ghost-launcher/src/main.rs` confirms:
     - `tick_interval_ms = 500`
     - `max_ticks_before_exit = 240`
     - `aem_t_s = 120`
   - Therefore the paper lifecycle safety-net horizon is approximately `120s` from open.
   - This candidate opened at `1774647163400` and was still emitting lifecycle ticks at `1774647250810`, i.e. roughly `87.4s` after open.
   - The run was closed before the lifecycle reached its configured auto-exit horizon; the candidate was cut off roughly `32.6s` too early.

3. **Economics failure is independent of the inflight closeout issue.**
   - A frozen-slice report ending just before the final candidate (`--session-end-ms 1774647162646`) yields:
     - `paper_lifecycle_complete = passed`
     - `trace_correlation = passed`
     - `economics_not_fatal = failed`
     - `total_net_pnl_sol = -0.000005771084625501386`
   - This proves the economics blocker is real for the closed-position subset and is not merely a side-effect of the late inflight candidate.

No new runtime patch is warranted based on this run alone.

## Architectural Impact

This ADR does not change architecture. It clarifies operational interpretation of the current architecture:

- `PostBuyRuntime` handoff durability is now sufficient for closeout trace correlation.
- Paper lifecycle completion remains bounded by configured lifecycle horizon, not by session-stop intent.
- Formal burn-in verdict remains sensitive to both:
  - unclosed paper positions at closeout time,
  - realized net PnL across closed paper positions.

## Risk Assessment

**Rate:** Medium

- If operators close a burn-in run before the last admitted paper position reaches its configured exit horizon, `paper_lifecycle_complete` may fail even with correct runtime behavior.
- If operators interpret a lifecycle-closeout failure as a runtime regression without checking lifecycle age versus configured horizon, they may trigger unnecessary code churn.
- Economics gate remains a real promotion blocker; forcing a go/no-go override without explicit authorization would break rollout discipline.

## Consequences

What becomes easier:

- Distinguishing runtime regressions from operator closeout timing.
- Reusing the frozen-slice method to isolate lifecycle versus economics failures.
- Trusting that the shutdown-drain fix removed the original orphan-trace defect.

What becomes harder:

- Operators must now respect lifecycle horizon when deciding the exact moment to stop a run.
- A run can still remain `NO-GO` after a runtime fix if economic outcomes remain slightly negative.

## Alternatives Considered

1. **Treat the latest inflight candidate as a new lifecycle bug**
   - Rejected because the candidate has a full paper lifecycle trace through `PositionOpened` and ongoing management ticks; only `PositionClosed` is missing before operator stop.

2. **Treat economics failure as noise and ignore it**
   - Rejected because the formal gate is configured with `min_net_pnl_sol = 0.0`, and no explicit operator override was provided.

3. **Patch the report to ignore inflight positions near shutdown**
   - Rejected because that would weaken the rollout contract instead of respecting configured lifecycle semantics.

## Validation Steps

1. Use the formal report on the latest session and confirm:
   - `trace_correlation = passed`
   - `paper_lifecycle_complete = failed`
   - `economics_not_fatal = failed`
2. Inspect the inflight candidate event stream and confirm:
   - admission + fill + open exist,
   - repeated management ticks exist,
   - no close event exists before session end.
3. Verify runtime lifecycle defaults in `ghost-launcher/src/main.rs`:
   - `tick_interval_ms = 500`
   - `max_ticks_before_exit = 240`
   - `aem_t_s = 120`
4. Run a frozen-slice report ending before the final candidate and confirm lifecycle becomes green while economics remains red.
5. For the next operational validation run, do not stop the session until at least one full lifecycle horizon has elapsed after the last admitted paper candidate.
