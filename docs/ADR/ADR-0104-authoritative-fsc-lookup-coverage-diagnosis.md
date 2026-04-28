# ADR-0104: Authoritative FSC Lookup Coverage Diagnosis

**Date:** 2026-04-17
**Status:** Accepted
**Author:** Ghost Father

## Context
An authoritative paper-burnin run enabled `seer.funding_lane_mode=full_chain` and showed:
- `fsc_authoritative_funding_stream_available = 1`
- `fsc_warmup_ready = 1`
- very low `fsc_lookup_hit_rate`

The objective was to diagnose why the authoritative lane could be alive while FSC lookup coverage remained poor.

## Decision
The investigation concluded that the most likely primary cause is cold/partial authoritative state rather than a dead lane:
- the dedicated full-chain funding lane is created with manual backfill explicitly disabled;
- lane warmup only requires stream connectivity plus one authoritative transfer;
- FSC lookups key strictly by `recipient_wallet == PoolTransaction.signer`;
- lookup ordering across different signatures in the same slot falls back to coarse event time and cross-stream arrival order.

The highest-value fix candidate is to add bounded authoritative funding-lane catch-up/backfill for `FundingLaneFullChain`, then tighten observability around lookup miss reasons.

## Architectural Impact
This affects:
- Seer funding-lane transport setup
- Yellowstone gRPC replay/backfill behavior
- Launcher-side `FundingSourceIndex` lookup semantics
- FSC readiness and operator interpretation of FSC metrics

The issue sits at the boundary between ingest completeness and runtime feature materialization.

## Risk Assessment
**Rate:** High

Low FSC coverage weakens sybil-resistance signal quality during rollout and can mislead operators because lane health gauges can look healthy while lookup usefulness remains poor.

## Consequences
- Makes clear that `warmup_ready=1` does not imply meaningful lookup coverage.
- Prioritizes authoritative replay/catch-up over policy tuning.
- Highlights secondary semantic risks around exact-wallet keying and same-slot cross-stream ordering.

## Alternatives Considered
1. **Treat raw full-chain event volume as sufficient evidence of coverage**
   - Rejected because `seer_events_received_total{source="grpc_funding_lane_full_chain"}` counts raw stream events, not extracted funding transfers or indexed recipient matches.
2. **Tune FSC thresholds only**
   - Rejected because the dominant gap appears upstream in population/lookup semantics, not downstream policy thresholds.
3. **Rely on current warmup semantics**
   - Rejected because one authoritative transfer is too weak a readiness contract for coverage-sensitive FSC behavior.

## Validation Steps
1. Add authoritative funding-lane backfill/catch-up and rerun paper-burnin.
2. Compare:
   - `seer_funding_transfer_observations_total{lane="authoritative_full_feed",coverage="full_chain_coverage"}`
   - `fsc_index_entries`
   - `fsc_lookup_hits_total`
   - `fsc_lookup_misses_total`
   - `fsc_lookup_hit_rate`
3. Add miss-reason telemetry distinguishing:
   - no recipient history,
   - recipient history present but no pre-buy match,
   - same-slot ordering ambiguity.
4. Confirm lookup hit-rate improves without relaxing authoritative semantics.
