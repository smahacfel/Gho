//! IPC integration for Trigger component
//!
//! This module provides integration with the Seer IPC layer,
//! allowing Trigger to receive DetectedPoolEvent messages.

use crate::{
    config::AmmType,
    jito_client::JitoClient,
    safety::SafetyConfig,
    transaction_builder::{AmmAccounts, GhostTransactionBuilder},
};
use ghost_core::{trading_constraints::TradingConstraints, SwapPlanBuilder};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature};
use solana_sdk::signer::Signer;
use std::str::FromStr;

/// Re-export of DetectedPoolEvent from Seer for use in Trigger
/// This ensures type compatibility between Seer and Trigger
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedPoolEvent {
    /// The detected pool candidate
    pub candidate: CandidatePool,

    /// Timestamp when the event was created (for latency tracking)
    pub detected_at: std::time::SystemTime,

    /// Event sequence number (for tracking drops)
    pub sequence_number: u64,

    /// Priority level
    pub priority: EventPriority,
}

/// Candidate pool information from Seer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidatePool {
    /// Slot when detected (if known)
    pub slot: Option<u64>,

    /// Transaction signature
    pub signature: String,

    /// AMM program ID
    pub amm_program_id: Pubkey,

    /// Pool AMM account ID
    pub pool_amm_id: Pubkey,

    /// Base token mint
    pub base_mint: Pubkey,

    /// Quote token mint
    pub quote_mint: Pubkey,

    /// Bonding curve account
    pub bonding_curve: Pubkey,

    /// Creator wallet (transaction signer/payer)
    pub creator: Pubkey,

    /// Timestamp when detected (Unix timestamp)
    pub timestamp: u64,

    /// Optional: Bonding curve progress (0.0 - 1.0)
    pub bonding_curve_progress: Option<f64>,

    /// Optional: Initial liquidity in SOL
    pub initial_liquidity_sol: Option<f64>,

    /// Optional: Token total supply
    pub token_total_supply: Option<u64>,

    /// Block time when pool was initialized
    pub block_time: Option<i64>,
}

/// Event priority level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventPriority {
    /// High priority - process immediately
    High,
    /// Normal priority - standard processing
    Normal,
    /// Low priority - can be dropped under backpressure
    Low,
}

impl Default for EventPriority {
    fn default() -> Self {
        EventPriority::Normal
    }
}

/// IPC event processor for Trigger
pub struct IpcEventProcessor {
    /// Configuration
    config: ProcessorConfig,
}

/// Configuration for IPC event processing
#[derive(Debug, Clone)]
pub struct ProcessorConfig {
    /// Whether to process High priority events first
    pub prioritize_by_priority: bool,

    /// Maximum processing latency target (milliseconds)
    pub target_latency_ms: u64,

    /// Whether to skip stale events (older than threshold)
    pub skip_stale_events: bool,

    /// Stale event threshold (seconds)
    pub stale_threshold_secs: u64,

    /// Maximum concurrent positions (held tokens) - safeguard for budget management
    pub max_concurrent_positions: usize,
}

impl Default for ProcessorConfig {
    fn default() -> Self {
        Self {
            prioritize_by_priority: true,
            target_latency_ms: 100,
            skip_stale_events: true,
            stale_threshold_secs: 5,
            // Default max positions: 3
            // Provides balance between diversification, risk management, and capital efficiency
            // Matches default in TriggerComponentConfig (ghost-launcher)
            max_concurrent_positions: 3,
        }
    }
}

impl IpcEventProcessor {
    /// Create a new IPC event processor
    pub fn new(config: ProcessorConfig) -> Self {
        Self { config }
    }

    /// Check if an event is stale
    pub fn is_stale(&self, event: &DetectedPoolEvent) -> bool {
        if !self.config.skip_stale_events {
            return false;
        }

        if let Ok(age) = event.detected_at.elapsed() {
            age.as_secs() > self.config.stale_threshold_secs
        } else {
            false
        }
    }

