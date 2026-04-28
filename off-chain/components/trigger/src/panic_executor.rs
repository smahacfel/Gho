//! Panic Executor - Isolated Emergency Sell Path
//!
//! This module implements the "Isolated Panic Path" (UDP/Leapfrog) - a dedicated
//! emergency channel that bypasses all standard queues, mutexes, and RPC infrastructure.
//!
//! ## Design Philosophy
//!
//! **FAIL-SAFE DOCTRINE**: When critical signals are detected (liquidity trap, survival
//! probability collapse, HFT manipulation, cabal distribution), the bot MUST execute
//! a sell order and terminate immediately. No retries, no waiting, no recovery.
//!
//! ## Architecture
//!
//! - **Direct UDP Socket**: Pre-allocated socket bypassing RPC infrastructure
//! - **Fire-and-Forget**: Zero waiting for confirmation, zero retries
//! - **Leapfrog TPU**: Sends directly to validator TPU (Transaction Processing Unit)
//! - **Dead-Man Switch**: Process termination after panic sell (prevents re-entry)
//!
//! ## Usage
//!
//! ```ignore
//! let executor = PanicExecutor::new(rpc_url, payer).await?;
//!
//! // When panic signal detected:
//! executor.execute_hard_kill(
//!     token_mint,
//!     token_amount,
//!     KillReason::LigmaVeto,
//! ).await;
//! // Bot process terminates immediately after this call
//! ```

use anyhow::{Context, Result};
use solana_sdk::{
    pubkey::Pubkey,
    signer::{keypair::Keypair, Signer},
    transaction::Transaction,
};
use std::net::UdpSocket;
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info};

use crate::direct_sell_builder::DirectSellBuilder;
use crate::leader_resolver::LeaderResolver;
use crate::udp_client::TpuClient;

/// Reason for triggering hard kill (panic sell + process termination)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillReason {
    /// LIGMA detected liquidity trap or PSI imbalance
    LigmaVeto,
    /// QEDD detected survival probability < 0.5
    QeddSurvival,
    /// PARADOX detected HFT manipulation
    ParadoxAnomaly,
    /// CLUSTER detected cabal distribution
    ClusterCabal,
}

impl KillReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            KillReason::LigmaVeto => "LIGMA_VETO",
            KillReason::QeddSurvival => "QEDD_SURVIVAL",
            KillReason::ParadoxAnomaly => "PARADOX_ANOMALY",
            KillReason::ClusterCabal => "CLUSTER_CABAL",
        }
    }
}

/// Panic Executor - Isolated emergency sell infrastructure
///
/// This executor maintains a dedicated UDP socket and TPU client
/// that operates independently from the main RPC infrastructure.
pub struct PanicExecutor {
    /// Dedicated UDP socket (pre-allocated at startup)
    _udp_socket: Arc<UdpSocket>,

    /// TPU client for direct validator communication
    tpu_client: Arc<TpuClient>,

    /// Payer keypair for signing transactions
    payer: Arc<Keypair>,

    /// RPC client for minimal blockhash retrieval (pre-allocated)
    rpc_client: Arc<solana_client::nonblocking::rpc_client::RpcClient>,

    /// Leader resolver for finding TPU endpoints
    leader_resolver: Arc<LeaderResolver>,
}

