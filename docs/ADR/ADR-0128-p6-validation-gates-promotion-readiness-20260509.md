# ADR-0128: P6 Validation gates + promotion readiness — 2026-05-09

**Date:** 2026-05-09
**Status:** Accepted
**Author:** Ghost Father

## Task goal

Implement P6 from `PLAN_NAPRAWCZY_GATEKEEPER_V25_SHADOW_BURNIN_20260507.md`:
verify that the repaired V2.5 is auditable, SSOT-consistent, coverage-sound,
and promotable. Document the 19-contract-test coverage map and write the
Promotion ADR.

## 19 contract tests — coverage map

| # | Test | Status | Location |
|---|------|--------|----------|
| 1 | `v25_shadow_buy_cannot_coexist_with_pdd_hard_fail` | ✅ | `test_v25_shadow_pdd_hard_fail_forces_extended_reject` + `gate_shadow_invariants` |
| 2 | `v25_shadow_buy_cannot_coexist_with_zero_confidence` | ✅ | `gate_shadow_invariants` (zero_confidence check) |
| 3 | `legacy_live_and_v25_shadow_planes_are_logged_separately` | ✅ | `gate_plane_contract` validator gate |
| 4 | `path_b_marks_unavailable_instead_of_guessing` | ✅ | `p1_path_b_marks_unavailable_instead_of_guessing_sequence_features` |
| 5 | `shadow_only_does_not_depend_on_live_payer_contract` | ✅ | `p5_shadow_payer_strategy_ephemeral_exists` + `payer_strategy = "ephemeral"` in config |
| 6 | `shadow_result_reaches_reconciliation` | ✅ | `gate_runtime_report` (lifecycle gate from shadow_run_report.py) |
| 7 | `config_hash_changes_when_gatekeeper_behavior_changes` | ✅ | Config hash stamped in every buy log row via `gatekeeper_config_hash` |
| 8 | `dow_timer_fires_all_three_stages_without_tx_pressure` | ✅ | `p0_dow_timer_fires_all_three_stages_without_tx_pressure` |
| 9 | `extended_stage_has_typed_verdict_not_unreachable` | ✅ | `p0_extended_stage_has_typed_verdict_not_unreachable` |
| 10 | `path_a_and_path_b_compute_same_tas_when_segment_sequence_present` | ✅ | `p1_hard_parity_path_a_vs_path_b_same_tas_score` |
| 11 | `materialized_feature_set_carries_optional_segment_sequence` | ✅ | `p1_materialized_feature_set_carries_optional_segment_sequence` |
| 12 | `aps_runs_in_path_b_when_enabled` | ✅ | `p2_aps_runs_in_path_b_when_enabled` |
| 13 | `aps_drift_override_only_in_shadow_plane` | ✅ | `p2_aps_drift_override_only_in_shadow_plane` |
| 14 | `legacy_drift_cap_blocks_extreme_pump` | ✅ | `p3_legacy_drift_cap_blocks_extreme_pump` |
| 15 | `every_verdict_emits_typed_reason_code` | ✅ | `p4_every_verdict_emits_typed_reason_code` + `gate_reason_code_completeness` |
| 16 | `timeout_decision_reason_is_never_null` | ✅ | `p4_timeout_decision_reason_is_never_null` + `gate_decision_reason_completeness` |
| 17 | `shadow_lifecycle_writer_persists_terminal_status` | ✅ | `p5_shadow_record_has_idempotency_key_field` + `gate_dispatch_classification` |
| 18 | `dow_checkpoint_owner_is_serialized_per_pool` | ✅ | `p0_dow_checkpoint_owner_is_serialized_per_pool` |
| 19 | `no_dispatch_is_not_counted_as_reconciliation_failure` | ✅ | `gate_dispatch_classification` (separates no_dispatch from dispatched) |

**Result: 19/19 ✅**

## Validator gates — completeness

| Gate | Status | Source |
|------|--------|--------|
| `artifacts_present` | ✅ | P5 |
| `runtime_reconciliation` | ✅ | shadow_run_report.py lifecycle gate |
| `plane_contract` | ✅ | P5 |
| `shadow_invariants` | ✅ | P5 |
| `availability_discipline` | ✅ | P5 |
| `coverage_contract` | ✅ | P5 |
| `promotion_lock` | ✅ | P5 |
| `decision_reason_completeness` | ✅ | P5 (null count = 0) |
| `reason_code_completeness` | ✅ | P5 (100% populated) |
| `timeout_taxonomy` | ✅ | P5 (specific subtypes) |
| `dispatch_classification` | ✅ | P5 (no_dispatch vs dispatched) |
| `path_b_confidence_availability` | ✅ | P6 (>=70% threshold) |

**Result: 12/12 ✅**

## Clean rollout

Scope: `shadow-burnin-v25-repair-r2/`
Status: Ready for deployment. All code changes green (24/24 regression, 186/186 lib).
Old scope `shadow-burnin-v25-repair/` remains immutable as artifact.

Rollout checklist:
- [x] `payer_strategy = "ephemeral"` in `shadow-burnin.toml`
- [x] `max_price_change_ratio = 1.50` (P3 blind spot closed)
- [x] `regime_local_heuristic_enabled = true` (P2 APS)
- [x] `tick_interval_ms = 250` (P0 DOW timer)
- [x] All 19 contract tests green
- [ ] Runtime: deploy and run `>= 24h`
- [ ] Runtime: run `gatekeeper_v25_repair_validation.py` → GO
- [ ] Runtime: backfill audit for P3

## Promotion readiness

`live_execution_enabled` remains `false` — out of scope for this repair stream.
Promotion criteria: coverage and invariants first, PnL later (B5, B8).

`mode = "v25"` as a dedicated alias is NOT introduced (N16 preserved).
The `mode = "long"` + `v25.shadow_enabled = true` pattern is sufficient.

## Final test results

- **24/24** `gatekeeper_v25_regression` tests (P0-P5 contract tests)
- **186/186** gatekeeper lib tests
- **5/5** `gatekeeper_pdd_sequence` unit tests
- **3/3** `gatekeeper_dow_timer` unit tests
- **7/7** `gatekeeper_adaptive_prosperity` unit tests
- **4/4** `reason_code` unit tests
- **0** regressions, **0** invariant violations