    /// Process a detected pool event
    ///
    /// This is the main entry point for processing events received from Seer.
    /// It should be called by the Trigger main loop.
    ///
    /// # Implementation Note
    /// This method validates the event but does NOT build/send transactions.
    /// Actual transaction building should be done by the caller using
    /// GhostTransactionBuilder after this validation passes.
    ///
    /// # Flow
    /// 1. Check event staleness
    /// 2. Track latency metrics
    /// 3. Return success if validation passes (ready for transaction building)
    pub async fn process_event(
        &self,
        event: DetectedPoolEvent,
    ) -> Result<ProcessingResult, ProcessingError> {
        // Check if event is stale
        if self.is_stale(&event) {
            return Ok(ProcessingResult::Skipped {
                reason: SkipReason::Stale,
                sequence: event.sequence_number,
            });
        }

        // Calculate event age for latency tracking
        let event_age_ms = event
            .detected_at
            .elapsed()
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // Log if we're exceeding latency target
        if event_age_ms > self.config.target_latency_ms {
            tracing::warn!(
                "Event processing latency ({} ms) exceeds target ({} ms) for seq={}",
                event_age_ms,
                self.config.target_latency_ms,
                event.sequence_number
            );
        }

        // Event validated - ready for transaction building by caller
        // The caller should:
        // 1. Call Safety::validate() for Bulkhead checks
        // 2. Build transaction using GhostTransactionBuilder
        // 3. Send via JitoClient or TpuClient
        Ok(ProcessingResult::Processed {
            sequence: event.sequence_number,
            pool_id: event.candidate.pool_amm_id,
            latency_ms: event_age_ms,
        })
    }

    /// Process a detected pool event with position limit check (gatekeeper)
    ///
    /// This method includes a safeguard to prevent opening more positions than configured.
    /// It checks the Revolver's active positions before proceeding with the buy.
    ///
    /// # Position Limit Logic
    /// If `max_concurrent_positions = 3`, the system allows:
    /// - 0, 1, or 2 active positions → BUY allowed
    /// - 3 or more active positions → BUY blocked
    ///
    /// The comparison `active_positions_count >= max_concurrent_positions` ensures that
    /// when we already have 3 positions, we don't open a 4th one.
    ///
    /// # Arguments
    /// * `event` - The detected pool event to process
    /// * `active_positions_count` - Current number of active positions (from Revolver.get_active_mints().len())
    ///
    /// # Returns
    /// * `Ok(ProcessingResult::Skipped)` if position limit reached
    /// * `Ok(ProcessingResult::Processed)` if buy can proceed
    /// * `Err(ProcessingError)` if processing fails
    ///
    /// # Example
    /// ```ignore
    /// let active_count = revolver.read().await.get_active_mints().len();
    /// let result = processor.process_event_with_limit(event, active_count).await?;
    /// ```
    pub async fn process_event_with_limit(
        &self,
        event: DetectedPoolEvent,
        active_positions_count: usize,
    ) -> Result<ProcessingResult, ProcessingError> {
        // 🛡️ GATEKEEPER: Check position limit before processing
        if active_positions_count >= self.config.max_concurrent_positions {
            tracing::warn!(
                target: "trigger_gatekeeper",
                "⛔ SKIPPING BUY: Max positions reached ({}/{}) for mint {} (seq={})",
                active_positions_count,
                self.config.max_concurrent_positions,
                event.candidate.base_mint,
                event.sequence_number
            );

            return Ok(ProcessingResult::Skipped {
                reason: SkipReason::MaxPositionsReached,
                sequence: event.sequence_number,
            });
        }

        // If we pass the gatekeeper, proceed with normal processing
        self.process_event(event).await
    }
}

