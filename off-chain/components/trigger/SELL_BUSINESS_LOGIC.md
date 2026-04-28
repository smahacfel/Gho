# SELL Business Logic Implementation

This document describes the implementation of the SELL business logic for the Revolver module, as specified in Sub-Issue 2.

## Overview

The SELL business logic implements automatic take-profit (TP) and stop-loss (panic) execution based on price targets calculated from entry prices. The system:

1. Calculates target prices (TP1, TP2, panic) based on entry price and multipliers
2. Builds pre-signed SELL transactions with proper `min_sol_output` slippage protection
3. Monitors price feeds and automatically fires bullets when targets are reached
4. Sends transactions via UDP to TPU leaders (and optionally Jito bundles)

## Architecture

```
┌─────────────┐
│ BUY Execute │ → entry_price known
└──────┬──────┘
       │
       ▼
┌──────────────────────────┐
│ Calculate TP/Panic Targets│
│ - tp1_price = entry * 1.2│
│ - tp2_price = entry * 2.0│
│ - panic_price = entry*0.8│
└──────┬───────────────────┘
       │
       ▼
┌─────────────────────────┐
│ Build SELL Bullets       │
│ - Calculate min_sol_out  │
│ - Sign transactions      │
│ - Load into Revolver     │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ Price Feed Worker        │
│ - Poll price oracle      │
│ - Check bullet targets   │
│ - Fire via UDP/Jito      │
└─────────────────────────┘
```

## Components

### 1. Price Logic (`revolver_price_logic.rs`)

#### TpPanicConfig

Defines the multipliers for take-profit and stop-loss levels:

```rust
use trigger::{TpPanicConfig, PositionPriceTargets};

// Default configuration
let config = TpPanicConfig::default();
// TP1: 1.2x (20% profit)
// TP2: 2.0x (100% profit)
// Panic: 0.8x (20% loss)

// Conservative strategy
let config = TpPanicConfig::conservative();
// TP1: 1.1x, TP2: 1.5x, Panic: 0.9x

// Aggressive strategy
let config = TpPanicConfig::aggressive();
// TP1: 1.5x, TP2: 3.0x, Panic: 0.7x

// Custom configuration
let config = TpPanicConfig::new(1.3, 2.5, 0.85)?;
```

#### PositionPriceTargets

Calculates concrete target prices from entry price:

```rust
let entry_price = 1_000_000; // lamports per token
let config = TpPanicConfig::default();
let targets = PositionPriceTargets::new(entry_price, config);

println!("TP1: {}", targets.tp1_target_price);    // 1,200,000
println!("TP2: {}", targets.tp2_target_price);    // 2,000,000
println!("Panic: {}", targets.panic_target_price); // 800,000

// Check if price has reached targets
if targets.has_reached_tp1(current_price) {
    println!("TP1 reached!");
}

// Get P&L percentage
let pnl = targets.get_pnl_percentage(current_price);
println!("P&L: {:.2}%", pnl);
```

### 2. SELL Transaction Builder (`revolver_sell_builder.rs`)

#### SellTxBuilder

Builds pre-signed SELL transactions with slippage protection:

```rust
use trigger::{SellTxBuilder, SellTxConfig};
use solana_sdk::{signature::Keypair, hash::Hash, pubkey::Pubkey};

let payer = Keypair::new();
let config = SellTxConfig::default();
let builder = SellTxBuilder::new(payer, config);

// Build SELL transaction
let mint = Pubkey::from_str("...")?;
let amount_in = 2_500_000; // tokens to sell
let min_sol_output = 2_970_000_000; // lamports (with slippage)
let blockhash = rpc_client.get_latest_blockhash().await?;

let tx_bytes = builder
    .build_signed_sell_tx(mint, amount_in, min_sol_output, blockhash)
    .await?;
```

#### Calculating min_sol_output

The `calculate_min_sol_output` function applies slippage protection:

```rust
// Formula: min_sol_output = (token_amount * target_price) * (1 - slippage_margin)

let token_amount = 1_000_000; // tokens
let target_price = 1_200_000; // lamports per token (TP1)
let slippage_bps = 100; // 1% slippage

let min_output = SellTxBuilder::calculate_min_sol_output(
    token_amount,
    target_price,
    slippage_bps,
);
// Result: 1,188,000,000 lamports (1.2M * 1M * 0.99)
```

