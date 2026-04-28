# Pool State SSOT — gRPC Yellowstone Price Layer

## Overview

The **Pool State SSOT** (Single Source of Truth) replaces PumpPortal-only pricing
with a unified price layer that tracks pool state in real time for BOTH phases:

- **Bonding Curve** (pump.fun) — virtual reserves pricing
- **AMM** (post-migration, e.g. Raydium) — real reserves constant-product pricing

## How Price Is Computed

### Bonding Curve Phase

```
mark_price = v_sol / v_tokens
```

**Buy quote** (SOL → tokens):
```
sol_in_eff  = sol_in × (1 − fee_bps / 10000)
tokens_out  = (v_tokens × sol_in_eff) / (v_sol + sol_in_eff)
```

**Sell quote** (tokens → SOL):
```
token_in_eff = token_in × (1 − fee_bps / 10000)
sol_out      = (v_sol × token_in_eff) / (v_tokens + token_in_eff)
```

Default fee: 100 bps (1%), configurable via `bonding_fee_bps_default`.

### AMM Phase

```
mark_price = reserve_sol / reserve_token
```

Same constant-product formulas as bonding curve, but using real reserves
from AMM vault accounts. Default fee: 25 bps (0.25%), or from pool state
if known. Configurable via `amm_fee_bps_default`.

### Quote Output

Every quote returns:
- `expected_out` — expected output amount
- `effective_price` — SOL per token after fee/impact
- `min_out` — minimum output after slippage tolerance
- `slippage_est_bps` — estimated slippage in basis points

## What Accounts Are Subscribed

### Bonding Curve Phase
- **Bonding curve state account** (bondingCurveKey PDA)
  - Fields parsed: `vSolInBondingCurve`, `vTokensInBondingCurve`

### AMM Phase
- **AMM pool state account** (pool config)
- **SOL vault token account** (for SOL reserve balance)
- **Token vault token account** (for token reserve balance)

Reserves come from vault account balances (account state), NOT from parsing
individual swap transaction instructions.

## How Phase Switch Happens

Phase detection follows these rules:

1. All pools start in `BondingCurve` phase
2. Transition to `Amm` triggers on ANY of:
   - AMM pool accounts become resolvable
   - Migration event observed
   - Bonding progress crosses threshold AND AMM accounts are resolvable
3. **Once `Amm`, NEVER reverts to `BondingCurve`** (one-way switch)

The switch is logged with reason:

```
INFO PHASE_SWITCH Bonding→Amm pool_id=<pubkey> base_mint=<pubkey> reason=amm_accounts_resolved
```

## How to Confirm in Logs That AMM Updates Are Flowing

The SSOT emits throttled structured logs (max 10 Hz per pool) on each snapshot update.

### Example: Bonding Phase
```
INFO SSOT snapshot update pool_id=<pubkey> base_mint=<pubkey> phase=BondingCurve source=Yellowstone age_ms=0 v_sol=30000000000 v_tokens=1073000000000000 price_mark=2.795e-5 quote_sell_effective_price=2.77e-5
```

### Example: Phase Switch
```
INFO PHASE_SWITCH Bonding→Amm pool_id=<pubkey> base_mint=<pubkey> reason=amm_accounts_resolved
```

### Example: AMM Phase (reserves changing over time)
```
INFO SSOT snapshot update pool_id=<pubkey> base_mint=<pubkey> phase=Amm source=Yellowstone age_ms=0 reserve_sol=50000000000 reserve_token=200000000000 price_mark=0.25 quote_sell_effective_price=0.2499
INFO SSOT snapshot update pool_id=<pubkey> base_mint=<pubkey> phase=Amm source=Yellowstone age_ms=0 reserve_sol=55000000000 reserve_token=180000000000 price_mark=0.3055 quote_sell_effective_price=0.3051
```

**Key verification**: `price_mark` and `quote_sell_effective_price` CHANGE between
updates. If they remain constant while gRPC updates flow, the system is broken.

### Metrics Counters
- `snapshots_updated_bonding` / `snapshots_updated_amm` — by phase
- `snapshots_source_yellowstone` / `snapshots_source_pumpportal` / `snapshots_source_fallback` — by source
- `snapshot_stale_total` — staleness events
- `phase_switches_total` — phase transitions

## Failure Modes

### Stale Snapshot → Conservative Behavior

When `now − snapshot.last_update_unix_ms > stale_ms` (default 1500ms):

1. `check_staleness()` returns `(true, age_ms)`
2. `snapshot_stale_total` metric incremented
3. AEM receives `oracle_stale_age_ms > 0` in `StateFeatures`
4. **Hard Safety** triggers `ORACLE_STALE`:
   - AEM goes conservative (Partial exit, no WAIT_RECLAIM)
   - Revolver avoids aggressive exits without fresh quote

### AMM Zero Reserves → ORACLE_STALE Guard

If an AMM update arrives with `reserve_sol == 0` or `reserve_token == 0`:
- The update is **rejected** (not stored)
- `snapshot_stale_total` metric incremented
- Warning logged

### PumpPortal After Migration → Rejected

Once phase == Amm, bonding-curve updates from PumpPortal are silently
rejected. PumpPortal can only serve as an "activity signal" post-migration.

## Configuration Knobs

| Config Key | Default | Description |
|---|---|---|
| `enable_yellowstone` | `true` | Enable Yellowstone gRPC as primary source |
| `yellowstone_endpoint` | `""` | Yellowstone gRPC endpoint URL |
| `yellowstone_auth_token` | `None` | Optional auth token |
| `stale_ms` | `1500` | Max snapshot age before ORACLE_STALE (ms) |
| `slippage_bps_default` | `100` | Default slippage tolerance for min_out (1%) |
| `bonding_fee_bps_default` | `100` | Bonding curve fee if unknown (1%) |
| `amm_fee_bps_default` | `25` | AMM fee if unknown (0.25%) |
| `bonding_progress_threshold_pct` | `95.0` | Threshold for progress-based phase switch |

## Module Structure

```
ghost-brain/src/pool_state_ssot/
├── mod.rs           — module root + re-exports
├── config.rs        — SsotConfig with all config knobs
├── phase.rs         — PoolPhase enum + should_switch_to_amm()
├── snapshot.rs      — PoolSnapshot SSOT data object
├── store.rs         — SnapshotStore (concurrent, phase-aware, metrics)
├── quote_engine.rs  — QuoteEngine (bonding + AMM executability quotes)
└── yellowstone.rs   — YellowstoneSubscriber (account subscription manager)
```

## Old Price Path Removed

Post-buy, the system uses SSOT `QuoteEngine` output exclusively:
- `mark_price` from `PoolSnapshot.price_mark_sol_per_token`
- Sell executability via `QuoteEngine::quote(snapshot, Sell, amount, config)`

PumpPortal / marketCap-based pricing is NOT used as SSOT after migration.
PumpPortal remains as an optional fast hint for bonding-curve phase ONLY.
