# ADR-0116: Early Top-3 Buy Concentration (3s) for Hidden Soft Rugs

**Date:** 2026-04-27
**Status:** Accepted
**Author:** Ghost Father

## Context

Fresh `shadow-burnin` evidence exposed a recurring "miekki rug" pattern that the existing full-window concentration guard did not catch reliably:

- dev buys small;
- 2-3 related wallets aggressively pump the curve in the first seconds;
- later flow dilutes full-window `top3_volume_pct`;
- the same cluster exits together a few minutes later and price collapses.

The active `max_top3_volume_pct = 0.70` guard works on the broader observation window, so it can miss **hidden early concentration** when later retail flow lowers the final ratio.

## Pre-patch State

Before this patch:

- `EarlyFingerprintMetrics` had no short-window owner-concentration metric;
- Gatekeeper Phase 4 could only use the existing full-window concentration signals;
- BUY JSONL had no explainability surface for hidden early top-3 concentration;
- `ghost_brain_config.toml` had no dedicated threshold for this failure mode.

In practice, the system could accept pools where:

- full-window `top3_volume_pct` stayed under `0.70`,
- but the first `2-3s` were already dominated by the same top buyers.

## Decision

Add a new fingerprint metric:

- `early_top3_buy_volume_pct_3s`

Contract:

1. Compute it inside `seer::early_fingerprint::FingerprintAggregator`.
2. Use owner-resolved buy flow from the first `3000 ms` after observation start.
3. Rank buyer owners by accumulated buy volume in that window.
4. Emit `top3_buy_volume / total_buy_volume` for that same window.
5. Gate it in Gatekeeper Phase 4 through a new config threshold:
   - `max_early_top3_buy_volume_pct_3s`
6. Keep schema defaults neutral (`1.0`) so the feature is fail-safe unless explicitly tuned.

Active rollout threshold:

- `max_early_top3_buy_volume_pct_3s = 0.71`

This metric is intentionally **additive**, not a replacement for full-window `top3_volume_pct`.

## Architectural Impact

Touched surfaces:

- `off-chain/components/seer/src/early_fingerprint.rs`
- `ghost-brain/src/config/ghost_brain_config.rs`
- `ghost-brain/ghost_brain_config.toml`
- `configs/rollout/shadow-burnin.toml`
- `ghost-launcher/src/components/gatekeeper_policy.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-brain/src/oracle/decision_logger.rs`
- `ghost-launcher/tests/gatekeeper_policy_tests.rs`
- `ghost-launcher/tests/full_pipeline_integration.rs`
- `ghost-launcher/tests/gatekeeper_v2_pipeline_integration.rs`

The BUY-log schema version is bumped to `15` because JSONL now exposes:

- observed `early_top3_buy_volume_pct_3s`
- configured `max_early_top3_buy_volume_pct_3s`

## Consequences

- Gatekeeper can now reject pools with hidden early concentration even when full-window `top3_volume_pct` still looks acceptable.
- Operators get explicit telemetry for this short-window choke in startup config logs and BUY JSONL.
- The metric reuses existing owner-delta plumbing; no new runtime feed or session refactor was required.

## Rollback

To return to the pre-patch behavior:

1. Set `max_early_top3_buy_volume_pct_3s = 1.0` in `ghost-brain/ghost_brain_config.toml`.
2. Keep the emitted metric as telemetry-only, or ignore the new JSONL fields in downstream consumers.
3. If an external parser depends on the old BUY-log contract, roll it back from schema `15`.

This restores the prior behavior where only the broader concentration guards participate in verdict.

## Validation

1. `cargo test -p seer early_fingerprint --lib`
2. `cargo test -p ghost-launcher --test gatekeeper_policy_tests`
3. `cargo test -p ghost-launcher gatekeeper_buy_log --lib`
4. `cargo test -p ghost-brain --test ghost_brain_config_load_test -- --nocapture`
5. `cargo check -p ghost-launcher --bin ghost-launcher`
