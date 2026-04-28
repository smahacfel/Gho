# ADR-0032: Shadow Ledger Bonding Curve Math and Gatekeeper Contract Audit

**Date:** 2026-03-23  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

User requested an audit of whether Shadow Ledger reconstructs Pump.fun bonding-curve state using mathematically correct rules and whether the same correct state is exposed downstream to Gatekeeper.

The audit scope was deliberately constrained to:
- canonical curve math in `ghost-core`
- Shadow Ledger bootstrap / storage / live evolution paths
- launcher-side enrichment and Gatekeeper Phase 6 consumption
- exact semantics of these fields:
  - `virtual_sol_reserves`
  - `virtual_token_reserves`
  - `real_sol_reserves`
  - `real_token_reserves`
  - `token_total_supply`
  - `curve_data_known`
  - `curve_finality`
  - `last_update_ts_ms`

The audit explicitly excluded unrelated coverage-ratio calculations.

## Decision

The repository currently encodes **one mathematically consistent authoritative model for Pump.fun virtual-reserve trading**, but **Gatekeeper does not receive the full raw Shadow Ledger curve tuple**.

### Confirmed as correct

1. **Canonical virtual-reserve math is internally consistent and protocol-shaped**
   - `ghost-core/src/market_state.rs`
   - `ghost-core/src/shadow_ledger/history_types.rs`
   - `ghost-core/src/shadow_ledger/simulation.rs`
   - `ghost-core/src/shadow_ledger/live_pipeline.rs`

   Verified properties:
   - invariant uses $k = v_{sol} \cdot v_{tok}$
   - BUY path applies 1% fee to SOL input before reserve update
   - SELL path applies 1% fee to SOL output after invariant-derived reserve move
   - `simulate_buy`, `simulate_sell`, `calculate_buy_price`, `calculate_sell_price`, and `apply_trade_strict` use the same virtual-reserve model
   - live append path uses `ReconstructedState::apply_trade_strict()` as the authority path

2. **Protocol genesis constants are centralized and canonical**
   - `ghost-core/src/shadow_ledger/genesis.rs`

   Canonical genesis tuple is:
   - `virtual_token_reserves = 1_073_000_000_000_000`
   - `virtual_sol_reserves = 30_000_000_000`
   - `real_token_reserves = 793_100_000_000_000`
   - `real_sol_reserves = 30_000_000_000`
   - `token_total_supply = 1_000_000_000_000_000`

3. **Confirmed bootstrap and account-update writes preserve curve provenance explicitly**
   - parser: `off-chain/components/seer/src/curve_parser.rs`
   - confirmed bootstrap: `off-chain/components/seer/src/lib.rs::store_confirmed_bootstrap`
   - write metadata / storage precedence: `ghost-core/src/shadow_ledger/ledger.rs`
   - WAL replay preserves `curve_finality` and `last_update_ts_ms`: `ghost-launcher/src/wal_recovery.rs`

4. **`curve_data_known` and `curve_finality` are propagated to launcher/Gatekeeper faithfully**
   - `ghost-launcher/src/oracle_runtime.rs::enrich_pool_tx_from_shadow_ledger`
   - `ghost-launcher/src/components/gatekeeper.rs`

### Confirmed boundary / limitation

1. **Gatekeeper does not receive full raw curve state through `PoolTransaction`**
   - `ghost-launcher/src/events.rs::PoolTransaction` contains:
     - `v_tokens_in_bonding_curve`
     - `v_sol_in_bonding_curve`
     - `market_cap_sol`
     - `curve_data_known`
     - `curve_finality`
   - It does **not** carry:
     - `real_sol_reserves`
     - `real_token_reserves`
     - `token_total_supply`
     - `last_update_ts_ms`
     - raw `virtual_*` integer fields

   Therefore the statement “Shadow Ledger exposes the same full curve state to Gatekeeper” is **false** if interpreted literally.

2. **Gatekeeper Phase 6 uses a derived virtual-reserve contract, not the full canonical reserve tuple**
   - `ghost-launcher/src/components/gatekeeper.rs::compute_bonding_curve_dynamics`

   Gatekeeper computes:
   - price from $v_{sol} / v_{tok}$
   - market cap from derived price times fixed genesis supply
   - bonding progress from virtual tokens remaining relative to constant `PUMP_GENESIS_TOKEN_SUPPLY`

   This is a valid derived signal path for early curve dynamics, but it is **not identical** to the canonical helper in `BondingCurve::get_bonding_progress()`, which uses:
   $$
   progress = \frac{real\_sol\_reserves}{MAX\_REAL\_SOL\_RESERVES} \cdot 100
   $$

