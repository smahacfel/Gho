# ADR-0091: Parser evolution prioritizes provenance before dedup rewrite

**Date:** 2026-04-09
**Status:** Accepted
**Author:** Ghost Father

## Context

The current Seer parser has two distinct concerns that were being conflated during debugging:

1. trade detection,
2. execution-tree provenance / wrapper attribution.

The codebase currently:

- detects Pump/PumpSwap trades across live and decoded paths,
- preserves `event_ordinal` and bridge compatibility downstream,
- lacks consistent use of `InnerInstructionGroup.index` in parser walkers,
- contains two different arbitration layers:
  - `dedup_trade_events(...)` on `ParsedPumpEvent`,
  - `dedup_trade_candidates(...)` on `TradeEvent`.

There was pressure to jump directly into dedup rewrites or Mayhem-specific classification. That would increase blast radius before the system can even explain where provenance and candidate arbitration diverge.

## Decision

The accepted parser evolution strategy is:

1. instrument current behavior first,
2. add execution-tree provenance second,
3. propagate provenance through `TradeEvent` and the launcher bridge,
4. only then reconsider arbitration/dedup behavior,
5. defer wrapper taxonomy such as explicit Mayhem classification until raw provenance data exists.

Key implementation principles:

- Do not overload `EventSemanticEnvelope` with wrapper/raw execution metadata.
- Introduce a dedicated provenance payload for parser-origin context.
- Keep live and decoded/backfill paths convergent.
- Treat dedup rewrites as data-driven follow-up work, not phase-one surgery.

## Architectural Impact

This decision establishes a strict ordering for parser work:

- provenance becomes first-class,
- arbitration becomes observable,
- wrapper classification becomes optional and evidence-driven.

It also reduces the risk of introducing regressions by changing the meaning of trades before the system can measure where information is actually lost.

## Risk Assessment

**Rate:** Medium

- **Low** immediate risk to current production behavior because early phases are telemetry/provenance focused.
- **Medium** risk of continued uncertainty until instrumentation is in place, but this is preferable to speculative semantic changes.
- **High** regression risk would exist if dedup logic were rewritten before provenance and metrics were added.

## Consequences

What becomes easier:

- Root-cause analysis can distinguish missing trade detection from lost attribution.
- Future Mayhem/wrapper support can be built on measured raw provenance.
- Dedup debates can be settled with metrics rather than intuition.

What becomes harder:

- The team must accept a staged rollout instead of a one-shot rewrite.
- Some desired classifications are deferred until provenance exists.

## Alternatives Considered

### 1. Rewrite dedup first

Rejected because the codebase contains two arbitration layers and there is insufficient provenance to know which one is responsible for any observed collapse.

### 2. Add explicit Mayhem classification immediately

Rejected because the parser does not yet preserve the raw outer-program provenance needed to classify wrapper flows confidently.

### 3. Keep the parser unchanged and only improve downstream analytics

Rejected because parser-local provenance loss would continue to obscure root cause and make downstream fixes speculative.

## Validation Steps

1. Add metrics and reason codes around both arbitration layers.
2. Add `group.index`-based execution provenance in both live and decoded walkers.
3. Verify provenance survives conversion into `TradeEvent`.
4. Verify launcher bridge still preserves `event_ordinal` and timestamp contract.
5. Re-evaluate arbitration changes only after measured before/after counts exist on real rollout samples.
