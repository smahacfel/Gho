# Jito Bundle Building Guide

This guide explains how to use the Jito bundle building functionality in the Trigger component to submit transactions with MEV protection and improved inclusion rates.

## Overview

The Jito bundle building system provides:

- **Dynamic Tip Calculation**: Automatically calculates optimal tips based on transaction value and priority
- **Configurable Redundancy**: Submit bundles multiple times (N+1, N+3, or N+5) for higher inclusion rates
- **Transaction Ordering**: Ensures InitializePool transaction executes before Ghost transactions
- **Nonce Staggering**: Optional feature to improve bundle inclusion safety
- **Comprehensive Diagnostics**: Detailed logging and analysis of each bundle submission

## Quick Start

### Basic Bundle Submission

```rust
use trigger::{BundleBuilder, BundleConfig, JitoClient};
use solana_sdk::hash::Hash;

// Create default configuration
let bundle_config = BundleConfig::default();

// Initialize Jito client
let jito_client = JitoClient::new("https://mainnet.block-engine.jito.wtf", bundle_config);
let bundle_builder = BundleBuilder::new(jito_client);

// Build and submit bundle
let (signature, diagnostics) = bundle_builder.build_and_submit_single(
    init_pool_tx,           // InitializePool transaction
    ghost_tx,               // Ghost transaction
    1_000_000_000,          // 1 SOL transaction value
    0.5,                    // Medium priority (50%)
    recent_blockhash,       // Recent blockhash
).await?;

println!("Bundle submitted: {}", signature);
println!("Tip: {} lamports ({:.2}%)", 
         diagnostics.tip_lamports, 
         diagnostics.tip_percent);
```

## Configuration

### Redundancy Policies

Choose how many times to submit each bundle:

```rust
use trigger::RedundancyPolicy;

// N+1: Submit 2 bundles (fastest, lowest inclusion rate)
let policy = RedundancyPolicy::NPlusOne;

// N+3: Submit 4 bundles (default, balanced)
let policy = RedundancyPolicy::NPlusThree;

// N+5: Submit 6 bundles (slowest, highest inclusion rate)
let policy = RedundancyPolicy::NPlusFive;
```

### Tip Configuration

Configure tip calculation parameters:

```rust
use trigger::TipConfig;

let tip_config = TipConfig::new(
    0.02,           // Base tip: 2% (used at priority 0.0)
    0.05,           // Dynamic tip: 5% (used at priority 1.0)
    0.05,           // Max tip: 5% (safety cap)
    10_000,         // Min tip: 0.00001 SOL
    100_000_000,    // Max tip: 0.1 SOL
);
```

**Tip Calculation Formula:**
```
tip_percent = base_tip + (dynamic_tip - base_tip) * priority
tip_percent = min(tip_percent, max_tip_percent)
tip_lamports = clamp(tx_value * tip_percent, min_tip, max_tip)
```

**Examples:**
- Priority 0.0 (base): 2% tip
- Priority 0.25: 2.75% tip
- Priority 0.5: 3.5% tip
- Priority 0.75: 4.25% tip
- Priority 1.0 (max): 5% tip

### Complete Bundle Configuration

```rust
use trigger::{BundleConfig, RedundancyPolicy, TipConfig};

let bundle_config = BundleConfig::new(
    RedundancyPolicy::NPlusThree,  // Submit 4 bundles
    TipConfig::default(),           // Use default tip config
    true,                           // Enable nonce staggering
    true,                           // Enable diagnostics logging
);
```

## Usage Patterns

### Single Ghost Transaction

For simple scenarios with one Ghost transaction:

```rust
let (signature, diagnostics) = bundle_builder.build_and_submit_single(
    init_pool_tx,
    ghost_tx,
    transaction_value,
    priority,
    recent_blockhash,
).await?;
```

### Multiple Ghost Transactions

For complex scenarios with multiple Ghost transactions:

```rust
let ghost_txs = vec![ghost_tx1, ghost_tx2, ghost_tx3];

let (signature, diagnostics) = bundle_builder.build_and_submit(
    init_pool_tx,
    ghost_txs,
    transaction_value,
    priority,
    recent_blockhash,
).await?;
```