3. **Repair/reconciliation path may intentionally degrade raw-field fidelity when only virtual reserves are known**
   - `ghost-core/src/shadow_ledger/reconciliation.rs::build_repair_curve`

   In repair-only situations, the code sets:
   - `real_* = virtual_*`
   - `token_total_supply = virtual_token_reserves`

   This is explicitly documented as a conservative fallback for healing/simulation continuity, not as exact on-chain truth.

## Architectural Impact

This audit confirms the following architectural contract:

- **Shadow Ledger SSOT for math**: yes, for canonical virtual-reserve CPMM behavior and curve provenance metadata.
- **Gatekeeper SSOT for full raw curve tuple**: no.
- **Gatekeeper consumer contract**: virtual reserves + derived price/mcap + truth/finality flags.
- **Scoring/candidate side** in `OracleRuntime` can access additional fields such as `token_total_supply` and a progress approximation, but this is separate from the `PoolTransaction` contract consumed by Gatekeeper Phase 6.

This means downstream consumers must not assume that Gatekeeper decisions/logs contain the complete authoritative Pump.fun account tuple even when Shadow Ledger internally has it.

## Risk Assessment

**Rate: Medium**

### Low-risk findings
- Core virtual-reserve math is coherent across simulation, ledger replay, and live append.
- Bootstrap/parser paths align on the same state model.
- `curve_data_known` / `curve_finality` semantics are explicit and normalized consistently.

### Medium-risk findings
- Gatekeeper progress gating is based on **virtual-token depletion**, while canonical `BondingCurve` helper exposes progress from **real SOL accumulation**.
- Missing `real_*`, `token_total_supply`, and `last_update_ts_ms` on `PoolTransaction` means forensic consumers can over-assume what Gatekeeper truly saw.
- Repair fallback can temporarily produce internally useful but non-literal raw fields.

### Not observed in this audit
- No evidence of multiple conflicting fee formulas in authoritative state evolution.
- No evidence that BUY/SELL inverse helpers are using a different fee regime from live reconstruction.

## Consequences

### Easier
- We can trust Shadow Ledger’s core virtual-reserve simulation math as a production basis.
- We can trust `curve_data_known` / `curve_finality` as meaningful truth/provenance flags.
- We can reason about Gatekeeper Phase 6 as operating on a clearly bounded derived contract.

### Harder
- We cannot claim that Gatekeeper directly consumes the full Pump.fun account state.
- Any forensic statement about `real_*` or `last_update_ts_ms` at Gatekeeper decision time must come from Shadow Ledger / WAL / account-update storage, not `PoolTransaction` alone.
- “Bonding progress” must be qualified by consumer path because there are at least two semantics in the repo:
  - canonical helper: real SOL based
  - Gatekeeper Phase 6: virtual token depletion based

## Alternatives Considered

1. **Treat current Gatekeeper contract as equivalent to full Shadow Ledger state**
   - Rejected because `PoolTransaction` structurally lacks `real_*`, `token_total_supply`, and `last_update_ts_ms`.

2. **Assume reconciliation repair fields are canonical truth**
   - Rejected because the implementation explicitly documents them as conservative fallbacks when only virtual reserves are available.

3. **Assume `curve_data_known=true` implies full raw tuple availability everywhere**
   - Rejected because launcher enrichment only exports a reduced derived view to Gatekeeper.

## Validation Steps

1. Read and compare canonical math implementations:
   - `ghost-core/src/market_state.rs`
   - `ghost-core/src/shadow_ledger/history_types.rs`
   - `ghost-core/src/shadow_ledger/simulation.rs`
   - `ghost-core/src/shadow_ledger/live_pipeline.rs`

2. Read and compare bootstrap / parser / storage paths:
   - `ghost-core/src/shadow_ledger/genesis.rs`
   - `off-chain/components/seer/src/curve_parser.rs`
   - `off-chain/components/seer/src/lib.rs`
   - `ghost-core/src/shadow_ledger/ledger.rs`
   - `ghost-launcher/src/wal_recovery.rs`

3. Read launcher export and Gatekeeper consumer contract:
   - `ghost-launcher/src/events.rs`
   - `ghost-launcher/src/oracle_runtime.rs`
   - `ghost-launcher/src/components/gatekeeper.rs`

4. Test verification run:
   - `cargo test -p ghost-core market_state::tests -- --nocapture`
   - `cargo test -p ghost-launcher --lib curve_data_known -- --nocapture`

5. Observed test outcome:
   - `ghost-core` test command completed successfully before launcher tests ran.
   - `ghost-launcher --lib` filtered tests passed:
     - `test_curve_data_known_range_check_pass`
     - `test_curve_data_known_range_check_fail`

6. Additional note:
   - unrelated `ghost-launcher` integration tests currently fail to compile because of missing `semantic` field initializers in `snapshot_engine_integration.rs`; this is outside the scope of this audit and does not invalidate the library-level curve contract checks.