/// Handle a buy action received over IPC by building and submitting a swap transaction.
///
/// This wires the validated `DetectedPoolEvent` into the hot-path:
/// 1. Performs a bulkhead balance check
/// 2. Builds a Ghost transaction
/// 3. Submits it via Jito as a single-transaction bundle
#[allow(dead_code)]
pub async fn handle_buy_action(
    event: DetectedPoolEvent,
    payer: &Keypair,
    rpc_client: &RpcClient,
    jito_client: &JitoClient,
    safety_config: &SafetyConfig,
) -> Result<Signature, ProcessingError> {
    let amm_program_id = event.candidate.amm_program_id;
    // Map AMM program to supported AmmType
    let pump_id = Pubkey::from_str(crate::validation::PUMP_PROGRAM_ID)
        .map_err(|e| ProcessingError::InvalidEventData(e.to_string()))?;
    let bonk_id = Pubkey::from_str(crate::validation::BONK_PROGRAM_ID)
        .map_err(|e| ProcessingError::InvalidEventData(e.to_string()))?;

    let amm_type = if amm_program_id == pump_id {
        AmmType::PumpFun
    } else if amm_program_id == bonk_id {
        AmmType::BonkFun
    } else if TradingConstraints::is_authorized_amm_program(&amm_program_id) {
        return Err(ProcessingError::InvalidEventData(
            "Authorized AMM program detected, but Trigger buy execution currently supports only Pump.fun and Bonk.fun"
                .to_string(),
        ));
    } else {
        return Err(ProcessingError::InvalidEventData(format!(
            "Unsupported AMM program: {}",
            amm_program_id
        )));
    };

    // Bulkhead: compute safe trade amount from current balance
    let balance_lamports = rpc_client
        .get_balance(&payer.pubkey())
        .await
        .map_err(|e| ProcessingError::TransactionBuildError(e.to_string()))?;
    let balance_sol = balance_lamports as f64 / 1_000_000_000f64;
    let safe_trade_amount_sol =
        crate::safety::calculate_safe_trade_amount(balance_sol, safety_config, 1.0);

    if safe_trade_amount_sol <= 0.0 {
        return Err(ProcessingError::TransactionBuildError(
            "No safe balance available for trade".to_string(),
        ));
    }

    let amount_in = (safe_trade_amount_sol * 1_000_000_000f64) as u64;
    if amount_in == 0 {
        return Err(ProcessingError::TransactionBuildError(
            "Trade amount resolved to zero lamports".to_string(),
        ));
    }

    // Build swap plan from candidate
    let swap_plan = SwapPlanBuilder::new(payer.pubkey(), event.candidate.pool_amm_id)
        .amount_in(amount_in)
        .min_amount_out(1) // minimal positive guard; slippage handled by builder
        .timeout_seconds(300)
        .build()
        .map_err(|e| ProcessingError::TransactionBuildError(e.to_string()))?;

    let amm_accounts = AmmAccounts {
        pool: event.candidate.pool_amm_id,
        amm_program_id: Some(amm_program_id),
        bonding_curve: Some(event.candidate.bonding_curve),
        additional_accounts: vec![],
    };

    let builder = GhostTransactionBuilder::new(swap_plan, amm_type, amm_accounts);
    let recent_blockhash = rpc_client
        .get_latest_blockhash()
        .await
        .map_err(|e| ProcessingError::TransactionBuildError(e.to_string()))?;

    // Build tx with safety checks (Bulkhead)
    let tx = builder
        .build_full_swap_tx_with_safety(payer, recent_blockhash, balance_sol, safety_config)
        .map_err(|e| ProcessingError::TransactionBuildError(e.to_string()))?;

    // Submit via Jito as a single-transaction bundle
    jito_client
        .submit_single_transaction(tx)
        .await
        .map(|receipt| receipt.signature)
        .map_err(|e| ProcessingError::TransactionSendError(e.to_string()))
}

/// Result of processing an event
#[derive(Debug, Clone)]
pub enum ProcessingResult {
    /// Event was successfully processed
    Processed {
        /// Event sequence number
        sequence: u64,
        /// Pool ID that was processed
        pool_id: Pubkey,
        /// Processing latency in milliseconds
        latency_ms: u64,
    },

    /// Event was skipped
    Skipped {
        /// Reason for skipping
        reason: SkipReason,
        /// Event sequence number
        sequence: u64,
    },
}

/// Reason for skipping an event
#[derive(Debug, Clone, Copy)]
pub enum SkipReason {
    /// Event is too old
    Stale,
    /// Pool already processed
    Duplicate,
    /// Insufficient liquidity
    InsufficientLiquidity,
    /// Maximum concurrent positions reached
    MaxPositionsReached,
    /// Other reason
    Other,
}

/// Error during event processing
#[derive(Debug, thiserror::Error)]
pub enum ProcessingError {
    #[error("Transaction building failed: {0}")]
    TransactionBuildError(String),