**Typical slippage values:**
- TP1/TP2: 100-150 bps (1-1.5%)
- Panic: 200-300 bps (2-3%) - allow more slippage to ensure execution

### 3. Price Feed Integration (`revolver_price_feed.rs`)

#### PriceFeedIntegration

Monitors price oracle and fires bullets when targets are reached:

```rust
use trigger::{PriceFeedIntegration, PriceOracleProvider, Revolver};
use tokio::net::UdpSocket;
use std::sync::Arc;

// Setup price oracle (implement PriceOracleProvider)
let price_oracle: Arc<dyn PriceOracleProvider> = ...;

// Setup UDP socket for TPU
let udp_socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
let leader_tpu_addr = "127.0.0.1:8001".parse()?;

// Create price feed
let price_feed = PriceFeedIntegration::new(
    price_oracle,
    udp_socket,
    leader_tpu_addr,
);

// Fire bullets manually at current price
let mut revolver = Revolver::new();
let mint = Pubkey::from_str("...")?;
let current_price = 1_300_000; // lamports

let fired_count = price_feed
    .try_fire_revolver_for_price(&mut revolver, mint, current_price)
    .await?;

println!("Fired {} bullets", fired_count);
```

#### Automatic Polling Worker

For continuous monitoring:

```rust
use tokio::sync::RwLock;

let revolver = Arc::new(RwLock::new(Revolver::new()));
let price_feed = Arc::new(PriceFeedIntegration::new(...));

// Start worker that polls every 5 seconds
let handle = price_feed.start_polling_worker(
    Arc::clone(&revolver),
    5, // poll interval in seconds
);

// Worker runs in background...

// Stop when done
handle.stop()?;
```

#### Using TpuClient for Redundancy

For higher reliability with N+3 redundancy:

```rust
use trigger::{PriceFeedWithTpuClient, TpuClient};

let tpu_client = Arc::new(TpuClient::new(rpc_url, Some(3))?);
let price_feed = PriceFeedWithTpuClient::new(price_oracle, tpu_client);

// This will send with redundancy for better inclusion rate
let fired_count = price_feed
    .try_fire_revolver_for_price(&mut revolver, mint, current_price)
    .await?;
```

## Complete Example: BUY to SELL Flow

```rust
use trigger::*;
use solana_sdk::{signature::Keypair, pubkey::Pubkey};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. BUY executed - we know entry price and position
    let mint = Pubkey::from_str("...")?;
    let entry_price = 1_000_000; // lamports per token
    let position_size = 10_000_000; // 10M tokens
    
    println!("BUY executed: {} tokens at {} lamports", position_size, entry_price);

    // 2. Calculate TP/panic targets
    let config = TpPanicConfig::default();
    let targets = PositionPriceTargets::new(entry_price, config);
    
    println!("TP1: {} (+20%)", targets.tp1_target_price);
    println!("TP2: {} (+100%)", targets.tp2_target_price);
    println!("Panic: {} (-20%)", targets.panic_target_price);

    // 3. Build SELL bullets
    let payer = Keypair::new();
    let rpc_client = RpcClient::new("https://api.devnet.solana.com".to_string());
    let blockhash = rpc_client.get_latest_blockhash().await?;
    
    let builder = SellTxBuilder::with_default_config(payer);
    
    // TP1 bullet: 25% of position
    let tp1_amount = position_size / 4;
    let tp1_min_output = SellTxBuilder::calculate_min_sol_output(
        tp1_amount,
        targets.tp1_target_price,
        100, // 1% slippage
    );
    let tp1_tx = builder
        .build_signed_sell_tx(mint, tp1_amount, tp1_min_output, blockhash)
        .await?;
    let tp1_bullet = Bullet::new(tp1_tx, targets.tp1_target_price, 2500)?;
    
    // TP2 bullet: 75% of position
    let tp2_amount = position_size * 3 / 4;
    let tp2_min_output = SellTxBuilder::calculate_min_sol_output(
        tp2_amount,
        targets.tp2_target_price,
        100,
    );
    let tp2_tx = builder
        .build_signed_sell_tx(mint, tp2_amount, tp2_min_output, blockhash)
        .await?;
    let tp2_bullet = Bullet::new(tp2_tx, targets.tp2_target_price, 7500)?;
    
    // 4. Load bullets into revolver
    let mut revolver = Revolver::new();
    revolver.load_magazine(mint, vec![tp1_bullet, tp2_bullet]);
    
    println!("Loaded {} bullets", revolver.total_bullet_count());

    // 5. Setup price feed and start monitoring
    let price_oracle: Arc<dyn PriceOracleProvider> = ...; // Your price oracle
    let tpu_client = Arc::new(TpuClient::new(rpc_url, Some(3))?);
    
    let price_feed = Arc::new(PriceFeedWithTpuClient::new(
        price_oracle,
        tpu_client,
    ));
    
    let revolver = Arc::new(RwLock::new(revolver));
    
    // Start polling worker
    let handle = tokio::spawn({
        let price_feed = Arc::clone(&price_feed);
        let revolver = Arc::clone(&revolver);
        async move {
            loop {
                let mut revolver_guard = revolver.write().await;
                let _ = price_feed.poll_and_fire_all(&mut revolver_guard).await;
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }
    });
    
    println!("Price feed monitoring started");
    
    // Worker will automatically fire bullets when prices are reached
    handle.await?;
    
    Ok(())
}
```

