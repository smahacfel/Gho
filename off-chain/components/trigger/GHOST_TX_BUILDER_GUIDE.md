# Ghost Transaction Builder - Usage Guide

## Overview

The Ghost Transaction Builder creates minimal, optimized transactions (~180 bytes with LUT) for DirectBuyBuilder protocol. It supports:

- **Pre-signing**: Prepare transactions ahead of time for ultra-fast submission
- **LUT Optimization**: Use Address Lookup Tables to minimize transaction size
- **Comprehensive Validation**: All inputs are validated before transaction creation
- **Multi-AMM Support**: Works with both Pump.fun and Bonk.fun

## Quick Start

### Basic Transaction Building

```rust
use trigger::{AmmAccounts, AmmType, GhostTransactionBuilder, LutConfig};
use ghost_core::SwapPlanBuilder;
use solana_sdk::{hash::Hash, signature::Keypair, signer::Signer};

// Setup
let payer = Keypair::new();
let config = LutConfig::new();

// Create swap plan
let swap_plan = SwapPlanBuilder::new(payer.pubkey(), config.pump_fun.program_id)
    .amount_in(1_000_000)      // 0.001 SOL
    .min_amount_out(900_000)   // 10% slippage tolerance
    .timeout_seconds(300)       // 5 minutes
    .with_score(85)            // Oracle score
    .with_strategy("snipe")    // Strategy name
    .build()?;

// Configure AMM accounts
let amm_accounts = AmmAccounts {
    pool: pool_pubkey,
    bonding_curve: Some(curve_pubkey),
    additional_accounts: vec![],
};

// Build transaction
let builder = GhostTransactionBuilder::new(
    swap_plan,
    AmmType::PumpFun,
    amm_accounts,
);

let blockhash = rpc_client.get_latest_blockhash()?;
let transaction = builder.build_initialize_intent_tx(&payer, blockhash)?;

// Send transaction
let signature = rpc_client.send_and_confirm_transaction(&transaction)?;
```

### Pre-signing for Ultra-Fast Execution

The key feature for the Ghost system is pre-signing transactions before the InitializePool event is detected:

```rust
// Pre-sign transaction ahead of time (before InitializePool event)
let blockhash = rpc_client.get_latest_blockhash()?;
let presigned = builder.presign_initialize_intent_tx(&payer, blockhash)?;

println!("Transaction pre-signed and ready!");
println!("Size: {} bytes", presigned.size_bytes);
println!("Valid for: 60 seconds");

// Later, when InitializePool event is detected...
// Check if still valid
let now = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_secs() as i64;

if presigned.is_valid(now) {
    // Submit immediately without additional signing
    let signature = rpc_client.send_transaction(&presigned.transaction)?;
    println!("Submitted: {}", signature);
} else {
    // Re-sign with fresh blockhash
    let new_blockhash = rpc_client.get_latest_blockhash()?;
    let presigned = builder.presign_initialize_intent_tx(&payer, new_blockhash)?;
    // ... submit
}
```

### Full Swap Transaction (Initialize + Execute)

For atomic execution:

```rust
let full_tx = builder.build_full_swap_tx(&payer, blockhash)?;
let signature = rpc_client.send_and_confirm_transaction(&full_tx)?;
```

## Transaction Size Optimization

### Without LUT
- Transaction size: ~300 bytes
- All addresses embedded in transaction

### With LUT (Recommended for Production)
- Transaction size: ~180 bytes target
- Common addresses loaded from lookup table

To use actual LUT:

```rust
use solana_sdk::address_lookup_table::AddressLookupTableAccount;

// Load LUT account from chain
let lut_account = AddressLookupTableAccount {
    key: lut_pubkey,
    addresses: builder.get_lut_addresses().to_vec(),
};

// Build with LUT
let transaction = builder.build_initialize_intent_tx_with_lut(
    &payer,
    blockhash,
    lut_account,
)?;

// Transaction is now ~180 bytes
println!("Optimized size: {} bytes", bincode::serialize(&transaction)?.len());
```

## Validation

The builder validates all inputs before creating transactions:

### Amount Validation
```rust
// ✓ Valid
SwapPlanBuilder::new(authority, pool)
    .amount_in(1_000_000)    // >= 1000 lamports
    .min_amount_out(900_000) // > 0
    .build()?;

// ✗ Invalid - amount too small
SwapPlanBuilder::new(authority, pool)
    .amount_in(500)          // < 1000 lamports
    .min_amount_out(450)
    .build()?; // Returns validation error
```

### Pool ID Validation
```rust
let config = LutConfig::new();

// ✓ Valid - whitelisted pool
SwapPlanBuilder::new(authority, config.pump_fun.program_id)
    .amount_in(1_000_000)
    .min_amount_out(900_000)
    .timeout_seconds(300)
    .build()?;

// ✗ Invalid - not whitelisted
SwapPlanBuilder::new(authority, Pubkey::new_unique())
    .amount_in(1_000_000)
    .min_amount_out(900_000)
    .timeout_seconds(300)
    .build()?; // Returns validation error
```

