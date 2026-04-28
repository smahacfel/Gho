//! WEST Listener Component
//!
//! This component listens to PoolTransaction events from the event bus
//! and feeds them to the WalletEnergyTracker for quantum-inspired state tracking.

use crate::events::{EventBusReceiver, GhostEvent};
use anyhow::Result;
use ghost_brain::oracle::WalletEnergyTracker;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, error, info};

/// Run the WEST (Wallet Energy & State Tracker) Listener component
///
/// This component subscribes to the event bus and processes PoolTransaction events,
/// updating wallet states and maintaining the quantum-inspired state vector |ψ(t)⟩.
pub async fn run(
    west_tracker: Arc<WalletEnergyTracker>,
    mut shutdown_rx: broadcast::Receiver<()>,
    mut event_bus_rx: EventBusReceiver,
) -> Result<()> {
    info!("WestListener: Initializing Wallet Energy & State Tracker");
    info!("  Tracking wallet particles with 60s TTL");
    info!("  Maintaining quantum-inspired state vector |ψ(t)⟩");

    let mut events_processed = 0u64;
    let mut last_log = std::time::Instant::now();
    let mut last_stats_log = std::time::Instant::now();

    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                info!("WestListener: Shutdown signal received");
                info!("  Total events processed: {}", events_processed);

                // Final stats
                let stats = west_tracker.get_stats();
                info!(
                    "WestListener: Final state - wallets={}, tokens={}, total_energy={:.2}, free={:.2}, locked={:.2}",
                    stats.active_wallets,
                    stats.observed_tokens,
                    stats.total_energy,
                    stats.free_energy,
                    stats.locked_energy
                );
                break;
            }
            event = event_bus_rx.recv() => {
                match event {
                    Ok(ghost_event) => {
                        match ghost_event {
                            GhostEvent::PoolTransaction(pool_tx) => {
                                // Parse pubkeys
                                match (
                                    Pubkey::from_str(&pool_tx.pool_amm_id),
                                    Pubkey::from_str(&pool_tx.signer),
                                    pool_tx.token_mint.as_ref().and_then(|m| Pubkey::from_str(m).ok()),
                                ) {
                                    (Ok(pool_pubkey), Ok(wallet), token_mint_opt) => {
                                        // Use token_mint if available, otherwise fall back to pool_pubkey
                                        let token_mint = token_mint_opt.unwrap_or(pool_pubkey);

                                        // Process transaction in WEST tracker
                                        west_tracker.process_transaction(
                                            pool_pubkey,
                                            wallet,
                                            token_mint,
                                            pool_tx.is_buy,
                                            pool_tx.volume_sol,
                                            pool_tx.timestamp_ms,
                                        );

                                        events_processed += 1;

                                        debug!(
                                            "WestListener: Processed tx - wallet={}, action={}, volume={:.4} SOL",
                                            pool_tx.signer,
                                            if pool_tx.is_buy { "BUY" } else { "SELL" },
                                            pool_tx.volume_sol
                                        );

                                        // Log processing stats periodically (every 60 seconds)
                                        if last_log.elapsed().as_secs() >= 60 {
                                            let elapsed_secs = last_log.elapsed().as_secs().max(1);
                                            info!(
                                                "WestListener: Processing stats - events={}, rate={:.1}/min",
                                                events_processed,
                                                events_processed as f64 / (elapsed_secs as f64 / 60.0)
                                            );
                                            last_log = std::time::Instant::now();
                                        }

                                        // Log state statistics periodically (every 30 seconds)
                                        if last_stats_log.elapsed().as_secs() >= 30 {
                                            let stats = west_tracker.get_stats();
                                            let state = west_tracker.get_state_vector();

                                            info!(
                                                "WestListener: State Vector |ψ(t)⟩ - wallets={}, tokens={}, total_energy={:.2}",
                                                stats.active_wallets,
                                                stats.observed_tokens,
                                                stats.total_energy
                                            );

                                            info!(
                                                "  Energy Distribution - free={:.2} ({:.1}%), locked={:.2} ({:.1}%)",
                                                stats.free_energy,
                                                if stats.total_energy > 0.0 { (stats.free_energy / stats.total_energy) * 100.0 } else { 0.0 },
                                                stats.locked_energy,
                                                if stats.total_energy > 0.0 { (stats.locked_energy / stats.total_energy) * 100.0 } else { 0.0 }
                                            );

                                            // Log top 3 tokens by energy
                                            let mut token_energies: Vec<_> = state.token_energies.iter().collect();
                                            token_energies.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());

                                            for (i, (mint, energy)) in token_energies.iter().take(3).enumerate() {
                                                let prob = if stats.total_energy > 0.0 {
                                                    (*energy / stats.total_energy) * 100.0
                                                } else {
                                                    0.0
                                                };

                                                info!(
                                                    "  Top {} Token: {} - energy={:.2} ({:.1}%)",
                                                    i + 1,
                                                    mint,
                                                    energy,
                                                    prob
                                                );
                                            }

                                            last_stats_log = std::time::Instant::now();
                                        }
                                    }
                                    _ => {
                                        error!(
                                            "WestListener: Failed to parse pubkeys - pool={}, signer={}",
                                            pool_tx.pool_amm_id,
                                            pool_tx.signer
                                        );
                                    }
                                }
                            }
                            _ => {
                                // Ignore other event types
                            }
                        }
                    }
                    Err(e) => {
                        error!("WestListener: Error receiving event: {}", e);
                    }
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_west_listener_basic_integration() {
        // Note: This test verifies the basic setup of WEST listener.
        // The WalletEnergyTracker core functionality is thoroughly tested in ghost-e2e.
        // Integration testing of the async event handling is better done in E2E tests.

        let west_tracker = Arc::new(WalletEnergyTracker::default());

        // Manually test the tracker directly (simulating what the listener does)
        let pool_pubkey = solana_sdk::pubkey::Pubkey::new_unique();
        let wallet = solana_sdk::pubkey::Pubkey::new_unique();
        let token_mint = solana_sdk::pubkey::Pubkey::new_unique();

        west_tracker.process_transaction(
            pool_pubkey,
            wallet,
            token_mint,
            true,          // is_buy
            5.0,           // volume_sol
            1700000000000, // timestamp_ms
        );

        let stats = west_tracker.get_stats();
        assert_eq!(stats.active_wallets, 1);
        assert!(stats.total_energy > 0.0);
        assert!(stats.locked_energy > 0.0);
    }

    #[tokio::test]
    async fn test_west_listener_buy_sell_logic() {
        // Note: This test verifies the buy/sell state transitions in WEST tracker.
        // The core functionality is tested directly without async event bus complexity.

        let west_tracker = Arc::new(WalletEnergyTracker::default());

        let pool_pubkey = solana_sdk::pubkey::Pubkey::new_unique();
        let wallet = solana_sdk::pubkey::Pubkey::new_unique();
        let token_mint = solana_sdk::pubkey::Pubkey::new_unique();

        // Simulate buy transaction
        west_tracker.process_transaction(
            pool_pubkey,
            wallet,
            token_mint,
            true, // is_buy
            10.0, // volume_sol
            1700000000000,
        );

        let stats_after_buy = west_tracker.get_stats();
        assert_eq!(stats_after_buy.active_wallets, 1);
        assert!(stats_after_buy.locked_energy > 0.0);
        assert_eq!(stats_after_buy.free_energy, 0.0);

        // Simulate sell transaction
        west_tracker.process_transaction(
            pool_pubkey,
            wallet,
            token_mint,
            false, // is_sell
            12.0,  // volume_sol
            1700000001000,
        );

        let stats_after_sell = west_tracker.get_stats();
        assert_eq!(stats_after_sell.active_wallets, 1);
        assert!(stats_after_sell.free_energy > 0.0);
        assert_eq!(stats_after_sell.locked_energy, 0.0);
    }
}
