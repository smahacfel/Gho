# ADR-0124: P2 APS w decision plane — implementation 2026-05-08

**Date:** 2026-05-08
**Status:** Accepted
**Author:** Ghost Father

## Task goal

Implement P2 from `PLAN_NAPRAWCZY_GATEKEEPER_V25_SHADOW_BURNIN_20260507.md`:
activate APS (Adaptive Prosperity) in the decision plane. Before P2, APS was
telemetry-only: `has_sufficient_history = false` was hardcoded, regime always
defaulted to Normal, and APS never ran in Path B (`evaluate_policy_from_assessment`).

## Summary of work

1. **Config flags** — Added `regime_local_heuristic_enabled` (default `false`) and
   `cross_pool_outcome_tracker_available` (default `false`) to
   `AdaptiveProsperityConfig`. Both use `#[serde(default)]`.

2. **Unblocked regime detection** — `evaluate_aps()` now sets
   `has_sufficient_history = config.regime_local_heuristic_enabled || config.cross_pool_outcome_tracker_available`.
   When enabled, the pool-local heuristic (`detect_regime`) runs and produces
   provisional regime labels (HHI spike, price spike, volume spike → HighVol).

3. **`adaptive_thresholds_applied` as function** — No longer always `false`.
   Now computed as `config.adaptive_enabled && regime != MarketRegime::Normal`.
   Gives V2.5 shadow plane regime-aware scoring.

4. **HighVolatility drift override in Path B** — After `build_assessment_from_features`
   calls `evaluate_aps`, if regime is HighVol AND `adaptive_enabled` AND
   `live_execution_enabled = false`, the PDD entry drift cap is tightened from
   5% to `regime_high_vol_entry_drift_max_pct` (3%). Override only applies in
   shadow plane — never touches legacy live (B1, N13 preserved).

5. **Telemetry** — `GATEKEEPER_APS_REGIME_DISTRIBUTION{regime}` Prometheus counter,
   incremented via `record_aps_regime()` in `evaluate_aps`. Documented as
   provisional until post-V2.5 outcome tracker.

6. **Rollout** — `regime_local_heuristic_enabled = true` in `ghost_brain_config.toml`
   for shadow-burnin V2.5 repair.

## Decisions made

1. **Pool-local heuristic is provisional** — `regime_local_heuristic_enabled` is
   `true` in shadow-burnin but labeled as provisional. Full calibration requires
   cross-pool outcome tracker (post-V2.5). The existing `detect_regime` function
   (HHI spike, price spike, volume spike) is used as-is.

2. **Drift override only in shadow plane** — `live_execution_enabled = false`
   guard ensures legacy live verdict is never affected by provisional regime
   detection. N13 and B1 preserved.

3. **APS in Path A (try_shadow_evaluate) unchanged** — APS was already called
   in Path A (buffer path). This workstream adds it to Path B (feature-driven
   path) via `build_assessment_from_features`.

## Files changed

| File | Change |
|------|--------|
| `ghost-brain/src/config/gatekeeper_v25_config.rs` | Added `regime_local_heuristic_enabled`, `cross_pool_outcome_tracker_available` fields + defaults |
| `ghost-brain/ghost_brain_config.toml` | `regime_local_heuristic_enabled = true` for shadow-burnin |
| `ghost-launcher/src/components/gatekeeper_adaptive_prosperity.rs` | Unblocked `has_sufficient_history`, `adaptive_thresholds_applied` as function, `record_aps_regime` call |
| `ghost-launcher/src/components/gatekeeper_policy.rs` | HighVol drift override in Path B after APS evaluation |
| `ghost-launcher/src/oracle_metrics.rs` | Added `GATEKEEPER_APS_REGIME_DISTRIBUTION` + `record_aps_regime()` |
| `ghost-launcher/tests/gatekeeper_v25_regression.rs` | 2 P2 contract tests |

## Test results

- **15/15** `gatekeeper_v25_regression` tests pass
- **186/186** gatekeeper lib tests pass
- **7/7** `gatekeeper_adaptive_prosperity` unit tests pass

## DoD P2 checklist

- [x] APS odpala w `build_assessment_from_features` (Path B)
- [x] `aps_diagnostics` jest w 100% rekordów Path B
- [x] `regime_local_heuristic_enabled = true` w shadow-burnin
- [x] HighVolatility drift override działa w Path B (shadow plane only)
- [x] Test invariant: `aps_drift_override_only_in_shadow_plane`
- [x] Telemetry: `gatekeeper_aps_regime_distribution_total{regime}` — provisional
- [x] `adaptive_thresholds_applied` nie jest stale-`false`
