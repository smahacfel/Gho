//! ExecutionBackend trait and core types
//!
//! This module defines the single contract for all execution: live, paper, or dual.
//! The rest of the pipeline talks ONLY to `ExecutionBackend` — no `if paper else live`.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::fmt;

// ─── Config SSOT ────────────────────────────────────────────────────────────

/// Single Source of Truth for execution mode.
/// Read from config once at startup; determines which backend is instantiated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionMode {
    Live,
    Paper,
    Shadow,
    Dual,
}

impl Default for ExecutionMode {
    fn default() -> Self {
        Self::Paper
    }
}

impl fmt::Display for ExecutionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Live => write!(f, "live"),
            Self::Paper => write!(f, "paper"),
            Self::Shadow => write!(f, "shadow"),
            Self::Dual => write!(f, "dual"),
        }
    }
}

// ─── Common types ───────────────────────────────────────────────────────────

pub type OrderId = String;
pub type QuoteId = String;
pub type PositionId = String;
pub type CandidateId = String;
pub type CommandId = String;

pub const MIN_SHADOW_TX_BUILD_COMPENSATION_MS: u64 = 250;
pub const ESTIMATED_SLOT_TIME_MS: u64 = 400;

/// Declares which timing path is expected to carry an entry from pre-send preparation
/// to actual dispatch/settlement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryTimingSource {
    LiveStandard,
    LiveJito,
    PaperBroker,
    LegacyBackend,
}

impl EntryTimingSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LiveStandard => "live_standard",
            Self::LiveJito => "live_jito",
            Self::PaperBroker => "paper_broker",
            Self::LegacyBackend => "legacy_backend",
        }
    }
}

/// Declares where the quote price reference used by the pre-send contract came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryPriceSource {
    SnapshotEngine,
    EffectiveFillFallback,
}

impl EntryPriceSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SnapshotEngine => "snapshot_engine",
            Self::EffectiveFillFallback => "effective_fill_fallback",
        }
    }
}

/// Declares what the caller expects to happen when the prepared quote is already stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryStalePolicy {
    EmitWarning,
    Reject,
}

impl EntryStalePolicy {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EmitWarning => "emit_warning",
            Self::Reject => "reject",
        }
    }
}

/// Frozen quote/timing contract carried by a prepared entry attempt.
#[derive(Debug, Clone)]
pub struct PreparedQuoteRef {
    pub quote_id: QuoteId,
    pub quote_ts_ms: u64,
    pub slot: Option<u64>,
    pub quote_price_ref: Option<f64>,
    pub price_source: EntryPriceSource,
    pub is_stale: bool,
    pub stale_age_ms: u64,
    pub stale_policy: EntryStalePolicy,
}

/// Shared pre-send entry contract that must be stable across live/shadow preparation.
#[derive(Debug, Clone)]
pub struct PreparedEntryExecution {
    pub order_id: OrderId,
    pub candidate: CandidateRef,
    pub submit_time_ms: u64,
    pub position_epoch: u64,
    pub quote: PreparedQuoteRef,
    pub timing_source: EntryTimingSource,
    pub predicted_slot: Option<u64>,
}

