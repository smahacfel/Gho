# IRONCLAD TRANSACTION PROTOCOL

## Overview

The IRONCLAD Transaction Protocol is a strict enforcement mechanism designed to eliminate the risk of "Delayed Inclusion" (buying after a dump) and honeypot attacks. It enforces Time-To-Live (TTL) constraints and mandatory pre-flight simulation to ensure transactions are either immediate and certain, or rejected entirely.

## Problem Statement

In high-frequency trading on Solana, two critical risks exist:

1. **Delayed Inclusion**: Transaction gets included 10+ seconds after submission (after price dump), resulting in poor entry
2. **Honeypot Attacks**: Malicious tokens that accept transfers but prevent sells (99% tax, revert on transfer, etc.)

Standard Solana blockhash validity (~90 seconds) is far too long for a predator bot. A transaction must execute in milliseconds or not at all.

## Protocol Components

### PART A: TTL Guard (Time-To-Live Enforcement)

#### 1. Blockhash Freshness Check

**Method**: `get_fresh_blockhash()`

- Measures blockhash fetch latency from RPC
- **Timeout**: 200ms maximum
- **Action**: ABORT if fetch exceeds 200ms (blockhash is stale on arrival)

**Rationale**: If the blockhash fetch itself takes > 200ms, network conditions are poor and the blockhash may already be several slots old by the time we receive it.

```rust
// Example usage
let blockhash = jito_client.get_fresh_blockhash().await?;
// Returns Err(TriggerError::StaleBlockhash) if fetch > 200ms
```

#### 2. Bundle Confirmation Timeout

**Method**: `submit_bundle_ironclad()`

- Enforces strict 1500ms timeout for entire bundle submission process
- **Timeout**: 1500ms maximum
- **Action**: Mark as FAILED, no retries

**Rationale**: Jito bundles should land within 1-2 slots (~800-1600ms). If confirmation takes longer, market conditions have changed and the opportunity is gone.

```rust
// Example: Bundle submission with TTL enforcement
let result = jito_client.submit_bundle_ironclad(bundle).await;
// Returns Err(TriggerError::TtlViolation) if submission > 1500ms
```

#### 3. Strict Retry Policy

- **Transaction retries within bundle**: 0 (no retries)
- **Bundle submission retries**: 2 (network errors only)
- **Simulation failure retries**: 0 (immediate abort)

**Rationale**: Retrying failed transactions wastes time. In a fast-moving market, if a transaction fails, the opportunity has passed. Move on to the next opportunity.

### PART B: Simulation Pre-Flight

#### 1. Mandatory Simulation

**Method**: `simulate_transaction_preflight()`

Before any transaction is sent to Jito, it must pass simulation checks:

```rust
// Example: Pre-flight simulation
jito_client.simulate_transaction_preflight(&tx).await?;
// Returns Err(TriggerError::SimulationFailed) if validation fails
```

#### 2. Validation Checks

The simulation performs the following checks:

**a) Simulation Error Check**
```rust
if simulation.value.err.is_some() {
    return Err(TriggerError::SimulationFailed);
}
```

**b) Log Scanning for Errors**
- `"insufficient funds"` → ABORT (not enough lamports)
- `"Custom Error"` → ABORT (potential honeypot)
- Generic `"error"` pattern → ABORT (unknown issue)

**c) Compute Units Sanity Check**
```rust
if units_consumed > 400_000 {
    return Err(TriggerError::SimulationFailed);
    // Honeypots often have loops in token transfer logic
}
```

**d) Latency Limit**
- **Timeout**: 100ms maximum
- **Action**: SKIP transaction if simulation > 100ms

**Rationale**: If simulation takes > 100ms, the market is moving too fast or RPC is overloaded. Skip this opportunity.

## Configuration

### Setting up RPC Client

The IRONCLAD protocol requires an RPC client for simulation:

```rust
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;

// Create RPC client
let rpc_client = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));

// Configure Jito client with RPC for IRONCLAD
let jito_client = JitoClientBuilder::new()
    .with_endpoint("https://mainnet.block-engine.jito.wtf/api/v1")
    .with_rpc_client(rpc_client)  // Enable IRONCLAD simulation
    .with_bundle_config(bundle_config)
    .build()?;
```

### Using IRONCLAD Protocol

```rust
// 1. Get fresh blockhash with TTL enforcement
let blockhash = jito_client.get_fresh_blockhash().await?;
// Aborts if fetch > 200ms

// 2. Build transaction with fresh blockhash
let tx = builder.build_initialize_intent_tx(&payer, blockhash)?;

// 3. Build bundle
let bundle = jito_client.build_bundle(
    init_pool_tx,
    vec![tx],
    1_000_000_000,  // 1 SOL value
    0.5,            // Medium priority
    blockhash,
    Some(&tip_payer),
)?;

// 4. Submit with IRONCLAD protocol (simulation + TTL)
let signature = jito_client.submit_bundle_ironclad(bundle).await?;
// Simulates all transactions
// Enforces 1500ms TTL
// Aborts on any validation failure
```

