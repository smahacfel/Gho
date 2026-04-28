# ADR-0067: Shadow-only BUY outcome diagnosis for 2026-04-01 triage

**Date:** 2026-04-01
**Status:** Accepted
**Author:** Ghost Father

## Context

A runtime triage was requested for three consecutive `gatekeeper_v2_buys.jsonl` entries from `2026-04-01`, all of which had `core_pass=true` but different shadow outcomes:

1. `2oiV8VruRsJ3ize8VvFLDvBoDa82SdWvZcUnrYtC6WPk` / `AhuyQ4KDspppYhPRHDto5hYFqieXZUL9wpyS3nqSpump`
   - `shadow_ready=true`
   - `shadow_execution_outcome="shadow_simulated"`
2. `BHpEkDhvYDQppZRyoX8bhBAC3D56owCmJePs2BcAataN` / `43Y6WCYT2HQcbTjB12QYhLxXt3Ntkc5CyhknJ2mHpump`
   - `shadow_ready=true`
   - `shadow_execution_outcome="shadow_simulation_error"`
3. `5bvJ3Dj714cvzkQDEVa9GvkUbZGjkBDc5JseRjzo6GzQ` / `313Ls6rdQFYihsNYbHAGJMzhEchLgG5TG6TKYbhvpump`
   - `shadow_ready=false`
   - `shadow_missing_fields=["initial_liquidity_sol"]`
   - `shadow_execution_outcome="shadow_skipped_not_ready"`

The question was not whether Gatekeeper passed, but why the BUY path diverged after `core_pass=true`.

## Decision

The three outcomes are explained by three different post-Gatekeeper branches, all consistent with current runtime logic:

1. **`shadow_skipped_not_ready` means metadata gating stopped execution before simulation.**
   - In `ghost-launcher/src/oracle_runtime.rs`, `compute_shadow_run_readiness(...)` requires valid shadow metadata including `initial_liquidity_sol`.
   - In shadow-only mode, if readiness is false, runtime sets `shadow_execution_outcome="shadow_skipped_not_ready"` and does not dispatch the shadow BUY.
   - For pool `5bvJ3...`, runtime logs show repeated reserve refresh failure and missing reserves:
     - `AccountNotFound`
     - `KRYTYK: Gatekeeper PASS ale WCIĄŻ brak danych rezerw!`
   - Therefore this case is a metadata-readiness stop, not a simulation failure.

2. **`shadow_simulation_error` means the shadow BUY was built and simulated, but the simulated transaction failed semantically.**
   - The concrete shadow report for `BHpEk...` was written to:
     - `logs/shadow_run/paper-burnin-buys.jsonl`
   - That report shows:
     - `err="InstructionError(1, Custom(2006))"`
     - `error_class="semantic"`
   - Existing repository ADR `ADR-0016-shadow-buy-creator-canonicalization-guard.md` documents that `Custom(2006)` maps to Anchor `ConstraintSeeds`.
   - Therefore this case is best classified as a PDA/seed-account mismatch on instruction 1 of the simulated BUY path, not transport, not logger failure, and not missing metadata.

3. **`shadow_simulated` means the shadow BUY completed simulation without semantic error.**
   - The concrete shadow report for `2oiV8...` shows:
     - `err=null`
     - `error_class=null`
   - Therefore runtime correctly classified the outcome as `shadow_simulated`.

## Architectural Impact

This diagnosis reinforces the existing runtime contract:

- `core_pass=true` is only the Gatekeeper decision boundary.
- Shadow BUY still depends on a second boundary:
  1. metadata readiness,
  2. valid BUY account derivation,
  3. successful semantic simulation.
- `gatekeeper_v2_buys.jsonl` is therefore a mixed record of:
  - Gatekeeper success,
  - shadow readiness,
  - shadow dispatch/simulation outcome.

Operationally, the three analyzed outcomes should be interpreted as:

- `shadow_skipped_not_ready` → data completeness failure,
- `shadow_simulation_error` → on-chain account/PDA semantic failure,
- `shadow_simulated` → healthy shadow execution.

## Risk Assessment

**Rate:** Medium

The main operational risk is misreading all three outcomes as one class of failure. That would cause incorrect remediation:

- metadata cases would be debugged as builder bugs,
- seed/account mismatch cases would be debugged as missing-liquidity issues,
- healthy simulations could be lumped into a false system-wide outage narrative.

## Consequences

What becomes easier:

- separating Gatekeeper success from actual shadow BUY execution health,
- triaging whether the next action belongs to metadata hydration or BUY account derivation,
- reading `gatekeeper_v2_buys.jsonl` without treating `core_pass=true` as equivalent to "BUY path is fine".

What becomes harder:

- operators must inspect both decision logs and shadow-run logs to fully explain a BUY-path anomaly,
- simple counts of `core_pass=true` remain insufficient as a proxy for executable BUY readiness.

## Alternatives Considered

### 1. Treat `shadow_simulation_error` as generic RPC or transport instability

Rejected because the concrete report shows `InstructionError(1, Custom(2006))` with `error_class="semantic"`.

### 2. Treat `shadow_skipped_not_ready` as a bug in the logger or BUY-only file routing

Rejected because runtime logs explicitly show reserve refresh failure and missing `initial_liquidity_sol`, and the readiness gate intentionally stops shadow-only execution in that state.

### 3. Treat all three entries as one broken BUY batch

Rejected because the third case never simulated, the second simulated and failed semantically, and the first simulated successfully.

## Validation Steps

1. Verify readiness logic in:
   - `ghost-launcher/src/oracle_runtime.rs::compute_shadow_run_readiness(...)`
2. Verify shadow-only skip behavior in:
   - `ghost-launcher/src/oracle_runtime.rs::execute_gatekeeper_buy_path(...)`
3. Verify concrete error report for `BHpEk...` in:
   - `logs/shadow_run/paper-burnin-buys.jsonl`
4. Verify concrete success report for `2oiV8...` in:
   - `logs/shadow_run/paper-burnin-buys.jsonl`
5. Verify reserve/metadata failure logs for `5bvJ3...` in:
   - `logs/rollout/paper-burnin/oracle.log.2026-04-01`
6. Cross-check semantic meaning of `Custom(2006)` against:
   - `docs/ADR/ADR-0016-shadow-buy-creator-canonicalization-guard.md`
