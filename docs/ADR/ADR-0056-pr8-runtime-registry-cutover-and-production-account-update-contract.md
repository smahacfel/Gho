# ADR-0056: PR8 Runtime Registry Cutover and Production AccountUpdate Contract

**Date:** 2026-03-30
**Status:** Accepted
**Author:** Ghost Father

## Context

`PLANS/REFACTOR.md` and `docs/ADR/ADR-0054-refactor-pr-by-pr-forensic-matrix.md` defined PR8 as the final closure step after the session-first and canonical-account-state workstreams. A fresh re-audit of the current repository state showed that PR3B, PR5, PR6, and PR7 are already materially wired in the active runtime path: production observation uses `SessionManager`, materialized feature evaluation, and `AccountStateCore`-first pricing/truth.

The remaining PR8 gap was not missing functionality but incomplete cutover. Production runtime ownership still retained legacy compatibility hooks around `PerPoolOracleState`, `OracleRuntime.pools`, and fallback lookups that could still consult compat state. In parallel, launcher startup still allowed `oracle.account_updates_enabled` to disable the canonical AccountUpdate ingest path even when `AccountStateCore` was enabled, which contradicted the intended production SSOT contract.

## Decision

The runtime was cut over so that production pool detection and canonical metadata lookup use session/runtime registries rather than the legacy compat pool map.

Implemented decisions:
- `GhostEvent::NewPoolDetected` now registers runtime pool identity through a registry-based path that does **not** populate `OracleRuntime.pools`.
- Production lookup helpers (`lookup_base_mint_for_pool`, shadow metadata resolution, session-open preparation, bonding-curve refresh) no longer rely on `PerPoolOracleState` fallback state.
- `mark_pool_committed` and `remove_pool` now recognize registry-backed/runtime-state-backed pools even when no legacy compat entry exists.
- Launcher startup now forces AccountUpdate ingest on whenever `[account_state_core].enable=true`; `oracle.account_updates_enabled=false` is treated as degraded/test-only compatibility and emits a warning instead of disabling canonical ingest.
- Documentation/comments/config semantics were updated to reflect that AccountUpdate ingest is canonical in production, while `account_updates_enabled=false` remains only a degraded/test fallback.
- `PerPoolOracleState` and `register_new_pool` remain in place strictly for legacy test/support scenarios, not as production runtime SSOT.

## Architectural Impact

This decision tightens the runtime ownership boundary:
- **Production pool lifecycle SSOT:** `SessionManager` + `PoolIdentityRegistry` + runtime pool state registry.
- **Canonical reserve/price truth:** `AccountStateCore` first, with `ShadowLedger` limited to bootstrap/live-pipeline/forensics fallbacks where explicitly preserved.
- **Compat state:** `PerPoolOracleState` is demoted to legacy test-support only.

The result reduces the risk of split-brain runtime state between active session/registry state and compat pool state, and it formalizes production startup so Seer IPC AccountUpdate flow cannot be silently disabled while canonical account-state processing is otherwise enabled.

## Risk Assessment

**Rate:** Medium

Regression risk exists in pool lifecycle transitions because runtime registration, commit marking, and pool removal now accept registry-backed pools without requiring a compat-map entry. If broken, symptoms would include stuck tracked pools, incomplete cleanup, or missed committed-state transitions. The account-update startup contract change also affects rollout expectations for configs that previously relied on `oracle.account_updates_enabled=false` while still enabling `AccountStateCore`.

## Consequences

Positive consequences:
- Production runtime ownership is simpler and more explicit.
- PR8 now closes the remaining active dependence on legacy compat runtime state.
- AccountUpdate ingest behavior is consistent with `AccountStateCore` as production SSOT.
- Structural tests now guard against accidental reintroduction of `register_new_pool` into the live `NewPoolDetected` path or fallback lookups into `self.pools`.

Trade-offs:
- Legacy compat fields remain in the codebase for tests/support, so PR8 is a production cutover rather than full symbolic deletion.
- Configs that expected to disable AccountUpdate ingest while keeping `AccountStateCore` enabled now receive warnings and no longer alter production behavior.

## Alternatives Considered

### 1. Full deletion of `PerPoolOracleState` and `OracleRuntime.pools` immediately
Rejected because the codebase still contains test/support flows that explicitly depend on these structures. A production cutover with guarded compat retention is lower risk and stays within PR8 scope.

### 2. Leave `NewPoolDetected` on `register_new_pool` and only update documentation
Rejected because that would preserve active production dependence on the legacy compat pool map, leaving PR8 substantively open.

### 3. Continue honoring `oracle.account_updates_enabled` as a production disable switch
Rejected because it conflicts with the intended `AccountStateCore` production contract and permits ambiguous runtime truth ownership.

## Validation Steps

Validated in this session with targeted tests:
- `cargo test -p ghost-launcher test_pr8_ -- --nocapture`
- `cargo test -p ghost-launcher test_pr7_invariant_oracle_runtime_keeps_canonical_truth_primary -- --nocapture`
- `cargo test -p ghost-launcher pool_observation_task_wires_pr5_checkpoint_and_materialization -- --nocapture`

Recommended follow-up validation:
- Run the broader `ghost-launcher` integration suite without test filters.
- Perform a staging startup with `[account_state_core].enable=true` and `oracle.account_updates_enabled=false` to confirm the warning is emitted and AccountUpdate ingest remains active.
- Observe pool lifecycle cleanup during rejected-pool paths to confirm registry-backed removal stays leak-free under live event ordering.
