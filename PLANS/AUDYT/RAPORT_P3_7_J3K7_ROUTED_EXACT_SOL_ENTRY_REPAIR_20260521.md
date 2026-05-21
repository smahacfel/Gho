# RAPORT P3.7-J3K7 Routed Exact-SOL-In Entry Repair

Date: 2026-05-21

## Verdict

`P3.7-J3K7 code-level repair = READY_FOR_SMOKE`

This is a narrow corrective repair after the J3K6 runtime smoke. J3K6
successfully converted non-authoritative creator-vault rows into precheck skips,
but all transported rows were `routed_exact_sol_in` rows with no
`entry_token_amount_raw`, so no probe entries were materialized.

J3K7 does not claim runtime PASS. The next gate is a fresh bounded smoke:

`shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k7-r1`

Collection, Phase B, P2, live, IWIM changes and threshold tuning remain
`HOLD / NO-GO`.

## Problem

R15 J3K6-r1 produced:

- `probe_transport_rows = 10`
- `probe_entry_rows = 0`
- `buy_variant = routed_exact_sol_in`
- `token_param_role = min_tokens_out`
- `entry_token_amount_raw = null`
- `probe_entry_materialization_status = transport_only_missing_token_quantity`

That means the probe plane could still consume dispatch quota and emit
transport rows that lacked enough token-quantity evidence to create
`probe_shadow_entries`.

## Code Changes

Touched file:

- `ghost-launcher/src/oracle_runtime.rs`

Changes:

- `p37_shadow_probe_derive_account_override_context_for_pool(...)` now populates
  `legacy_buy_curve` for both `LegacyBuy` and `RoutedExactSolIn` when a
  decision-time-safe curve snapshot is available.
- `p37_shadow_probe_execution_precheck(...)` now fails closed for
  `RoutedExactSolIn` with no local curve snapshot:
  `missing_routed_entry_quote_curve`.
- `p37_shadow_probe_transport_from_event(...)` now logs
  `entry_token_amount_raw` from the simulation event first, falling back to the
  prepared request token params only when the event has no token quantity.

This keeps routed exact-SOL-in rows in one of two safe states:

- entry quantity is available and entry materialization can proceed;
- the row is skipped before dispatch and does not become transport-only noise.

## Tests Added

New Rust coverage:

- `p37_shadow_probe_execution_precheck_requires_routed_entry_quote_curve`
- `p37_shadow_probe_uses_selection_curve_snapshot_for_routed_entry_quote_after_runtime_cleanup`
- `p37_shadow_probe_transport_uses_simulated_token_quantity_when_request_quote_missing`

These tests cover the fail-closed precheck, the routed curve fallback from the
selection snapshot, and transport logging of simulation-derived token quantity.

## Validation

Local validation before smoke:

- `cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture` PASS
- `cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture` PASS
- `python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py` PASS
- `python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v` PASS

Pending before launch:

- release build for the runtime binary
- J3K7 bounded smoke in a clean namespace

## R15 J3K7 Smoke Gate

Config:

`configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k7-r1.toml`

Runtime namespace:

`shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k7-r1`

Minimal PASS:

- strict V3 replay OK;
- probe selection/transport/entry exact decision/V3 join remains 100%;
- creator-vault non-authoritative rows remain precheck skips;
- routed exact-SOL-in rows either materialize `entry_token_amount_raw` and
  produce entries, or are skipped before dispatch as
  `missing_routed_entry_quote_curve`;
- `transport_only_missing_token_quantity` no longer dominates transported rows;
- active BUY rows remain zero;
- live/P2 paths remain untouched.

If the smoke still produces transport-only routed rows with missing token
quantity, collection remains blocked and the next fix must target routed token
amount derivation or route identity.
