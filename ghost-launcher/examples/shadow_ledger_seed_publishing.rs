//! Shadow Ledger Synthetic Seed Publishing Example
//!
//! This example demonstrates how to integrate Shadow Ledger synthetic seed
//! generation with the EventBus for immediate downstream processing.
//!
//! ## Flow:
//!
//! 1. Detect new pool via Seer
//! 2. Insert curve into Shadow Ledger
//! 3. Generate synthetic seed transactions
//! 4. Publish to EventBus as GeyserEvent::Transaction with synthetic=true
//! 5. Downstream components (Oracle, Trigger) receive synthetic transactions
//!
//! ## Usage:
//!
//! ```bash
//! cargo run --example shadow_ledger_seed_publishing
//! ```

use ghost_core::market_state::BondingCurve;
use ghost_core::shadow_ledger::{BootstrapMetrics, ShadowLedger, SyntheticTransaction};
use ghost_launcher::events::{create_event_bus, GhostEvent};
use seer::types::RawBytesMissingReason;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::collections::HashMap;
use std::sync::Arc;

// Constants for synthetic transaction identification
const SYNTHETIC_SIGNER: &str = "ShadowLedgerSynthetic";
const SYNTHETIC_PAYLOAD_SIZE: usize = 8;
const SYNTHETIC_SIGNATURE_FORMAT: &str = "synthetic_{}_{}_{}"; // pool_mint, timestamp_ms, index

/// Simulate pool detection and synthetic seed publishing
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("Starting Shadow Ledger Synthetic Seed Publishing Example");

    // Create EventBus
    let (event_tx, mut event_rx) = create_event_bus();

    // Create Shadow Ledger and Metrics
    let ledger = Arc::new(ShadowLedger::new());
    let metrics = Arc::new(BootstrapMetrics::new());

    // Simulate pool detection
    let pool_mint = Pubkey::new_unique();
    let current_slot = 12345678u64;

    tracing::info!(
        pool = %pool_mint,
        slot = current_slot,
        "Detected new pool"
    );

    // Create initial bonding curve state (as received from Geyser/RPC)
    let initial_curve = BondingCurve {
        discriminator: 0x6966e3f87bc25c31,
        virtual_token_reserves: 1_073_000_000_000, // 1.073T tokens
        virtual_sol_reserves: 30_000_000_000,      // 30 SOL
        real_token_reserves: 793_100_000_000,      // 793.1B tokens
        real_sol_reserves: 79_820_000,             // 0.07982 SOL
        token_total_supply: 1_073_000_000_000,
        complete: 0,
        _padding: [0; 7],
    };

    // Insert into Shadow Ledger
    ledger.insert_with_slot(pool_mint, initial_curve, current_slot);
    tracing::info!(
        pool = %pool_mint,
        "Inserted bonding curve into Shadow Ledger"
    );

    // Generate synthetic seed with 100ms watchdog timeout
    let synthetic_txs = match ledger.bootstrap_with_seed_generation_watchdog(
        pool_mint,
        Some(current_slot),
        10_000_000, // 0.01 SOL for simulation
        100,        // 100ms timeout
        Some(&metrics),
    ) {
        Ok(txs) => {
            tracing::info!(
                pool = %pool_mint,
                synthetic_txs = txs.len(),
                "Successfully generated synthetic seed"
            );
            txs
        }
        Err(e) => {
            tracing::error!(
                pool = %pool_mint,
                error = ?e,
                "Failed to generate synthetic seed"
            );
            return Err(e.into());
        }
    };

    // Publish synthetic transactions to EventBus
    let published_count = publish_synthetic_transactions_to_eventbus(
        &event_tx,
        pool_mint,
        current_slot,
        &synthetic_txs,
    )?;

    tracing::info!(
        pool = %pool_mint,
        count = published_count,
        "SL_PUBLISHED_GEYSEVENT"
    );

    // Simulate receiving events (in production, this would be in Oracle/Trigger components)
    tokio::spawn(async move {
        let mut received = 0;
        while let Ok(event) = event_rx.recv().await {
            match event {
                GhostEvent::PoolTransaction(tx_arc) => {
                    // Synthetic transactions can be identified by:
                    // 1. Signer name matches SYNTHETIC_SIGNER
                    // 2. mpcf_payload contains the 8-byte payload
                    let is_synthetic = tx_arc.signer == SYNTHETIC_SIGNER
                        && tx_arc.mpcf_payload.len() == SYNTHETIC_PAYLOAD_SIZE;

                    tracing::info!(
                        pool = %tx_arc.pool_amm_id,
                        signature = %tx_arc.signature,
                        slot = tx_arc.slot,
                        timestamp_ms = tx_arc.timestamp_ms,
                        is_synthetic = is_synthetic,
                        "Received synthetic transaction event"
                    );
                    received += 1;
                    if received >= published_count {
                        break;
                    }
                }
                _ => {}
            }
        }
        tracing::info!("Received all {} synthetic events", received);
    });

    // Wait a bit for async processing
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    // Display metrics
    tracing::info!(
        "=== Metrics Summary ===\n\
         seeds_generated_total: {}\n\
         seed_generation_failure_total: {}\n\
         bootstrap_success: {}\n\
         bootstrap_fail: {}",
        metrics.seeds_generated_total(),
        metrics.seed_generation_failure_total(),
        metrics.success_count(),
        metrics.fail_count(),
    );

    tracing::info!("Example completed successfully");
    Ok(())
}

