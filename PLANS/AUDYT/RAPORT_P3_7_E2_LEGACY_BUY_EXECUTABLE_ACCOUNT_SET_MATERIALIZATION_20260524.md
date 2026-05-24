# RAPORT P3.7-E2 Legacy Buy Executable Account Set Materialization

Date: 2026-05-24
Scope: code-level implementation and targeted validation only

## Verdict

P3.7-E2 code-level: PASS

Runtime smoke E2S: NOT RUN

L2D2 / thresholds / Gatekeeper policy / P2 / live / Phase B: unchanged and NO-GO.

## What Changed

E2 adds explicit legacy_buy account-set materialization and diagnostics for the fallback route.

Core changes:

- `BuyAccountOverrides` now carries `legacy_buy_curve_pubkey`, `legacy_buy_curve_source`, and `legacy_buy_curve_authority_status`.
- Probe and active shadow diagnostics now emit legacy_buy route/account fields:
  - `legacy_buy_account_set_status`
  - `legacy_buy_curve_pubkey`
  - `legacy_buy_curve_source`
  - `legacy_buy_curve_authority_status`
  - `legacy_buy_curve_rpc_load_status`
  - `legacy_buy_curve_rpc_load_ready`
  - `legacy_buy_required_roles`
  - `legacy_buy_missing_roles`
  - `legacy_buy_missing_pubkeys`
  - `legacy_buy_route_ready`
  - `legacy_buy_route_not_ready_reason`
- Legacy curve materialization uses authoritative runtime sources:
  - `account_state_core` for active BUY path
  - materialized feature/pool snapshot for probe fallback context
- Route resolution now fails closed when primary `legacy_buy` lacks a ready simulation-load account set.
- Existing routed_exact_sol_in BCV2 is not reused as the legacy core bonding curve.

## Important Boundary

This does not change Gatekeeper policy, thresholds, IWIM, P2/live behavior, or Phase B status.

This also does not claim that legacy_buy is executable in runtime. The current builder path still needs all simulation-load accounts to be ready before `simulate_buy`; E2 only materializes and audits the legacy core curve correctly, then lets the existing precheck/resolver fail closed if other required accounts remain missing.

## Audit Counters Added

`scripts/v3_p37_mfs_lifecycle_join_key_audit.py` now reports, for probe and active shadow:

- `legacy_buy_route_attempted_rows`
- `legacy_buy_route_ready_rows`
- `legacy_buy_route_not_ready_rows`
- `legacy_buy_missing_core_curve_account_rows`
- `legacy_buy_missing_associated_bonding_curve_rows`
- `legacy_buy_authoritative_curve_rows`
- `legacy_buy_rpc_load_ready_rows`
- `legacy_buy_successful_entry_rows`
- legacy_buy source/authority/RPC-load/not-ready reason distributions

## Validation

Targeted Rust:

```bash
cargo test -p ghost-launcher --lib p37_route_resolver -- --nocapture
```

Result: PASS, 4 tests passed.

Targeted Rust materialization tests:

```bash
cargo test -p ghost-launcher --lib p37_shadow_probe_uses_selection_curve_snapshot -- --nocapture
```

Result: PASS, 2 tests passed.

Python audit:

```bash
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
```

Result: PASS, 21 tests passed.

## Known Limits

- E2S runtime smoke has not been run yet.
- PASS-A execution unlock is not claimed.
- If E2S still reports `legacy_buy_route_ready_rows = 0`, the next decision remains route support expansion or route scope restriction, not Gatekeeper tuning.

## Next Step

Run one targeted E2S smoke only after this code-level checkpoint:

P3.7-E2S legacy_buy executable route smoke

Expected decision after E2S:

- successful entries appear: `GO_R18_EXECUTABLE_ROUTE_SCOPED_RUN`
- clean fail-closed with specific missing account class: route support remains blocker
- post-simulation AccountNotFound or silent fallback: E2 implementation FAIL
