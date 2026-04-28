# Trigger - Ghost Transaction Builder and Sender

The Trigger component is responsible for building minimal Ghost Transactions (~180B) using Address Lookup Tables (LUT), sending them with N+3 redundancy for high inclusion rate (target: ≥92%), and integrating with Jito Bundle for MEV extraction.

## Features

- **Minimal Transaction Building**: Constructs transactions using LUT compression to achieve ~180B target size
- **N+3 Redundancy**: Sends transactions multiple times to maximize inclusion rate
- **Jito Bundle Integration**: Full implementation with dynamic tip calculation and configurable redundancy
  - N+1 / N+3 / N+5 redundancy policies
  - Dynamic tip calculation (2-5% with safety caps)
  - Nonce staggering for inclusion safety
  - Comprehensive diagnostics logging
- **Prometheus Metrics**: Comprehensive metrics for monitoring performance
- **LUT Configuration**: Pre-configured addresses for Pump.fun and Bonk.fun AMM integrations

## Architecture

```
SwapPlan (from Oracle/Features)
    ↓
GhostTransactionBuilder (with LUT compression)
    ↓
TpuClient (N+3 redundancy) or JitoClient (bundles)
    ↓
Solana Network (TPU leaders)
```

## Components

### Transaction Builder (`transaction_builder.rs`)

Builds minimal Ghost Transactions with:
- `build_initialize_intent_tx()`: Creates intent registration transaction
- `build_full_swap_tx()`: Creates combined intent + execution transaction
- Validation of SwapPlan parameters
- LUT integration for address compression

### TPU Client (`udp_client.rs`)

Handles transaction sending with:
- N+3 redundancy (default: 3 additional sends)
- Leader slot synchronization
- Estimated inclusion rate: 92%+ with N+3
- Retry logic and confirmation polling

### Jito Client (`jito_client.rs`)

**Production-ready implementation for:**
- Bundle creation with InitializePool + Ghost transactions
- Dynamic tip calculation (2-5% with configurable parameters)
- Configurable redundancy policies (N+1, N+3, N+5)
- Nonce staggering for inclusion safety
- Comprehensive diagnostics and logging

See [JITO_BUNDLE_GUIDE.md](./JITO_BUNDLE_GUIDE.md) for detailed usage instructions.

### Bundle Builder (`bundle_builder.rs`)

High-level API for bundle building:
- `build_and_submit()`: Build and submit bundles with multiple Ghost TXs
- `build_and_submit_single()`: Convenience method for single Ghost TX
- Integrated diagnostics and logging
- Automatic tip calculation based on priority

### LUT Configuration (`config.rs`)

Pre-configured addresses for:

#### Pump.fun
- Program ID: `6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P`
- Fee Recipient: `CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM`
- Global Config: (placeholder address, may be updated)

#### Bonk.fun
- Program ID: `LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj`
- Fee Recipient: `C8Qf4o5ZwJbSz7Y6srR4gvfXx4Z4qyhW5AsYLSRQA8nc`
- Global Config: `FfYek5vEz23cMkWsdJwG2oa6EphsvXSHrGpdALN4g6W1`

#### Common Mints
- SOL: `So11111111111111111111111111111111111111112`
- USDC: `EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v`
- BONK: `DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263`

#### System Programs
- Token Program: `TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA`
- Associated Token: `ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL`
- System Program: `11111111111111111111111111111111`
- Rent Sysvar: `SysvarRent111111111111111111111111111111111`

### Metrics (`metrics.rs`)

Prometheus metrics exposed:
- `trigger_transactions_sent_total`: Total transactions sent
- `trigger_transactions_confirmed_total`: Total confirmed
- `trigger_transactions_failed_total`: Total failed
- `trigger_inclusion_rate`: Current inclusion rate (0.0-1.0)
- `trigger_send_latency_ms`: Send latency histogram
- `trigger_confirmation_latency_ms`: Confirmation latency histogram
- `trigger_pending_transactions`: Currently pending count
- `trigger_bytes_sent_total`: Total bytes sent
- `trigger_redundancy_sends_total`: Total redundant sends
- `trigger_jito_bundles_submitted_total`: Jito bundles submitted
- `trigger_jito_bundles_successful_total`: Successful Jito bundles

