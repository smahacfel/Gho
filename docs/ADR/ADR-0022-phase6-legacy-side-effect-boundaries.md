# ADR-0022: Phase 6 legacy side-effect boundaries

**Date:** 2026-03-22  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

Phase 6 of `PLAN_UPORZADKOWANIA_ARCHITEKTURY_PIPELINE_20260320.md` requires a hard separation between canonical runtime execution and historical/compatibility paths that may still observe or score the same pool lifecycle.

Before this change, the codebase was already semantically close to the desired state:
- canonical BUY execution happened in `OracleRuntime -> TriggerComponent`,
- `Trigger` treated `PoolScored` as effectively ignored,
- `PostBuyRuntime` already acted as a thin adapter,
- compare-only shadow execution already existed.

However, the contract was still partly implicit:
- legacy paths were not classified in code,
- blocked legacy side-effect attempts were not counted,
- logs did not use a shared execution-plane vocabulary,
- `Trigger` still looked like a component that might own scoring/execution authority if someone read the code shallowly.

That was acceptable for folklore, not acceptable for closure mode.

## Decision

We introduced an explicit Phase-6 contract for execution boundaries:

1. Added shared execution-plane semantics in `ghost-launcher/src/events.rs`:
   - `canonical_decision`
   - `legacy_observation`
   - `shadow_simulation`
   - `post_buy_monitoring`

2. Added a typed legacy-path descriptor model:
   - `LegacyPathDescriptor`
   - `LegacyPathClassification`

3. Classified the active Trigger legacy surfaces in code:
   - `trigger_pool_scored_observer` → `observability_only`
   - `trigger_embedded_oracle_pipeline` → `compatibility_only`
   - `trigger_no_event_bus_fallback` → `disabled_in_production`

4. Added explicit metrics for legacy-path observability:
   - `legacy_path_event_total{path=...}`
   - `legacy_path_side_effect_block_total{path=...}`

5. Made the legacy `PoolScored` path a first-class blocked path:
   - `PoolScored(passed=true)` no longer merely “does nothing” by convention,
   - it now records a blocked side-effect attempt and logs under `runtime_plane=legacy_observation`.

6. Normalized logging across the execution boundary:
   - canonical BUY logs use `runtime_plane=canonical_decision`,
   - compare-only shadow execution uses `runtime_plane=shadow_simulation`,
   - `PostBuyRuntime` uses `runtime_plane=post_buy_monitoring`.

## Architectural Impact

This change does not alter the canonical BUY authority path.

The authoritative execution path remains:

`PoolTransaction -> OracleRuntime -> TriggerComponent -> TransactionSent/PostBuySubmitted`

What changes is that the competing historical surfaces are now explicitly modeled as non-authoritative.

This reduces ambiguity in:
- code review,
- incident triage,
- metrics interpretation,
- future deletion work for Trigger-side legacy scoring surfaces.

Components directly affected:
- `ghost-launcher/src/events.rs`
- `ghost-launcher/src/components/trigger/component.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/components/post_buy_runtime.rs`
- runbook / architecture ADR documentation

## Risk Assessment

**Rate:** Medium

Primary regression risks:
1. accidental compile issues in `trigger/component.rs` due to newly introduced helpers,
2. metric cardinality drift if path labels are later expanded casually,
3. false operator assumptions if someone treats `legacy_path_event_total` as canonical execution telemetry.

Mitigations:
- legacy paths use a tiny, fixed label set,
- descriptors encode `allows_authoritative_buy=false`,
- tests assert the blocked semantics for `PoolScored`.

## Consequences

### Positive
- Canonical vs legacy vs shadow vs post-buy boundaries are now explicit.
- Operators can distinguish “legacy activity still exists” from “legacy activity executed a BUY”.
- Phase 6 exit criteria become testable and auditable.

### Negative / Trade-offs
- Trigger now carries a small amount of explicit legacy-boundary metadata.
- There is modest extra logging/metrics noise for legacy-path observations.
- This does not delete legacy code yet; it fences it first.

## Alternatives Considered

### 1. Leave current behavior as-is and rely on comments
Rejected because the plan explicitly forbids relying on naming convention or lore for runtime guard semantics.

### 2. Delete all legacy Trigger surfaces immediately
Rejected because Phase 6 is about hard separation of side effects, not broad deletion/refactor of legacy code in one jump.

### 3. Add only metrics without typed path classification
Rejected because metrics without a typed contract still leave ambiguity in code review and future maintenance.

## Validation Steps

1. Run targeted unit tests for new Trigger legacy-path helpers.
2. Run targeted event-plane tests in `events.rs`.
3. Re-run focused OracleRuntime/Trigger tests to confirm canonical BUY path still emits expected events.
4. Verify logs show:
   - `runtime_plane=canonical_decision`
   - `runtime_plane=legacy_observation`
   - `runtime_plane=shadow_simulation`
   - `runtime_plane=post_buy_monitoring`
5. Confirm `PoolScored(passed=true)` increments `legacy_path_side_effect_block_total` rather than causing a BUY execution.
