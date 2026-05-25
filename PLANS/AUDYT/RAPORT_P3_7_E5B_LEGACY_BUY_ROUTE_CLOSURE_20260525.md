# P3.7-E5B Legacy Buy Route Closure

Date: 2026-05-25

## Verdict

`LEGACY_BUY_UNSUPPORTED_REMOVED_FROM_FALLBACK`

E5B closes the current `legacy_buy` fallback path as unsupported under the
current builder/account-layout support.

No runtime smoke was run for E5B. This is an offline/code-level route-support
closure based on E5A.

## Decision Basis

E5A produced:

```text
BUILDER_LEGACY_LAYOUT_USES_BCV2
```

The current `DirectBuyBuilder` `LegacyBuy` path uses the legacy discriminator
and payload shape, but it still builds the extended account layout containing
`bonding_curve_v2` at account index 16.

No authoritative clean true-legacy ABI/account-position map that omits
`bonding_curve_v2` is present in the repo. Therefore E5B does not implement a
new true-legacy builder layout.

## Runtime Contract Change

`legacy_buy` remains visible as diagnostics, but it is no longer allowed to
unlock execution as a fallback route.

When primary `routed_exact_sol_in` is blocked by missing/non-load-ready BCV2 and
the only fallback candidate is the current `legacy_buy` builder path:

```text
route_resolution_status = no_executable_route_account_set
selected_route_kind = null
fallback_route_kind = legacy_buy
fallback_route_attempted = false
fallback_route_ready = false
fallback_route_not_ready_reason = unsupported_builder_layout_requires_bcv2
fallback_failure_class = fallback_unsupported_builder_layout
no_executable_route_account_set_reason =
  unsupported_builder_layout_requires_bcv2:bonding_curve_v2:<pubkey>
legacy_buy_route_not_ready_reason =
  legacy_buy_unsupported_builder_layout_requires_bcv2
```

This preserves curve/account-set readiness telemetry while preventing the hybrid
legacy payload + extended BCV2 account layout from entering precheck/simulation
as an executable fallback.

## Audit Additions

`scripts/v3_p37_mfs_lifecycle_join_key_audit.py` now reports:

```text
legacy_buy_route_unsupported_builder_layout_rows
legacy_buy_excluded_from_execution_route_universe_rows
legacy_buy_removed_from_fallback_candidates_rows
active_shadow_legacy_buy_route_unsupported_builder_layout_rows
active_shadow_legacy_buy_excluded_from_execution_route_universe_rows
active_shadow_legacy_buy_removed_from_fallback_candidates_rows
```

Unsupported builder layout is classified as:

```text
fallback_unsupported_builder_layout
```

and remains a route-exclusion class, not a repairable account-source bug.

## Non-Goals

- No true legacy ABI was invented.
- No DirectBuyBuilder account layout was changed.
- No Gatekeeper thresholds or policy were changed.
- No V3 selector/L2/R18/P2/live behavior was changed.
- No runtime smoke was run after this closure-only change.

## Validation

```text
python3 scripts/check_no_committed_chainstack_tokens.py
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py
cargo test -p ghost-launcher --lib p37_route_resolver -- --nocapture
cargo test -p ghost-launcher --lib p37_legacy_buy -- --nocapture
cargo test -p ghost-launcher --lib selected_legacy_buy_builder_final_manifest_bcv2_blocks_simulation -- --nocapture
```

All commands passed locally. Cargo emitted pre-existing warnings.

## Next Path

Return to the E1 route-support matrix:

```text
E6 = next route target from E1 matrix
```

or apply scope restriction to route classes with verified executable builder
support.

Do not run another `legacy_buy` smoke unless a clean true-legacy ABI/account
layout is supplied and test-proven first.
