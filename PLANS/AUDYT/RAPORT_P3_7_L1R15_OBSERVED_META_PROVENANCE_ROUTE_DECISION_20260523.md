# P3.7-L1R15 / J3Q Observed-Meta Provenance Validation

Status: CODE-LEVEL PASS / RUNTIME NOT VALIDATED

## Context

R16-r11 validated L1R14 as a runtime PASS-B: observed transaction account
metadata can identify a route-specific `bonding_curve_v2`, but it no longer
implies execution readiness. Rows with `missing_on_rpc_precheck` remain
fail-closed and no longer reach post-simulation `AccountNotFound` for BCV2.

L1R15 addresses the remaining ambiguity: whether the observed BCV2 identity was
extracted from the correct instruction account position and route, or whether
the parser could be confusing instruction account position 16 with global
`account_keys[16]`.

## Code Changes

Implemented additively:

- Added `ObservedAccountMetaProvenance` to Seer trade events and launcher
  `PoolTransaction`.
- Extended the Seer binary parser to capture BCV2 provenance from the matched
  Pump.fun buy instruction:
  - source transaction signature;
  - source slot and source slot index;
  - source instruction index;
  - source program id;
  - instruction discriminator / buy variant;
  - instruction account position;
  - resolved message account index;
  - resolved pubkey;
  - transaction success / meta error;
  - provenance status.
- Propagated the provenance through `BuyAccountOverrides`, probe account
  manifest entries, probe selection/transport/entry diagnostics and active
  shadow account diagnostics.
- Tightened BCV2 authority semantics:
  - `observed_tx_account_meta` is `authoritative_observed_tx` only when
    `observed_bcv2_provenance_status=route_compatible`;
  - missing provenance becomes `observed_tx_unverified`;
  - incompatible provenance becomes `observed_meta_not_route_compatible`.
- Extended `scripts/v3_p37_mfs_lifecycle_join_key_audit.py` with observed BCV2
  provenance counters for probe and active shadow rows.

## Important Limitation

`observed_bcv2_loaded_address_source` is currently emitted as
`resolved_transaction_account_keys`. The runtime event provides the resolved
account-key set; it does not currently preserve whether a key came from static
message keys or an address lookup table. L1R15 therefore proves instruction
position to resolved pubkey mapping, but not static-versus-loaded provenance.

## Tests

Passed:

```text
cargo test -p seer --lib enrich_trade_optional_accounts_from_source_ix_salvages_buy_overrides -- --nocapture
cargo test -p seer --lib enrich_trade_optional_accounts_resolves_bcv2_instruction_index_not_global_index -- --nocapture
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
```

Key test coverage:

- route-compatible observed BCV2 remains authoritative;
- missing or route-incompatible observed provenance is not authoritative;
- instruction account position 16 can resolve to a different message account
  index, proving the parser does not use global `account_keys[16]`;
- active shadow readiness blocks `authoritative_observed_tx` without
  route-compatible provenance;
- legacy rows without the new fields still parse in the audit.

## Runtime Gate

No runtime smoke was started in this code-level report.

Next runtime gate:

```text
R16-r12 / L1R15 observed-meta provenance smoke
```

Acceptance:

- strict replay = `full_replay_ok`;
- diagnostic quality = PASS;
- identity/hash contract = PASS;
- observed BCV2 provenance status populated for rows with observed BCV2;
- no row with provenance other than `route_compatible` reports
  `authoritative_observed_tx`;
- no row with `missing_on_rpc_precheck` reports
  `builder_required_curve_account_ready=true`;
- post-simulation BCV2 `AccountNotFound` remains 0;
- P2/live, thresholds, PDD, HHI, prosperity and IWIM remain untouched.

## Decision

L1R15 is code-level complete. The next decision is runtime-only:

- If provenance is route-compatible and BCV2 is RPC-load ready for some rows,
  return to R16 diagnostic policy path.
- If provenance is route-compatible but BCV2 remains missing on RPC, proceed to
  route fallback or route exclusion.
- If provenance is not route-compatible, fix parser/source extraction before
  any route decision.

L2 ablation, collection, Phase B, P2/live and threshold tuning remain HOLD.
