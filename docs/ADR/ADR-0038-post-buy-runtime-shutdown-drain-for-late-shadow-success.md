# ADR-0038: PostBuyRuntime shutdown drain for late shadow success

**Date:** 2026-03-27
**Status:** Accepted
**Author:** Ghost Father

## Context

Paper burn-in closeout was blocked by a single `trace_correlation` orphan. The missing trace had a successful shadow simulation record and canonical BUY decision, but no matching paper lifecycle events. Forensics showed the shadow success completed while `PostBuyRuntime` was already processing shutdown: the runtime received shutdown at `2026-03-27T20:09:27.222Z`, while the affected shadow prepare/simulate path finalized around `2026-03-27T20:09:29.896Z` to `2026-03-27T20:09:30.107Z`.

`PostBuyRuntime` previously broke out of its subscriber loop immediately on shutdown, then only awaited already-started lifecycle handles. That created a race: late `PostBuySubmitted` events emitted by background shadow observers during graceful shutdown had no active consumer and produced no paper trace.

## Decision

`PostBuyRuntime` now enters a bounded shutdown-drain phase instead of terminating immediately. After receiving shutdown, it keeps consuming the event bus for `10_000 ms`, specifically allowing late `PostBuySubmitted` events from already-admitted shadow successes to start their paper lifecycle before final exit.

A regression test was added to verify that a `PostBuySubmitted` event emitted after shutdown begins is still processed through to `PositionClosed`.

## Architectural Impact

This changes shutdown semantics for the post-buy monitoring plane only:

- `ghost-launcher/src/components/post_buy_runtime.rs` now remains subscribed briefly during graceful shutdown.
- Background shadow-success hand-off remains event-bus based; no SSOT was moved out of the bus.
- Oracle runtime and shadow observer logic remain unchanged.

The broader system keeps the same architectural contract: authoritative BUY/shadow decisions originate upstream, and paper lifecycle remains fully owned by `ghost-brain`.

## Risk Assessment

**Rate:** Medium

Regression risk is limited to shutdown behavior:

- shutdown may take longer when late shadow successes arrive during the drain window,
- additional paper lifecycles can start during graceful stop, but only for already-emitted post-buy hand-offs,
- hidden tests around shutdown timing and lifecycle completion are directly affected.

No account layouts, Solana transaction flows, or SSOT data schemas are changed.

## Consequences

What becomes easier:

- graceful run closeout preserves trace correlation for late shadow successes,
- shutdown behavior now matches formal burn-in expectations,
- late hand-off races are reproducible and covered by test.

What becomes harder:

- shutdown is no longer instantaneous for the post-buy runtime,
- operators may observe a short drain window before final exit.

## Alternatives Considered

1. **Patch the report to treat gatekeeper `Candidate(pool_id)` as paper evidence**
   - Rejected because it would hide a real missing post-buy lifecycle and produce a false `GO`.

2. **Emit fallback paper events directly from oracle runtime on shutdown**
   - Rejected because it would duplicate `ghost-brain` lifecycle ownership and violate SSOT boundaries.

3. **Persist a dedicated durable post-buy queue**
   - Rejected for this phase as too invasive relative to the narrowly observed shutdown race.

## Validation Steps

1. Run the new regression test covering late `PostBuySubmitted` arrival during shutdown.
2. Reproduce paper-burnin closeout and confirm the former orphan candidate now produces paper lifecycle events.
3. Re-run `scripts/shadow_run_report.py` against the closed session and verify `trace_correlation` passes.
4. Inspect shutdown logs to confirm drain start, bounded drain completion, and orderly lifecycle flush.
