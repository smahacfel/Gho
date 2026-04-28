# gRPC Yellowstone → SnapshotStore SSOT — Operational Runbook

## 1. Enabling gRPC Yellowstone

### Required Configuration

Set the following fields in your `SsotConfig` (or equivalent TOML/env):

| Field | Description | Example |
|---|---|---|
| `enable_yellowstone` | Enable Yellowstone gRPC as primary data source | `true` |
| `yellowstone_endpoint` | gRPC endpoint URL | `http://your-yellowstone:10000` |
| `yellowstone_auth_token` | Optional auth token | `"your-token"` or `None` |
| `stale_ms` | Max snapshot age before ORACLE_STALE (ms) | `1500` |
| `slippage_bps_default` | Default slippage tolerance (basis points) | `100` |
| `bonding_fee_bps_default` | Fee for bonding curve phase (bps) | `100` |
| `amm_fee_bps_default` | Fee for AMM phase (bps) | `25` |

### Startup Wiring

1. Create `YellowstoneSubscriber::new(config)` and wrap in `Arc`:
   ```rust
   let subscriber = Arc::new(YellowstoneSubscriber::new(config));
   ```
2. For each pool in bonding curve phase:
   ```rust
   subscriber.subscribe_bonding_curve(pool_id, base_mint, bonding_curve_account);
   ```
3. For each pool post-migration (AMM):
   ```rust
   subscriber.subscribe_amm(pool_id, base_mint, amm_pool_account, Some(sol_vault), Some(token_vault));
   ```
4. Build gRPC subscription filters:
   ```rust
   let filters = subscriber.build_account_filters();
   // Include `filters` in SubscribeRequest.accounts
   ```
5. On each raw account update from gRPC stream:
   ```rust
   subscriber.on_raw_account_update(&store, &account_pubkey, &account_data_bytes);
   ```
6. Start the vault cache GC loop (**required for production** — prevents unbounded memory growth):
   ```rust
   let gc_handle = subscriber.spawn_gc_task(2); // GC every 2 seconds
   // On shutdown: gc_handle.abort();
   ```

### Runtime Integration Call-Sites

#### Where to start `spawn_gc_task`

In the seer / gRPC runtime startup (e.g. `start_oracle_runtime_task` or `main.rs`), immediately
after constructing `subscriber` and `store`:

```rust
// Seer runtime startup — after subscriber + store are created
let gc_handle = subscriber.spawn_gc_task(2);

// In the shutdown handler or select! loop:
gc_handle.abort();
```

This **must** run before the gRPC stream starts delivering updates. Without it,
`vault_cache` grows unboundedly during pool churn / migration.

#### Where and when to call `unsubscribe_pool`

**SSOT invariant**: a snapshot must not outlive its subscription. Call `unsubscribe_pool`
at every point where the system stops observing a pool:

| Trigger | Call-site |
|---|---|
| Pool prune / timeout | Pool manager prune loop |
| Migration finished (bonding → AMM, cleanup old BC sub) | Migration handler |
| Manual stop / operator command | Admin API handler |
| Guardian / risk kill-switch | Guardian shutdown path |

```rust
// Always pass Some(&store) to enforce SSOT cleanup
subscriber.unsubscribe_pool(&pool_id, Some(&store));
```

If the runtime does not yet have a prune/timeout loop that removes pools, add a minimal
one. Example pattern:

```rust
// In seer tick loop / periodic task:
for pool_id in pools_to_prune {
    subscriber.unsubscribe_pool(&pool_id, Some(&snapshot_store));
    info!(pool_id = %pool_id, "pruned inactive pool");
}
```

### AMM Update Safety: Both-Sides-Fresh Guard

AMM reserve updates only fire when **both** vault sides (SOL + token) have a fresh
entry in `vault_cache` within the TTL window (default 3s). There is **no fallback**
to stale snapshot data — this prevents "mixed freshness" pricing where one side is
from a fresh update and the other from an old snapshot.

