# RAPORT P3.7-L1R13 Route Builder BCV2 Source Repair / Route Fallback

## Status

P3.7-L1R13 code-level status: PASS.

Runtime smoke status: not run in this commit. This report claims code-level
repair and targeted validation only.

No L2 ablation, collection, Phase B, P2, live execution, IWIM change, or
Gatekeeper threshold change was performed.

## Problem

L1R12 showed that the builder-provided `bonding_curve_v2` identities in the
R16-r9 sample were not merely absent from DIAG/MFS. They were also missing on
current RPC preflight:

```text
matrix_rows = 5
rpc_get_account_status_counts = {"missing": 5}
matrix_classifications = {"builder_bcv2_missing_on_rpc": 5}
diag_seen_exact_builder_bcv2_rows = 0
diag_seen_other_curve_for_mint_rows = 5
mfs_contains_builder_bcv2_pubkey_rows = 0
recommended_next_path = route_builder_source_repair_or_route_fallback
```

That makes the bug a route-builder source problem. A builder-only derived
`bonding_curve_v2` PDA cannot be treated as an executable route account when the
account is absent from RPC. If an observed transaction account meta supplies the
route-specific BCV2 account, that observed account is a stronger authoritative
source than blind derivation.

## Code Changes

L1R13 adds an additive observed-route-account path:

- `TradeEvent` now carries optional `bonding_curve_v2`.
- `PoolTransaction` now carries optional `bonding_curve_v2`.
- Seer binary parser extracts `bonding_curve_v2` from Pump.fun buy instruction
  account meta index 16 when available.
- `trade_event_to_pool_transaction()` propagates the observed BCV2 into launcher
  transactions.
- `BuyAccountOverrides` now carries optional `bonding_curve_v2`.
- `DirectBuyBuilder` has
  `build_buy_ix_with_accounts_and_bonding_curve_v2(...)`; the existing
  `build_buy_ix_with_accounts(...)` remains backward-compatible and passes no
  override.
- Prepared buy request construction uses the observed BCV2 override at account
  meta index 16 when present.
- Probe account manifests classify matching observed BCV2 overrides as
  `observed_tx_account_meta`.
- BCV2 source authority treats `observed_tx_account_meta` as
  `authoritative_observed_tx`.
- Builder-only / derived-unverified BCV2 remains non-authoritative and
  fail-closed under the L1R11/L1R12 readiness contract.

## Audit Changes

`scripts/v3_p37_mfs_lifecycle_join_key_audit.py` now reports:

```text
builder_bcv2_authoritative_observed_tx_rows
builder_bcv2_authoritative_mfs_rows
builder_bcv2_derived_unverified_rows
route_excluded_bcv2_missing_rows
route_fallback_attempted_rows
route_fallback_success_rows
no_executable_route_account_set_rows
account_not_found_after_simulation_rows
```

Active-shadow equivalents are reported under the `active_shadow_*` prefix.

## Validation

Python:

```text
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
python3 -m py_compile scripts/v3_p37_l1_reject_diagnostics.py scripts/v3_p37_probe_execution_account_readiness_report.py scripts/v3_p37_bonding_curve_v2_reconciliation.py scripts/v3_p37_bonding_curve_v2_coverage_matrix.py scripts/v3_p37_mfs_lifecycle_join_key_audit.py
```

Result:

```text
OK - 18 tests
```

Rust:

```text
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
cargo test -p trigger test_build_buy_ix_uses_observed_bonding_curve_v2_override -- --nocapture
cargo test -p seer enrich_trade_optional_accounts_from_source_ix_salvages_buy_overrides -- --nocapture
cargo test -p ghost-launcher --lib test_build_prepared_buy_request_uses_observed_bonding_curve_v2_override -- --nocapture
```

Results:

```text
p37_shadow_probe: 57 passed
p37_counterfactual_probe: 8 passed
trigger observed BCV2 override: 1 passed
seer observed BCV2 extraction: 1 passed
ghost-launcher prepared request observed BCV2 override: 1 passed
```

Formatting/static checks:

```text
rustfmt --edition 2021 --check <touched Rust files>
git diff --check
```

Result: PASS.

Existing repository warnings were observed during cargo tests. They were not
introduced as part of L1R13 and were not treated as blockers for this narrow
repair.

## Known Limitations

L1R13 does not prove runtime entry recovery by itself. It makes observed route
account identity available to the builder and preserves fail-closed behavior for
builder-only missing BCV2. A fresh R16 smoke is still required to prove whether
the current stream produces enough observed authoritative BCV2 rows to unblock
probe or active-shadow entries.

No full route fallback implementation was added in this commit. The audit now
has route fallback counters, but runtime fallback behavior remains a separate
step unless the observed BCV2 source path is enough.

## Next Runtime Gate

Recommended next runtime gate:

```text
R16-r10 Route Builder BCV2 Source Smoke
```

Acceptance:

- strict replay remains `full_replay_ok`;
- identity/hash contract remains PASS;
- exact decision/V3 join remains 100%;
- rows with observed BCV2 report `authoritative_observed_tx`;
- builder-only missing BCV2 does not reach simulation as AccountNotFound;
- successful probe/active-shadow entries appear, or route exclusion is explicit
  and counted;
- active BUY, live, P2, Phase B, IWIM, and thresholds remain untouched.

If observed BCV2 does not produce entries, the next decision is route
fallback/exclusion for this route class, not L2 ablation or threshold tuning.
