# ADR-0068: Shadow BUY metadata and account override hardening

**Date:** 2026-04-01
**Status:** Accepted
**Author:** Ghost Father

## Context

A deeper RCA was requested for shadow-only BUY divergence on `2026-04-01`, specifically to replace vague explanations with deterministic failure points and preventive fixes.

The investigation established four concrete defects in the current BUY path:

1. `initial_liquidity_sol` could remain missing even when observed BUY transactions already carried valid reserve data (`reserve_quote` / `v_sol_in_bonding_curve`).
2. `associated_bonding_curve` harvested from observed BUY transactions was trusted too early and could be forwarded into the BUY builder without canonical validation.
3. shadow metadata readiness treated creator presence too loosely: a non-empty string could count as creator metadata even if it was not a valid non-default `Pubkey`.
4. runtime registration of `NewPoolDetected` metadata could discard the creator when storing runtime fallback metadata, weakening later BUY-path fallback hydration.

Operational evidence tied these defects to real outcomes:

- `shadow_skipped_not_ready` cases where `initial_liquidity_sol` stayed absent,
- `InstructionError(1, Custom(2006))` semantic failures consistent with account/seed mismatch,
- incomplete runtime fallback metadata in registry-backed paths.

## Decision

The shadow BUY path is hardened around one rule:

**observed BUY telemetry may assist metadata hydration, but only canonical or validated account inputs may reach the builder.**

Implemented decisions:

1. **Observed BUY reserve data is now an official fallback for shadow readiness**
   - `ghost-launcher/src/oracle_runtime.rs` now backfills `initial_liquidity_sol` from the latest successful observed BUY carrying `reserve_quote` or `v_sol_in_bonding_curve`.
   - This fallback is applied both after runtime reserve lookup and after synchronous reserve refresh attempts.

2. **Creator metadata must be a real non-default Solana pubkey**
   - shadow readiness and metadata merge logic now treat creator as present only when it parses as a valid `Pubkey` different from `Pubkey::default()`.
   - This restores the canonicalization contract expected by earlier creator-vault hardening.

3. **`associated_bonding_curve` overrides are now canonicalized, not blindly replayed**
   - `off-chain/components/trigger/src/direct_buy_builder.rs` now derives the canonical bonding-curve ATA and accepts an override only when it exactly matches that derivation.
   - invalid overrides are ignored and the canonical ATA is used instead.
   - `ghost-launcher/src/components/trigger/component.rs` sanitizes and logs dropped invalid overrides before transaction preparation.

4. **Runtime fallback metadata now preserves detected creator when available**
   - `ghost-launcher/src/oracle_runtime.rs` now passes the parsed creator pubkey into runtime registration for `GhostEvent::NewPoolDetected`, preventing creator loss in runtime fallback metadata.

5. **Metadata-preparation failures are classified explicitly**
   - dispatch errors such as missing canonical creator metadata are now classified as `shadow_metadata_missing` instead of falling into broader buckets.

## Architectural Impact

This decision keeps the current architecture intact while tightening responsibility boundaries:

- `OracleRuntime` owns BUY-path metadata hydration and readiness.
- observed trade flow may enrich metadata, but does not become account-layout SSOT.
- `TriggerComponent` remains responsible for preflight sanitation before build/dispatch.
- `DirectBuyBuilder` enforces the canonical ATA contract for bonding-curve token accounts.

The result is a stricter separation between:

- **metadata fallback** (acceptable from validated observed runtime evidence), and
- **transaction account truth** (must be canonical or explicitly validated).

## Risk Assessment

**Rate:** Medium

Primary risks:

- stricter creator validation may downgrade some previously “ready” shadow cases into explicit metadata-missing states,
- invalid observed `associated_bonding_curve` values that were previously replayed will now be ignored,
- BUY readiness can improve earlier in a pool lifecycle because observed reserve data is now consumed intentionally.

These risks are acceptable because they replace ambiguous or semantically unsafe behavior with deterministic contracts.

## Consequences

### Positive

- shadow-only BUY no longer depends solely on runtime reserve visibility when observed BUY reserve data already exists,
- invalid observed `associated_bonding_curve` values can no longer poison BUY preparation,
- creator metadata validation is once again consistent with canonical creator-vault expectations,
- runtime fallback metadata retains creator information more reliably,
- metadata failures become easier to distinguish from transport or simulation failures.

### Trade-offs

- BUY-path sanitation is stricter and slightly more complex,
- some legacy observed-account samples will no longer be replayed literally,
- readiness now depends on a stronger creator contract than a simple non-empty string check.

## Alternatives Considered

### 1. Continue relying only on runtime reserve refresh for `initial_liquidity_sol`

Rejected because observed BUY reserve data was already available in real failing cases and not using it produced avoidable `shadow_skipped_not_ready` outcomes.

### 2. Keep accepting arbitrary observed `associated_bonding_curve` overrides

Rejected because this preserves semantic BUY-account mismatch risk and contradicts the repository hardening direction documented in `PLANS/SHADOW_RUN_FINAL.md`.

### 3. Classify creator readiness by string presence only

Rejected because it reintroduces the exact class of creator-canonicalization regressions that earlier hardening explicitly tried to prevent.

## Validation Steps

Validated in this session with targeted checks:

1. Source diagnostics on touched files:
   - no editor errors in:
     - `ghost-launcher/src/oracle_runtime.rs`
     - `ghost-launcher/src/components/trigger/component.rs`
     - `off-chain/components/trigger/src/direct_buy_builder.rs`

2. Targeted builder tests:
   - `cargo test -q -p trigger --lib associated_bonding_curve_override`
   - result: `2 passed; 0 failed`

3. Targeted trigger-component sanitation tests:
   - `cargo test -q -p ghost-launcher --lib sanitize_associated_bonding_curve_override`
   - result: `2 passed; 0 failed`

4. Targeted shadow-readiness tests:
   - `cargo test -p ghost-launcher --lib shadow_readiness`
   - verified:
     - `test_shadow_readiness_accepts_liquidity_backfilled_from_buffered_buy_reserves`
     - `test_shadow_readiness_accepts_liquidity_backfilled_from_refreshed_reserves`
     - `test_shadow_readiness_rejects_invalid_creator_pubkey_string`
     - `shadow_only_skips_when_shadow_readiness_is_incomplete`
   - result: `4 passed; 0 failed`

5. Targeted dispatch classification test:
   - `cargo test -p ghost-launcher shadow_dispatch_error_classifies_metadata_missing`
   - result: `1 passed; 0 failed`
