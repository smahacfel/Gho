# ADR-0061: Bootstrap market-cap fallback for feature-driven Gatekeeper

**Date:** 2026-03-30
**Status:** Accepted
**Author:** Ghost Father

## Context

Production Gatekeeper V2 logs showed widespread false hard rejects of the form:

- `HARD_FAIL: market_cap=0.0 < 1.0`

The rejection itself was not the defect. The defect was upstream feature materialization.

During feature-driven evaluation, `PoolObservationSession::materialize_features()` builds `MaterializedFeatureSet` from two different truth sources:

1. `AccountStateCore` / canonical account updates for `account_features`, and
2. `GatekeeperBuffer` / observed transaction curve history for `curve_readiness` and curve dynamics.

When no canonical account update had arrived yet, bootstrap `account_features` were synthesized from `candidate_snapshot`. In the active runtime path, `build_session_open_request(...)` populated that snapshot with identity/basic metadata only, so bootstrap `account_features.market_cap_sol` defaulted to `0.0`.

At the same time, enriched pool transactions could already carry valid curve data (`v_sol_in_bonding_curve`, `v_tokens_in_bonding_curve`, `market_cap_sol`, `curve_data_known=true`) via canonical/shadow transaction enrichment. That made `GatekeeperBuffer` report actionable curve readiness and non-empty curve history while `account_features.market_cap_sol` still remained synthetic zero.

The result was a cross-source contract violation:

- curve path said: **known, actionable curve exists**,
- account feature path said: **market cap is literally zero**.

`bonding_curve_from_features(...)` then combined those two states and surfaced a false `current_market_cap_sol = 0.0`, which triggered the hard-fail threshold.

## Decision

We fixed the defect at the session bootstrap feature-materialization boundary.

Implemented decisions:

1. **Bootstrap account features now backfill from observed curve dynamics when canonical account state is absent**
   - in `ghost-launcher/src/session/observation.rs`, `current_account_features()` now derives bootstrap fallback values from `GatekeeperBuffer::current_curve_dynamics()` before falling back to empty defaults
   - the bootstrap path now provides non-zero fallback values for:
     - `price_sol`
     - `market_cap_sol`
     - `bonding_progress`
     - `price_change_since_t0_pct`
   - this ensures that once transaction-derived curve state exists, bootstrap `account_features` no longer misrepresent market cap as literal zero

2. **Bootstrap curve finality is aligned with the active Gatekeeper buffer state**
   - bootstrap `account_features.curve_finality` now mirrors `gatekeeper_buffer.curve_finality_state()` instead of always forcing speculative finality
   - this keeps bootstrap feature snapshots internally consistent with the curve-readiness latch

3. **Regression coverage was added for the exact failure mode**
   - `session_lifecycle_tests` now verifies that a session with no canonical account updates but with observed curve transactions materializes a positive bootstrap market cap
   - `gatekeeper_policy_tests` now verifies that a seeded organic flow with known curve data and no account updates reaches `BUY` instead of false `market_cap=0.0` hard reject

## Architectural Impact

This ADR does not change the system's canonical authority model.

The architecture remains:

- `AccountStateCore` is still the only canonical account-state authority,
- `GatekeeperBuffer` remains the runtime observation authority for transaction-derived curve history,
- `PoolObservationSession` remains the feature materialization bridge between those sources.

The change is specifically about **bootstrap consistency**:

- before: bootstrap account features could contradict already-known curve history,
- after: bootstrap account features degrade from the best runtime-observed curve state until canonical account updates arrive.

This reduces false rejects without promoting bootstrap state to canonical state.

## Risk Assessment

**Risk Level:** Medium

Why medium:

- the change affects feature materialization used directly by Gatekeeper policy,
- bootstrap sessions without canonical account updates now consume runtime curve observations more aggressively,
- any logic that implicitly relied on bootstrap `market_cap_sol == 0.0` will now observe a positive fallback value instead.

Why not high:

- canonical account-state precedence is unchanged,
- the fallback activates only when `account_features.update_count == 0`,
- the fix is tightly scoped to bootstrap materialization rather than broad policy-threshold changes,
- targeted and file-level regression suites passed in-session after the change.

## Consequences

### Positive

- false `market_cap=0.0` hard-fails are eliminated for pools that already have valid curve observations but have not yet received canonical account updates,
- bootstrap session features now better reflect observed runtime truth,
- Gatekeeper policy sees internally consistent `curve_readiness` and `account_features` during the bootstrap window,
- the failure mode is now pinned by regression tests.

### Negative / Trade-offs

- bootstrap feature snapshots are now more coupled to `GatekeeperBuffer` curve history than before,
- bootstrap `market_cap_sol` is still best-effort derived state, not canonical account truth,
- some diagnostics may now show positive market-cap values earlier in the lifecycle than they did previously.

## Alternatives Considered

### 1. Lower or disable the minimum market-cap hard-fail threshold

Rejected.

Reason: the threshold was not the defect. The defect was false zero-valued feature materialization upstream.

### 2. Change Gatekeeper policy to ignore market-cap checks unless canonical account updates exist

Rejected.

Reason: this would weaken protection semantics and couple policy correctness to account-update timing rather than fixing the inconsistent bootstrap feature contract.

### 3. Populate `candidate_snapshot` more richly at session-open time and leave bootstrap feature logic unchanged

Rejected as the primary fix.

Reason: while richer session-open metadata may still be useful, the root defect was at feature materialization time. Fixing only the open-session snapshot would not guarantee consistency once transaction-derived curve history evolved during the session.

## Validation Steps

Validated in this session with targeted and file-level test execution:

1. Regression tests for the exact bootstrap failure mode:
   - `cargo test -p ghost-launcher --test session_lifecycle_tests session_materializes_curve_market_cap_without_account_updates`
   - `cargo test -p ghost-launcher --test gatekeeper_policy_tests feature_policy_buys_seeded_flow_without_account_updates_when_curve_data_is_known`

2. Broader affected-suite validation after the final code change:
   - `cargo test -p ghost-launcher --test session_lifecycle_tests --test gatekeeper_policy_tests`

3. Additional verification performed during investigation:
   - traced the reject log emission back from `ghost-launcher/src/oracle_runtime.rs`
   - verified the hard-fail check itself was correct in `ghost-launcher/src/components/gatekeeper_policy.rs`
   - confirmed the zero value originated from bootstrap session feature materialization in `ghost-launcher/src/session/observation.rs`
