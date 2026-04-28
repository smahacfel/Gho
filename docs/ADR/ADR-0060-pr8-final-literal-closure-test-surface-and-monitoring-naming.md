# ADR-0060: PR8 final literal closure on test surface and monitoring naming

**Date:** 2026-03-30
**Status:** Accepted
**Author:** Ghost Father

## Context

`ADR-0059` declared PR8 literally closed after the planned cleanup delta, but a follow-up repo audit against `PLANS/PLAN_DELTA_LITERAL_PR8_CLEANUP_20260330.md` and `PLANS/REFACTOR.md` found three remaining literal mismatches:

1. external integration tests in `ghost-launcher/tests/*` still depended on deprecated inline-verdict helpers (`legacy_test_verdict_from_transaction(...)`),
2. those helpers therefore still existed in production builds as public methods on `PoolObservationSession` and `GatekeeperBuffer`,
3. reconciliation observability still exposed residual repair-centric naming (`repair threshold`, `*_repairs_total`, `legacy_repairs=`) even though the runtime contract is monitoring-only.

The user explicitly requested that every concrete gap from that audit be fixed in code, with no scope expansion beyond the PR8 literal-cleanup boundary.

## Decision

We executed one final PR8 cleanup pass focused only on the remaining literal mismatches.

Implemented decisions:

1. **All external integration tests were migrated off deprecated inline-verdict helpers**
   - replaced integration-test use of `PoolObservationSession::legacy_test_verdict_from_transaction(...)` with `ingest_transaction(...)`
   - rewrote feature-policy tests to seed sessions via production ingest and then evaluate via materialized features
   - rewrote the SnapshotEngine integration path to use `PoolObservationSession` + `GatekeeperIngressOutcome` + feature-driven evaluation instead of `GatekeeperBuffer::legacy_test_verdict_from_transaction(...)`
   - updated the affected `oracle_runtime` unit tests to seed sessions with `ingest_transaction(...)` instead of the deprecated session wrapper

2. **Legacy inline-verdict helpers were removed from production surface**
   - `PoolObservationSession::legacy_test_verdict_from_transaction(...)` is now test-only (`#[cfg(test)] pub(crate)`) rather than a production-visible method
   - `PoolObservationSession::on_transaction(...)` remains test-only compatibility ballast only
   - `GatekeeperBuffer::legacy_test_verdict_from_transaction(...)` is now test-only (`#[cfg(test)] pub(crate)`) rather than a production-visible method
   - no external integration test depends on those helpers anymore

3. **Monitoring-only reconciliation naming was aligned with the dormant-compat reality**
   - `DriftSeverity::Meaningful` documentation now references the severe threshold rather than a repair threshold
   - dormant reconciliation metrics were renamed from repair-centric wording to legacy-signal wording:
     - `oracle_runtime_reconciliation_cycle_legacy_repair_signals_total`
     - `shadow_ledger_runtime_legacy_repair_signals_total`
   - `OracleRuntime` health logging now emits `legacy_repair_signals=` instead of `legacy_repairs=`
   - nearby reconciliation test comments/variable names were updated so diagnostic-only behavior is described as retained / unchanged state rather than repaired state

## Architectural Impact

This ADR does not change the runtime authority model.

The architecture remains:

- `PoolObservationSession` is the runtime observation owner,
- production cutover stays on `ingest_transaction(...)` + feature-driven evaluation,
- `AccountStateCore` remains canonical state authority,
- reconciliation remains observational and non-authoritative,
- legacy inline-verdict helpers remain available only as tightly fenced test-only ballast.

The impact is therefore on **literal repository truth**, **test coupling**, and **operator-facing observability naming**, not on production decision math.

## Risk Assessment

**Risk Level:** Medium

Why medium:

- integration tests were rewired to different public APIs,
- the SnapshotEngine integration test moved from direct Gatekeeper helper usage to session-driven evaluation,
- dormant reconciliation metric names changed and may require dashboard/query follow-up if any out-of-scope consumers referenced them.

Why not high:

- production ownership and scoring math were not changed,
- the helper removals were fence-tight (`#[cfg(test)]`) rather than broad deletions,
- all touched paths were revalidated with targeted cargo test runs in this session.

## Consequences

### Positive

- PR8 now matches its literal cleanup wording on the remaining test/public-surface gap.
- external tests exercise the same production ingest contract that runtime code uses.
- deprecated inline-verdict helpers no longer leak into production builds.
- reconciliation observability no longer advertises active repair semantics where only legacy compatibility counters remain.

### Negative / Trade-offs

- test-only helper ballast still exists inside `#[cfg(test)]` scope instead of being fully deleted.
- dormant metric names changed, so any out-of-band dashboards keyed to the old strings must be updated separately.
- `ADR-0059` is no longer the final word on PR8 literal closure; this ADR supersedes its optimistic closure claim.

## Alternatives Considered

### 1. Keep `ADR-0059` as-is and accept the remaining external test/helper coupling

Rejected.

Reason: the repo would still fail the explicit PR8 literal-cleanup wording around deprecated inline scoring runtime API.

### 2. Fully delete every remaining test-only inline-verdict helper

Rejected for this pass.

Reason: the user asked to close the audited gaps only, not expand the workstream into broader test-infrastructure deletion.

### 3. Keep old repair-centric metric names for dashboard compatibility

Rejected.

Reason: the explicit audit gap was residual repair-centric surface language in a monitoring-only subsystem. Literal PR8 closure required naming cleanup, not just behavior cleanup.

## Validation Steps

Validated in this session with targeted test execution:

1. External integration suites updated away from inline-verdict helpers:
   - `cargo test -p ghost-launcher --test session_lifecycle_tests --test full_pipeline_integration --test tx_intelligence_tests --test gatekeeper_policy_tests --test snapshot_engine_integration`

2. `oracle_runtime` cutover/unit verification after switching helper seeding to `ingest_transaction(...)`:
   - `cargo test -p ghost-launcher --lib feature_driven_terminal_verdict && cargo test -p ghost-launcher --lib resolve_feature_trigger_outcome_terminalizes_without_legacy_on_transaction`
   - `cargo test -p ghost-launcher --lib cutover_feature_driven_terminal_verdict_can_override_legacy_reject_with_feature_buy && cargo test -p ghost-launcher --lib cutover_feature_driven_terminal_verdict_resumes_after_pending_curve`

3. Monitoring-only reconciliation verification after naming cleanup:
   - `cargo test -p ghost-core --lib reconciliation_runtime && cargo test -p ghost-core --lib reconciliation`

4. Repository acceptance checks performed in-session:
   - verified no external integration test still calls `legacy_test_verdict_from_transaction(...)`
   - verified no `oracle_runtime` unit test still seeds sessions through `session.on_transaction(...)`
   - verified old repair-centric metric strings/log label were removed from touched sources