### Timeout Validation
```rust
// ✓ Valid - future timeout
SwapPlanBuilder::new(authority, pool)
    .amount_in(1_000_000)
    .min_amount_out(900_000)
    .timeout_seconds(300)    // 5 minutes from now
    .build()?;

// ✗ Invalid - timeout too far in future
SwapPlanBuilder::new(authority, pool)
    .amount_in(1_000_000)
    .min_amount_out(900_000)
    .timeout_seconds(7 * 24 * 60 * 60 + 1) // > 7 days
    .build()?; // Returns validation error
```

## AMM Configuration

### Pump.fun
```rust
let config = LutConfig::new();

let swap_plan = SwapPlanBuilder::new(payer.pubkey(), config.pump_fun.program_id)
    .amount_in(1_000_000)
    .min_amount_out(900_000)
    .timeout_seconds(300)
    .build()?;

let builder = GhostTransactionBuilder::new(
    swap_plan,
    AmmType::PumpFun,
    amm_accounts,
);
```

### Bonk.fun
```rust
let config = LutConfig::new();

let swap_plan = SwapPlanBuilder::new(payer.pubkey(), config.bonk_fun.program_id)
    .amount_in(1_000_000)
    .min_amount_out(900_000)
    .timeout_seconds(300)
    .build()?;

let builder = GhostTransactionBuilder::new(
    swap_plan,
    AmmType::BonkFun,
    amm_accounts,
);
```

## LUT Address Management

Get addresses that should be in the LUT:

```rust
let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

// Get all LUT addresses for this AMM
let lut_addresses = builder.get_lut_addresses();
println!("LUT contains {} addresses", lut_addresses.len());

// Get AMM-specific addresses
let amm_addresses = builder.get_amm_addresses();
println!("Program ID: {}", amm_addresses.program_id);
println!("Fee Recipient: {}", amm_addresses.fee_recipient);
println!("Global Config: {}", amm_addresses.global_config);
```

## Error Handling

```rust
use trigger::TriggerError;

match builder.build_initialize_intent_tx(&payer, blockhash) {
    Ok(tx) => {
        // Transaction built successfully
        println!("Transaction ready: {} bytes", bincode::serialize(&tx)?.len());
    }
    Err(TriggerError::InvalidSwapPlan(msg)) => {
        // Validation error
        eprintln!("Invalid swap plan: {}", msg);
    }
    Err(TriggerError::TransactionBuildFailed(msg)) => {
        // Transaction construction error
        eprintln!("Failed to build transaction: {}", msg);
    }
    Err(e) => {
        // Other error
        eprintln!("Error: {}", e);
    }
}
```

## Best Practices

### 1. Pre-sign Early
Pre-sign transactions as soon as you have a valid swap plan and recent blockhash:

```rust
// T-60s: Pre-sign
let presigned = builder.presign_initialize_intent_tx(&payer, blockhash)?;

// T=0: InitializePool detected
// Submit immediately
rpc_client.send_transaction(&presigned.transaction)?;
```

### 2. Refresh Blockhash
Blockhashes are valid for ~60 seconds. Refresh if needed:

```rust
const REFRESH_INTERVAL: i64 = 50; // Refresh after 50 seconds

if !presigned.is_valid(now) || (now - presigned.signed_at) > REFRESH_INTERVAL {
    let new_blockhash = rpc_client.get_latest_blockhash()?;
    presigned = builder.presign_initialize_intent_tx(&payer, new_blockhash)?;
}
```

### 3. Use LUT in Production
Create and use LUT for minimal transaction size:

```rust
// Create LUT once (one-time setup)
let lut_pubkey = create_lookup_table(&payer, &rpc_client, builder.get_lut_addresses())?;

// Use LUT for all transactions
let lut_account = load_lookup_table(&rpc_client, lut_pubkey)?;
let tx = builder.build_initialize_intent_tx_with_lut(&payer, blockhash, lut_account)?;
```

### 4. Validate Before Building
Always use SwapPlanBuilder to ensure valid parameters:

```rust
let swap_plan = SwapPlanBuilder::new(authority, pool)
    .amount_in(amount)
    .min_amount_out(min_out)
    .timeout_seconds(timeout)
    .build()?; // Validates here

let builder = GhostTransactionBuilder::new(swap_plan, amm_type, accounts);
// Builder will also validate, but catching errors early is better
```

## Testing

Run all tests:

```bash
# Unit tests
cargo test --package trigger --lib transaction_builder

# Integration tests
cargo test --package trigger --test ghost_tx_integration

# All tests
cargo test --package trigger
```

## Architecture

```
SwapPlan (from Oracle/Features)
    ↓
GhostTransactionBuilder
    ↓ (validates)
build_initialize_intent_tx / presign_initialize_intent_tx
    ↓ (uses direct_buy-client)
VersionedTransaction (with LUT support)
    ↓
Submit to Solana network
```

## See Also

- [Transaction Builder Source](src/transaction_builder.rs)
- [Integration Tests](tests/ghost_tx_integration.rs)
- [DirectBuyBuilder Client](../../../direct_buy-client/)
- [LUT Configuration](src/config.rs)