## Usage

### Jito Bundle Submission (Recommended)

```rust
use trigger::{
    BundleBuilder, BundleConfig, JitoClientBuilder,
    RedundancyPolicy, TipConfig,
};

// Configure Jito client
let jito_client = JitoClientBuilder::new()
    .with_endpoint("https://mainnet.block-engine.jito.wtf")
    .with_redundancy_policy(RedundancyPolicy::NPlusThree)
    .with_tip_config(TipConfig::default())
    .with_diagnostics(true)
    .build()?;

// Create bundle builder
let bundle_builder = BundleBuilder::new(jito_client);

// Build and submit bundle
let (signature, diagnostics) = bundle_builder.build_and_submit_single(
    init_pool_tx,           // InitializePool transaction
    ghost_tx,               // Ghost transaction
    1_000_000_000,          // 1 SOL transaction value
    0.5,                    // Medium priority (50%)
    recent_blockhash,
).await?;

println!("Bundle {}: tip {} lamports ({:.2}%)",
         signature,
         diagnostics.tip_lamports,
         diagnostics.tip_percent);
```

See [JITO_BUNDLE_GUIDE.md](./JITO_BUNDLE_GUIDE.md) for comprehensive documentation.

### Traditional TPU Sending (Fallback)

```rust
use trigger::{
    GhostTransactionBuilder, TpuClient, AmmAccounts, AmmType, LutConfig
};
use ghost_core::SwapPlan;

// Build transaction
let amm_accounts = AmmAccounts {
    pool: pool_pubkey,
    bonding_curve: Some(bonding_curve_pda),
    additional_accounts: vec![],
};

let builder = GhostTransactionBuilder::new(
    swap_plan,
    AmmType::PumpFun,
    amm_accounts,
);

let tx = builder.build_initialize_intent_tx(&payer, recent_blockhash)?;

// Send with N+3 redundancy
let tpu_client = TpuClient::new(
    "https://api.devnet.solana.com".to_string(),
    Some(3) // N+3 redundancy
)?;

let signature = tpu_client.send_transaction_with_redundancy(&tx).await?;
```

### As a Service

```bash
cargo run --bin trigger
```

The service will:
1. Initialize TPU and (optionally) Jito clients
2. Start Prometheus metrics server on port 9091
3. Listen for SwapPlan messages (placeholder loop currently)
4. Build and send transactions with configured redundancy
5. Track confirmations and update metrics

## Configuration

Default configuration (can be customized in `main.rs`):

```rust
TriggerConfig {
    rpc_url: "https://api.devnet.solana.com",
    use_jito: false,
    jito_endpoint: None,
    redundancy_count: 3,  // N+3
    metrics_port: 9091,
}
```

## Testing

Run all tests:

```bash
cargo test
```

Run specific test module:

```bash
cargo test transaction_builder
cargo test config
cargo test metrics
```

## Performance Targets

- **Transaction Size**: ~180 bytes (via LUT compression)
- **Inclusion Rate**: ≥92% (with N+3 redundancy)
- **Send Latency**: <50ms (target)
- **Confirmation Latency**: <2s (typical on Solana)

## Future Enhancements

1. **Full Jito Integration**: Complete implementation of bundle submission
2. **Dynamic LUT**: Support for pool-specific addresses
3. **Advanced Redundancy**: Adaptive redundancy based on network conditions
4. **Websocket Integration**: Real-time connection to Oracle/Features for SwapPlan messages
5. **Priority Fees**: Dynamic priority fee calculation based on network congestion
6. **Multi-RPC**: Support for multiple RPC endpoints with failover

## Dependencies

- `solana-sdk`: Solana blockchain SDK
- `solana-client`: RPC and TPU client functionality
- `tokio`: Async runtime
- `prometheus`: Metrics collection
- `ghost-core`: SwapPlan types
- `direct_buy-client`: DirectBuyBuilder instruction building (planned integration)

## Related Components

- **Seer**: Detects InitializePool events and generates candidates
- **Oracle/Features**: Validates candidates and creates SwapPlans
- **DirectBuyBuilder**: On-chain program for swap intent management

## License

See project root for license information.