## Constants

All IRONCLAD protocol constants are defined in `jito_client.rs`:

```rust
/// Maximum time allowed for blockhash fetch (200ms)
const MAX_BLOCKHASH_FETCH_MS: u128 = 200;

/// Maximum time to wait for bundle confirmation (1500ms)
const BUNDLE_CONFIRMATION_TIMEOUT_MS: u64 = 1500;

/// Maximum retries for bundle submission (2, network errors only)
const MAX_BUNDLE_RETRIES: usize = 2;

/// Maximum time allowed for transaction simulation (100ms)
const MAX_SIMULATION_TIME_MS: u128 = 100;

/// Maximum compute units for a simple swap (400k)
const MAX_COMPUTE_UNITS_SIMPLE_SWAP: u64 = 400_000;
```

## Error Handling

The IRONCLAD protocol introduces new error types:

```rust
pub enum TriggerError {
    /// Blockhash fetch took too long (> 200ms)
    StaleBlockhash(String),
    
    /// Bundle confirmation timeout exceeded (> 1500ms)
    TtlViolation(String),
    
    /// Transaction simulation failed validation
    SimulationFailed(String),
    
    /// Transaction aborted due to failed checks
    TransactionAborted(String),
}
```

### Example Error Handling

```rust
match jito_client.submit_bundle_ironclad(bundle).await {
    Ok(signature) => {
        info!("✅ Transaction landed: {}", signature);
    }
    Err(TriggerError::StaleBlockhash(msg)) => {
        warn!("⚠️ Blockhash too stale: {}", msg);
        // Retry with fresh blockhash
    }
    Err(TriggerError::SimulationFailed(msg)) => {
        error!("❌ Honeypot detected: {}", msg);
        // Blacklist token, move to next opportunity
    }
    Err(TriggerError::TtlViolation(msg)) => {
        warn!("⏱️ TTL exceeded: {}", msg);
        // Opportunity passed, move to next
    }
    Err(e) => {
        error!("❌ Unknown error: {}", e);
    }
}
```

## Trade-offs

### Benefits
- ✅ Zero risk of delayed inclusion (max 1500ms)
- ✅ Honeypot detection before transaction submission
- ✅ No wasted fees on failed transactions
- ✅ Predictable behavior under all network conditions

### Costs
- ⚠️ Slightly higher latency (~100-200ms for simulation)
- ⚠️ May skip legitimate opportunities in high-latency conditions
- ⚠️ Requires RPC client configuration

## Performance Considerations

### RPC Endpoint Selection

For optimal IRONCLAD performance:
- Use **dedicated RPC** (not public endpoints)
- Prefer **geographically close** RPC nodes
- Consider **self-hosted validator** for minimum latency

### Recommended Setup

```rust
// Priority: Low-latency dedicated RPC
let rpc_urls = vec![
    "https://your-dedicated-rpc.com",
    "https://backup-rpc.com",
];

// Use connection pooling for best performance
let rpc_client = Arc::new(RpcClient::new(rpc_urls[0].to_string()));
```

## Testing

Run IRONCLAD protocol tests:

```bash
# Run all jito_client tests (includes IRONCLAD tests)
cargo test -p trigger --lib jito_client::tests

# Run specific IRONCLAD tests
cargo test -p trigger --lib jito_client::tests::test_ironclad_constants
cargo test -p trigger --lib jito_client::tests::test_builder_with_rpc_client
```

## Monitoring

Log markers for IRONCLAD protocol:

- `🛡️ IRONCLAD:` - General protocol messages
- `✅ IRONCLAD SUCCESS:` - Successful submission within TTL
- `⚠️ IRONCLAD ABORT:` - Transaction aborted (simulation failure)
- `⚠️ IRONCLAD SKIP:` - Transaction skipped (timeout)
- `❌ IRONCLAD FAILED:` - Bundle submission failed

## Future Enhancements

Potential improvements to the protocol:

1. **Adaptive Timeouts**: Adjust TTL based on network congestion
2. **Parallel Simulation**: Simulate multiple transactions concurrently
3. **Simulation Caching**: Cache simulation results for identical transactions
4. **Custom Honeypot Detectors**: Plugin architecture for token-specific checks

## References

- [Solana Blockhash Lifetime](https://docs.solana.com/developing/programming-model/transactions#blockhash-format)
- [Jito Bundle Documentation](https://jito-labs.gitbook.io/mev)
- [Transaction Simulation](https://docs.solana.com/developing/clients/jsonrpc-api#simulatetransaction)
