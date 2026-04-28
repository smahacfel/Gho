# ADR-0115: Strict Prosperity Overlay for Shadow Burn-in

**Date:** 2026-04-27
**Status:** Accepted
**Author:** Ghost Father

## Context

Balanced v1 prosperity filtering was already wired and enabled, but fresh `shadow-burnin` evidence showed that the old success claim based on an aggregated BUY→PnL snapshot was stale. The report-based `16W / 5L = 76.19%` snapshot lagged behind raw lifecycle truth and excluded newer losing trades that were already present in `shadow_lifecycle.jsonl`.

After re-anchoring the analysis on the current strict shadow regime and fresh decision/lifecycle artifacts, the problem was narrower than “Balanced does nothing”:

- the active strict regime was already extremely selective;
- the remaining underperformers were dominated by overextended B2/B3-style entries;
- the best practical improvement came from a **small overlay on top of already-matched prosperity branches**, not from re-tuning the whole Gatekeeper again.

## Pre-patch State

Before this patch:

- `evaluate_prosperity_filter(...)` only enforced:
  - prosperity market-cap floor,
  - prosperity CPV ceiling,
  - Balanced branch B1/B2/B3;
- there was no second-stage prosperity overlay for overextension or structural quality;
- `ProsperityRejectTrigger` and `ProsperityFilterDiagnostics` could not explain post-branch rejections;
- BUY JSONL did not expose any overlay diagnostics or thresholds;
- `ghost_brain_config.toml` had no explicit switch/thresholds for the strict-regime overlay.

In other words: once a pool matched B2/B3, prosperity had no built-in protection against the specific strict-regime pattern of:

- elevated price extension,
- late bonding progress,
- weak FTDI,
- or overly dirty sell/buy structure.

## Decision

Implement a config-backed **strict prosperity overlay** as an additive layer after Balanced branch matching.

Applied contract:

1. Global overlay, only after a prosperity branch has matched:
   - `price_change_ratio <= 2.2`
   - `bonding_progress_pct <= 85`
   - `fee_topology_diversity_index >= 0.10`
2. Additional branch-specific overlay:
   - `large_cap_buy_dominance` also requires `price_change_ratio <= 2.0`
   - `large_cap_buy_dominance` and `organic_structure` both require `sell_buy_ratio <= 0.18`
3. The overlay is config-backed and disabled by default in schema defaults.
4. The active `ghost_brain_config.toml` enables it explicitly for the current shadow-burnin profile.

## Architectural Impact

Touched surfaces:

- `ghost-brain/src/config/ghost_brain_config.rs`
- `ghost-brain/ghost_brain_config.toml`
- `configs/rollout/shadow-burnin.toml`
- `ghost-launcher/src/components/gatekeeper_policy.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-brain/src/oracle/decision_logger.rs`
- `ghost-launcher/tests/gatekeeper_policy_tests.rs`
- `ghost-launcher/tests/full_pipeline_integration.rs`
- `ghost-launcher/tests/gatekeeper_v2_pipeline_integration.rs`

The BUY-log schema version is bumped to `14` because the emitted JSONL contract now contains additive overlay diagnostics and thresholds.

## Consequences

- Prosperity no longer stops at “branch matched”; it can now veto overextended strict-regime candidates with explicit reasons.
- Operators can see whether rejection came from:
  - missing FTDI,
  - missing sell/buy ratio,
  - global price extension,
  - global late bonding,
  - low overlay FTDI,
  - high overlay sell/buy ratio,
  - or the stricter B2-only price cap.
- The pre-patch Balanced behavior remains available by setting `enable_prosperity_overlay = false`.

## Rollback

To return to the pre-patch prosperity contract:

1. Set `enable_prosperity_overlay = false` in `ghost-brain/ghost_brain_config.toml`.
2. Ignore/remove the new overlay-only threshold keys.
3. If external JSONL consumers require the old contract, roll them back from schema `14` to the pre-overlay parser assumptions.

This restores the previous behavior where prosperity stops after B1/B2/B3 matching.

## Validation

1. `cargo test -p ghost-launcher --test gatekeeper_policy_tests`
2. `cargo test -p ghost-launcher gatekeeper_buy_log --lib`
3. `cargo test -p ghost-brain --test ghost_brain_config_load_test -- --nocapture`
