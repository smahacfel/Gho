//! Message types for the actor system
//!
//! This module defines all message types used for communication between actors.

use crate::oracle::quantum_oracle::ScoredCandidate;
use crate::oracle::scorer::AggregatedRiskScore;
use crate::oracle::transaction_monitor::MonitoredTransaction;
use crate::oracle::types::{Outcome, TransactionRecord};
use crate::types::{PremintCandidate, Pubkey};
use actix::prelude::*;

// ============================================================================
// Oracle Actor Messages
// ============================================================================

/// Message to score a candidate
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<(), String>")]
pub struct ScoreCandidate {
    pub candidate: PremintCandidate,
}

/// Message sent when a token has been bought (triggers post-buy analysis)
/// This initiates the Ghost Intelligence analysis pipeline.
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<AggregatedRiskScore, String>")]
pub struct TokenBought {
    /// The mint address of the bought token
    pub mint: Pubkey,
    /// The creator address of the token
    pub creator: Pubkey,
    /// Buy slot for timing reference
    pub slot: u64,
    /// Unix timestamp of the buy
    pub timestamp: u64,
}

/// Message to assess a held token using Ghost Intelligence
/// (DevProfiler + ClusterHunter + VisionCritic)
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<AggregatedRiskScore, String>")]
pub struct AssessHeldToken {
    /// The mint address of the token to assess
    pub mint: Pubkey,
    /// The creator address of the token (for DevProfiler)
    pub creator: Pubkey,
    /// Optional metadata URI for VisionCritic analysis
    pub metadata_uri: Option<String>,
}

/// Message to trigger an emergency sell (Panic Sell)
/// Sent when risk analysis indicates critical risk.
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct EmergencySell {
    /// The mint address of the token to sell
    pub mint: Pubkey,
    /// Reason for the emergency sell
    pub reason: String,
    /// Risk score that triggered the sell
    pub risk_score: f32,
}

/// Strategy update configuration for trailing stop loss
#[derive(Debug, Clone)]
pub enum TrailingStopLossMode {
    /// Tight stop loss (quick exit on small drops)
    Tight,
    /// Loose stop loss (allow larger swings, let profits run)
    Loose,
}

/// Message to update the trading strategy for a position
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct UpdateStrategy {
    /// The mint address of the token
    pub mint: Pubkey,
    /// New trailing stop loss mode
    pub trailing_stop_loss: TrailingStopLossMode,
    /// Reason for the strategy update
    pub reason: String,
}

/// Message to update oracle configuration
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct UpdateOracleConfig {
    pub weights: Option<crate::oracle::types::FeatureWeights>,
    pub thresholds: Option<crate::oracle::types::ScoreThresholds>,
}

/// Message to get oracle metrics
#[derive(Message, Debug, Clone)]
#[rtype(result = "OracleMetrics")]
pub struct GetOracleMetrics;

/// Oracle metrics response
#[derive(Debug, Clone)]
pub struct OracleMetrics {
    pub total_scored: u64,
    pub avg_scoring_time: f64,
    pub high_score_count: u64,
}

// ============================================================================
// Storage Actor Messages (DecisionLedger)
// ============================================================================

/// Message to record a transaction decision
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<(), String>")]
pub struct RecordDecision {
    pub record: TransactionRecord,
}

/// Message to update transaction outcome
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<(), String>")]
pub struct UpdateOutcome {
    pub signature: String,
    pub outcome: Outcome,
    pub buy_price: Option<f64>,
    pub sell_price: Option<f64>,
    pub sol_spent: Option<f64>,
    pub sol_received: Option<f64>,
    pub evaluated_at: Option<u64>,
    pub is_verified: bool,
}

/// Message to query storage statistics
#[derive(Message, Debug, Clone)]
#[rtype(result = "StorageStats")]
pub struct GetStorageStats;

/// Storage statistics response
#[derive(Debug, Clone)]
pub struct StorageStats {
    pub total_decisions: u64,
    pub pending_outcomes: u64,
}

// ============================================================================
// Monitor Actor Messages (TransactionMonitor)
// ============================================================================

/// Message to add a transaction to monitoring
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<(), String>")]
pub struct MonitorTransaction {
    pub transaction: MonitoredTransaction,
}

/// Message to get monitoring statistics
#[derive(Message, Debug, Clone)]
#[rtype(result = "MonitorStats")]
pub struct GetMonitorStats;

/// Monitor statistics response
#[derive(Debug, Clone)]
pub struct MonitorStats {
    pub active_transactions: usize,
    pub completed_transactions: u64,
}

// ============================================================================
// Supervisor Actor Messages
// ============================================================================

/// Message to get system health status
#[derive(Message, Debug, Clone)]
#[rtype(result = "SystemHealth")]
pub struct GetSystemHealth;

/// System health response
#[derive(Debug, Clone)]
pub struct SystemHealth {
    pub oracle_healthy: bool,
    pub storage_healthy: bool,
    pub monitor_healthy: bool,
    pub uptime_secs: u64,
}

/// Message to gracefully shutdown the system
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct ShutdownSystem;