impl PreparedEntryExecution {
    #[must_use]
    pub fn candidate_id(&self) -> &str {
        &self.candidate.candidate_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryTimingPath {
    StandardPath,
    JitoBatchPath,
    FallbackPath,
}

impl EntryTimingPath {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StandardPath => "standard_path",
            Self::JitoBatchPath => "jito_batch_path",
            Self::FallbackPath => "fallback_path",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryTimingPlan {
    pub timing_path: EntryTimingPath,
    pub reference_slot: Option<u64>,
    pub predicted_slot: Option<u64>,
    pub planned_settle_time_ms: u64,
    pub compensation_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ExecutionAttemptContext {
    pub prepared: PreparedEntryExecution,
    pub timing: EntryTimingPlan,
}

#[must_use]
pub fn normalize_shadow_compensation_ms(configured_ms: u64) -> u64 {
    configured_ms.max(MIN_SHADOW_TX_BUILD_COMPENSATION_MS)
}

#[must_use]
pub fn predicted_slot_from_reference(reference_slot: Option<u64>) -> Option<u64> {
    reference_slot.map(|slot| slot.saturating_add(1))
}

#[must_use]
pub fn build_entry_timing_plan(
    prepared: &PreparedEntryExecution,
    reference_slot: Option<u64>,
    compensation_ms: u64,
) -> EntryTimingPlan {
    let compensation_ms = normalize_shadow_compensation_ms(compensation_ms);
    let reference_slot = reference_slot.or(prepared.quote.slot);

    match prepared.timing_source {
        EntryTimingSource::LiveStandard => EntryTimingPlan {
            timing_path: EntryTimingPath::StandardPath,
            reference_slot,
            predicted_slot: prepared.predicted_slot,
            planned_settle_time_ms: prepared.submit_time_ms.saturating_add(compensation_ms),
            compensation_ms,
        },
        EntryTimingSource::LiveJito => {
            let predicted_slot = prepared
                .predicted_slot
                .or_else(|| predicted_slot_from_reference(reference_slot));
            let timing_path = if reference_slot.is_some() && predicted_slot.is_some() {
                EntryTimingPath::JitoBatchPath
            } else {
                EntryTimingPath::FallbackPath
            };
            let slot_delay_ms = predicted_slot
                .zip(reference_slot)
                .map(|(predicted, reference)| {
                    predicted
                        .saturating_sub(reference)
                        .saturating_mul(ESTIMATED_SLOT_TIME_MS)
                })
                .unwrap_or(0);
            EntryTimingPlan {
                timing_path,
                reference_slot,
                predicted_slot,
                planned_settle_time_ms: prepared
                    .submit_time_ms
                    .saturating_add(compensation_ms)
                    .saturating_add(slot_delay_ms),
                compensation_ms,
            }
        }
        EntryTimingSource::PaperBroker | EntryTimingSource::LegacyBackend => EntryTimingPlan {
            timing_path: EntryTimingPath::FallbackPath,
            reference_slot,
            predicted_slot: prepared
                .predicted_slot
                .or_else(|| predicted_slot_from_reference(reference_slot)),
            planned_settle_time_ms: prepared.submit_time_ms.saturating_add(compensation_ms),
            compensation_ms,
        },
    }
}

impl ExecutionAttemptContext {
    #[must_use]
    pub fn new(
        prepared: PreparedEntryExecution,
        reference_slot: Option<u64>,
        compensation_ms: u64,
    ) -> Self {
        let timing = build_entry_timing_plan(&prepared, reference_slot, compensation_ms);
        Self { prepared, timing }
    }
}

/// Which lane an event/order belongs to (for DUAL mode tagging).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lane {
    Live,
    Paper,
    Shadow,
    /// Single-mode (not dual) — no lane distinction.
    Single,
}

impl Default for Lane {
    fn default() -> Self {
        Self::Single
    }
}

impl fmt::Display for Lane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Live => write!(f, "live"),
            Self::Paper => write!(f, "paper"),
            Self::Shadow => write!(f, "shadow"),
            Self::Single => write!(f, "single"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_mode_shadow_serializes_and_formats() {
        assert_eq!(
            serde_json::to_string(&ExecutionMode::Shadow).expect("serialize mode"),
            "\"shadow\""
        );
        assert_eq!(ExecutionMode::Shadow.to_string(), "shadow");
    }

    #[test]
    fn lane_shadow_serializes_and_formats() {
        assert_eq!(
            serde_json::to_string(&Lane::Shadow).expect("serialize lane"),
            "\"shadow\""
        );
        assert_eq!(Lane::Shadow.to_string(), "shadow");
    }

    #[test]
    fn build_entry_timing_plan_resolves_jito_path_without_placeholder() {
        let prepared = PreparedEntryExecution {
            order_id: "order-1".to_string(),
            candidate: CandidateRef {
                candidate_id: "cand-1".to_string(),
                base_mint: Pubkey::new_unique(),
                pool_amm_id: Pubkey::new_unique(),
                entry_amount_lamports: 1_000_000,
                min_tokens_out: 50_000,
            },
            submit_time_ms: 1_000,
            position_epoch: 1,
            quote: PreparedQuoteRef {
                quote_id: "quote-1".to_string(),
                quote_ts_ms: 900,
                slot: Some(250),
                quote_price_ref: Some(0.02),
                price_source: EntryPriceSource::SnapshotEngine,
                is_stale: false,
                stale_age_ms: 100,
                stale_policy: EntryStalePolicy::EmitWarning,
            },
            timing_source: EntryTimingSource::LiveJito,
            predicted_slot: None,
        };

        let timing = build_entry_timing_plan(&prepared, Some(300), 100);
        assert_eq!(timing.timing_path, EntryTimingPath::JitoBatchPath);
        assert_eq!(timing.reference_slot, Some(300));
        assert_eq!(timing.predicted_slot, Some(301));
        assert_eq!(
            timing.planned_settle_time_ms,
            1_000 + MIN_SHADOW_TX_BUILD_COMPENSATION_MS + ESTIMATED_SLOT_TIME_MS
        );
    }

    #[test]
    fn build_entry_timing_plan_uses_fallback_when_jito_metadata_missing() {
        let prepared = PreparedEntryExecution {
            order_id: "order-2".to_string(),
            candidate: CandidateRef {
                candidate_id: "cand-2".to_string(),
                base_mint: Pubkey::new_unique(),
                pool_amm_id: Pubkey::new_unique(),
                entry_amount_lamports: 1_000_000,
                min_tokens_out: 50_000,
            },
            submit_time_ms: 1_000,
            position_epoch: 1,
            quote: PreparedQuoteRef {
                quote_id: "quote-2".to_string(),
                quote_ts_ms: 900,
                slot: None,
                quote_price_ref: Some(0.02),
                price_source: EntryPriceSource::SnapshotEngine,
                is_stale: false,
                stale_age_ms: 100,
                stale_policy: EntryStalePolicy::EmitWarning,
            },
            timing_source: EntryTimingSource::LiveJito,
            predicted_slot: None,
        };

        let timing = build_entry_timing_plan(&prepared, None, 250);
        assert_eq!(timing.timing_path, EntryTimingPath::FallbackPath);
        assert_eq!(timing.predicted_slot, None);
        assert_eq!(timing.planned_settle_time_ms, 1_250);
    }
}

/// Reference to a candidate that passed Gatekeeper.
#[derive(Debug, Clone)]
pub struct CandidateRef {
    pub candidate_id: CandidateId,
    pub base_mint: Pubkey,
    pub pool_amm_id: Pubkey,
    pub entry_amount_lamports: u64,
    pub min_tokens_out: u64,
}

/// Side of an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderSide {
    Entry,
    Exit,
}

/// Status of a fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FillStatus {
    Filled,
    Failed,
    Stale,
    Sent,
    Confirmed,
    Unknown,
}

/// A fill event returned by `poll_fills`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillEvent {
    pub order_id: OrderId,
    pub position_id: Option<PositionId>,
    pub side: OrderSide,
    pub status: FillStatus,
    pub fill_price: f64,
    pub fill_qty: u64,
    pub quote_id_used: QuoteId,
    pub fill_time_ms: u64,
    pub latency_ms: u64,
    pub lane: Lane,
}

