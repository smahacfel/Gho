# ADR-0059: PR8 literal cleanup closure

**Date:** 2026-03-30
**Status:** Accepted
**Author:** Ghost Father

## Context

`ADR-0057` established that PR8 was already closed operationally as a production cutover, but not yet closed literally against the strongest wording in `PLANS/REFACTOR.md`.

`ADR-0058` then constrained the remaining work to a narrow, cleanup-only delta with four ordered stages:

1. remove legacy compat pool state from runtime,
2. remove deprecated inline scoring runtime API,
3. hard-fence `account_updates_enabled` as degraded/test-only compatibility ballast,
4. align reconciliation language and surface area with the already-declared monitoring-only contract.

The user explicitly authorized full autonomous execution of that delta without reopening PR3B / PR5 / PR6 / PR7 as workstreams and without adding adjacent improvements.

## Decision

We executed the full PR8 literal-cleanup delta and closed the remaining discrepancy between operational cutover and literal repo wording.

Implemented decisions:

1. **Compat pool state removed from production runtime ownership**
   - deleted `PerPoolOracleState` from production `ghost-launcher/src/oracle_runtime.rs`
   - deleted `OracleRuntime.pools`
   - rewrote `register_new_pool(...)`, `register_pool_tx(...)`, `pool_count()`, `mark_pool_scored(...)`, `prune_stale_pools(...)`, `inspect_candidate_reserves(...)`, and `score_pool(...)` onto registry/session/runtime-state truth
   - kept helper orphan adoption logic without reintroducing local compat per-pool state

2. **Deprecated inline scoring runtime API removed from public runtime surface**
   - replaced public `GatekeeperBuffer::on_transaction(...)` runtime usage with explicit test-support helper naming
   - replaced public `PoolObservationSession::on_transaction(...)` runtime usage with explicit test-support helper naming
   - migrated integration tests to `legacy_test_verdict_from_transaction(...)`
   - kept only `#[cfg(test)]` compatibility wrappers for unit-test scope; no public runtime `on_transaction(...)` remains for Gatekeeper/session surfaces

3. **`account_updates_enabled` reduced to explicit degraded/test compatibility semantics**
   - clarified in launcher and Seer config/docs/comments that production canonical ingest is derived from `AccountStateCore` enablement, not from this field
   - added the missing contradictory-config warning for `account_updates_enabled=true` while `[account_state_core].enable=false`
   - preserved the flag only as a representable degraded/test compatibility path rather than a normal production selector

4. **Reconciliation surface aligned to monitoring-only semantics**
   - updated reconciliation docs/comments so severe drift is described as diagnostic escalation rather than active repair/reset
   - retained `ReconciliationAction::Repaired` and `total_repairs` only as legacy compatibility ballast, explicitly documented as dormant in the monitoring-only runtime
   - updated `OracleRuntime` reconciliation health/log language to report `legacy_repairs` rather than implying an active repair engine

## Architectural Impact

This ADR does **not** introduce a new architecture.

It tightens the repo so that the codebase now matches the architecture already enforced in production:

- `SessionManager` / `PoolObservationSession` remain the runtime owners,
- `AccountStateCore` remains canonical truth,
- Gatekeeper remains feature-driven,
- Seer `AccountUpdate` ingest remains mandatory whenever core-enabled startup is active,
- reconciliation remains observational and non-authoritative relative to the canonical runtime state.

The impact is therefore on repository truthfulness, public/runtime surface area, and operator-facing semantics rather than on production decision math or ownership.

## Risk Assessment

**Risk Level:** Medium

Why medium:

- Stage 1 rewired several legacy helper paths in `oracle_runtime.rs`.
- Stage 2 changed test/support entrypoints and required integration-test migration.
- Stage 3 adjusted runtime warnings and config narrative around a legacy flag.
- Stage 4 changed observability wording around reconciliation and legacy compat counters.

Why not high:

- the production hot path was already cut over before this ADR,
- no policy math or SSOT ownership was changed,
- validation stayed focused on compile/build integrity and the targeted integration surfaces touched by the cleanup.

## Consequences

### Positive

- PR8 is now closed not only operationally but also literally against the cleanup delta defined in `ADR-0058`.
- production code no longer carries the old compat per-pool state container.
- Gatekeeper/session runtime surface no longer exposes deprecated inline scoring as a public path.
- `account_updates_enabled` no longer reads like a competing production ownership switch.
- reconciliation no longer advertises itself as an active repair engine in production logs/docs.

### Negative / Trade-offs

- explicit test-support helpers remain for legacy-verdict parity tests instead of deleting every last legacy-verdict code path.
- dormant compatibility counters (`ReconciliationAction::Repaired`, `total_repairs`) still exist for compatibility rather than being fully deleted in this delta.
- compile output still contains unrelated workspace warnings outside the PR8 cleanup scope.

## Alternatives Considered

### 1. Stop after Stage 2 and leave Seer/reconciliation semantics as-is

Rejected.

Reason: that would leave the repo still semantically ambiguous about production account-update ownership and reconciliation authority.

### 2. Fully delete every last legacy-verdict helper and dormant repair compatibility type

Rejected for this delta.

Reason: it would expand the workstream beyond the narrow cleanup boundary authorized in `ADR-0058` and risk unnecessary collateral test churn.

### 3. Reopen PR3B / PR5 / PR6 / PR7 as architecture workstreams

Rejected.

Reason: the user explicitly prohibited reopening those workstreams. The remaining work was cleanup, not architectural migration.

## Validation Steps

Validated in this session with targeted build/test execution:

1. Stage 1 + Stage 2 runtime/test validation:
   - `cargo test -q -p ghost-launcher --lib --no-run`
   - `cargo test -q -p ghost-launcher --test session_lifecycle_tests -- --nocapture`
   - `cargo test -q -p ghost-launcher --test full_pipeline_integration -- --nocapture`
   - `cargo test -q -p ghost-launcher --test gatekeeper_policy_tests -- --nocapture`
   - `cargo test -q -p ghost-launcher --test snapshot_engine_integration -- --nocapture`
   - `cargo test -q -p ghost-launcher --test tx_intelligence_tests -- --nocapture`
   - `cargo test -q -p ghost-launcher --test genesis_repro_check -- --nocapture`
   - `cargo test -q -p ghost-launcher --test oracle_event_bus_integration -- --nocapture`

2. Final delta validation after Stage 3 / Stage 4 cleanup:
   - `cargo test -q -p ghost-core --lib --no-run`
   - `cargo test -q -p ghost-launcher --lib --no-run`
   - `cargo test -q -p ghost-launcher test_pr8_startup_forces_account_updates_when_account_state_core_enabled -- --nocapture`
   - repeated targeted `ghost-launcher` integration suite listed above

3. Repo-level acceptance checks performed in-session:
   - verified no production `PerPoolOracleState`
   - verified no production `OracleRuntime.pools`
   - verified no public runtime `GatekeeperBuffer::on_transaction(...)`
   - verified no public runtime `PoolObservationSession::on_transaction(...)`
   - verified launcher warning semantics keep `account_updates_enabled` outside the production ownership contract
   - verified reconciliation surface now describes monitoring-only behavior rather than active repair authority