If only one vault updates, the amount is cached but no store update occurs until the
sibling vault also provides a fresh value.

## 2. Verifying Wiring is Working

### Diagnostic Counters

Access `subscriber.diagnostics` (type `SubscriberDiagnostics`) for real-time counts:

| Counter | Meaning |
|---|---|
| `curve_updates_applied` | Bonding curve updates successfully parsed and applied |
| `amm_vault_updates_applied` | AMM vault pairs fully resolved and applied |
| `ignored_updates` | Updates for unknown/unsubscribed pubkeys |
| `parse_fail_curve` | Bonding curve parse failures |
| `parse_fail_token` | SPL token account parse failures (too short) |
| `unsupported_token_layout` | Token-2022/extension accounts detected (> 165 bytes) |

### Log Signals

Look for these structured log messages:

- `"SSOT snapshot update"` — emitted on each snapshot change (throttled to 10 Hz per pool)
  - Fields: `pool_id`, `phase` ("BondingCurve" or "Amm"), `source`, `age_ms`, reserves, `price_mark`
- `"PHASE_SWITCH Bonding→Amm"` — emitted once per pool on migration
- `"SSOT Yellowstone: subscribed to bonding curve account"` — on subscribe
- `"SSOT Yellowstone: subscribed to AMM pool accounts"` — on AMM subscribe

### Health Check Commands

```bash
# Check if curve updates are flowing:
grep "curve_updates_applied" logs.jsonl | tail -5

# Check for AMM updates:
grep "amm_vault_updates_applied" logs.jsonl | tail -5

# Check snapshot update frequency:
grep "SSOT snapshot update" logs.jsonl | tail -20
```

## 3. Recognizing Missing AMM Data

### Token-2022 / Unsupported Layout

If `unsupported_token_layout` counter is rising, AMM vaults use Token-2022 extensions.
Log message: `"SSOT Yellowstone: unsupported token layout (Token-2022/extensions), skipping"`

**Impact**: AMM reserves will not update for that pool. Quote engine will return `None` for AMM quotes.

### Missing Vault Sibling

If only one vault side updates but the sibling never arrives:
- Log: `"partial AMM vault update cached, waiting for sibling"`
- Vault cache entries expire after TTL (default 3s)
- The pool snapshot won't transition to AMM until both vault amounts are known
- **No fallback** to stale snapshot data — both sides must be fresh

### TTL Expired

If `age_ms` in snapshot logs exceeds `stale_ms` threshold:
- `check_staleness()` will return `(true, age_ms)`
- Metric `snapshot_stale_total` increments
- Position manager should enter safety mode (no trade execution)

## 4. Confirming Position Manager Uses SSOT

### Quote Source Verification

The `SnapshotStore` provides:
```rust
// Unified SSOT price quote API
store.get_price_quote(&pool_id, trade_amount, QuoteSide::Sell) -> Option<Quote>

// Check current phase
store.phase(&pool_id) -> PoolPhase  // BondingCurve or Amm

// Check snapshot source
store.get(&pool_id).map(|s| s.source) -> Option<SnapshotSource>
```

### What to Look For

1. **Before migration**: `phase = "BondingCurve"`, `source = "Yellowstone"`, `v_sol`/`v_tokens` changing
2. **After migration**: `phase = "Amm"`, `source = "Yellowstone"`, `reserve_sol`/`reserve_token` changing
3. **Price not stale**: `age_ms` < `stale_ms` (default 1500ms)
4. **Prices changing**: `price_mark` values differ between consecutive log entries

### Red Flags

- `price_mark` is constant across multiple log entries → data feed is stale
- `source` shows `PumpPortal` after AMM migration → SSOT not wired (PumpPortal rejected post-AMM)
- `phase` stuck on `BondingCurve` after migration event → phase switch not triggered
- All quotes return `None` → no snapshot data or zero reserves