impl PanicExecutor {
    /// Create a new panic executor with pre-allocated resources
    ///
    /// This function allocates all resources needed for emergency sells
    /// during initialization, so that panic execution has zero allocation overhead.
    pub async fn new(
        rpc_url: String,
        payer: Arc<Keypair>,
        leader_resolver: Arc<LeaderResolver>,
    ) -> Result<Self> {
        info!("Initializing Panic Executor (Isolated Emergency Path)");

        // Pre-allocate UDP socket (bind to any available port)
        let udp_socket =
            UdpSocket::bind("0.0.0.0:0").context("Failed to bind UDP socket for panic executor")?;

        // Set socket to non-blocking mode for fire-and-forget
        udp_socket
            .set_nonblocking(true)
            .context("Failed to set UDP socket to non-blocking")?;

        info!(
            "Panic Executor UDP socket bound to: {:?}",
            udp_socket.local_addr()
        );

        // Create TPU client with minimal redundancy (speed over reliability)
        let tpu_client = TpuClient::with_leader_resolver(
            rpc_url.clone(),
            Some(1), // N+1 redundancy (minimal for speed)
            Arc::clone(&leader_resolver),
        )?;

        // Pre-allocate RPC client for emergency blockhash retrieval
        let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
            rpc_url,
        ));

        info!("Panic Executor initialized successfully");

        Ok(Self {
            _udp_socket: Arc::new(udp_socket),
            tpu_client: Arc::new(tpu_client),
            payer,
            rpc_client,
            leader_resolver,
        })
    }

    /// Execute hard kill: Emergency sell + process termination
    ///
    /// **THIS FUNCTION NEVER RETURNS NORMALLY**
    ///
    /// Flow:
    /// 1. Log reason (RAM only, no blocking I/O)
    /// 2. Build atomic sell transaction (100% amount)
    /// 3. Fire via UDP to TPU (bypass RPC queues)
    /// 4. Activate dead-man switch (terminate process)
    ///
    /// # Arguments
    ///
    /// * `token_mint` - Token to sell
    /// * `amount` - Token amount to sell (100% of position)
    /// * `reason` - Why the panic was triggered
    pub async fn execute_hard_kill(
        &self,
        token_mint: Pubkey,
        amount: u64,
        reason: KillReason,
    ) -> ! {
        let start = Instant::now();

        // Step 1: Log reason (RAM only, blocking IO forbidden)
        error!(
            "🚨 HARD KILL INITIATED: reason={}, mint={}, amount={}",
            reason.as_str(),
            token_mint,
            amount
        );

        // Step 2: Build atomic sell transaction
        let tx_result = self.build_panic_sell_tx(token_mint, amount).await;

        let tx = match tx_result {
            Ok(transaction) => transaction,
            Err(e) => {
                error!(
                    "💀 HARD KILL: Failed to build sell transaction: {}. Terminating anyway.",
                    e
                );
                // Even if we can't build the transaction, we MUST terminate
                // to prevent re-entry on corrupted data
                Self::activate_dead_man_switch(reason, start.elapsed().as_millis());
            }
        };

        // Step 3: BYPASS ALL QUEUES -> Direct UDP Shot
        let fire_result = self.fire_leapfrog(tx).await;

        match fire_result {
            Ok(signature) => {
                let elapsed_ms = start.elapsed().as_millis();
                error!(
                    "🚨 PANIC SELL FIRED: signature={}, elapsed_ms={}, reason={}",
                    signature,
                    elapsed_ms,
                    reason.as_str()
                );
            }
            Err(e) => {
                error!(
                    "💀 HARD KILL: Failed to fire panic sell: {}. Terminating anyway.",
                    e
                );
            }
        }

        // Step 4: ACTIVATE DEAD-MAN SWITCH
        // No matter what happened above, we MUST terminate to prevent re-entry
        Self::activate_dead_man_switch(reason, start.elapsed().as_millis());
    }

    /// Build emergency sell transaction (100% position, maximum slippage tolerance)
    ///
    /// Uses aggressive slippage settings to ensure execution under any market conditions.
    async fn build_panic_sell_tx(&self, token_mint: Pubkey, amount: u64) -> Result<Transaction> {
        // Get latest blockhash (this is the only RPC call in panic path)
        // Uses pre-allocated RPC client to minimize latency
        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .await
            .context("Failed to get blockhash for panic sell")?;

        // Build sell instruction
        // NOTE: min_sol_out = 0 is intentional for panic sells
        // In emergency situations, we prioritize execution over price
        // Better to get SOME value out than be stuck with worthless tokens
        // This is explicitly documented in the security considerations
        let sell_ix = DirectSellBuilder::build_sell_ix(
            &self.payer.pubkey(),
            &token_mint,
            amount,
            0, // min_sol_out = 0 (accept any amount in panic mode)
        );

        // Build and sign transaction
        let tx = Transaction::new_signed_with_payer(
            &[sell_ix],
            Some(&self.payer.pubkey()),
            &[&*self.payer],
            recent_blockhash,
        );

        Ok(tx)
    }

    /// Fire transaction via UDP leapfrog (direct to TPU)
    ///
    /// This bypasses the RPC layer entirely and sends directly to
    /// the current validator's TPU.
    async fn fire_leapfrog(&self, tx: Transaction) -> Result<String> {
        // Serialize transaction
        let wire_transaction =
            bincode::serialize(&tx).context("Failed to serialize panic sell transaction")?;

        // Get current slot (from leader resolver cache, no RPC call)
        let current_slot = self.leader_resolver.get_current_slot().unwrap_or(0);

        info!(
            "🚀 LEAPFROG PANIC: Firing {} bytes to TPU at slot {}",
            wire_transaction.len(),
            current_slot
        );

        // Fire-and-forget: send to TPU via UDP
        let config = crate::config::LeapfrogConfig::new(1, false); // N+1, UDP (not QUIC)

        let signature = self
            .tpu_client
            .send_leapfrog(&wire_transaction, current_slot, &config)
            .await
            .context(
                "CRITICAL: Failed to send PANIC SELL via leapfrog - emergency exit in progress",
            )?;

        // Convert Signature to String
        Ok(signature.to_string())
    }

    /// Activate dead-man switch: Terminate process immediately
    ///
    /// **THIS FUNCTION NEVER RETURNS**
    ///
    /// Purpose: Prevent any further trading activity on potentially corrupted data.
    /// The bot MUST be manually restarted by the operator after investigation.
    fn activate_dead_man_switch(reason: KillReason, elapsed_ms: u128) -> ! {
        error!("💀💀💀 SYSTEM TERMINATED BY DEATH PROTOCOL 💀💀💀");
        error!("Reason: {}, Elapsed: {}ms", reason.as_str(), elapsed_ms);
        error!("MANUAL RESTART REQUIRED - Do not restart automatically!");
        error!("Investigate the reason for panic before resuming operations.");

        // Exit with non-zero code to indicate abnormal termination
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kill_reason_strings() {
        assert_eq!(KillReason::LigmaVeto.as_str(), "LIGMA_VETO");
        assert_eq!(KillReason::QeddSurvival.as_str(), "QEDD_SURVIVAL");
        assert_eq!(KillReason::ParadoxAnomaly.as_str(), "PARADOX_ANOMALY");
        assert_eq!(KillReason::ClusterCabal.as_str(), "CLUSTER_CABAL");
    }
}