### Using the Builder Pattern

For custom client configuration:

```rust
use trigger::JitoClientBuilder;

let jito_client = JitoClientBuilder::new()
    .with_endpoint("https://custom.jito.endpoint")
    .with_redundancy_policy(RedundancyPolicy::NPlusFive)
    .with_tip_config(custom_tip_config)
    .with_diagnostics(true)
    .build()?;

let bundle_builder = BundleBuilder::new(jito_client);
```

## Priority Levels

Choose appropriate priority based on market conditions:

### Low Priority (0.0 - 0.3)
- **Use Case**: Normal market conditions, non-urgent transactions
- **Tip Range**: 2.0% - 2.9%
- **Cost**: Lower
- **Inclusion Rate**: Moderate

### Medium Priority (0.3 - 0.7)
- **Use Case**: Competitive markets, time-sensitive transactions
- **Tip Range**: 2.9% - 4.1%
- **Cost**: Moderate
- **Inclusion Rate**: Good

### High Priority (0.7 - 1.0)
- **Use Case**: Highly competitive launches, critical timing
- **Tip Range**: 4.1% - 5.0%
- **Cost**: Higher
- **Inclusion Rate**: Best

## Diagnostics

The bundle diagnostics provide comprehensive information:

```rust
pub struct BundleDiagnostics {
    pub bundle_id: Signature,           // Bundle ID
    pub transaction_count: usize,       // Number of transactions
    pub tip_lamports: u64,              // Tip amount in lamports
    pub tip_percent: f64,               // Tip as percentage
    pub priority_factor: f64,           // Priority used (0.0-1.0)
    pub redundancy_count: usize,        // Number of submissions
    pub nonce_staggered: bool,          // Whether nonce was staggered
    pub explanation: String,            // Detailed explanation
}
```

Example diagnostic output:
```
=== Bundle Diagnostics ===
  Bundle ID: 5xJ8...k2L9
  Transaction Count: 3
  Tip: 35000000 lamports (3.50%)
  Priority Factor: 0.50
  Redundancy: N+3 (4 bundles)
  Nonce Staggered: true
  Explanation: Bundle 5xJ8...k2L9 contains 3 transaction(s). 
               Tip: 35000000 lamports (3.50% of 1000000000 lamports value).
               Priority factor: 0.50. Redundancy: N+3 (will submit 4 bundles).
               Nonce staggering: enabled. Tip range: 2.0%-5.0% (capped at 5.0%).
=========================
```

## Best Practices

### 1. Choose Appropriate Redundancy

- **N+1**: Use for low-value transactions or when speed is critical
- **N+3**: Default choice for most scenarios (good balance)
- **N+5**: Use for high-value or critical transactions

### 2. Set Priority Based on Conditions

```rust
let priority = if is_highly_competitive {
    0.8  // High priority
} else if is_moderately_competitive {
    0.5  // Medium priority
} else {
    0.2  // Low priority
};
```

### 3. Configure Tip Limits

Always set reasonable tip limits to prevent excessive costs:

```rust
let tip_config = TipConfig::new(
    0.02,           // 2% base
    0.05,           // 5% dynamic
    0.05,           // 5% max (important safety cap!)
    10_000,         // Min 0.00001 SOL
    100_000_000,    // Max 0.1 SOL (adjust based on risk tolerance)
);
```

### 4. Enable Diagnostics During Development

```rust
let bundle_config = BundleConfig::new(
    redundancy_policy,
    tip_config,
    true,  // Enable nonce staggering
    true,  // Enable diagnostics (useful for debugging)
);
```

Disable in production if logging overhead is a concern:

```rust
let bundle_config = BundleConfig::new(
    redundancy_policy,
    tip_config,
    true,   // Keep nonce staggering
    false,  // Disable diagnostics in production
);
```

### 5. Monitor Bundle Success Rates

Track bundle submission success:

