# P3.7-E6 Route Support Next Target Decision

Generated at: `2026-05-25T08:53:48.655220+00:00`

## Decision

- final_decision: `BLOCK_ROUTE_SUPPORT_ABI_DISCOVERY_REQUIRED`
- recommended_next_path: `collect_or_extract_observed_route_abi_map`
- recommended_next_route_to_implement: `no_implementable_route_found`
- decision_reason: `E1 observed only legacy_buy and routed_exact_sol_in. E5B closes legacy_buy, and routed_exact_sol_in remains blocked by route-required BCV2 load readiness, so there is no safe next route implementation target in the current artifacts.`

## Inputs

```json
{
  "e1_final_decision": "GO_E2_IMPLEMENT_TOP_ROUTE_SUPPORT",
  "e1_recommended_next_path": "implement_legacy_buy_executable_account_set_materialization",
  "e5a_verdict": "BUILDER_LEGACY_LAYOUT_USES_BCV2",
  "e5b_verdict": "LEGACY_BUY_UNSUPPORTED_REMOVED_FROM_FALLBACK"
}
```

Runtime was not run. E6 is an offline decision over the existing E1/E5A/E5B
artifact chain.

## Summary

```json
{
  "builder_legacy_layout_uses_bcv2": true,
  "candidate_route_classes": [],
  "closed_route_classes": [
    "legacy_buy"
  ],
  "evaluated_route_classes": [
    "legacy_buy",
    "routed_exact_sol_in"
  ],
  "legacy_buy_closed_by_e5b": true,
  "next_route_to_implement": "no_implementable_route_found",
  "observed_route_variant_counts": {
    "legacy_buy": 5,
    "routed_exact_sol_in": 5
  },
  "route_classes_excluded_from_l2": [
    "legacy_buy",
    "routed_exact_sol_in"
  ],
  "scope_restriction_supported_route_count": 0,
  "supported_executable_route_classes": []
}
```

## Route Decision Matrix

| route_variant | observed_count | account_count | map_complete | builder_support | requires_bcv2 | rpc_ready | closure_status | recommendation |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| legacy_buy | 5 | 21 | False | unsupported_closed_builder_layout_requires_bcv2 | True | 0/10 | closed_unsupported_builder_layout_requires_bcv2 | do_not_use_as_fallback_without_clean_true_legacy_abi |
| routed_exact_sol_in | 5 | 21 | False | supported_by_current_builder | True | 0/7 | blocked_bcv2_not_load_ready | exclude_until_bcv2_load_ready_or_alternate_route |

## Interpretation

`legacy_buy` is closed by E5B as `unsupported_builder_layout_requires_bcv2`.
It must not be used as a fallback unless a clean true-legacy ABI/account layout
is supplied and test-proven first.

`routed_exact_sol_in` remains a known builder path, but E1 classifies it as
non-executable because the route-required `bonding_curve_v2` account is not
RPC-load-ready in the observed artifacts. That is not a new route class to
implement in E7.

The current supported executable route scope is therefore empty:

```json
[]
```

## Consequence

Do not run another `legacy_buy` smoke.
Do not run R18, L2D2, Phase B, P2/live, V3 selector, or threshold tuning from
this artifact set.

The next valid work is route ABI discovery / observed account-map extraction
for additional Pump.fun buy variants, or an explicit product decision that the
current execution universe is empty until route support expands.

## Route Details

```json
{
  "legacy_buy": {
    "account_position_map_complete": false,
    "account_position_map_coverage": 1,
    "builder_support_status": "unsupported_closed_builder_layout_requires_bcv2",
    "expected_unlock_value": 0,
    "final_manifest_requires_bcv2": true,
    "implementation_risk": "closed_without_clean_true_legacy_abi",
    "instruction_discriminator": "66063d1201daebea",
    "loaded_addresses_required": true,
    "observed_account_count": 21,
    "observed_count": 5,
    "prepared_request_support_status": "fallback_account_set_available",
    "primary_failure_class": "fallback_route_requires_same_bcv2_simulation_load_account",
    "program_id": "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
    "recommendation": "do_not_use_as_fallback_without_clean_true_legacy_abi",
    "route_closure_status": "closed_unsupported_builder_layout_requires_bcv2",
    "route_variant": "legacy_buy",
    "rpc_load_readiness_observed": {
      "rate": 0.0,
      "ready_false": 10,
      "ready_true": 0
    },
    "shadow_simulation_support_status": "not_executable_no_executable_route_account_set",
    "simulation_support_possible": false
  },
  "routed_exact_sol_in": {
    "account_position_map_complete": false,
    "account_position_map_coverage": 14,
    "builder_support_status": "supported_by_current_builder",
    "expected_unlock_value": 0,
    "final_manifest_requires_bcv2": true,
    "implementation_risk": "not_a_new_builder_target_bcv2_readiness_blocker",
    "instruction_discriminator": "unknown",
    "loaded_addresses_required": false,
    "observed_account_count": 21,
    "observed_count": 5,
    "prepared_request_support_status": "prepared_request_manifest_available",
    "primary_failure_class": "bonding_curve_v2_identity_authoritative_but_not_load_ready",
    "program_id": "unknown",
    "recommendation": "exclude_until_bcv2_load_ready_or_alternate_route",
    "route_closure_status": "blocked_bcv2_not_load_ready",
    "route_variant": "routed_exact_sol_in",
    "rpc_load_readiness_observed": {
      "rate": 0.0,
      "ready_false": 7,
      "ready_true": 0
    },
    "shadow_simulation_support_status": "not_executable_no_executable_route_account_set",
    "simulation_support_possible": false
  }
}
```
