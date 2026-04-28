# ADR-0090: Dedup is structural, not a signature/timestamp heuristic

**Date:** 2026-04-09
**Status:** Accepted
**Author:** Ghost Father

## Context

A follow-up claim asserted that the observed coverage gap could be primarily explained by `dedup_trade_events(...)`, allegedly because the dedup logic behaves like a heuristic over:

- signature,
- mint,
- side,
- near timestamp proximity,

and therefore rapid Mayhem-style buy/sell sequences inside one transaction could be dropped as duplicates.

This claim needed verification against the actual dedup implementation and tests in `off-chain/components/seer/src/binary_parser.rs`.

## Decision

The claim is **not supported by the current implementation**.

Accepted findings:

1. `dedup_trade_events(...)` does **not** use a heuristic over signature, timestamp, or "close in time" events.
2. Pump.fun dedup drops an ix-level `ParsedEventKind::Trade` only when a matching `ParsedEventKind::CpiTrade` exists with:
   - same side,
   - same mint,
   - same curve mapping when registry data is available.
3. PumpSwap dedup drops an ix-level `ParsedEventKind::SwapTrade` only when a matching `CpiSwapBuy` / `CpiSwapSell` exists with:
   - same side,
   - same pool,
   - same resolved base mint when registry data is available,
   - or when pool-side match exists but registry has not yet resolved the mint.
4. Unit tests explicitly assert that dedup prefers the CPI event and keeps it while dropping only the lower-confidence ix-level duplicate.
5. `parse_geyser_transaction(...)` — the backfill path highlighted in the discussion — does **not** call `dedup_trade_events(...)` at all.

Therefore:

- dedup may still be relevant for disagreements between ix-level and CPI-level event shape,
- but it is **not** a generic timestamp-nearness suppressor,
- and it is **not** evidence that rapid Mayhem-style multi-trade bursts are being collapsed merely for being close together.

## Architectural Impact

This decision narrows the dedup discussion to what the code actually does:

- dedup is a structural arbitration layer between two representations of the same trade,
- not a time-series clustering heuristic.

This matters because debugging should focus on:

- whether ix and CPI events are truly alternate views of one trade,
- whether registry resolution changes the arbitration outcome,
- whether provenance should be preserved separately from dedup decisions.

## Risk Assessment

**Rate:** Low

- Low risk that current dedup logic is silently collapsing unrelated Mayhem bursts based on time proximity, because the code does not do that.
- Medium risk remains for semantic arbitration mistakes if two structurally similar events are not actually duplicates.

## Consequences

What becomes easier:

- Teams can stop reasoning about dedup as a hidden timestamp-based compressor.
- Future debugging can concentrate on event semantics, registry state, and provenance.

What becomes harder:

- Any real dedup bug must now be proven with a concrete structural counterexample, not with generic "rapid flow" speculation.

## Alternatives Considered

### 1. Accept the timestamp-heuristic theory

Rejected because the implementation contains no such fields or comparisons in the dedup logic.

### 2. Dismiss dedup entirely from future debugging

Rejected because dedup still performs structural arbitration and can matter when ix and CPI contain complementary information.

## Validation Steps

1. Inspect `has_matching_pumpfun_cpi(...)` and `has_matching_pumpswap_cpi(...)`.
2. Verify absence of signature/timestamp comparisons in `dedup_trade_events(...)`.
3. Run unit tests asserting CPI preference over ix-level duplicates.
4. If future concern remains, construct a regression test with multiple same-side trades in one transaction proving whether dedup over-collapses them.