/// Publish synthetic transactions to EventBus
///
/// This function converts SyntheticTransaction instances into GhostEvent::PoolTransaction
/// events and publishes them to the EventBus for downstream processing.
///
/// # Arguments
///
/// * `event_tx` - EventBus sender
/// * `pool_mint` - Pool/mint Pubkey
/// * `slot` - Current slot number
/// * `synthetic_txs` - Vector of synthetic transactions
///
/// # Returns
///
/// Number of transactions successfully published
fn publish_synthetic_transactions_to_eventbus(
    event_tx: &ghost_launcher::events::EventBusSender,
    pool_mint: Pubkey,
    slot: u64,
    synthetic_txs: &[SyntheticTransaction],
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut published = 0;

    for (idx, tx) in synthetic_txs.iter().enumerate() {
        // Create a synthetic PoolTransaction event
        let pool_tx = ghost_launcher::events::PoolTransaction {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_mint.to_string(),
            slot: Some(slot),
            event_ordinal: Some(idx as u32),
            tx_index: None,
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: tx.timestamp_ms,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: tx.timestamp_ms,
            signer: SYNTHETIC_SIGNER.to_string(),
            owner_token_deltas: vec![],
            is_buy: true,     // Synthetic transactions are treated as buys
            volume_sol: 0.01, // Minimal synthetic volume
            sol_amount_lamports: Some((0.01 * 1_000_000_000.0) as u64),
            token_amount_units: Some(10_000),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            // Generate unique signature using pool_mint, timestamp, and index
            signature: format!("synthetic_{}_{}_{}", pool_mint, tx.timestamp_ms, idx),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            mpcf_payload: tx.payload.to_vec(), // Use payload as marker bytes
            mpcf_payload_missing_reason: RawBytesMissingReason::NotMissing,
            token_mint: Some(pool_mint.to_string()),
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            bonding_curve_v2: None,
            bonding_curve_v2_provenance: None,
            buy_remaining_accounts: Vec::new(),
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: seer::types::ToolchainFingerprintInput::default(),
            curve_data_known: false,
            curve_finality: ghost_core::CurveFinality::Speculative,
        };

        // Publish to EventBus
        event_tx.send(GhostEvent::pool_transaction(pool_tx))?;
        published += 1;
    }

    Ok(published)
}
