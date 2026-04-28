use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct SessionId(pub u64);

impl SessionId {
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerdictOutcome {
    Pass { reason: String },
    Fail { reason: String },
    Timeout { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    Created,
    Accumulating,
    Evaluating,
    Decided(VerdictOutcome),
    Closed,
}

impl Default for SessionStatus {
    fn default() -> Self {
        Self::Created
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub session_id: SessionId,
    pub pool_amm_id: Pubkey,
    pub base_mint: Pubkey,
    pub observation_duration_ms: u64,
    pub is_dev_known: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionDiagnostics {
    pub total_tx_seen: u64,
    pub total_account_updates: u64,
    pub checkpoint_count: u32,
    pub first_tx_ts_ms: Option<u64>,
    pub last_tx_ts_ms: Option<u64>,
    pub reject_reasons: Vec<String>,
}
