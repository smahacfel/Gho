# ADR-0001: Seer session gate hardening

**Date:** 2026-03-17  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

The launcher-side Seer bridge had already been updated to buffer tx-first trades until an in-session `PoolDetected` arrived, but the implementation still had several operational gaps:
- no duplicate suppression inside the short race buffer,
- pending-trade expiry happened only on subsequent bridge activity,
- the detected-pool session registry could grow without lifecycle control,
- tests validated helper behavior but did not validate real `SeerEvent -> GhostEvent` bridge semantics,
- an existing Oracle event-bus integration test expected a `PoolScored` bus event that is not emitted by the current `start_oracle_runtime_task` contract.

These gaps risked duplicate replays, stale in-memory state, and misleading regression signals during validation.

## Decision

Harden the launcher-local Seer session gate without modifying Seer core or public SSOT contracts.

Implemented changes:
- Added dedupe keys for buffered session trades using `(pool, signature, event_ordinal)`.
- Added active periodic pruning for both pending trade buffer TTL and detected-pool registry TTL.
- Bound detected-pool session registry using runtime-derived TTL/cap aligned with Seer watched-pool settings.
- Added internal bridge helpers that process actual `SeerEvent` payload semantics into event-bus emissions.
- Added tests covering:
  - no `PoolTransaction` emission for tx-first trades without in-session `PoolDetected`,
  - exactly-once replay after `PoolDetected`,
  - duplicate suppression,
  - detected-pool registry TTL pruning.
- Updated stale test call sites for `start_oracle_runtime_task(..., account_updates_enabled)`.
- Corrected `oracle_event_bus_integration` to validate the real runtime contract (observation completion / cleanup), not a nonexistent `GhostEvent::PoolScored` emission.

## Architectural Impact

The authoritative filter boundary remains in `ghost-launcher/src/components/seer.rs`, not in Seer core. This preserves Seer’s recovery and mapping semantics while ensuring launcher-local session canonicalization.

Operationally:
- duplicate tx-first bursts no longer amplify replay volume,
- stale buffered trades and stale detected-pool registry entries are reclaimed proactively,
- validation now covers the actual event-bus boundary used in production.

## Risk Assessment

**Rate:** Medium

Primary regression risks:
- over-aggressive registry pruning could forget a long-idle pool during a very long session,
- dedupe keyed by `(pool, signature, event_ordinal)` assumes this tuple is stable for bridge replay identity,
- aligning registry cap/TTL with Seer watched-pool limits couples lifecycle behavior more tightly to runtime config.

Mitigations:
- pruning/cap are launcher-local and do not mutate Seer core state,
- replay behavior is now explicitly covered by tests,
- runtime config values are reused instead of inventing an unrelated lifecycle policy.

## Consequences

What becomes easier:
- cold-start spam protections are harder to bypass,
- bridge memory pressure is controlled,
- regression detection is more precise.

Trade-offs:
- session registry semantics are no longer “remember forever”; they are bounded by runtime lifecycle policy,
- the bridge now carries slightly more internal bookkeeping.

## Alternatives Considered

### 1. Modify Seer core directly
Rejected because Seer intentionally supports create/trade race recovery and broader mapping semantics that should remain intact.

### 2. Keep event-driven expiry only
Rejected because stale state could persist indefinitely during quiet periods.

### 3. Add a public test-only API for external integration tests
Rejected to avoid widening public contracts solely for tests.

### 4. Preserve the `PoolScored` timeout test as-is
Rejected because the test asserted behavior not implemented by the runtime contract.

## Validation Steps

- Run `cargo test -p ghost-launcher --lib components::seer -- --nocapture`
- Run `cargo test -p ghost-launcher --test seer_shadow_ledger_bridge_tests -- --nocapture`
- Run `cargo test -p ghost-launcher --test oracle_event_bus_integration -- --nocapture`
- Confirm no compile errors in `ghost-launcher/src/components/seer.rs`
- Confirm stale `start_oracle_runtime_task` call sites compile in touched test files