```rust
let mut successful = 0;
let mut total = 0;

for swap in swaps {
    total += 1;
    match bundle_builder.build_and_submit(...).await {
        Ok(_) => successful += 1,
        Err(e) => eprintln!("Bundle failed: {}", e),
    }
}

let success_rate = (successful as f64 / total as f64) * 100.0;
println!("Success rate: {:.2}%", success_rate);
```

## Performance Considerations

### Transaction Size

- Each bundle contains: InitializePool TX + Ghost TX(s)
- Typical bundle size: 2-5 transactions
- Network overhead: ~500-1500 bytes per bundle

### Submission Timing

With redundancy enabled, submissions are staggered:
- N+3: ~40ms total submission time (10ms between each)
- N+5: ~60ms total submission time (10ms between each)

### Network Load

Consider network conditions when choosing redundancy:
- Low congestion: N+1 or N+3 sufficient
- High congestion: N+5 recommended for critical transactions

## Error Handling

Handle common errors appropriately:

```rust
use trigger::TriggerError;

match bundle_builder.build_and_submit(...).await {
    Ok((sig, diagnostics)) => {
        println!("Success: {}", sig);
        println!("Tip: {} lamports", diagnostics.tip_lamports);
    }
    Err(TriggerError::JitoBundleError(msg)) => {
        eprintln!("Bundle error: {}", msg);
        // Retry with different configuration or fallback to TPU
    }
    Err(e) => {
        eprintln!("Unexpected error: {}", e);
        // Handle other errors
    }
}
```

## Integration Example

Complete example integrating with existing transaction building:

```rust
use trigger::{
    BundleBuilder, BundleConfig, JitoClientBuilder,
    RedundancyPolicy, TipConfig,
    GhostTransactionBuilder, AmmType, AmmAccounts,
};
use ghost_core::SwapPlan;

async fn submit_ghost_bundle(
    swap_plan: SwapPlan,
    init_pool_tx: VersionedTransaction,
    priority: f64,
) -> Result<()> {
    // Configure Jito client
    let jito_client = JitoClientBuilder::new()
        .with_endpoint("https://mainnet.block-engine.jito.wtf")
        .with_redundancy_policy(RedundancyPolicy::NPlusThree)
        .with_diagnostics(true)
        .build()?;

    // Create bundle builder
    let bundle_builder = BundleBuilder::new(jito_client);

    // Build Ghost transaction
    let amm_accounts = AmmAccounts {
        pool: swap_plan.pool_amm_id,
        bonding_curve: Some(bonding_curve_pda),
        additional_accounts: vec![],
    };

    let tx_builder = GhostTransactionBuilder::new(
        swap_plan.clone(),
        AmmType::PumpFun,
        amm_accounts,
    );

    let ghost_tx = tx_builder.build_initialize_intent_tx(
        &payer,
        recent_blockhash,
    )?;

    // Submit bundle
    let (signature, diagnostics) = bundle_builder.build_and_submit_single(
        init_pool_tx,
        ghost_tx,
        swap_plan.amount_in,
        priority,
        recent_blockhash,
    ).await?;

    println!("Bundle {} submitted with tip {} lamports ({:.2}%)",
             signature,
             diagnostics.tip_lamports,
             diagnostics.tip_percent);

    Ok(())
}
```

## Testing

The implementation includes comprehensive tests. Run them with:

```bash
# Run all tests
cargo test

# Run only Jito integration tests
cargo test --test jito_bundle_integration

# Run with output
cargo test --test jito_bundle_integration -- --nocapture
```

## Future Enhancements

The current implementation is a foundation that can be extended with:

1. **Actual Jito gRPC Integration**: Full implementation of Jito bundle submission API
2. **Dynamic Priority Adjustment**: Automatically adjust priority based on network conditions
3. **Bundle Status Tracking**: Real-time monitoring of bundle landing status
4. **Advanced Tip Strategies**: Market-based tip calculation algorithms
5. **Bundle Simulation**: Pre-submission validation and gas estimation

## Support

For issues or questions:
- GitHub Issues: [Project Issues](https://github.com/Mezoscope/ProjectSolanaGhost/issues)
- Documentation: See `README.md` in the trigger component directory