/// Execution-related errors.
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
pub enum ExecutionError {
    #[error("insufficient balance")]
    InsufficientBalance,
    #[error("slippage exceeded")]
    SlippageExceeded,
    #[error("transaction failed: {0}")]
    TransactionFailed(String),
    #[error("quote stale: age {age_ms}ms > max {max_age_ms}ms")]
    QuoteStale { age_ms: u64, max_age_ms: u64 },
    #[error("position limit reached")]
    PositionLimitReached,
    #[error("broker queue full")]
    BrokerQueueFull,
    #[error("network error: {0}")]
    NetworkError(String),
    #[error("simulated failure: {reason}")]
    SimulatedFailure { reason: String },
}

/// Snapshot of execution stress for a position (extended with bucket).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionStressSnapshot {
    pub requeue_count: u32,
    pub send_fail_count: u32,
    pub relax_count: u32,
    pub oracle_stale_age_ms: u64,
    pub last_sell_attempt_age_ms: Option<u64>,
    pub stress_bucket: StressBucket,
    pub concurrent_exits_count: u32,
    pub injected: bool,
}

impl Default for ExecutionStressSnapshot {
    fn default() -> Self {
        Self {
            requeue_count: 0,
            send_fail_count: 0,
            relax_count: 0,
            oracle_stale_age_ms: 0,
            last_sell_attempt_age_ms: None,
            stress_bucket: StressBucket::Low,
            concurrent_exits_count: 0,
            injected: false,
        }
    }
}

/// Stress bucket (mirrors aem::types::StressBucket).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StressBucket {
    Low,
    Med,
    High,
}

impl Default for StressBucket {
    fn default() -> Self {
        Self::Low
    }
}

// ─── ExecutionBackend trait ─────────────────────────────────────────────────

/// The single execution contract.
///
/// Pipeline components (Revolver, AEM, …) talk **only** to this trait.
/// Implementations: `LiveBackend`, `PaperBackend`, `DualBackend`.
#[async_trait]
pub trait ExecutionBackend: Send + Sync {
    /// Submit a BUY order.
    async fn submit_entry(
        &self,
        candidate: &CandidateRef,
        quote_ref: QuoteId,
        position_epoch: u64,
    ) -> Result<OrderId, ExecutionError>;

    /// Submit a SELL / partial-exit order.
    async fn submit_exit(
        &self,
        position_id: &PositionId,
        fraction_bps: u16,
        quote_ref: QuoteId,
        command_ref: Option<CommandId>,
    ) -> Result<OrderId, ExecutionError>;

    /// Poll for completed fills since last call.
    async fn poll_fills(&self, now_ms: u64) -> Vec<FillEvent>;

    /// Get execution stress snapshot for a position.
    fn get_execution_stress(&self, position_id: &PositionId) -> ExecutionStressSnapshot;

    /// Which lane this backend represents.
    fn lane(&self) -> Lane;
}
