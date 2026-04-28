use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;

use crate::aem::{config::AemConfig, error::AemError};

pub type UnixMs = u64;
pub type PositionId = String;
pub type DecisionEventId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActionChosen {
    SellNow,
    WaitReclaim,
    Partial,
    Panic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CommandPriority {
    Default,
    AemPolicy,
    HardSafety,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReclaimFlag {
    None,
    Partial,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StressBucket {
    Low,
    Med,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SlopeBucket {
    FastDown,
    SlowDown,
    Flat,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DrawdownBucket {
    Dd0_20,
    Dd20_40,
    Dd40Plus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TimeBucket {
    T0_30s,
    T30_120s,
    T120Plus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RegimeTag {
    Capitulation,
    ReclaimAttempt,
    Stabilizing,
    DeadSlide,
    DriftUp,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RegimeKey {
    pub drawdown_bucket: DrawdownBucket,
    pub time_bucket: TimeBucket,
    pub slope_bucket: SlopeBucket,
    pub reclaim_flag: ReclaimFlag,
    pub stress_bucket: StressBucket,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateFeatures {
    pub position_id: PositionId,
    pub pool_amm_id: Pubkey,
    pub base_mint: Pubkey,

    pub entry_price_or_mcap: f64,
    pub current_price_or_mcap: f64,
    pub peak_since_entry: f64,

    pub drawdown_pct: f64,
    pub unrealized_pnl_pct: f64,
    pub slope_pct_per_s: f64,
    pub volatility_proxy: Option<f64>,

    pub reclaim_flag: ReclaimFlag,
    pub time_since_entry_s: u32,
    pub time_since_last_peak_s: u32,

    pub requeue_count: u32,
    pub send_fail_count: u32,
    pub relax_count: u32,
    pub oracle_stale_age_ms: u64,
    pub last_sell_attempt_age_ms: Option<u64>,
    pub stress_bucket: StressBucket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SafetyReasonCode {
    OracleStaleHard,
    StressHigh,
    ExecutionNotPossible,
    HardDrawdownCatastrophic,
    ExternalHardLock,
    InvalidFeatureData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyAction {
    pub action: ActionChosen,
    pub reason_code: SafetyReasonCode,
    pub hard_lock_until_unix_ms: UnixMs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandDirective {
    Noop,
    SetTightStop,
    SetLooseStop,
    ForceExitAll,
    ForceExitFractionBps { fraction_bps: u16 },
    FreezePanic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlCommand {
    pub position_id: PositionId,
    pub action: ActionChosen,
    pub directive: CommandDirective,
    pub issued_at_unix_ms: UnixMs,
    pub valid_from_unix_ms: UnixMs,
    pub expires_at_unix_ms: UnixMs,
    pub position_epoch: u64,
    pub priority: CommandPriority,
    pub reason_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActionStats {
    pub n: u32,
    pub mean_delta_pnl: f64,
    pub std_delta_pnl: Option<f64>,
    pub tail_risk_rate: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiBounds {
    pub lcb: f64,
    pub ucb: f64,
    pub k: f64,
    pub n: u32,
}

pub trait HardSafetyCheck: Send + Sync {
    fn evaluate(
        &self,
        features: &StateFeatures,
        now_unix_ms: UnixMs,
        cfg: &AemConfig,
    ) -> Option<SafetyAction>;
}

#[derive(Debug, Default, Clone)]
pub struct RegimeBook {
    pub stats: HashMap<(RegimeKey, ActionChosen), ActionStats>,
}

#[derive(Debug, Clone, Default)]
pub struct PolicyEngine;

#[derive(Debug, Clone)]
pub struct PolicyDecision {
    pub action_chosen: ActionChosen,
    pub reason_code: String,
    pub wait_ci: Option<CiBounds>,
    pub sell_ci: Option<CiBounds>,
    pub partial_ci: Option<CiBounds>,
    pub ci_check_passed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagementDecisionEvent {
    pub decision_event_id: DecisionEventId,
    pub position_id: PositionId,
    pub position_epoch: u64,
    pub timestamp_decision_unix_ms: UnixMs,
    pub regime_key: RegimeKey,
    pub regime_tag: RegimeTag,
    pub action_chosen: ActionChosen,
    pub features_snapshot: StateFeatures,
    pub hard_safety_triggered: bool,
    pub hard_safety_reason_code: Option<SafetyReasonCode>,
    pub control_command: ControlCommand,
    pub rollout_mode: RolloutMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagementOutcomeEvent {
    pub outcome_event_id: String,
    pub decision_event_id: DecisionEventId,
    pub position_id: PositionId,
    pub timestamp_outcome_unix_ms: UnixMs,
    pub price_at_decision: f64,
    pub price_at_t: Option<f64>,
    pub peak_in_t: Option<f64>,
    pub reclaim_happened: bool,
    pub time_to_reclaim_ms: Option<u64>,
    pub counterfactual_delta_pnl: f64,
    pub tail_loss_flag: bool,
    pub outcome_data_gap: bool,
    pub outcome_reason_code: String,
}

#[derive(Debug, Clone)]
pub struct ReplayPair {
    pub decision: ManagementDecisionEvent,
    pub outcome: ManagementOutcomeEvent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RolloutMode {
    Shadow,
    PilotLive,
    FullLive,
}

pub trait AemLedgerWriter: Send + Sync {
    fn append_decision(&self, event: &ManagementDecisionEvent) -> Result<(), AemError>;
    fn append_outcome(&self, event: &ManagementOutcomeEvent) -> Result<(), AemError>;
    fn append_time_index(&self, idx: &TimeIndexRecord) -> Result<(), AemError>;
    fn append_regime_index(&self, idx: &RegimeIndexRecord) -> Result<(), AemError>;
}

pub trait AemLedgerReader: Send + Sync {
    fn replay_pairs_in_window(
        &self,
        window_start_unix_ms: UnixMs,
        window_end_unix_ms: UnixMs,
        max_events: usize,
    ) -> Result<Vec<ReplayPair>, AemError>;

    fn decisions_without_outcome(
        &self,
        window_start_unix_ms: UnixMs,
        window_end_unix_ms: UnixMs,
        max_events: usize,
    ) -> Result<Vec<ManagementDecisionEvent>, AemError>;
}

pub trait AemLedgerIo: AemLedgerWriter + AemLedgerReader {}

impl<T> AemLedgerIo for T where T: AemLedgerWriter + AemLedgerReader {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeIndexRecord {
    pub timestamp_unix_ms: UnixMs,
    pub event_type: String,
    pub event_id: String,
    pub file_offset: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeIndexRecord {
    pub regime_key_hash: u64,
    pub action: ActionChosen,
    pub timestamp_unix_ms: UnixMs,
    pub decision_event_id: DecisionEventId,
    pub file_offset: u64,
}

pub trait TriggerControlAdapter {
    fn apply_control_command(
        &mut self,
        cmd: &ControlCommand,
        now_unix_ms: UnixMs,
    ) -> CommandApplyResult;

    fn get_execution_stress(&self, position_id: &str) -> Option<ExecutionStressSnapshot>;

    fn register_position_epoch(&mut self, position_id: &str, position_epoch: u64);

    fn unregister_position_epoch(&mut self, position_id: &str);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandApplyResult {
    pub accepted: bool,
    pub reject_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionStressSnapshot {
    pub requeue_count: u32,
    pub send_fail_count: u32,
    pub relax_count: u32,
    pub oracle_stale_age_ms: u64,
    pub last_sell_attempt_age_ms: Option<u64>,
}
