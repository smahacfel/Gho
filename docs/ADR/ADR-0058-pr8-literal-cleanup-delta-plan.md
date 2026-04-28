# ADR-0058: PR8 literal cleanup delta plan after production cutover

**Date:** 2026-03-30
**Status:** Accepted
**Author:** Ghost Father

## Context

Repo was re-audited against both:

- `PLANS/REFACTOR.md`
- `docs/ADR/ADR-0054-refactor-pr-by-pr-forensic-matrix.md`

That re-audit established an important boundary already captured in `ADR-0057`:

- PR8 is **closed operationally** as a production runtime cutover,
- but PR8 is **not yet closed literally** if the strongest wording from `PLANS/REFACTOR.md` is interpreted as requiring physical removal of remaining legacy symbols and compatibility shims.

The remaining discrepancy is narrow. Production hot-path ownership is already on:

- `SessionManager`
- `PoolObservationSession`
- `AccountStateCore`
- feature-driven Gatekeeper evaluation
- production-enforced `AccountUpdate` ingest when `account_state_core.enable=true`

However, repo still contains legacy ballast that keeps the literal PR8 wording from being true word-for-word, including:

- `PerPoolOracleState`
- `OracleRuntime.pools`
- compat helpers that still reference legacy pool state
- deprecated inline scoring wrappers (`on_transaction(...)`)
- a residual Seer compatibility flag for account updates
- reconciliation wording/semantics that still use `repair` language despite declared diagnostic-only intent

The user explicitly requested a **very narrow, no-scope-creep delta plan** for only the remaining work required to make the broadest literal PR8 wording true.

## Decision

We will treat the remaining PR8 work as a **strict cleanup-only delta**, not as a reopened runtime refactor.

Execution is constrained to exactly four ordered cleanup stages:

1. **Remove legacy compat pool state from runtime**
   - delete `PerPoolOracleState`
   - delete `OracleRuntime.pools`
   - eliminate production helpers that still depend on compat per-pool state
   - migrate any affected tests to session/runtime-state/test-only helpers

2. **Remove deprecated inline scoring runtime API**
   - delete `GatekeeperBuffer::on_transaction(...)`
   - delete `PoolObservationSession::on_transaction(...)`
   - migrate tests to `ingest_transaction(...)`, checkpoints, feature materialization, and feature evaluation helpers

3. **Close Seer account-update compatibility semantics**
   - preserve only a clearly separated degraded/test-only path if needed
   - do not leave `account_updates_enabled` as an architecture-ambiguous production contract switch

4. **Align reconciliation with monitoring-only contract**
   - remove `repair` language and semantics that imply active state-authority
   - keep reconciliation diagnostic/observational only

This delta must not expand into adjacent improvements, policy work, or new runtime capabilities.

## Architectural Impact

This decision does **not** change the already-established production architecture.

It only tightens the repo so that implementation artifacts match the architecture already in force:

- `SessionManager` remains sole runtime owner
- `AccountStateCore` remains sole primary truth
- Gatekeeper remains feature-driven
- Seer account updates remain mandatory for production core-enabled startup
- Shadow/reconciliation components remain non-authoritative relative to canonical truth

The impact is therefore mainly on:

- symbol surface area,
- test harness wiring,
- config semantics clarity,
- documentation and log correctness.

## Risk Assessment

**Risk Level:** Medium

Why medium instead of low:

- removing compat helpers can break tests or hidden helper assumptions,
- deleting deprecated wrappers can expose un-migrated test fixtures,
- narrowing Seer compatibility semantics can affect degraded-mode harnesses,
- renaming reconciliation semantics can ripple through metrics/log assertions.

Why not high:

- production hot path is already cut over,
- no core architectural rework is required,
- scope is intentionally limited to cleanup around already-verified runtime behavior.

## Consequences

### Positive

- `PLANS/REFACTOR.md` becomes materially and literally aligned with the codebase
- repo stops advertising legacy fallback surfaces that are no longer legitimate runtime owners
- tests become aligned with final public APIs instead of deprecated wrappers
- future audits become simpler and less ambiguous

### Negative / Trade-offs

- some tests and support fixtures must be rewritten
- degraded/test-only affordances may become more explicit and less convenient
- a few helper methods that were historically available for ad-hoc debugging may disappear from production builds

## Alternatives Considered

### 1. Stop at ADR-0057 and declare PR8 fully closed

Rejected.

Reason: acceptable operationally, but not truthful against the broadest literal wording in `PLANS/REFACTOR.md`. The user explicitly asked for the delta needed to close that gap.

### 2. Reopen PR8 as a broad architecture workstream

Rejected.

Reason: this would be scope creep. The remaining work is cleanup, not a new architectural migration.

### 3. Leave all legacy symbols in place but mark them deprecated forever

Rejected.

Reason: this preserves audit ambiguity and keeps the repo in a half-closed state inconsistent with the declared target architecture.

## Validation Steps

1. Search repo and confirm production code no longer contains:
   - `PerPoolOracleState`
   - `OracleRuntime.pools`
   - production `register_new_pool(...)`
   - public runtime `on_transaction(...)` scoring wrappers
2. Run targeted `ghost-launcher` tests covering:
   - PR8 runtime registration and startup invariants
   - PR7 canonical-truth invariants
   - session feature/materialization path
3. Verify Seer production config/docs no longer imply `account_updates_enabled=false` as a normal core-enabled runtime mode.
4. Verify reconciliation logs/comments/metrics no longer imply active repair authority.
5. Re-audit against:
   - `PLANS/REFACTOR.md`
   - `ADR-0054`
   - `ADR-0057`
   and confirm no remaining discrepancy between operational closure and literal cleanup.