## Metrics

The implementation tracks the following metrics:

- `trigger_bullet_fired_total`: Total bullets fired successfully
- `trigger_bullet_failed_not_ready_total`: Bullets that failed (empty tx_bytes, etc.)

Access via `TriggerMetrics`:

```rust
use trigger::TriggerMetrics;
use prometheus::Registry;

let metrics = TriggerMetrics::new();
let registry = Registry::new();
metrics.register(&registry)?;

// Metrics are automatically updated by PriceFeedIntegration
```

## Key Design Decisions

### 1. Price Format
- All prices stored as `u64` lamports per token
- Consistent with Solana's native units
- Avoids floating-point precision issues

### 2. Slippage Protection
- `min_sol_output` calculated per bullet
- Formula: `expected_output * (1 - slippage_bps/10000)`
- Transaction fails on-chain if output < minimum
- Typical values: 1-2% for TP, 2-3% for panic

### 3. Bullet Firing Logic
- Current implementation uses `>=` for target price (suitable for TP)
- Panic/stop-loss would require custom logic with `<=` comparison
- This is documented in tests and ready for extension

### 4. No RPC in Hot Path
- All transactions pre-signed
- Only UDP send during firing
- Worker handles blockhash refresh asynchronously

### 5. UDP vs Jito
- Direct UDP: Fastest, ~1ms latency
- TpuClient: N+3 redundancy for ~92%+ inclusion
- Jito bundles: Optional for MEV protection

## Testing

Run the comprehensive test suite:

```bash
# All library tests
cargo test --lib

# Integration tests only
cargo test --test sell_logic_integration

# With output
cargo test --test sell_logic_integration -- --nocapture
```

Test scenarios covered:
1. TP/panic target calculation
2. min_sol_output calculation with various slippage values
3. SELL transaction building
4. Complete BUY → TP1 → TP2 flow
5. Panic/stop-loss detection
6. Conservative vs aggressive strategies

## Future Enhancements

1. **Stop-Loss Bullets**: Add support for `<=` comparison in `Bullet::should_fire()`
2. **Trailing Stop**: Dynamic panic target that follows price upward
3. **Partial TP**: More granular position scaling (e.g., 10% increments)
4. **Time-Based Exit**: Add timeout for position closure
5. **Jito Bundle Integration**: Automatic bundle submission for high-value trades
6. **Price Feed Redundancy**: Multiple oracle sources with consensus

## Security Considerations

1. **Slippage Limits**: Always set reasonable `min_sol_output` to prevent sandwich attacks
2. **Blockhash Expiry**: Worker refreshes signatures every 60 seconds
3. **Empty TX Validation**: Price feed skips bullets with empty `tx_bytes`
4. **Metrics Monitoring**: Track `bullet_failed_not_ready_total` for issues

## References

- Issue: Sub-Issue 2: Biznesowa logika SELL
- Related modules: `revolver.rs`, `price_oracle.rs`, `udp_client.rs`
- Tests: `tests/sell_logic_integration.rs`
