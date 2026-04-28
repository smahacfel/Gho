# ADR-0073: PumpSwap canonical account-update bootstrap decoding

**Date:** 2026-04-02
**Status:** Accepted
**Author:** Ghost Father

## Context
Phase 3 proof windows were still failing for fresh PumpSwap-like pools even after the prior AccountUpdate identity-miss flood fix. Fresh failing pools showed:

- `diagnostics.canonical_update_count=0`
- `CURVE_SEED_RPC_FAIL ... unexpected_owner: pAMMBay6... (expected 6EF8...)`
- no `DIAG_ACCOUNT_UPDATE_RELAY`, `DIAG_ACCOUNT_UPDATE_RUNTIME_INGRESS`, or `DIAG_ACCOUNT_UPDATE_APPLIED` for the affected windows

Seer already models PumpSwap create events by setting `bonding_curve = pool_amm_id` because PumpSwap has no separate pump.fun-style bonding curve PDA. However, launcher RPC bootstrap/refresh paths still assumed every `bonding_curve` key pointed to a pump.fun-owned curve account and decoded only the pump.fun `BondingCurve` layout.

## Decision
Treat canonical reserve bootstrap/refresh decoding as **layout-driven by supported AMM owner + Seer canonical account decoder**, not as pump.fun-only bonding-curve parsing.

Implemented changes:

1. Exposed `seer::decode_canonical_account_update` and its payload accessors for shared canonical decoding.
2. Updated `ghost-launcher/src/components/seer.rs` RPC curve seeder to:
   - accept both Pump.fun and PumpSwap program owners
   - decode account data through Seer canonical account decoding
   - seed ShadowLedger bootstrap state from decoded canonical reserves
3. Updated `ghost-launcher/src/oracle_runtime.rs` RPC refresh and reconciliation-cycle paths to decode canonical account updates from both pump.fun curve accounts and PumpSwap AMM pool accounts.
4. Added targeted regressions proving PumpSwap AMM pool layout decodes correctly in launcher bootstrap/reconciliation paths.

## Architectural Impact
This keeps the SSOT for canonical reserve decoding inside Seer and removes duplicated pump.fun-only assumptions from launcher bootstrap consumers.

Affected components:

- `off-chain/components/seer/src/lib.rs`
- `ghost-launcher/src/components/seer.rs`
- `ghost-launcher/src/oracle_runtime.rs`

This aligns:

- create-event identity semantics (`bonding_curve = pool key` for PumpSwap)
- gRPC account-update decoding
- RPC bootstrap seeding
- RPC refresh / reconciliation fallback

## Risk Assessment
**Rate:** Medium

Primary risk is mis-decoding unsupported accounts during bootstrap/refresh. This was mitigated by:

- retaining explicit owner allow-listing (`Pump.fun` or `PumpSwap`)
- using the existing Seer canonical decoder already used on the live AccountUpdate ingest path
- adding regressions for PumpSwap decoding in launcher paths

## Consequences
PumpSwap pools can now receive canonical reserve bootstrap/refresh truth through the same semantic model used by live account updates. This removes the owner/layout mismatch that previously left some fresh windows at zero canonical updates.

Trade-off: launcher fallback paths are now coupled to Seer’s canonical decoder API instead of a local pump.fun-specific parser. That coupling is intentional because Seer is already the canonical account-layout decoder.

## Alternatives Considered
1. **Suppress the unexpected-owner warning**  
   Rejected: would hide the symptom without restoring canonical reserve truth.

2. **Special-case PumpSwap only in logs/metrics**  
   Rejected: would not fix RPC refresh/bootstrap behavior.

3. **Introduce a new dedicated field separate from `bonding_curve` for PumpSwap pool identity**  
   Rejected for now: larger surface-area change than necessary for proof closure. Current semantic contract already intentionally aliases PumpSwap pool pubkey into `bonding_curve`.

## Validation Steps
1. Run targeted launcher regressions:
   - `cargo test -p ghost-launcher rpc_curve_seeder_accepts_pumpswap_owner_and_decodes_pool_layout -- --nocapture`
   - `cargo test -p ghost-launcher test_decode_rpc_canonical_account_update_supports_pumpswap_pool_layout -- --nocapture`
2. Run existing Seer PumpSwap replay regression:
   - `cargo test -p seer pumpswap_account_update_before_mapping_replays -- --nocapture`
3. Re-run Phase 3 fresh proof and verify affected PumpSwap pools no longer close with `canonical_update_count=0`.
4. Confirm logs now show canonical account-update diagnostics or successful bootstrap/reconciliation for newly detected PumpSwap pools.