    #[error("Transaction send failed: {0}")]
    TransactionSendError(String),

    #[error("Invalid event data: {0}")]
    InvalidEventData(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_event() -> DetectedPoolEvent {
        DetectedPoolEvent {
            candidate: CandidatePool {
                slot: Some(100),
                signature: "test_sig".to_string(),
                amm_program_id: Pubkey::new_unique(),
                pool_amm_id: Pubkey::new_unique(),
                base_mint: Pubkey::new_unique(),
                quote_mint: Pubkey::new_unique(),
                bonding_curve: Pubkey::new_unique(),
                creator: Pubkey::new_unique(),
                timestamp: 1234567890,
                bonding_curve_progress: Some(50.0),
                initial_liquidity_sol: Some(10.0),
                token_total_supply: Some(1_000_000),
                block_time: Some(1234567890),
            },
            detected_at: std::time::SystemTime::now(),
            sequence_number: 1,
            priority: EventPriority::Normal,
        }
    }

    #[tokio::test]
    async fn test_processor_creation() {
        let processor = IpcEventProcessor::new(ProcessorConfig::default());
        let event = create_test_event();

        let result = processor.process_event(event).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_stale_event_detection() {
        let config = ProcessorConfig {
            skip_stale_events: true,
            stale_threshold_secs: 1,
            ..Default::default()
        };
        let processor = IpcEventProcessor::new(config);

        // Create old event
        let mut event = create_test_event();
        event.detected_at = std::time::SystemTime::now() - std::time::Duration::from_secs(5);

        assert!(processor.is_stale(&event));
    }

    #[tokio::test]
    async fn test_fresh_event_processing() {
        let processor = IpcEventProcessor::new(ProcessorConfig::default());
        let event = create_test_event();

        assert!(!processor.is_stale(&event));

        let result = processor.process_event(event).await.unwrap();
        match result {
            ProcessingResult::Processed { .. } => {} // Expected
            ProcessingResult::Skipped { .. } => panic!("Event should not be skipped"),
        }
    }

    #[tokio::test]
    async fn test_max_positions_gatekeeper_allows_buy_under_limit() {
        let config = ProcessorConfig {
            max_concurrent_positions: 3,
            ..Default::default()
        };
        let processor = IpcEventProcessor::new(config);
        let event = create_test_event();

        // With 2 active positions (under limit of 3), buy should proceed
        let result = processor.process_event_with_limit(event, 2).await.unwrap();
        match result {
            ProcessingResult::Processed { .. } => {} // Expected - buy allowed
            ProcessingResult::Skipped { .. } => {
                panic!("Event should not be skipped when under limit")
            }
        }
    }

    #[tokio::test]
    async fn test_max_positions_gatekeeper_blocks_buy_at_limit() {
        let config = ProcessorConfig {
            max_concurrent_positions: 3,
            ..Default::default()
        };
        let processor = IpcEventProcessor::new(config);
        let event = create_test_event();

        // With 3 active positions (at limit of 3), buy should be blocked
        let result = processor.process_event_with_limit(event, 3).await.unwrap();
        match result {
            ProcessingResult::Skipped { reason, .. } => {
                assert!(matches!(reason, SkipReason::MaxPositionsReached));
            }
            ProcessingResult::Processed { .. } => panic!("Event should be skipped when at limit"),
        }
    }

    #[tokio::test]
    async fn test_max_positions_gatekeeper_blocks_buy_over_limit() {
        let config = ProcessorConfig {
            max_concurrent_positions: 3,
            ..Default::default()
        };
        let processor = IpcEventProcessor::new(config);
        let event = create_test_event();

        // With 5 active positions (over limit of 3), buy should be blocked
        let result = processor.process_event_with_limit(event, 5).await.unwrap();
        match result {
            ProcessingResult::Skipped { reason, .. } => {
                assert!(matches!(reason, SkipReason::MaxPositionsReached));
            }
            ProcessingResult::Processed { .. } => panic!("Event should be skipped when over limit"),
        }
    }

    #[tokio::test]
    async fn test_default_max_positions_is_three() {
        let config = ProcessorConfig::default();
        assert_eq!(config.max_concurrent_positions, 3);
    }
}
