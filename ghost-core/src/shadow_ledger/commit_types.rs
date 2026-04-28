//! Neutral commit/config types extracted from the transitional core gatekeeper.
//!
//! These types do not own runtime policy. They only describe buffered-history
//! commit outcomes and simple storage-oriented configuration.

use super::history_types::BufferedTx;
use super::ledger::CommitHistoryResult;
use super::trade_types::TxKey;
use super::types::MarketSnapshot;

/// Internal stage marker for commit progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitStage {
    Idle,
    Building,
    Finalizing,
}

impl CommitStage {
    pub(crate) fn to_u8(self) -> u8 {
        match self {
            CommitStage::Idle => 0,
            CommitStage::Building => 1,
            CommitStage::Finalizing => 2,
        }
    }

    pub(crate) fn from_u8(value: u8) -> Self {
        match value {
            1 => CommitStage::Building,
            2 => CommitStage::Finalizing,
            _ => CommitStage::Idle,
        }
    }
}

/// Result of an atomic commit operation to ShadowLedger.
#[derive(Debug, Clone)]
pub struct CommitResult {
    /// Semantic persisted-write outcome from ShadowLedger.
    pub commit_history_result: CommitHistoryResult,
    /// Number of snapshots committed to ShadowLedger.
    pub committed_count: usize,
    /// Number of transactions observed during commit (dead-window protected).
    pub merged_pending_count: usize,
    /// Last committed TxKey (for idempotency checks).
    pub last_committed_tx_key: Option<TxKey>,
    /// Last committed MarketSnapshot (for LivePipeline initialization).
    pub last_snapshot: Option<MarketSnapshot>,
    /// Transactions that arrived while commit was in progress.
    pub pending_live: Vec<BufferedTx>,
}

/// Statistics about the Gatekeeper registry.
#[derive(Debug, Clone)]
pub struct GatekeeperStats {
    pub active_buffers: usize,
    pub total_created: u64,
    pub total_committed: u64,
    pub total_dropped: u64,
}
