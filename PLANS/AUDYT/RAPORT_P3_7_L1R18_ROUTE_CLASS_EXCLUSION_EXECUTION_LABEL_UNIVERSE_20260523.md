# P3.7-L1R18 Route-Class Exclusion from Execution-Label Universe

Date: 2026-05-23

Status: CODE-LEVEL IMPLEMENTED / OFFLINE VALIDATION REQUIRED

## Problem

L1R17 closed the route fallback decision audit:

```text
fallback_repairable = false
recommended_next_path = route_class_exclusion_from_execution_label_universe
```

The current route class is not executable under the available route/account
support:

- primary `routed_exact_sol_in` fails because route-required `bonding_curve_v2`
  is not load-ready;
- active `legacy_buy` fallback fails with `fallback_missing_core_curve_account`;
- probe fallback fails with `fallback_builder_account_source_unverified`;
- `executable_route_ready_rows = 0`.

These rows must not poison buy-quality labels. They are execution feasibility
rejects, not `buy_quality_bad` rows.

## Code Changes

Runtime rows now carry additive execution-feasibility fields:

```text
execution_feasibility_status
execution_feasibility_reason
route_resolution_terminal_reason
lifecycle_label_eligibility
```

For `route_resolution_status = no_executable_route_account_set`, the normalized
classification is:

```text
execution_feasibility_status = not_executable_route
execution_feasibility_reason = no_executable_route_account_set
lifecycle_label_eligibility = not_lifecycle_label_eligible
```

The join-key audit now reports execution feasibility separately:

```text
decision_rows_total
probe_selected_rows
route_executable_rows
route_non_executable_rows
successful_entry_rows
lifecycle_eligible_rows
lifecycle_labeled_rows
buy_quality_labeled_rows
execution_feasibility_reject_rows
active_buy_execution_infeasible_rows
```

The lifecycle labeler classifies non-executable route rows as:

```text
execution_verification_class = shadow_execution_infeasible
buy_quality_class = buy_quality_not_executable
label_quality = not_executable
```

The feature availability report excludes `buy_quality_not_executable` rows from
the buy-quality denominator and reports:

```text
buy_quality_denominator_rows
execution_feasibility_reject_rows
execution_feasibility_coverage
```

If execution-infeasible rows dominate and the buy-quality denominator is too
small, the selector reason becomes:

```text
execution_feasibility_coverage_too_low
```

## Files Changed

- `ghost-launcher/src/events.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `scripts/v3_p37_mfs_lifecycle_join_key_audit.py`
- `scripts/v3_p37_shadow_lifecycle_labeler.py`
- `scripts/v3_p37_shadow_lifecycle_feature_availability.py`
- `scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py`
- `scripts/test_v3_p37_shadow_lifecycle_labeler.py`
- `scripts/test_v3_p37_shadow_lifecycle_feature_availability.py`
- `PLANS/PLAN_P3_7_J3_COUNTERFACTUAL_SHADOW_PROBE_PLANE_20260519.md`

## Acceptance

L1R18 passes if offline validation on R16-r13/L1R17-style artifacts shows:

- `no_executable_route_account_set` rows are not counted as
  `buy_quality_bad`;
- `execution_feasibility_reject_rows > 0`;
- buy-quality denominator excludes non-executable rows;
- active BUY execution-infeasible rows are counted separately;
- route-excluded rows remain lifecycle-ineligible;
- feature availability can report
  `execution_feasibility_coverage_too_low`.

## Non-Goals

No route fallback implementation, threshold tuning, IWIM change, collection,
Phase B, P2/live change, or L2 ablation is included in L1R18.

## Next Decision

After L1R18, L2 ablation can only use rows with executable route and lifecycle
labels. If the executable universe is too small, the next work is route support
expansion or explicit scope restriction to executable route classes.
