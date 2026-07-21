//! Pure HET Position Manager V2 policy and its shadow-manager contracts.
//!
//! Evaluation remains side-effect free.  In `authoritative_shadow` mode the
//! engine consumes its completed decisions through the existing guarded shadow
//! sell/apply path; this module never performs mutation itself.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::execution::backend::Lane;

use super::config::{CrashGuardMode, HetPmV2Config, HetPmV2Mode, PostBuyGuardianConfig};
use super::exit_policy_v1::{
    CrashGuardPreQuoteDecision, CrashGuardQuoteDecision, CrashGuardQuoteRejectionReason,
    CrashGuardQuoteRequirementV1, EffectiveExitPolicyV1Config, ExecutableExitQuote,
    ExitCandidateReason, ExitPolicyV1, MarkEvidenceStatus, PostBuyDecisionSnapshot,
    PreQuoteDecision, QuoteEvidenceRevisionV1, EXIT_POLICY_V1_ID, EXIT_POLICY_V1_VERSION,
};
use super::trajectory_v1::{TrajectoryFeaturesV1, TrajectoryQualityV1};

pub(super) const HET_PM_V2_POLICY_ID: &str = "hierarchical_executable_trajectory_pm_v2";
pub(super) const HET_PM_V2_POLICY_VERSION: u16 = 2;
/// Schema V3 adds an additive, per-gate candidate lattice.  It does not alter
/// the V2 pure-policy semantics. In observe-only mode V1 remains the shadow
/// decision owner; in authoritative-shadow mode V2 selects the shadow action
/// while the shared shadow executor remains the only apply path. Live
/// authority is never enabled by this schema.
pub(super) const HET_PM_V2_SCHEMA_VERSION: u16 = 3;
pub(super) const HET_PM_V2_SAMPLING_MODE: &str = "latest_canonical_state_per_monitor_tick";
pub(super) const HET_PM_V2_TRAJECTORY_GRADE: &str = "online_non_lookahead_sampled_trajectory";
pub(super) const HET_PM_V2_PUMP_ROUTE_ID: &str = "pump_curve";
pub(super) const HET_PM_V2_QUOTE_MODEL_ID: &str = "price_truth_resolver_shadow_curve_v1";
pub(super) const HET_PM_V2_MAX_QUOTE_CELLS: usize = 2;
pub(super) const HET_PM_V2_MAX_SERIALIZED_RECORD_BYTES: usize = 64 * 1024;
const HET_PM_V2_MAX_WRITER_QUEUE_CAPACITY: usize = 4_096;
const HET_PM_V2_MAX_TERMINAL_WRITE_BUDGET_MS: u64 = 100;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum HetPmV2ConfigError {
    #[error("HET-PM V2 trajectory windows must all be greater than zero")]
    InvalidTrajectoryWindow,
    #[error("HET-PM V2 trajectory windows must satisfy short < medium < long")]
    InvalidTrajectoryWindowOrder,
    #[error("HET-PM V2 maximum newest sample age must be greater than zero")]
    InvalidMaximumSampleAge,
    #[error("HET-PM V2 trailing arm return must be within 0..=1000000 bps")]
    InvalidTrailingArmReturn,
    #[error("HET-PM V2 trailing mark drawdown must be within 1..=10000 bps")]
    InvalidTrailingMarkDrawdown,
    #[error("HET-PM V2 executable trailing breach must be within 0..=10000 bps")]
    InvalidTrailingExecutableBreach,
    #[error("HET-PM V2 peak anchor step must be within 1..=10000 bps")]
    InvalidPeakAnchorStep,
    #[error("HET-PM V2 forced anchor refresh interval must be greater than zero")]
    InvalidPeakAnchorRefresh,
    #[error("HET-PM V2 vitality minimum age must be greater than zero")]
    InvalidVitalityMinimumAge,
    #[error("HET-PM V2 vitality non-alive window count must be greater than zero")]
    InvalidVitalityWindowCount,
    #[error("HET-PM V2 vitality time-since-peak threshold must be greater than zero")]
    InvalidVitalityPeakAge,
    #[error("HET-PM V2 vitality recovery return must be within 0..=10000 bps")]
    InvalidVitalityRecoveryReturn,
    #[error("HET-PM V2 writer queue capacity must be within 1..=4096")]
    InvalidWriterQueueCapacity,
    #[error("HET-PM V2 terminal write budget must be within 1..=100 ms")]
    InvalidTerminalWriteBudget,
    #[error("HET-PM V2 vitality requires time_stop_v2.enabled=true")]
    VitalitySourceDisabled,
    #[error("HET-PM V2 config could not be serialized for hashing")]
    ConfigHashSerialization,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(super) struct EffectiveHetPmV2Config {
    enabled: bool,
    mode: HetPmV2Mode,
    trajectory_short_ms: u64,
    trajectory_medium_ms: u64,
    trajectory_long_ms: u64,
    max_newest_sample_age_ms: u64,
    trailing_arm_mark_return_bps: i32,
    trailing_mark_candidate_drawdown_bps: u32,
    trailing_executable_breach_bps: u32,
    peak_anchor_min_step_bps: u32,
    peak_anchor_force_refresh_on_new_peak_after_ms: u64,
    vitality_min_age_ms: u64,
    vitality_required_non_alive_windows: u32,
    vitality_min_time_since_peak_ms: u64,
    vitality_recovery_return_bps: i32,
    writer_queue_capacity: usize,
    terminal_write_budget_ms: u64,
    policy_id: &'static str,
    policy_version: u16,
    config_hash: String,
}

impl EffectiveHetPmV2Config {
    pub(super) fn from_guardian(
        guardian: &PostBuyGuardianConfig,
    ) -> Result<Self, HetPmV2ConfigError> {
        let effective = Self::from_config(&guardian.het_pm_v2)?;
        if guardian.het_pm_v2.enabled && !guardian.time_stop_v2.enabled {
            return Err(HetPmV2ConfigError::VitalitySourceDisabled);
        }
        Ok(effective)
    }

    fn from_config(config: &HetPmV2Config) -> Result<Self, HetPmV2ConfigError> {
        if config.trajectory_short_ms == 0
            || config.trajectory_medium_ms == 0
            || config.trajectory_long_ms == 0
        {
            return Err(HetPmV2ConfigError::InvalidTrajectoryWindow);
        }
        if !(config.trajectory_short_ms < config.trajectory_medium_ms
            && config.trajectory_medium_ms < config.trajectory_long_ms)
        {
            return Err(HetPmV2ConfigError::InvalidTrajectoryWindowOrder);
        }
        if config.max_newest_sample_age_ms == 0 {
            return Err(HetPmV2ConfigError::InvalidMaximumSampleAge);
        }
        if !(0..=1_000_000).contains(&config.trailing_arm_mark_return_bps) {
            return Err(HetPmV2ConfigError::InvalidTrailingArmReturn);
        }
        if !(1..=10_000).contains(&config.trailing_mark_candidate_drawdown_bps) {
            return Err(HetPmV2ConfigError::InvalidTrailingMarkDrawdown);
        }
        if config.trailing_executable_breach_bps > 10_000 {
            return Err(HetPmV2ConfigError::InvalidTrailingExecutableBreach);
        }
        if !(1..=10_000).contains(&config.peak_anchor_min_step_bps) {
            return Err(HetPmV2ConfigError::InvalidPeakAnchorStep);
        }
        if config.peak_anchor_force_refresh_on_new_peak_after_ms == 0 {
            return Err(HetPmV2ConfigError::InvalidPeakAnchorRefresh);
        }
        if config.vitality_min_age_ms == 0 {
            return Err(HetPmV2ConfigError::InvalidVitalityMinimumAge);
        }
        if config.vitality_required_non_alive_windows == 0 {
            return Err(HetPmV2ConfigError::InvalidVitalityWindowCount);
        }
        if config.vitality_min_time_since_peak_ms == 0 {
            return Err(HetPmV2ConfigError::InvalidVitalityPeakAge);
        }
        if !(0..=10_000).contains(&config.vitality_recovery_return_bps) {
            return Err(HetPmV2ConfigError::InvalidVitalityRecoveryReturn);
        }
        if !(1..=HET_PM_V2_MAX_WRITER_QUEUE_CAPACITY).contains(&config.writer_queue_capacity) {
            return Err(HetPmV2ConfigError::InvalidWriterQueueCapacity);
        }
        if !(1..=HET_PM_V2_MAX_TERMINAL_WRITE_BUDGET_MS).contains(&config.terminal_write_budget_ms)
        {
            return Err(HetPmV2ConfigError::InvalidTerminalWriteBudget);
        }
        #[derive(Serialize)]
        struct HashInput {
            enabled: bool,
            mode: HetPmV2Mode,
            trajectory_short_ms: u64,
            trajectory_medium_ms: u64,
            trajectory_long_ms: u64,
            max_newest_sample_age_ms: u64,
            trailing_arm_mark_return_bps: i32,
            trailing_mark_candidate_drawdown_bps: u32,
            trailing_executable_breach_bps: u32,
            peak_anchor_min_step_bps: u32,
            peak_anchor_force_refresh_on_new_peak_after_ms: u64,
            vitality_min_age_ms: u64,
            vitality_required_non_alive_windows: u32,
            vitality_min_time_since_peak_ms: u64,
            vitality_recovery_return_bps: i32,
            writer_queue_capacity: usize,
            terminal_write_budget_ms: u64,
        }

        let encoded = serde_json::to_vec(&HashInput {
            enabled: config.enabled,
            mode: config.mode,
            trajectory_short_ms: config.trajectory_short_ms,
            trajectory_medium_ms: config.trajectory_medium_ms,
            trajectory_long_ms: config.trajectory_long_ms,
            max_newest_sample_age_ms: config.max_newest_sample_age_ms,
            trailing_arm_mark_return_bps: config.trailing_arm_mark_return_bps,
            trailing_mark_candidate_drawdown_bps: config.trailing_mark_candidate_drawdown_bps,
            trailing_executable_breach_bps: config.trailing_executable_breach_bps,
            peak_anchor_min_step_bps: config.peak_anchor_min_step_bps,
            peak_anchor_force_refresh_on_new_peak_after_ms: config
                .peak_anchor_force_refresh_on_new_peak_after_ms,
            vitality_min_age_ms: config.vitality_min_age_ms,
            vitality_required_non_alive_windows: config.vitality_required_non_alive_windows,
            vitality_min_time_since_peak_ms: config.vitality_min_time_since_peak_ms,
            vitality_recovery_return_bps: config.vitality_recovery_return_bps,
            writer_queue_capacity: config.writer_queue_capacity,
            terminal_write_budget_ms: config.terminal_write_budget_ms,
        })
        .map_err(|_| HetPmV2ConfigError::ConfigHashSerialization)?;

        Ok(Self {
            enabled: config.enabled,
            mode: config.mode,
            trajectory_short_ms: config.trajectory_short_ms,
            trajectory_medium_ms: config.trajectory_medium_ms,
            trajectory_long_ms: config.trajectory_long_ms,
            max_newest_sample_age_ms: config.max_newest_sample_age_ms,
            trailing_arm_mark_return_bps: config.trailing_arm_mark_return_bps,
            trailing_mark_candidate_drawdown_bps: config.trailing_mark_candidate_drawdown_bps,
            trailing_executable_breach_bps: config.trailing_executable_breach_bps,
            peak_anchor_min_step_bps: config.peak_anchor_min_step_bps,
            peak_anchor_force_refresh_on_new_peak_after_ms: config
                .peak_anchor_force_refresh_on_new_peak_after_ms,
            vitality_min_age_ms: config.vitality_min_age_ms,
            vitality_required_non_alive_windows: config.vitality_required_non_alive_windows,
            vitality_min_time_since_peak_ms: config.vitality_min_time_since_peak_ms,
            vitality_recovery_return_bps: config.vitality_recovery_return_bps,
            writer_queue_capacity: config.writer_queue_capacity,
            terminal_write_budget_ms: config.terminal_write_budget_ms,
            policy_id: HET_PM_V2_POLICY_ID,
            policy_version: HET_PM_V2_POLICY_VERSION,
            config_hash: blake3::hash(&encoded).to_hex().to_string(),
        })
    }

    pub(super) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(super) const fn authoritative_shadow(&self) -> bool {
        matches!(self.mode, HetPmV2Mode::AuthoritativeShadow)
    }
    pub(super) fn trajectory_short_ms(&self) -> u64 {
        self.trajectory_short_ms
    }
    pub(super) fn trajectory_medium_ms(&self) -> u64 {
        self.trajectory_medium_ms
    }
    pub(super) fn trajectory_long_ms(&self) -> u64 {
        self.trajectory_long_ms
    }
    pub(super) fn max_newest_sample_age_ms(&self) -> u64 {
        self.max_newest_sample_age_ms
    }
    pub(super) fn config_hash(&self) -> &str {
        &self.config_hash
    }
    pub(super) const fn writer_queue_capacity(&self) -> usize {
        self.writer_queue_capacity
    }
    pub(super) const fn terminal_write_budget_ms(&self) -> u64 {
        self.terminal_write_budget_ms
    }
    pub(super) fn status(&self, crash_guard_mode: CrashGuardMode) -> HetPmV2Status {
        let v2_shadow_authority = self.authoritative_shadow();
        HetPmV2Status {
            policy_id: self.policy_id.to_string(),
            policy_version: self.policy_version,
            schema_version: HET_PM_V2_SCHEMA_VERSION,
            config_hash: self.config_hash.clone(),
            enabled: self.enabled,
            mode: self.mode,
            sampling_mode: HET_PM_V2_SAMPLING_MODE.to_string(),
            trajectory_grade: HET_PM_V2_TRAJECTORY_GRADE.to_string(),
            trajectory_windows_ms: [
                self.trajectory_short_ms,
                self.trajectory_medium_ms,
                self.trajectory_long_ms,
            ],
            max_newest_sample_age_ms: self.max_newest_sample_age_ms,
            trailing_arm_mark_return_bps: self.trailing_arm_mark_return_bps,
            trailing_mark_candidate_drawdown_bps: self.trailing_mark_candidate_drawdown_bps,
            trailing_executable_breach_bps: self.trailing_executable_breach_bps,
            peak_anchor_min_step_bps: self.peak_anchor_min_step_bps,
            peak_anchor_force_refresh_on_new_peak_after_ms: self
                .peak_anchor_force_refresh_on_new_peak_after_ms,
            vitality_min_age_ms: self.vitality_min_age_ms,
            vitality_required_non_alive_windows: self.vitality_required_non_alive_windows,
            vitality_min_time_since_peak_ms: self.vitality_min_time_since_peak_ms,
            vitality_recovery_return_bps: self.vitality_recovery_return_bps,
            writer_queue_capacity: self.writer_queue_capacity,
            terminal_write_budget_ms: self.terminal_write_budget_ms,
            crash_guard_mode,
            crash_guard_mode_source: "effective_exit_policy_v1_config".to_string(),
            v1_shadow_authority: !v2_shadow_authority,
            v2_shadow_authority,
            live_authority: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HetPmV2Status {
    pub policy_id: String,
    pub policy_version: u16,
    pub schema_version: u16,
    pub config_hash: String,
    pub enabled: bool,
    pub mode: HetPmV2Mode,
    pub sampling_mode: String,
    pub trajectory_grade: String,
    pub trajectory_windows_ms: [u64; 3],
    pub max_newest_sample_age_ms: u64,
    pub trailing_arm_mark_return_bps: i32,
    pub trailing_mark_candidate_drawdown_bps: u32,
    pub trailing_executable_breach_bps: u32,
    pub peak_anchor_min_step_bps: u32,
    pub peak_anchor_force_refresh_on_new_peak_after_ms: u64,
    pub vitality_min_age_ms: u64,
    pub vitality_required_non_alive_windows: u32,
    pub vitality_min_time_since_peak_ms: u64,
    pub vitality_recovery_return_bps: i32,
    pub writer_queue_capacity: usize,
    pub terminal_write_budget_ms: u64,
    pub crash_guard_mode: CrashGuardMode,
    pub crash_guard_mode_source: String,
    pub v1_shadow_authority: bool,
    pub v2_shadow_authority: bool,
    pub live_authority: bool,
}

pub fn validate_het_pm_v2_config(
    guardian: &PostBuyGuardianConfig,
) -> Result<HetPmV2Status, HetPmV2ConfigError> {
    EffectiveHetPmV2Config::from_guardian(guardian)
        .map(|cfg| cfg.status(guardian.exit_policy_v1.crash_guard_mode))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum VitalityStateV1 {
    Alive,
    Weak,
    HeartbeatOnly,
    StaleOrUnknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TimeStopV2WindowStatus {
    Alive,
    Weak,
    Heartbeat,
    StaleOrInsufficient,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TimeStopV2Subreason {
    AliveMeaningfulProgress,
    LowVitalityNoMeaningfulProgress,
    MicroTxHeartbeatNoPriceProgress,
    StaleOrMissingMarketSample,
    MissingMarketSample,
    InvalidMarketSample,
    NoNewMarketSample,
    MixedFailedVitalityWindows,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct TimeStopV2ProjectionV1 {
    pub(super) current_status: TimeStopV2WindowStatus,
    pub(super) current_subreason: TimeStopV2Subreason,
    pub(super) consecutive_non_alive_windows: u32,
    pub(super) last_window_at_ms: Option<u64>,
    pub(super) last_alive_at_ms: Option<u64>,
    pub(super) latest_window_price_delta_bps: Option<i32>,
    pub(super) latest_window_state_update_delta: Option<u64>,
    pub(super) source_window_index: Option<u32>,
    pub(super) source_checkpoint_slot: Option<u64>,
    pub(super) source_latest_slot: Option<u64>,
    pub(super) quality_fresh: bool,
}

impl Default for TimeStopV2ProjectionV1 {
    fn default() -> Self {
        Self {
            current_status: TimeStopV2WindowStatus::StaleOrInsufficient,
            current_subreason: TimeStopV2Subreason::MissingMarketSample,
            consecutive_non_alive_windows: 0,
            last_window_at_ms: None,
            last_alive_at_ms: None,
            latest_window_price_delta_bps: None,
            latest_window_state_update_delta: None,
            source_window_index: None,
            source_checkpoint_slot: None,
            source_latest_slot: None,
            quality_fresh: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct VitalityFeaturesV1 {
    pub(super) current_state: VitalityStateV1,
    pub(super) consecutive_non_alive_windows: u32,
    pub(super) last_window_at_ms: Option<u64>,
    pub(super) last_alive_at_ms: Option<u64>,
    pub(super) latest_window_price_delta_bps: Option<i32>,
    pub(super) latest_window_state_update_delta: Option<u64>,
    pub(super) quality_fresh: bool,
}

impl From<&TimeStopV2ProjectionV1> for VitalityFeaturesV1 {
    fn from(value: &TimeStopV2ProjectionV1) -> Self {
        Self {
            current_state: match value.current_status {
                TimeStopV2WindowStatus::Alive => VitalityStateV1::Alive,
                TimeStopV2WindowStatus::Weak => VitalityStateV1::Weak,
                TimeStopV2WindowStatus::Heartbeat => VitalityStateV1::HeartbeatOnly,
                TimeStopV2WindowStatus::StaleOrInsufficient => VitalityStateV1::StaleOrUnknown,
            },
            consecutive_non_alive_windows: value.consecutive_non_alive_windows,
            last_window_at_ms: value.last_window_at_ms,
            last_alive_at_ms: value.last_alive_at_ms,
            latest_window_price_delta_bps: value.latest_window_price_delta_bps,
            latest_window_state_update_delta: value.latest_window_state_update_delta,
            quality_fresh: value.quality_fresh,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RouteStatusV1 {
    PumpCurveSupported,
    CurveCompletePumpSwapUnsupported,
    Unknown,
}

impl RouteStatusV1 {
    pub(super) fn route_id(self) -> &'static str {
        match self {
            Self::PumpCurveSupported => HET_PM_V2_PUMP_ROUTE_ID,
            Self::CurveCompletePumpSwapUnsupported => "pumpswap_unsupported",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum EntryValueSourceV1 {
    PersistedEntryAmount,
    DiagnosticPriceTimesQuantityFallback,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ExecutablePeakAnchorV1 {
    pub(super) position_id: String,
    pub(super) position_epoch: u64,
    pub(super) remaining_quantity_raw: u64,
    pub(super) route_id: String,
    pub(super) quote_model_id: String,
    pub(super) policy_config_hash: String,
    pub(super) quote_state_revision: u64,
    pub(super) source_snapshot_id: String,
    pub(super) source_sample_slot: Option<u64>,
    pub(super) source_sample_timestamp_ms: Option<u64>,
    pub(super) peak_mark_price_sol: f64,
    pub(super) executable_value_quote_raw: Option<u64>,
    pub(super) executable_value_sol: f64,
    pub(super) executable_gross_return_bps: Option<i32>,
    pub(super) anchor_seq: u64,
    pub(super) created_at_ms: u64,
}

#[derive(Debug, Clone)]
pub(super) struct PostBuyDecisionExtrasV2 {
    pub(super) run_id: String,
    pub(super) trajectory: TrajectoryFeaturesV1,
    pub(super) vitality: VitalityFeaturesV1,
    pub(super) route_status: RouteStatusV1,
    pub(super) executable_peak_anchor: Option<ExecutablePeakAnchorV1>,
    pub(super) entry_value_quote_raw: Option<u64>,
    pub(super) entry_value_source: EntryValueSourceV1,
    pub(super) entry_value_authoritative_for_shadow: bool,
}

#[derive(Debug, Clone)]
pub(super) struct PostBuySnapshotBundle {
    pub(super) base: PostBuyDecisionSnapshot,
    pub(super) v2: PostBuyDecisionExtrasV2,
}

#[derive(Clone, Copy)]
pub(super) struct PostBuyDecisionViewV2<'a> {
    pub(super) base: &'a PostBuyDecisionSnapshot,
    pub(super) extras: &'a PostBuyDecisionExtrasV2,
}

impl PostBuySnapshotBundle {
    pub(super) fn view(&self) -> PostBuyDecisionViewV2<'_> {
        PostBuyDecisionViewV2 {
            base: &self.base,
            extras: &self.v2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum HetPmGateV2 {
    Pending,
    Integrity,
    Crash,
    HardLoss,
    ExecutableTrailing,
    VitalityDecay,
    AbsoluteMaxHold,
    Hold,
}

impl HetPmGateV2 {
    fn bit(self) -> u16 {
        1 << self as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum HetPmExitReasonV2 {
    Crash,
    HardLoss,
    ExecutableTrailing,
    VitalityDecay,
    AbsoluteMaxHold,
}

impl HetPmExitReasonV2 {
    /// Map the V2 decision to the existing guarded full-position execution
    /// envelope.  The envelope owns retries, fill application and terminal
    /// cleanup; it does not choose this reason.
    pub(super) const fn exit_candidate_reason(self) -> ExitCandidateReason {
        match self {
            Self::Crash => ExitCandidateReason::CrashGuard,
            Self::HardLoss => ExitCandidateReason::StopLoss,
            Self::ExecutableTrailing => ExitCandidateReason::ExecutableTrailing,
            Self::VitalityDecay => ExitCandidateReason::VitalityDecay,
            Self::AbsoluteMaxHold => ExitCandidateReason::AbsoluteMaxHold,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum HetPmUnknownReasonV2 {
    PolicyDisabled,
    InvalidPositionContract,
    EntryCapitalUnavailable,
    MarkUnavailable,
    MarkStale,
    MarkInvalid,
    TrajectoryUnavailable,
    TrajectoryStale,
    TrajectoryInvalid,
    RouteUnsupported,
    RouteUnknown,
    VitalityEvidenceStale,
    AnchorUnavailable,
    AnchorPositionMismatch,
    AnchorEpochMismatch,
    AnchorQuantityMismatch,
    AnchorRouteMismatch,
    AnchorQuoteModelMismatch,
    AnchorPolicyConfigMismatch,
    AnchorRevisionAhead,
    QuoteUnavailable,
    QuoteQuantityMismatch,
    QuoteInvalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub(super) enum HetPmCandidateV2 {
    Hold,
    Pending,
    Blocked(HetPmUnknownReasonV2),
    QuoteRequired(HetPmExitReasonV2),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HetPmPreQuoteEvaluationV2 {
    pub(super) candidate: HetPmCandidateV2,
    pub(super) winning_gate: HetPmGateV2,
    pub(super) suppressed_gates_mask: u16,
}

/// Pure pre-quote result for one exit gate in the same immutable decision
/// snapshot.  It deliberately records lower-priority candidates too: an
/// observe-only higher gate must not erase the evidence needed to assess a
/// selectively promoted lower gate later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HetPmGatePreQuoteEvidenceV2 {
    pub(super) reason: HetPmExitReasonV2,
    pub(super) candidate: HetPmCandidateV2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum HetPmGateQuoteStatusV2 {
    NotRequired,
    Pending,
    BlockedPreQuote,
    QuoteUnavailable,
    Resolved,
    RejectedByQuote,
    BlockedByData,
    BlockedAfterQuote,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct HetPmGateEvaluationV2 {
    pub(super) gate: HetPmExitReasonV2,
    pub(super) prequote: HetPmCandidateV2,
    pub(super) final_decision: HetPmFinalDecisionV2,
    pub(super) quote_status: HetPmGateQuoteStatusV2,
    pub(super) executable_gross_return_bps: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(super) struct ExecutableQuoteKeyV2 {
    pub(super) position_id: String,
    pub(super) position_epoch: u64,
    pub(super) state_revision: u64,
    pub(super) remaining_quantity_raw: u64,
    pub(super) route_id: String,
    pub(super) quote_model_id: String,
    pub(super) sample_slot: Option<u64>,
    pub(super) sample_timestamp_ms: Option<u64>,
}

impl ExecutableQuoteKeyV2 {
    pub(super) fn from_view(view: PostBuyDecisionViewV2<'_>) -> Self {
        Self {
            position_id: view.base.guard().position_id().to_string(),
            position_epoch: view.base.guard().position_epoch(),
            state_revision: view.base.guard().state_revision(),
            remaining_quantity_raw: view.base.remaining_token_amount_raw(),
            route_id: view.extras.route_status.route_id().to_string(),
            quote_model_id: HET_PM_V2_QUOTE_MODEL_ID.to_string(),
            sample_slot: view.extras.trajectory.newest_sample_slot,
            sample_timestamp_ms: view.extras.trajectory.newest_sample_timestamp_ms,
        }
    }

    pub(super) fn stable_label(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}:{}:{}:{}",
            self.position_id,
            self.position_epoch,
            self.state_revision,
            self.remaining_quantity_raw,
            self.route_id,
            self.quote_model_id,
            self.sample_slot
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            self.sample_timestamp_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum PeakAnchorPreQuoteDecisionV1 {
    NoChange,
    QuoteRequired {
        key: ExecutableQuoteKeyV2,
        peak_mark_price_sol: f64,
        source_snapshot_id: String,
    },
    Blocked {
        reason: HetPmUnknownReasonV2,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub(super) enum HetPmFinalDecisionV2 {
    Hold,
    Pending,
    Blocked(HetPmUnknownReasonV2),
    CrashRejectedByQuote {
        reason: CrashGuardQuoteRejectionReason,
    },
    CrashBlockedByData,
    ExitAll {
        reason: HetPmExitReasonV2,
        quantity_raw: u64,
        executable_gross_return_bps: i32,
    },
}

pub(super) struct ExitPolicyV2;

pub(super) struct HetPmQuoteFinalizationInputV2<'a> {
    pub(super) quote: Option<&'a ExecutableExitQuote>,
    pub(super) quote_key: Option<&'a ExecutableQuoteKeyV2>,
    pub(super) quote_evidence: Option<QuoteEvidenceRevisionV1>,
    pub(super) crash_requirement: Option<&'a CrashGuardQuoteRequirementV1>,
}

impl ExitPolicyV2 {
    fn mark_return_bps(entry_price: f64, mark_price: f64) -> i32 {
        if entry_price > 0.0 && mark_price.is_finite() && mark_price > 0.0 {
            (10_000.0 * (mark_price / entry_price - 1.0))
                .round()
                .clamp(i32::MIN as f64, i32::MAX as f64) as i32
        } else {
            i32::MIN
        }
    }

    fn trailing_arm_mark_return_bps(view: PostBuyDecisionViewV2<'_>) -> i32 {
        let entry_price = view.base.entry_price_sol().unwrap_or_default();
        match view.extras.trajectory.peak_mark_price_sol {
            Some(peak_mark) => Self::mark_return_bps(entry_price, peak_mark),
            None => i32::MIN,
        }
    }

    // Production always supplies the explicit absolute-max-hold boundary.
    // This convenience wrapper exists solely to keep focused policy tests
    // readable, so do not retain it in the non-test runtime binary.
    #[cfg(test)]
    pub(super) fn evaluate_prequote(
        view: PostBuyDecisionViewV2<'_>,
        v1_prequote: &PreQuoteDecision,
        crash_prequote: &CrashGuardPreQuoteDecision,
        config: &EffectiveHetPmV2Config,
    ) -> HetPmPreQuoteEvaluationV2 {
        let absolute_max_hold_due = matches!(
            v1_prequote,
            PreQuoteDecision::QuoteRequired { candidate }
                if matches!(candidate.reason(), ExitCandidateReason::AbsoluteMaxHold)
        );
        Self::evaluate_prequote_with_absolute_max_hold(
            view,
            v1_prequote,
            crash_prequote,
            config,
            absolute_max_hold_due,
        )
    }

    /// Evaluate the hierarchy with the hard position-age ceiling supplied by
    /// the runtime.  The ceiling is deliberately independent of the ordinary
    /// V1 prequote winner: at its deadline an older V1 take-profit or
    /// inactivity candidate must not hide it, and a V2 data blocker must not
    /// leave the position open indefinitely.
    pub(super) fn evaluate_prequote_with_absolute_max_hold(
        view: PostBuyDecisionViewV2<'_>,
        v1_prequote: &PreQuoteDecision,
        crash_prequote: &CrashGuardPreQuoteDecision,
        config: &EffectiveHetPmV2Config,
        absolute_max_hold_due: bool,
    ) -> HetPmPreQuoteEvaluationV2 {
        let mut suppressed = 0_u16;
        let finish = |candidate, winning_gate, suppressed_gates_mask| HetPmPreQuoteEvaluationV2 {
            candidate,
            winning_gate,
            suppressed_gates_mask,
        };
        let max_hold_or_blocked = |reason, suppressed_gates_mask| {
            if absolute_max_hold_due {
                finish(
                    HetPmCandidateV2::QuoteRequired(HetPmExitReasonV2::AbsoluteMaxHold),
                    HetPmGateV2::AbsoluteMaxHold,
                    suppressed_gates_mask,
                )
            } else {
                finish(
                    HetPmCandidateV2::Blocked(reason),
                    HetPmGateV2::Integrity,
                    suppressed_gates_mask,
                )
            }
        };

        if !config.enabled() {
            return finish(
                HetPmCandidateV2::Blocked(HetPmUnknownReasonV2::PolicyDisabled),
                HetPmGateV2::Integrity,
                0,
            );
        }
        if view.base.has_pending_proposal() {
            return finish(HetPmCandidateV2::Pending, HetPmGateV2::Pending, 0);
        }
        suppressed |= HetPmGateV2::Pending.bit();

        if !matches!(view.base.lane(), Lane::Shadow)
            || view.base.remaining_token_amount_raw() == 0
            || view.base.entry_token_amount_raw() == 0
        {
            return finish(
                HetPmCandidateV2::Blocked(HetPmUnknownReasonV2::InvalidPositionContract),
                HetPmGateV2::Integrity,
                suppressed,
            );
        }
        if view.extras.entry_value_quote_raw.is_none() {
            return max_hold_or_blocked(HetPmUnknownReasonV2::EntryCapitalUnavailable, suppressed);
        }
        // AbsoluteMaxHold can bypass missing *decision data* so a position
        // does not remain open forever merely because trajectory/vitality
        // evidence degraded.  It must never bypass execution capability:
        // this runtime has no PumpSwap route, so a migrated or unknown route
        // cannot be represented as a simulated Pump-curve fill.
        match view.extras.route_status {
            RouteStatusV1::CurveCompletePumpSwapUnsupported => {
                return finish(
                    HetPmCandidateV2::Blocked(HetPmUnknownReasonV2::RouteUnsupported),
                    HetPmGateV2::Integrity,
                    suppressed,
                );
            }
            RouteStatusV1::Unknown => {
                return finish(
                    HetPmCandidateV2::Blocked(HetPmUnknownReasonV2::RouteUnknown),
                    HetPmGateV2::Integrity,
                    suppressed,
                );
            }
            RouteStatusV1::PumpCurveSupported => {}
        }
        let mark_blocker = match view.base.mark_evidence_status() {
            MarkEvidenceStatus::Available => None,
            MarkEvidenceStatus::Unavailable => Some(HetPmUnknownReasonV2::MarkUnavailable),
            MarkEvidenceStatus::Stale => Some(HetPmUnknownReasonV2::MarkStale),
            MarkEvidenceStatus::Invalid => Some(HetPmUnknownReasonV2::MarkInvalid),
        };
        if let Some(reason) = mark_blocker {
            return max_hold_or_blocked(reason, suppressed);
        }
        match view.extras.trajectory.quality {
            TrajectoryQualityV1::Invalid => {
                return max_hold_or_blocked(HetPmUnknownReasonV2::TrajectoryInvalid, suppressed);
            }
            TrajectoryQualityV1::Stale => {
                return max_hold_or_blocked(HetPmUnknownReasonV2::TrajectoryStale, suppressed);
            }
            TrajectoryQualityV1::Unavailable | TrajectoryQualityV1::InsufficientSamples => {
                return max_hold_or_blocked(
                    HetPmUnknownReasonV2::TrajectoryUnavailable,
                    suppressed,
                );
            }
            TrajectoryQualityV1::PartialHistory | TrajectoryQualityV1::Usable => {}
        }
        suppressed |= HetPmGateV2::Integrity.bit();

        if matches!(
            crash_prequote,
            CrashGuardPreQuoteDecision::QuoteRequired { .. }
        ) {
            return finish(
                HetPmCandidateV2::QuoteRequired(HetPmExitReasonV2::Crash),
                HetPmGateV2::Crash,
                suppressed,
            );
        }
        suppressed |= HetPmGateV2::Crash.bit();

        if matches!(
            v1_prequote,
            PreQuoteDecision::QuoteRequired { candidate }
                if matches!(candidate.reason(), ExitCandidateReason::StopLoss)
        ) {
            return finish(
                HetPmCandidateV2::QuoteRequired(HetPmExitReasonV2::HardLoss),
                HetPmGateV2::HardLoss,
                suppressed,
            );
        }
        suppressed |= HetPmGateV2::HardLoss.bit();

        let trailing_mark_candidate = Self::trailing_arm_mark_return_bps(view)
            >= config.trailing_arm_mark_return_bps
            && view
                .extras
                .trajectory
                .drawdown_from_peak_bps
                .is_some_and(|value| value >= config.trailing_mark_candidate_drawdown_bps as i32);
        if trailing_mark_candidate {
            if view.extras.executable_peak_anchor.is_none() {
                return finish(
                    HetPmCandidateV2::Blocked(HetPmUnknownReasonV2::AnchorUnavailable),
                    HetPmGateV2::ExecutableTrailing,
                    suppressed,
                );
            }
            return finish(
                HetPmCandidateV2::QuoteRequired(HetPmExitReasonV2::ExecutableTrailing),
                HetPmGateV2::ExecutableTrailing,
                suppressed,
            );
        }
        suppressed |= HetPmGateV2::ExecutableTrailing.bit();

        let vitality = &view.extras.vitality;
        let vitality_state_candidate = view.base.absolute_age_ms() >= config.vitality_min_age_ms
            && matches!(
                vitality.current_state,
                VitalityStateV1::Weak | VitalityStateV1::HeartbeatOnly
            )
            && vitality.consecutive_non_alive_windows >= config.vitality_required_non_alive_windows;
        if vitality_state_candidate {
            if !vitality.quality_fresh
                || view.extras.trajectory.time_since_peak_ms.is_none()
                || view.extras.trajectory.return_5s_bps.is_none()
            {
                return finish(
                    HetPmCandidateV2::Blocked(HetPmUnknownReasonV2::VitalityEvidenceStale),
                    HetPmGateV2::VitalityDecay,
                    suppressed,
                );
            }
            let recovered = view
                .extras
                .trajectory
                .return_5s_bps
                .is_some_and(|value| value >= config.vitality_recovery_return_bps);
            let peak_too_recent = view
                .extras
                .trajectory
                .time_since_peak_ms
                .is_some_and(|value| value < config.vitality_min_time_since_peak_ms);
            if !recovered && !peak_too_recent {
                return finish(
                    HetPmCandidateV2::QuoteRequired(HetPmExitReasonV2::VitalityDecay),
                    HetPmGateV2::VitalityDecay,
                    suppressed,
                );
            }
        }
        if matches!(vitality.current_state, VitalityStateV1::StaleOrUnknown)
            && view.base.absolute_age_ms() >= config.vitality_min_age_ms
        {
            return finish(
                HetPmCandidateV2::Blocked(HetPmUnknownReasonV2::VitalityEvidenceStale),
                HetPmGateV2::VitalityDecay,
                suppressed,
            );
        }
        suppressed |= HetPmGateV2::VitalityDecay.bit();

        if absolute_max_hold_due {
            return finish(
                HetPmCandidateV2::QuoteRequired(HetPmExitReasonV2::AbsoluteMaxHold),
                HetPmGateV2::AbsoluteMaxHold,
                suppressed,
            );
        }
        suppressed |= HetPmGateV2::AbsoluteMaxHold.bit();
        finish(HetPmCandidateV2::Hold, HetPmGateV2::Hold, suppressed)
    }

    /// Materialize the pure result of every exit gate from one snapshot.
    ///
    /// `evaluate_prequote` intentionally returns only the hierarchy winner for
    /// the PR-A observer.  Promotion evidence needs a second, non-authoritative
    /// view: when e.g. Crash and ExecutableTrailing are both candidates, the
    /// latter must remain observable even though Crash is the observed winner.
    /// No quote is requested here and this function has no mutation surface.
    // See `evaluate_prequote`: the runtime uses the explicit ceiling variant
    // below, while unit tests exercise the default ceiling derivation.
    #[cfg(test)]
    pub(super) fn evaluate_gate_lattice(
        view: PostBuyDecisionViewV2<'_>,
        v1_prequote: &PreQuoteDecision,
        crash_prequote: &CrashGuardPreQuoteDecision,
        config: &EffectiveHetPmV2Config,
    ) -> Vec<HetPmGatePreQuoteEvidenceV2> {
        let absolute_max_hold_due = matches!(
            v1_prequote,
            PreQuoteDecision::QuoteRequired { candidate }
                if matches!(candidate.reason(), ExitCandidateReason::AbsoluteMaxHold)
        );
        Self::evaluate_gate_lattice_with_absolute_max_hold(
            view,
            v1_prequote,
            crash_prequote,
            config,
            absolute_max_hold_due,
        )
    }

    pub(super) fn evaluate_gate_lattice_with_absolute_max_hold(
        view: PostBuyDecisionViewV2<'_>,
        v1_prequote: &PreQuoteDecision,
        crash_prequote: &CrashGuardPreQuoteDecision,
        config: &EffectiveHetPmV2Config,
        absolute_max_hold_due: bool,
    ) -> Vec<HetPmGatePreQuoteEvidenceV2> {
        let route_blocker = match view.extras.route_status {
            RouteStatusV1::PumpCurveSupported => None,
            RouteStatusV1::CurveCompletePumpSwapUnsupported => Some(HetPmCandidateV2::Blocked(
                HetPmUnknownReasonV2::RouteUnsupported,
            )),
            RouteStatusV1::Unknown => Some(HetPmCandidateV2::Blocked(
                HetPmUnknownReasonV2::RouteUnknown,
            )),
        };
        let global = if !config.enabled() {
            Some(HetPmCandidateV2::Blocked(
                HetPmUnknownReasonV2::PolicyDisabled,
            ))
        } else if view.base.has_pending_proposal() {
            Some(HetPmCandidateV2::Pending)
        } else if !matches!(view.base.lane(), Lane::Shadow)
            || view.base.remaining_token_amount_raw() == 0
            || view.base.entry_token_amount_raw() == 0
        {
            Some(HetPmCandidateV2::Blocked(
                HetPmUnknownReasonV2::InvalidPositionContract,
            ))
        } else if view.extras.entry_value_quote_raw.is_none() {
            Some(HetPmCandidateV2::Blocked(
                HetPmUnknownReasonV2::EntryCapitalUnavailable,
            ))
        } else if let Some(route_blocker) = route_blocker {
            Some(route_blocker)
        } else {
            match view.base.mark_evidence_status() {
                MarkEvidenceStatus::Available => match view.extras.trajectory.quality {
                    TrajectoryQualityV1::Invalid => Some(HetPmCandidateV2::Blocked(
                        HetPmUnknownReasonV2::TrajectoryInvalid,
                    )),
                    TrajectoryQualityV1::Stale => Some(HetPmCandidateV2::Blocked(
                        HetPmUnknownReasonV2::TrajectoryStale,
                    )),
                    TrajectoryQualityV1::Unavailable | TrajectoryQualityV1::InsufficientSamples => {
                        Some(HetPmCandidateV2::Blocked(
                            HetPmUnknownReasonV2::TrajectoryUnavailable,
                        ))
                    }
                    TrajectoryQualityV1::PartialHistory | TrajectoryQualityV1::Usable => None,
                },
                MarkEvidenceStatus::Unavailable => Some(HetPmCandidateV2::Blocked(
                    HetPmUnknownReasonV2::MarkUnavailable,
                )),
                MarkEvidenceStatus::Stale => {
                    Some(HetPmCandidateV2::Blocked(HetPmUnknownReasonV2::MarkStale))
                }
                MarkEvidenceStatus::Invalid => {
                    Some(HetPmCandidateV2::Blocked(HetPmUnknownReasonV2::MarkInvalid))
                }
            }
        };
        let reasons = [
            HetPmExitReasonV2::Crash,
            HetPmExitReasonV2::HardLoss,
            HetPmExitReasonV2::ExecutableTrailing,
            HetPmExitReasonV2::VitalityDecay,
            HetPmExitReasonV2::AbsoluteMaxHold,
        ];
        if let Some(candidate) = global {
            return reasons
                .into_iter()
                .map(|reason| HetPmGatePreQuoteEvidenceV2 {
                    reason,
                    // Absolute max hold is the last hard occupancy ceiling
                    // for missing decision data.  It is not an executor and
                    // must not manufacture a Pump-curve exit when the route
                    // is unsupported or unknown.
                    candidate: if matches!(reason, HetPmExitReasonV2::AbsoluteMaxHold)
                        && absolute_max_hold_due
                        && !matches!(
                            candidate,
                            HetPmCandidateV2::Blocked(HetPmUnknownReasonV2::RouteUnsupported)
                                | HetPmCandidateV2::Blocked(HetPmUnknownReasonV2::RouteUnknown)
                        ) {
                        HetPmCandidateV2::QuoteRequired(HetPmExitReasonV2::AbsoluteMaxHold)
                    } else {
                        candidate.clone()
                    },
                })
                .collect();
        }

        let crash = if matches!(
            crash_prequote,
            CrashGuardPreQuoteDecision::QuoteRequired { .. }
        ) {
            HetPmCandidateV2::QuoteRequired(HetPmExitReasonV2::Crash)
        } else {
            HetPmCandidateV2::Hold
        };
        let hard_loss = if matches!(
            v1_prequote,
            PreQuoteDecision::QuoteRequired { candidate }
                if matches!(candidate.reason(), ExitCandidateReason::StopLoss)
        ) {
            HetPmCandidateV2::QuoteRequired(HetPmExitReasonV2::HardLoss)
        } else {
            HetPmCandidateV2::Hold
        };
        let trailing = if Self::trailing_arm_mark_return_bps(view)
            >= config.trailing_arm_mark_return_bps
            && view
                .extras
                .trajectory
                .drawdown_from_peak_bps
                .is_some_and(|value| value >= config.trailing_mark_candidate_drawdown_bps as i32)
        {
            if view.extras.executable_peak_anchor.is_none() {
                HetPmCandidateV2::Blocked(HetPmUnknownReasonV2::AnchorUnavailable)
            } else {
                HetPmCandidateV2::QuoteRequired(HetPmExitReasonV2::ExecutableTrailing)
            }
        } else {
            HetPmCandidateV2::Hold
        };
        let vitality = &view.extras.vitality;
        let vitality_state_candidate = view.base.absolute_age_ms() >= config.vitality_min_age_ms
            && matches!(
                vitality.current_state,
                VitalityStateV1::Weak | VitalityStateV1::HeartbeatOnly
            )
            && vitality.consecutive_non_alive_windows >= config.vitality_required_non_alive_windows;
        let vitality_decay = if vitality_state_candidate {
            if !vitality.quality_fresh
                || view.extras.trajectory.time_since_peak_ms.is_none()
                || view.extras.trajectory.return_5s_bps.is_none()
            {
                HetPmCandidateV2::Blocked(HetPmUnknownReasonV2::VitalityEvidenceStale)
            } else {
                let recovered = view
                    .extras
                    .trajectory
                    .return_5s_bps
                    .is_some_and(|value| value >= config.vitality_recovery_return_bps);
                let peak_too_recent = view
                    .extras
                    .trajectory
                    .time_since_peak_ms
                    .is_some_and(|value| value < config.vitality_min_time_since_peak_ms);
                if !recovered && !peak_too_recent {
                    HetPmCandidateV2::QuoteRequired(HetPmExitReasonV2::VitalityDecay)
                } else {
                    HetPmCandidateV2::Hold
                }
            }
        } else if matches!(vitality.current_state, VitalityStateV1::StaleOrUnknown)
            && view.base.absolute_age_ms() >= config.vitality_min_age_ms
        {
            HetPmCandidateV2::Blocked(HetPmUnknownReasonV2::VitalityEvidenceStale)
        } else {
            HetPmCandidateV2::Hold
        };
        let absolute_max_hold = if absolute_max_hold_due {
            HetPmCandidateV2::QuoteRequired(HetPmExitReasonV2::AbsoluteMaxHold)
        } else {
            HetPmCandidateV2::Hold
        };
        vec![
            HetPmGatePreQuoteEvidenceV2 {
                reason: HetPmExitReasonV2::Crash,
                candidate: crash,
            },
            HetPmGatePreQuoteEvidenceV2 {
                reason: HetPmExitReasonV2::HardLoss,
                candidate: hard_loss,
            },
            HetPmGatePreQuoteEvidenceV2 {
                reason: HetPmExitReasonV2::ExecutableTrailing,
                candidate: trailing,
            },
            HetPmGatePreQuoteEvidenceV2 {
                reason: HetPmExitReasonV2::VitalityDecay,
                candidate: vitality_decay,
            },
            HetPmGatePreQuoteEvidenceV2 {
                reason: HetPmExitReasonV2::AbsoluteMaxHold,
                candidate: absolute_max_hold,
            },
        ]
    }

    pub(super) fn evaluate_anchor_request(
        view: PostBuyDecisionViewV2<'_>,
        now_ms: u64,
        config: &EffectiveHetPmV2Config,
    ) -> PeakAnchorPreQuoteDecisionV1 {
        if !config.enabled() {
            return PeakAnchorPreQuoteDecisionV1::NoChange;
        }
        if !matches!(view.extras.route_status, RouteStatusV1::PumpCurveSupported) {
            return PeakAnchorPreQuoteDecisionV1::Blocked {
                reason: if matches!(
                    view.extras.route_status,
                    RouteStatusV1::CurveCompletePumpSwapUnsupported
                ) {
                    HetPmUnknownReasonV2::RouteUnsupported
                } else {
                    HetPmUnknownReasonV2::RouteUnknown
                },
            };
        }
        if matches!(
            view.extras.trajectory.quality,
            TrajectoryQualityV1::Unavailable
                | TrajectoryQualityV1::Stale
                | TrajectoryQualityV1::Invalid
        ) {
            return PeakAnchorPreQuoteDecisionV1::Blocked {
                reason: HetPmUnknownReasonV2::TrajectoryUnavailable,
            };
        }
        let Some(peak_price) = view.extras.trajectory.peak_mark_price_sol else {
            return PeakAnchorPreQuoteDecisionV1::NoChange;
        };
        let is_newest_peak = view.extras.trajectory.peak_sample_timestamp_ms
            == view.extras.trajectory.newest_sample_timestamp_ms
            && view.extras.trajectory.peak_sample_slot == view.extras.trajectory.newest_sample_slot;
        if !is_newest_peak {
            return PeakAnchorPreQuoteDecisionV1::NoChange;
        }
        let should_refresh = match view.extras.executable_peak_anchor.as_ref() {
            None => true,
            Some(anchor) if peak_price <= anchor.peak_mark_price_sol => false,
            Some(anchor) => {
                let step_bps = 10_000.0 * (peak_price / anchor.peak_mark_price_sol - 1.0);
                step_bps >= config.peak_anchor_min_step_bps as f64
                    || now_ms.saturating_sub(anchor.created_at_ms)
                        >= config.peak_anchor_force_refresh_on_new_peak_after_ms
            }
        };
        if !should_refresh {
            return PeakAnchorPreQuoteDecisionV1::NoChange;
        }
        PeakAnchorPreQuoteDecisionV1::QuoteRequired {
            key: ExecutableQuoteKeyV2::from_view(view),
            peak_mark_price_sol: peak_price,
            source_snapshot_id: view.base.snapshot_id().to_string(),
        }
    }

    /// Select the actual shadow exit from the already finalized lattice.
    ///
    /// The vector is in the fixed hierarchy order.  A Crash observation that
    /// has no authority is skipped, not converted into Hold and not allowed to
    /// suppress a lower active rule such as executable trailing.
    pub(super) fn select_authoritative_exit(
        evaluations: &[HetPmGateEvaluationV2],
        crash_guard_mode: CrashGuardMode,
    ) -> Option<HetPmExitReasonV2> {
        evaluations.iter().find_map(|evaluation| {
            let HetPmFinalDecisionV2::ExitAll { reason, .. } = evaluation.final_decision else {
                return None;
            };
            if matches!(reason, HetPmExitReasonV2::Crash)
                && !matches!(crash_guard_mode, CrashGuardMode::AuthoritativeShadow)
            {
                return None;
            }
            Some(reason)
        })
    }

    pub(super) fn finalize_with_quote(
        view: PostBuyDecisionViewV2<'_>,
        prequote: &HetPmPreQuoteEvaluationV2,
        resolution: HetPmQuoteFinalizationInputV2<'_>,
        v1_config: &EffectiveExitPolicyV1Config,
        config: &EffectiveHetPmV2Config,
    ) -> HetPmFinalDecisionV2 {
        let HetPmQuoteFinalizationInputV2 {
            quote,
            quote_key,
            quote_evidence,
            crash_requirement,
        } = resolution;
        match &prequote.candidate {
            HetPmCandidateV2::Hold => HetPmFinalDecisionV2::Hold,
            HetPmCandidateV2::Pending => HetPmFinalDecisionV2::Pending,
            HetPmCandidateV2::Blocked(reason) => HetPmFinalDecisionV2::Blocked(*reason),
            HetPmCandidateV2::QuoteRequired(reason) => {
                let route_blocker = match view.extras.route_status {
                    RouteStatusV1::PumpCurveSupported => None,
                    RouteStatusV1::CurveCompletePumpSwapUnsupported => {
                        Some(HetPmUnknownReasonV2::RouteUnsupported)
                    }
                    RouteStatusV1::Unknown => Some(HetPmUnknownReasonV2::RouteUnknown),
                };
                if let Some(reason) = route_blocker {
                    return HetPmFinalDecisionV2::Blocked(reason);
                }
                let Some(quote) = quote else {
                    return if matches!(reason, HetPmExitReasonV2::Crash) {
                        HetPmFinalDecisionV2::CrashBlockedByData
                    } else {
                        HetPmFinalDecisionV2::Blocked(HetPmUnknownReasonV2::QuoteUnavailable)
                    };
                };
                let Some(key) = quote_key else {
                    return if matches!(reason, HetPmExitReasonV2::Crash) {
                        HetPmFinalDecisionV2::CrashBlockedByData
                    } else {
                        HetPmFinalDecisionV2::Blocked(HetPmUnknownReasonV2::QuoteUnavailable)
                    };
                };
                if matches!(reason, HetPmExitReasonV2::Crash) {
                    let (Some(quote_evidence), Some(requirement)) =
                        (quote_evidence, crash_requirement)
                    else {
                        return HetPmFinalDecisionV2::CrashBlockedByData;
                    };
                    return match ExitPolicyV1::evaluate_crash_guard_quote(
                        view.base,
                        quote,
                        quote_evidence,
                        requirement,
                        v1_config,
                    ) {
                        CrashGuardQuoteDecision::Confirmed => HetPmFinalDecisionV2::ExitAll {
                            reason: HetPmExitReasonV2::Crash,
                            quantity_raw: quote.quantity_raw(),
                            executable_gross_return_bps: (quote.gross_return_pct() * 100.0)
                                .round()
                                .clamp(i32::MIN as f64, i32::MAX as f64)
                                as i32,
                        },
                        CrashGuardQuoteDecision::RejectedByQuote { reason } => {
                            HetPmFinalDecisionV2::CrashRejectedByQuote { reason }
                        }
                        CrashGuardQuoteDecision::BlockedByData => {
                            HetPmFinalDecisionV2::CrashBlockedByData
                        }
                    };
                }
                if key.remaining_quantity_raw != view.base.remaining_token_amount_raw()
                    || quote.quantity_raw() != view.base.remaining_token_amount_raw()
                {
                    return HetPmFinalDecisionV2::Blocked(
                        HetPmUnknownReasonV2::QuoteQuantityMismatch,
                    );
                }
                if !quote.exit_value_sol().is_finite() || quote.exit_value_sol() <= 0.0 {
                    return HetPmFinalDecisionV2::Blocked(HetPmUnknownReasonV2::QuoteInvalid);
                }
                if matches!(reason, HetPmExitReasonV2::ExecutableTrailing) {
                    let Some(anchor) = view.extras.executable_peak_anchor.as_ref() else {
                        return HetPmFinalDecisionV2::Blocked(
                            HetPmUnknownReasonV2::AnchorUnavailable,
                        );
                    };
                    if let Err(reason) = comparable_anchor(anchor, key, config.config_hash()) {
                        return HetPmFinalDecisionV2::Blocked(reason);
                    }
                    if !anchor.executable_value_sol.is_finite()
                        || anchor.executable_value_sol <= 0.0
                    {
                        return HetPmFinalDecisionV2::Blocked(
                            HetPmUnknownReasonV2::AnchorUnavailable,
                        );
                    }
                    let drawdown_bps =
                        10_000.0 * (1.0 - quote.exit_value_sol() / anchor.executable_value_sol);
                    if !drawdown_bps.is_finite()
                        || drawdown_bps < config.trailing_executable_breach_bps as f64
                    {
                        return HetPmFinalDecisionV2::Hold;
                    }
                }
                HetPmFinalDecisionV2::ExitAll {
                    reason: *reason,
                    quantity_raw: quote.quantity_raw(),
                    executable_gross_return_bps: (quote.gross_return_pct() * 100.0)
                        .round()
                        .clamp(i32::MIN as f64, i32::MAX as f64)
                        as i32,
                }
            }
        }
    }

    pub(super) const fn gate_quote_status(
        prequote: &HetPmCandidateV2,
        final_decision: &HetPmFinalDecisionV2,
    ) -> HetPmGateQuoteStatusV2 {
        match prequote {
            HetPmCandidateV2::Hold => HetPmGateQuoteStatusV2::NotRequired,
            HetPmCandidateV2::Pending => HetPmGateQuoteStatusV2::Pending,
            HetPmCandidateV2::Blocked(_) => HetPmGateQuoteStatusV2::BlockedPreQuote,
            HetPmCandidateV2::QuoteRequired(_) => match final_decision {
                HetPmFinalDecisionV2::ExitAll { .. } | HetPmFinalDecisionV2::Hold => {
                    HetPmGateQuoteStatusV2::Resolved
                }
                HetPmFinalDecisionV2::CrashRejectedByQuote { .. } => {
                    HetPmGateQuoteStatusV2::RejectedByQuote
                }
                HetPmFinalDecisionV2::CrashBlockedByData => HetPmGateQuoteStatusV2::BlockedByData,
                HetPmFinalDecisionV2::Blocked(HetPmUnknownReasonV2::QuoteUnavailable) => {
                    HetPmGateQuoteStatusV2::QuoteUnavailable
                }
                HetPmFinalDecisionV2::Blocked(_) => HetPmGateQuoteStatusV2::BlockedAfterQuote,
                HetPmFinalDecisionV2::Pending => HetPmGateQuoteStatusV2::Pending,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum V1AuthorityTickOutcomeV1 {
    Hold,
    ProposalStarted,
    ExitApplied,
    PendingRecovery,
    Blocked,
    ApplyRejected,
}

impl V1AuthorityTickOutcomeV1 {
    pub(super) const fn as_label(self) -> &'static str {
        match self {
            Self::Hold => "Hold",
            Self::ProposalStarted => "ProposalStarted",
            Self::ExitApplied => "ExitApplied",
            Self::PendingRecovery => "PendingRecovery",
            Self::Blocked => "Blocked",
            Self::ApplyRejected => "ApplyRejected",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum V1ExitApplyStatusV1 {
    NotApplied,
    Applied,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum V1TerminalCommitStatusV1 {
    NotRequired,
    Pending,
    Committed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct V1AuthorityTickReceiptV1 {
    pub(super) snapshot_id: String,
    pub(super) state_revision: u64,
    pub(super) remaining_quantity_raw: u64,
    pub(super) outcome: V1AuthorityTickOutcomeV1,
    pub(super) exit_apply_status: V1ExitApplyStatusV1,
    /// Terminal persistence state observed when this comparison was finalized.
    /// The final canonical outcome is joined later through comparison/action IDs.
    pub(super) terminal_commit_status: V1TerminalCommitStatusV1,
    pub(super) action_id: Option<String>,
    pub(super) reason: Option<String>,
    pub(super) crash_quote_decision: Option<CrashGuardQuoteDecision>,
}

fn comparable_anchor(
    anchor: &ExecutablePeakAnchorV1,
    key: &ExecutableQuoteKeyV2,
    policy_config_hash: &str,
) -> Result<(), HetPmUnknownReasonV2> {
    if anchor.position_id != key.position_id {
        return Err(HetPmUnknownReasonV2::AnchorPositionMismatch);
    }
    if anchor.position_epoch != key.position_epoch {
        return Err(HetPmUnknownReasonV2::AnchorEpochMismatch);
    }
    if anchor.remaining_quantity_raw != key.remaining_quantity_raw {
        return Err(HetPmUnknownReasonV2::AnchorQuantityMismatch);
    }
    if anchor.route_id != key.route_id {
        return Err(HetPmUnknownReasonV2::AnchorRouteMismatch);
    }
    if anchor.quote_model_id != key.quote_model_id {
        return Err(HetPmUnknownReasonV2::AnchorQuoteModelMismatch);
    }
    if anchor.policy_config_hash != policy_config_hash {
        return Err(HetPmUnknownReasonV2::AnchorPolicyConfigMismatch);
    }
    if anchor.quote_state_revision > key.state_revision {
        return Err(HetPmUnknownReasonV2::AnchorRevisionAhead);
    }
    Ok(())
}

pub(super) fn build_entry_value_contract(
    entry_size_lamports: u64,
    entry_price_sol: Option<f64>,
    entry_token_amount_raw: u64,
) -> (Option<u64>, EntryValueSourceV1, bool) {
    if entry_size_lamports > 0 {
        return (
            Some(entry_size_lamports),
            EntryValueSourceV1::PersistedEntryAmount,
            true,
        );
    }
    let fallback = entry_price_sol.and_then(|price| {
        let value_lamports =
            price * (entry_token_amount_raw as f64 / 1_000_000.0) * 1_000_000_000.0;
        (value_lamports.is_finite() && value_lamports > 0.0 && value_lamports <= u64::MAX as f64)
            .then_some(value_lamports.round() as u64)
    });
    match fallback {
        Some(value) => (
            Some(value),
            EntryValueSourceV1::DiagnosticPriceTimesQuantityFallback,
            false,
        ),
        None => (None, EntryValueSourceV1::Unavailable, false),
    }
}

pub(super) fn materialize_anchor(
    request: &PeakAnchorPreQuoteDecisionV1,
    quote: &ExecutableExitQuote,
    entry_value_quote_raw: Option<u64>,
    next_anchor_seq: u64,
    now_ms: u64,
    policy_config_hash: &str,
) -> Result<ExecutablePeakAnchorV1, HetPmUnknownReasonV2> {
    let PeakAnchorPreQuoteDecisionV1::QuoteRequired {
        key,
        peak_mark_price_sol,
        source_snapshot_id,
    } = request
    else {
        return Err(HetPmUnknownReasonV2::AnchorUnavailable);
    };
    if quote.quantity_raw() != key.remaining_quantity_raw {
        return Err(HetPmUnknownReasonV2::QuoteQuantityMismatch);
    }
    if !quote.exit_value_sol().is_finite() || quote.exit_value_sol() <= 0.0 {
        return Err(HetPmUnknownReasonV2::QuoteInvalid);
    }
    let executable_gross_return_bps = entry_value_quote_raw.and_then(|entry_raw| {
        let entry_sol = entry_raw as f64 / 1_000_000_000.0;
        (entry_sol > 0.0).then(|| {
            (10_000.0 * (quote.exit_value_sol() / entry_sol - 1.0))
                .round()
                .clamp(i32::MIN as f64, i32::MAX as f64) as i32
        })
    });
    Ok(ExecutablePeakAnchorV1 {
        position_id: key.position_id.clone(),
        position_epoch: key.position_epoch,
        remaining_quantity_raw: key.remaining_quantity_raw,
        route_id: key.route_id.clone(),
        quote_model_id: key.quote_model_id.clone(),
        policy_config_hash: policy_config_hash.to_string(),
        quote_state_revision: key.state_revision,
        source_snapshot_id: source_snapshot_id.clone(),
        source_sample_slot: key.sample_slot,
        source_sample_timestamp_ms: key.sample_timestamp_ms,
        peak_mark_price_sol: *peak_mark_price_sol,
        executable_value_quote_raw: None,
        executable_value_sol: quote.exit_value_sol(),
        executable_gross_return_bps,
        anchor_seq: next_anchor_seq,
        created_at_ms: now_ms,
    })
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct V1V2ComparisonRecord {
    pub(super) schema_version: u16,
    pub(super) comparison_id: String,
    pub(super) policy_id: String,
    pub(super) policy_version: u16,
    pub(super) policy_config_hash: String,
    pub(super) v1_policy_id: String,
    pub(super) v1_policy_version: u16,
    pub(super) v1_policy_config_hash: String,
    pub(super) time_stop_v2_config_hash: String,
    pub(super) run_id: String,
    pub(super) writer_instance_id: String,
    pub(super) lane: Lane,
    pub(super) position_id: String,
    pub(super) position_epoch: u64,
    pub(super) state_revision: u64,
    pub(super) remaining_quantity_raw: u64,
    pub(super) snapshot_id: String,
    pub(super) observation_timestamp_ms: u64,
    pub(super) terminal_tick: bool,
    pub(super) trajectory_sampling_mode: String,
    pub(super) trajectory_measurement_grade: String,
    pub(super) monitor_tick_ms: u64,
    pub(super) v1_prequote: String,
    pub(super) v1_crash_prequote: String,
    pub(super) v1_final: Option<String>,
    pub(super) v1_authority_receipt: Option<V1AuthorityTickReceiptV1>,
    pub(super) v2_prequote: String,
    pub(super) v2_final: Option<String>,
    /// Exact shared-envelope reason selected by an authoritative V2 tick.
    ///
    /// `v2_winning_gate` intentionally remains the pure hierarchy result for
    /// diagnostics.  It can therefore be a higher typed blocker while a lower
    /// hard ceiling (for example AbsoluteMaxHold) is the actual V2 action.
    /// This field records only the candidate that V2 handed to the guarded
    /// executor on this snapshot, so finalization never has to infer ownership
    /// from the diagnostic winner or from a shared V1 reason label.
    pub(super) v2_selected_execution_reason: Option<String>,
    pub(super) v2_crash_quote_decision: Option<CrashGuardQuoteDecision>,
    pub(super) v2_winning_gate: HetPmGateV2,
    pub(super) v2_suppressed_gates_mask: u16,
    /// Additive Schema V3 lattice: one pure evaluation for each exit gate on
    /// this exact snapshot and quote cell.  `v2_winning_gate` remains the
    /// observed pure-hierarchy winner used by PR-A diagnostics.
    pub(super) v2_gate_evaluations: Vec<HetPmGateEvaluationV2>,
    pub(super) consumed_by_policy: bool,
    pub(super) v1_shadow_authority: bool,
    pub(super) v2_shadow_authority: bool,
    pub(super) live_authority: bool,
    pub(super) v2_economic_mutation: bool,
    pub(super) v2_proposal_created: bool,
    pub(super) v2_time_stop_mutation: bool,
    pub(super) duplicate_action_observed: bool,
    pub(super) route_build_authority_changed: bool,
    pub(super) terminal_isolation_violation: bool,
    pub(super) trajectory: TrajectoryFeaturesV1,
    pub(super) vitality: VitalityFeaturesV1,
    pub(super) route_status: RouteStatusV1,
    pub(super) entry_value_quote_raw: Option<u64>,
    pub(super) entry_value_source: EntryValueSourceV1,
    pub(super) entry_value_authoritative_for_shadow: bool,
    pub(super) anchor_before: Option<ExecutablePeakAnchorV1>,
    pub(super) anchor_request: Option<String>,
    pub(super) anchor_applied: bool,
    pub(super) quote_keys: Vec<String>,
    pub(super) quote_resolution_count: u8,
    pub(super) quote_statuses: Vec<String>,
    pub(super) current_executable_value_sol: Option<f64>,
    pub(super) current_executable_gross_return_bps: Option<i32>,
    pub(super) known_estimated_costs_sol: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HetComparisonCorrelationV1 {
    pub(super) comparison_id: String,
    pub(super) source_snapshot_id: String,
    pub(super) run_id: String,
    pub(super) writer_instance_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TerminalV2ComparisonSkipReasonV1 {
    CoreSemanticValidationFailed,
    FinalSemanticValidationFailed,
    SerializationFailed,
    PayloadOversized,
    WriterNotConfigured,
    WriterUnavailable,
    WriterQueueFull,
    WriterQueueClosed,
    WriterTimedOutBeforeWrite,
    WriterIoFailed,
}

impl TerminalV2ComparisonSkipReasonV1 {
    pub(super) const fn as_label(self) -> &'static str {
        match self {
            Self::CoreSemanticValidationFailed => "core_semantic_validation_failed",
            Self::FinalSemanticValidationFailed => "final_semantic_validation_failed",
            Self::SerializationFailed => "serialization_failed",
            Self::PayloadOversized => "payload_oversized",
            Self::WriterNotConfigured => "writer_not_configured",
            Self::WriterUnavailable => "writer_unavailable",
            Self::WriterQueueFull => "writer_queue_full",
            Self::WriterQueueClosed => "writer_queue_closed",
            Self::WriterTimedOutBeforeWrite => "writer_timed_out_before_write",
            Self::WriterIoFailed => "writer_io_failed",
        }
    }

    fn from_validation_failure(reason: &'static str, core: bool) -> Self {
        match reason {
            "serialization_failed" => Self::SerializationFailed,
            "comparison_payload_oversized" => Self::PayloadOversized,
            _ if core => Self::CoreSemanticValidationFailed,
            _ => Self::FinalSemanticValidationFailed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TerminalV2ComparisonOutcomeUnknownReasonV1 {
    WriterAckTimedOut,
    WriterAckChannelClosed,
}

impl TerminalV2ComparisonOutcomeUnknownReasonV1 {
    pub(super) const fn as_label(self) -> &'static str {
        match self {
            Self::WriterAckTimedOut => "writer_ack_timed_out",
            Self::WriterAckChannelClosed => "writer_ack_channel_closed",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum PreparedV1V2ComparisonCoreV1 {
    Ready(Box<V1V2ComparisonRecord>),
    Skipped {
        correlation: HetComparisonCorrelationV1,
        reason: TerminalV2ComparisonSkipReasonV1,
        detail: String,
    },
}

#[derive(Debug, Clone)]
pub(super) enum PreparedHetComparisonV1 {
    Ready {
        correlation: HetComparisonCorrelationV1,
        action_id: Option<String>,
        encoded: Vec<u8>,
    },
    Skipped {
        correlation: HetComparisonCorrelationV1,
        action_id: Option<String>,
        reason: TerminalV2ComparisonSkipReasonV1,
        detail: String,
    },
}

impl PreparedHetComparisonV1 {
    pub(super) fn correlation(&self) -> &HetComparisonCorrelationV1 {
        match self {
            Self::Ready { correlation, .. } | Self::Skipped { correlation, .. } => correlation,
        }
    }

    pub(super) fn action_id(&self) -> Option<&str> {
        match self {
            Self::Ready { action_id, .. } | Self::Skipped { action_id, .. } => action_id.as_deref(),
        }
    }
}

fn v2_final_exit_reason_label(value: Option<&str>) -> Option<&'static str> {
    let value = value?;
    if value.starts_with("ExitAll { reason: Crash,") {
        Some("crash")
    } else if value.starts_with("ExitAll { reason: HardLoss,") {
        Some("hard_loss")
    } else if value.starts_with("ExitAll { reason: ExecutableTrailing,") {
        Some("executable_trailing")
    } else if value.starts_with("ExitAll { reason: VitalityDecay,") {
        Some("vitality_decay")
    } else if value.starts_with("ExitAll { reason: AbsoluteMaxHold,") {
        Some("absolute_max_hold")
    } else {
        None
    }
}

impl PreparedV1V2ComparisonCoreV1 {
    pub(super) fn censoring_evidence(
        &self,
    ) -> (HetComparisonCorrelationV1, bool, Option<&'static str>) {
        match self {
            Self::Ready(record) => {
                let reason = v2_final_exit_reason_label(record.v2_final.as_deref());
                (
                    HetComparisonCorrelationV1 {
                        comparison_id: record.comparison_id.clone(),
                        source_snapshot_id: record.snapshot_id.clone(),
                        run_id: record.run_id.clone(),
                        writer_instance_id: record.writer_instance_id.clone(),
                    },
                    reason.is_some(),
                    reason,
                )
            }
            Self::Skipped { correlation, .. } => (correlation.clone(), false, None),
        }
    }

    pub(super) fn prepare(record: V1V2ComparisonRecord) -> Self {
        let correlation = HetComparisonCorrelationV1 {
            comparison_id: record.comparison_id.clone(),
            source_snapshot_id: record.snapshot_id.clone(),
            run_id: record.run_id.clone(),
            writer_instance_id: record.writer_instance_id.clone(),
        };
        match record.validate_core() {
            Ok(()) => Self::Ready(Box::new(record)),
            Err(detail) => Self::Skipped {
                correlation,
                reason: TerminalV2ComparisonSkipReasonV1::from_validation_failure(detail, true),
                detail: detail.to_string(),
            },
        }
    }

    pub(super) fn finalize(
        self,
        receipt: V1AuthorityTickReceiptV1,
        anchor_applied: bool,
    ) -> PreparedHetComparisonV1 {
        let action_id = receipt.action_id.clone();
        match self {
            Self::Ready(record) => {
                let mut record = *record;
                let correlation = HetComparisonCorrelationV1 {
                    comparison_id: record.comparison_id.clone(),
                    source_snapshot_id: record.snapshot_id.clone(),
                    run_id: record.run_id.clone(),
                    writer_instance_id: record.writer_instance_id.clone(),
                };
                record.terminal_tick =
                    matches!(receipt.exit_apply_status, V1ExitApplyStatusV1::Applied)
                        || !matches!(
                            receipt.terminal_commit_status,
                            V1TerminalCommitStatusV1::NotRequired
                        );
                record.v1_final = Some(receipt.outcome.as_label().to_string());
                // An authoritative-V2 tick can still encounter an action that
                // was already started by V1 before cutover. That action must
                // finish unchanged, but it must not be attributed to V2. The
                // engine records the exact V2 candidate it actually passed to
                // the shared envelope; comparing that value avoids both a
                // HardLoss/StopLoss label mismatch and the false inference
                // from a higher diagnostic blocker over AbsoluteMaxHold.
                let v2_started_this_action = record.v2_shadow_authority
                    && receipt.reason.as_deref() == record.v2_selected_execution_reason.as_deref();
                let v2_proposal_created = v2_started_this_action && receipt.action_id.is_some();
                let v2_economic_mutation = v2_started_this_action
                    && matches!(receipt.exit_apply_status, V1ExitApplyStatusV1::Applied);
                record.v1_authority_receipt = Some(receipt);
                record.v2_proposal_created = v2_proposal_created;
                record.v2_economic_mutation = v2_economic_mutation;
                record.anchor_applied = anchor_applied;
                match record.validate_and_serialize() {
                    Ok(encoded) => PreparedHetComparisonV1::Ready {
                        correlation,
                        action_id,
                        encoded,
                    },
                    Err(detail) => PreparedHetComparisonV1::Skipped {
                        correlation,
                        action_id,
                        reason: TerminalV2ComparisonSkipReasonV1::from_validation_failure(
                            detail, false,
                        ),
                        detail: detail.to_string(),
                    },
                }
            }
            Self::Skipped {
                correlation,
                reason,
                detail,
            } => PreparedHetComparisonV1::Skipped {
                correlation,
                action_id,
                reason,
                detail,
            },
        }
    }
}

impl V1V2ComparisonRecord {
    fn validate_common(&self) -> Result<(), &'static str> {
        if self.schema_version != HET_PM_V2_SCHEMA_VERSION
            || self.policy_id != HET_PM_V2_POLICY_ID
            || self.policy_version != HET_PM_V2_POLICY_VERSION
            || self.trajectory_sampling_mode != HET_PM_V2_SAMPLING_MODE
            || self.trajectory_measurement_grade != HET_PM_V2_TRAJECTORY_GRADE
        {
            return Err("comparison_schema_or_policy_mismatch");
        }
        if self.v1_policy_id != EXIT_POLICY_V1_ID
            || self.v1_policy_version != EXIT_POLICY_V1_VERSION
            || self.policy_config_hash.is_empty()
            || self.v1_policy_config_hash.is_empty()
            || self.time_stop_v2_config_hash.is_empty()
        {
            return Err("comparison_source_config_identity_mismatch");
        }
        if self.comparison_id.is_empty()
            || self.run_id.is_empty()
            || self.writer_instance_id.is_empty()
            || self.position_id.is_empty()
            || self.snapshot_id.is_empty()
            || self.remaining_quantity_raw == 0
        {
            return Err("comparison_identity_contract_mismatch");
        }
        if !matches!(self.lane, Lane::Shadow) {
            return Err("comparison_lane_is_not_shadow");
        }
        if self.v1_shadow_authority == self.v2_shadow_authority || self.live_authority {
            return Err("comparison_authority_contract_mismatch");
        }
        if self.consumed_by_policy != self.v2_shadow_authority {
            return Err("comparison_policy_consumption_mismatch");
        }
        if let Some(selected_reason) = self.v2_selected_execution_reason.as_deref() {
            let selected_reason_is_real_v2_exit =
                self.v2_gate_evaluations.iter().any(|evaluation| {
                    matches!(
                        &evaluation.final_decision,
                        HetPmFinalDecisionV2::ExitAll { reason, .. }
                            if reason.exit_candidate_reason().as_label() == selected_reason
                    )
                });
            if !self.v2_shadow_authority || !selected_reason_is_real_v2_exit {
                return Err("comparison_v2_selected_execution_reason_mismatch");
            }
        }
        if (!self.v2_shadow_authority && (self.v2_economic_mutation || self.v2_proposal_created))
            || self.v2_time_stop_mutation
            || self.duplicate_action_observed
            || self.route_build_authority_changed
            || self.terminal_isolation_violation
        {
            return Err("observe_only_record_contains_forbidden_mutation");
        }
        if self.quote_keys.len() > HET_PM_V2_MAX_QUOTE_CELLS
            || self.quote_statuses.len() > HET_PM_V2_MAX_QUOTE_CELLS
            || self.quote_resolution_count as usize > HET_PM_V2_MAX_QUOTE_CELLS
        {
            return Err("quote_plan_exceeds_bounded_limit");
        }
        if self.quote_keys.len() != self.quote_statuses.len()
            || self.quote_resolution_count as usize != self.quote_statuses.len()
        {
            return Err("quote_plan_cardinality_mismatch");
        }
        let expected = [
            HetPmExitReasonV2::Crash,
            HetPmExitReasonV2::HardLoss,
            HetPmExitReasonV2::ExecutableTrailing,
            HetPmExitReasonV2::VitalityDecay,
            HetPmExitReasonV2::AbsoluteMaxHold,
        ];
        if self.v2_gate_evaluations.len() != expected.len()
            || self
                .v2_gate_evaluations
                .iter()
                .zip(expected)
                .any(|(evaluation, reason)| evaluation.gate != reason)
        {
            return Err("comparison_gate_lattice_contract_mismatch");
        }
        if self.v2_gate_evaluations.iter().any(|evaluation| {
            evaluation.executable_gross_return_bps.is_some()
                != matches!(
                    evaluation.final_decision,
                    HetPmFinalDecisionV2::ExitAll { .. }
                )
        }) {
            return Err("comparison_gate_lattice_return_contract_mismatch");
        }
        let anchor_non_finite = self.anchor_before.as_ref().is_some_and(|anchor| {
            !anchor.peak_mark_price_sol.is_finite() || !anchor.executable_value_sol.is_finite()
        });
        if self
            .trajectory
            .peak_mark_price_sol
            .is_some_and(|value| !value.is_finite())
            || self
                .current_executable_value_sol
                .is_some_and(|value| !value.is_finite())
            || self
                .known_estimated_costs_sol
                .is_some_and(|value| !value.is_finite())
            || anchor_non_finite
        {
            return Err("comparison_contains_non_finite_metric");
        }
        Ok(())
    }

    fn serialize_bounded(&self) -> Result<Vec<u8>, &'static str> {
        let encoded = serde_json::to_vec(self).map_err(|_| "serialization_failed")?;
        if encoded.len() > HET_PM_V2_MAX_SERIALIZED_RECORD_BYTES {
            return Err("comparison_payload_oversized");
        }
        Ok(encoded)
    }

    /// Validate the immutable comparison boundary before V1 can mutate the
    /// position. Final authority fields are deliberately absent at this stage.
    pub(super) fn validate_core(&self) -> Result<(), &'static str> {
        self.validate_common()?;
        if self.v1_final.is_some() || self.v1_authority_receipt.is_some() || self.terminal_tick {
            return Err("comparison_core_contains_post_authority_outcome");
        }
        self.serialize_bounded().map(|_| ())
    }

    pub(super) fn validate_and_serialize(&self) -> Result<Vec<u8>, &'static str> {
        self.validate_common()?;
        let Some(receipt) = self.v1_authority_receipt.as_ref() else {
            return Err("v1_authority_receipt_missing");
        };
        if receipt.snapshot_id != self.snapshot_id
            || receipt.state_revision != self.state_revision
            || receipt.remaining_quantity_raw != self.remaining_quantity_raw
        {
            return Err("v1_authority_receipt_snapshot_mismatch");
        }
        if self.v1_final.as_deref() != Some(receipt.outcome.as_label()) {
            return Err("v1_final_receipt_outcome_mismatch");
        }
        let terminal_tick_expected =
            matches!(receipt.exit_apply_status, V1ExitApplyStatusV1::Applied)
                || !matches!(
                    receipt.terminal_commit_status,
                    V1TerminalCommitStatusV1::NotRequired
                );
        if self.terminal_tick != terminal_tick_expected {
            return Err("terminal_tick_receipt_outcome_mismatch");
        }
        let receipt_axes_valid = match receipt.outcome {
            V1AuthorityTickOutcomeV1::ExitApplied => {
                matches!(receipt.exit_apply_status, V1ExitApplyStatusV1::Applied)
                    && matches!(
                        receipt.terminal_commit_status,
                        V1TerminalCommitStatusV1::Pending | V1TerminalCommitStatusV1::Committed
                    )
            }
            V1AuthorityTickOutcomeV1::ApplyRejected => {
                matches!(receipt.exit_apply_status, V1ExitApplyStatusV1::Rejected)
                    && matches!(
                        receipt.terminal_commit_status,
                        V1TerminalCommitStatusV1::NotRequired
                    )
            }
            V1AuthorityTickOutcomeV1::PendingRecovery => {
                matches!(receipt.exit_apply_status, V1ExitApplyStatusV1::NotApplied)
                    && matches!(
                        receipt.terminal_commit_status,
                        V1TerminalCommitStatusV1::NotRequired
                            | V1TerminalCommitStatusV1::Pending
                            | V1TerminalCommitStatusV1::Committed
                    )
            }
            V1AuthorityTickOutcomeV1::Hold
            | V1AuthorityTickOutcomeV1::ProposalStarted
            | V1AuthorityTickOutcomeV1::Blocked => {
                matches!(receipt.exit_apply_status, V1ExitApplyStatusV1::NotApplied)
                    && matches!(
                        receipt.terminal_commit_status,
                        V1TerminalCommitStatusV1::NotRequired
                    )
            }
        };
        if !receipt_axes_valid {
            return Err("v1_receipt_apply_commit_axes_mismatch");
        }
        self.serialize_bounded()
    }
}

pub(super) fn prequote_label(decision: &PreQuoteDecision) -> String {
    match decision {
        PreQuoteDecision::Hold => "hold".to_string(),
        PreQuoteDecision::UnknownEvidence { reason } => format!("unknown:{reason:?}"),
        PreQuoteDecision::QuoteRequired { candidate } => {
            format!("quote_required:{}", candidate.reason().as_label())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::exit_policy_v1::{
        CrashSampleV1, CrashVectorV1, ExitCandidate, PositionSnapshotGuard,
    };
    use super::super::trajectory_v1::TrajectoryFlagsV1;
    use super::*;
    use trigger::PriceTruthSource;

    fn effective() -> EffectiveHetPmV2Config {
        let mut cfg = HetPmV2Config::default();
        cfg.enabled = true;
        EffectiveHetPmV2Config::from_config(&cfg).unwrap()
    }

    fn usable_trajectory() -> TrajectoryFeaturesV1 {
        TrajectoryFeaturesV1 {
            return_1500ms_bps: Some(-100),
            return_5s_bps: Some(-100),
            return_15s_bps: Some(500),
            peak_mark_price_sol: Some(1.5),
            peak_sample_slot: Some(10),
            peak_sample_timestamp_ms: Some(10_000),
            drawdown_from_peak_bps: Some(2_000),
            time_since_peak_ms: Some(6_000),
            peak_giveback_velocity_bps_per_sec: Some(333),
            newest_sample_slot: Some(20),
            newest_sample_timestamp_ms: Some(16_000),
            newest_sample_age_ms: Some(0),
            distinct_slots_1500ms: 2,
            state_update_delta_since_previous_sample: 1,
            quality: TrajectoryQualityV1::Usable,
            flags: TrajectoryFlagsV1::default(),
        }
    }

    fn vitality(state: VitalityStateV1, windows: u32) -> VitalityFeaturesV1 {
        VitalityFeaturesV1 {
            current_state: state,
            consecutive_non_alive_windows: windows,
            last_window_at_ms: Some(16_000),
            last_alive_at_ms: Some(4_000),
            latest_window_price_delta_bps: Some(-100),
            latest_window_state_update_delta: Some(1),
            quality_fresh: true,
        }
    }

    fn bundle(
        pending: bool,
        age_ms: u64,
        mark_price: f64,
        route: RouteStatusV1,
        trajectory: TrajectoryFeaturesV1,
        vitality: VitalityFeaturesV1,
        anchor: Option<ExecutablePeakAnchorV1>,
    ) -> PostBuySnapshotBundle {
        bundle_with_crash_vector(
            pending,
            age_ms,
            mark_price,
            route,
            trajectory,
            vitality,
            anchor,
            CrashVectorV1::default(),
            "v1-hash",
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn bundle_with_crash_vector(
        pending: bool,
        age_ms: u64,
        mark_price: f64,
        route: RouteStatusV1,
        trajectory: TrajectoryFeaturesV1,
        vitality: VitalityFeaturesV1,
        anchor: Option<ExecutablePeakAnchorV1>,
        crash_vector: CrashVectorV1,
        v1_policy_config_hash: &str,
    ) -> PostBuySnapshotBundle {
        let guard =
            PositionSnapshotGuard::new("position".to_string(), 1, 7, 100, Some(20), Some(16_000));
        let base = PostBuyDecisionSnapshot::new(
            guard,
            Lane::Shadow,
            Some(1.0),
            100,
            100,
            1_000,
            age_ms,
            1_000,
            Some(mark_price),
            MarkEvidenceStatus::Available,
            PriceTruthSource::CanonicalAccountStateSnapshot,
            Some(20),
            Some(16_000),
            Some(0),
            Some(1_000.0),
            Some(1.0),
            Some(50.0),
            Some(-10.0),
            1.5,
            Some(20.0),
            crash_vector,
            pending,
            v1_policy_config_hash.to_string(),
        );
        PostBuySnapshotBundle {
            base,
            v2: PostBuyDecisionExtrasV2 {
                run_id: "run".to_string(),
                trajectory,
                vitality,
                route_status: route,
                executable_peak_anchor: anchor,
                entry_value_quote_raw: Some(1_000_000_000),
                entry_value_source: EntryValueSourceV1::PersistedEntryAmount,
                entry_value_authoritative_for_shadow: true,
            },
        }
    }

    fn crash_bundle(v1_config: &EffectiveExitPolicyV1Config) -> PostBuySnapshotBundle {
        let crash_vector = CrashVectorV1::new(
            1.5,
            Some(CrashSampleV1::new(1.4, 19, 15_000)),
            Some(CrashSampleV1::new(1.2, 19, 15_500)),
            Some(CrashSampleV1::new(0.9, 20, 16_000)),
            Some(0),
            2,
            Some(0.35),
            Some(0.40),
            true,
            true,
        );
        bundle_with_crash_vector(
            false,
            20_000,
            0.9,
            RouteStatusV1::PumpCurveSupported,
            usable_trajectory(),
            vitality(VitalityStateV1::Alive, 0),
            None,
            crash_vector,
            v1_config.config_hash(),
        )
    }

    fn crash_prequote() -> CrashGuardPreQuoteDecision {
        CrashGuardPreQuoteDecision::QuoteRequired {
            candidate: ExitCandidate::from_reason(ExitCandidateReason::CrashGuard),
        }
    }

    fn crash_v1_config(max_executable_return_pct: f64) -> EffectiveExitPolicyV1Config {
        let mut guardian = PostBuyGuardianConfig::default();
        guardian.target_threshold = Some(50.0);
        guardian.stoploss_threshold = Some(50.0);
        guardian.exit_policy_v1.crash_guard_mode = CrashGuardMode::ObserveOnly;
        guardian.exit_policy_v1.crash_max_executable_return_pct = max_executable_return_pct;
        EffectiveExitPolicyV1Config::from_guardian(&guardian).expect("valid V1 CrashGuard config")
    }

    fn finalize_crash(
        bundle: &PostBuySnapshotBundle,
        v1_config: &EffectiveExitPolicyV1Config,
        quote: &ExecutableExitQuote,
        quote_evidence: QuoteEvidenceRevisionV1,
    ) -> HetPmFinalDecisionV2 {
        let het_config = effective();
        let crash_prequote = crash_prequote();
        let prequote = ExitPolicyV2::evaluate_prequote(
            bundle.view(),
            &PreQuoteDecision::Hold,
            &crash_prequote,
            &het_config,
        );
        let key = ExecutableQuoteKeyV2::from_view(bundle.view());
        let requirement = ExitPolicyV1::crash_guard_quote_requirement(&bundle.base)
            .expect("crash candidate requirement");
        ExitPolicyV2::finalize_with_quote(
            bundle.view(),
            &prequote,
            HetPmQuoteFinalizationInputV2 {
                quote: Some(quote),
                quote_key: Some(&key),
                quote_evidence: Some(quote_evidence),
                crash_requirement: Some(&requirement),
            },
            v1_config,
            &het_config,
        )
    }

    fn anchor(config: &EffectiveHetPmV2Config) -> ExecutablePeakAnchorV1 {
        ExecutablePeakAnchorV1 {
            position_id: "position".to_string(),
            position_epoch: 1,
            remaining_quantity_raw: 100,
            route_id: HET_PM_V2_PUMP_ROUTE_ID.to_string(),
            quote_model_id: HET_PM_V2_QUOTE_MODEL_ID.to_string(),
            policy_config_hash: config.config_hash().to_string(),
            quote_state_revision: 6,
            source_snapshot_id: "old".to_string(),
            source_sample_slot: Some(10),
            source_sample_timestamp_ms: Some(10_000),
            peak_mark_price_sol: 1.5,
            executable_value_quote_raw: None,
            executable_value_sol: 1.4,
            executable_gross_return_bps: Some(4_000),
            anchor_seq: 1,
            created_at_ms: 10_000,
        }
    }

    fn comparison_record() -> V1V2ComparisonRecord {
        V1V2ComparisonRecord {
            schema_version: HET_PM_V2_SCHEMA_VERSION,
            comparison_id: "comparison".to_string(),
            policy_id: HET_PM_V2_POLICY_ID.to_string(),
            policy_version: HET_PM_V2_POLICY_VERSION,
            policy_config_hash: "hash".to_string(),
            v1_policy_id: EXIT_POLICY_V1_ID.to_string(),
            v1_policy_version: EXIT_POLICY_V1_VERSION,
            v1_policy_config_hash: "v1-hash".to_string(),
            time_stop_v2_config_hash: "time-stop-hash".to_string(),
            run_id: "run".to_string(),
            writer_instance_id: "writer".to_string(),
            lane: Lane::Shadow,
            position_id: "position".to_string(),
            position_epoch: 1,
            state_revision: 2,
            remaining_quantity_raw: 100,
            snapshot_id: "snapshot".to_string(),
            observation_timestamp_ms: 16_000,
            terminal_tick: false,
            trajectory_sampling_mode: HET_PM_V2_SAMPLING_MODE.to_string(),
            trajectory_measurement_grade: HET_PM_V2_TRAJECTORY_GRADE.to_string(),
            monitor_tick_ms: 500,
            v1_prequote: "hold".to_string(),
            v1_crash_prequote: "Disabled".to_string(),
            v1_final: Some("Hold".to_string()),
            v1_authority_receipt: Some(V1AuthorityTickReceiptV1 {
                snapshot_id: "snapshot".to_string(),
                state_revision: 2,
                remaining_quantity_raw: 100,
                outcome: V1AuthorityTickOutcomeV1::Hold,
                exit_apply_status: V1ExitApplyStatusV1::NotApplied,
                terminal_commit_status: V1TerminalCommitStatusV1::NotRequired,
                action_id: None,
                reason: None,
                crash_quote_decision: None,
            }),
            v2_prequote: "Hold".to_string(),
            v2_final: Some("Hold".to_string()),
            v2_selected_execution_reason: None,
            v2_crash_quote_decision: None,
            v2_winning_gate: HetPmGateV2::Hold,
            v2_suppressed_gates_mask: 0,
            v2_gate_evaluations: [
                HetPmExitReasonV2::Crash,
                HetPmExitReasonV2::HardLoss,
                HetPmExitReasonV2::ExecutableTrailing,
                HetPmExitReasonV2::VitalityDecay,
                HetPmExitReasonV2::AbsoluteMaxHold,
            ]
            .into_iter()
            .map(|gate| HetPmGateEvaluationV2 {
                gate,
                prequote: HetPmCandidateV2::Hold,
                final_decision: HetPmFinalDecisionV2::Hold,
                quote_status: HetPmGateQuoteStatusV2::NotRequired,
                executable_gross_return_bps: None,
            })
            .collect(),
            consumed_by_policy: false,
            v1_shadow_authority: true,
            v2_shadow_authority: false,
            live_authority: false,
            v2_economic_mutation: false,
            v2_proposal_created: false,
            v2_time_stop_mutation: false,
            duplicate_action_observed: false,
            route_build_authority_changed: false,
            terminal_isolation_violation: false,
            trajectory: usable_trajectory(),
            vitality: vitality(VitalityStateV1::Alive, 0),
            route_status: RouteStatusV1::PumpCurveSupported,
            entry_value_quote_raw: Some(1_000_000_000),
            entry_value_source: EntryValueSourceV1::PersistedEntryAmount,
            entry_value_authoritative_for_shadow: true,
            anchor_before: None,
            anchor_request: None,
            anchor_applied: false,
            quote_keys: Vec::new(),
            quote_resolution_count: 0,
            quote_statuses: Vec::new(),
            current_executable_value_sol: None,
            current_executable_gross_return_bps: None,
            known_estimated_costs_sol: None,
        }
    }

    #[test]
    fn defaults_are_disabled_and_authoritative_shadow_is_explicitly_enabled() {
        let cfg = PostBuyGuardianConfig::default();
        let effective = EffectiveHetPmV2Config::from_guardian(&cfg).unwrap();
        assert!(!effective.enabled());

        let mut authoritative = cfg;
        authoritative.het_pm_v2.enabled = true;
        authoritative.time_stop_v2.enabled = true;
        authoritative.het_pm_v2.mode = HetPmV2Mode::AuthoritativeShadow;
        assert!(EffectiveHetPmV2Config::from_guardian(&authoritative)
            .unwrap()
            .authoritative_shadow());
    }

    #[test]
    fn config_hash_is_het_only_and_deterministic() {
        let first = effective();
        let second = effective();
        assert_eq!(first.config_hash(), second.config_hash());

        let mut guardian = PostBuyGuardianConfig::default();
        guardian.het_pm_v2.enabled = true;
        guardian.time_stop_v2.enabled = true;
        guardian.exit_policy_v1.crash_guard_mode = CrashGuardMode::ObserveOnly;
        let changed_v1 = EffectiveHetPmV2Config::from_guardian(&guardian).unwrap();
        assert_eq!(first.config_hash(), changed_v1.config_hash());
        assert_eq!(
            changed_v1
                .status(guardian.exit_policy_v1.crash_guard_mode)
                .crash_guard_mode,
            CrashGuardMode::ObserveOnly
        );

        guardian.het_pm_v2.enabled = false;
        let disabled = EffectiveHetPmV2Config::from_guardian(&guardian).unwrap();
        assert_eq!(
            disabled
                .status(guardian.exit_policy_v1.crash_guard_mode)
                .crash_guard_mode,
            CrashGuardMode::ObserveOnly,
            "toggling HET must not rewrite the effective CrashGuard mode"
        );

        guardian.het_pm_v2.enabled = true;
        guardian.het_pm_v2.writer_queue_capacity += 1;
        let changed_writer_contract =
            EffectiveHetPmV2Config::from_guardian(&guardian).expect("valid writer contract");
        assert_ne!(
            first.config_hash(),
            changed_writer_contract.config_hash(),
            "writer sampling/backpressure semantics belong to the HET config identity"
        );
    }

    #[test]
    fn writer_queue_and_terminal_budget_are_bounded_at_startup() {
        let mut guardian = PostBuyGuardianConfig::default();
        guardian.het_pm_v2.writer_queue_capacity = 0;
        assert_eq!(
            EffectiveHetPmV2Config::from_guardian(&guardian),
            Err(HetPmV2ConfigError::InvalidWriterQueueCapacity)
        );

        guardian.het_pm_v2.writer_queue_capacity = HET_PM_V2_MAX_WRITER_QUEUE_CAPACITY;
        guardian.het_pm_v2.terminal_write_budget_ms = 0;
        assert_eq!(
            EffectiveHetPmV2Config::from_guardian(&guardian),
            Err(HetPmV2ConfigError::InvalidTerminalWriteBudget)
        );

        guardian.het_pm_v2.terminal_write_budget_ms = HET_PM_V2_MAX_TERMINAL_WRITE_BUDGET_MS + 1;
        assert_eq!(
            EffectiveHetPmV2Config::from_guardian(&guardian),
            Err(HetPmV2ConfigError::InvalidTerminalWriteBudget)
        );
    }

    #[test]
    fn comparison_validation_is_bounded_typed_and_fail_closed() {
        assert!(comparison_record().validate_and_serialize().is_ok());

        let mut schema_mismatch = comparison_record();
        schema_mismatch.schema_version += 1;
        assert_eq!(
            schema_mismatch.validate_and_serialize(),
            Err("comparison_schema_or_policy_mismatch")
        );

        let mut forbidden_mutation = comparison_record();
        forbidden_mutation.v2_proposal_created = true;
        assert_eq!(
            forbidden_mutation.validate_and_serialize(),
            Err("observe_only_record_contains_forbidden_mutation")
        );

        let mut bad_cardinality = comparison_record();
        bad_cardinality.quote_resolution_count = 1;
        assert_eq!(
            bad_cardinality.validate_and_serialize(),
            Err("quote_plan_cardinality_mismatch")
        );

        let mut serialization_failure = comparison_record();
        serialization_failure.current_executable_value_sol = Some(f64::NAN);
        assert_eq!(
            serialization_failure.validate_and_serialize(),
            Err("comparison_contains_non_finite_metric")
        );

        let mut oversized = comparison_record();
        oversized.run_id = "x".repeat(HET_PM_V2_MAX_SERIALIZED_RECORD_BYTES);
        assert_eq!(
            oversized.validate_and_serialize(),
            Err("comparison_payload_oversized")
        );
    }

    #[test]
    fn comparison_rejects_v1_final_receipt_mismatch() {
        let mut record = comparison_record();
        record.v1_final = Some("ExitApplied".to_string());

        assert_eq!(
            record.validate_and_serialize(),
            Err("v1_final_receipt_outcome_mismatch")
        );
    }

    #[test]
    fn comparison_rejects_terminal_tick_receipt_mismatch() {
        let mut record = comparison_record();
        record.terminal_tick = true;

        assert_eq!(
            record.validate_and_serialize(),
            Err("terminal_tick_receipt_outcome_mismatch")
        );
    }

    #[test]
    fn comparison_core_is_validated_before_authority_outcome() {
        let mut record = comparison_record();
        record.v1_final = None;
        record.v1_authority_receipt = None;

        assert!(record.validate_core().is_ok());

        record.terminal_tick = true;
        assert_eq!(
            record.validate_core(),
            Err("comparison_core_contains_post_authority_outcome")
        );
    }

    #[test]
    fn exit_applied_requires_separate_pending_or_committed_terminal_axis() {
        let mut record = comparison_record();
        let receipt = record.v1_authority_receipt.as_mut().unwrap();
        receipt.outcome = V1AuthorityTickOutcomeV1::ExitApplied;
        receipt.exit_apply_status = V1ExitApplyStatusV1::Applied;
        receipt.terminal_commit_status = V1TerminalCommitStatusV1::Pending;
        receipt.action_id = Some("action".to_string());
        record.v1_final = Some("ExitApplied".to_string());
        record.terminal_tick = true;
        assert!(record.validate_and_serialize().is_ok());

        record
            .v1_authority_receipt
            .as_mut()
            .unwrap()
            .terminal_commit_status = V1TerminalCommitStatusV1::NotRequired;
        assert_eq!(
            record.validate_and_serialize(),
            Err("v1_receipt_apply_commit_axes_mismatch")
        );
    }

    #[test]
    fn authoritative_v2_max_hold_is_attributed_even_when_pure_hierarchy_is_blocked() {
        let mut record = comparison_record();
        record.v1_shadow_authority = false;
        record.v2_shadow_authority = true;
        record.consumed_by_policy = true;
        record.v1_final = None;
        record.v1_authority_receipt = None;
        // The pure diagnostic hierarchy can be blocked above MaxHold. The
        // separately stored lattice still proves the hard ceiling was the
        // precise V2 action passed to the shared executor.
        record.v2_final = Some("Blocked(VitalityEvidenceStale)".to_string());
        record.v2_selected_execution_reason = Some("absolute_max_hold".to_string());
        let max_hold = record
            .v2_gate_evaluations
            .iter_mut()
            .find(|evaluation| evaluation.gate == HetPmExitReasonV2::AbsoluteMaxHold)
            .expect("absolute-max-hold lattice entry");
        max_hold.prequote = HetPmCandidateV2::QuoteRequired(HetPmExitReasonV2::AbsoluteMaxHold);
        max_hold.final_decision = HetPmFinalDecisionV2::ExitAll {
            reason: HetPmExitReasonV2::AbsoluteMaxHold,
            quantity_raw: 100,
            executable_gross_return_bps: -95,
        };
        max_hold.quote_status = HetPmGateQuoteStatusV2::Resolved;
        max_hold.executable_gross_return_bps = Some(-95);

        let prepared = PreparedV1V2ComparisonCoreV1::prepare(record);
        let finalized = prepared.finalize(
            V1AuthorityTickReceiptV1 {
                snapshot_id: "snapshot".to_string(),
                state_revision: 2,
                remaining_quantity_raw: 100,
                outcome: V1AuthorityTickOutcomeV1::ExitApplied,
                exit_apply_status: V1ExitApplyStatusV1::Applied,
                terminal_commit_status: V1TerminalCommitStatusV1::Pending,
                action_id: Some("action".to_string()),
                reason: Some("absolute_max_hold".to_string()),
                crash_quote_decision: None,
            },
            false,
        );

        let PreparedHetComparisonV1::Ready { encoded, .. } = finalized else {
            panic!("authoritative V2 MaxHold comparison must serialize");
        };
        let row: serde_json::Value =
            serde_json::from_slice(&encoded).expect("serialized comparison JSON");
        assert_eq!(row["v2_selected_execution_reason"], "absolute_max_hold");
        assert_eq!(row["v2_proposal_created"], true);
        assert_eq!(row["v2_economic_mutation"], true);
    }

    #[test]
    fn v1_action_is_not_attributed_when_v2_selected_no_exit() {
        let mut record = comparison_record();
        record.v1_final = None;
        record.v1_authority_receipt = None;
        let finalized = PreparedV1V2ComparisonCoreV1::prepare(record).finalize(
            V1AuthorityTickReceiptV1 {
                snapshot_id: "snapshot".to_string(),
                state_revision: 2,
                remaining_quantity_raw: 100,
                outcome: V1AuthorityTickOutcomeV1::ExitApplied,
                exit_apply_status: V1ExitApplyStatusV1::Applied,
                terminal_commit_status: V1TerminalCommitStatusV1::Pending,
                action_id: Some("action".to_string()),
                reason: Some("absolute_max_hold".to_string()),
                crash_quote_decision: None,
            },
            false,
        );

        let PreparedHetComparisonV1::Ready { encoded, .. } = finalized else {
            panic!("V1 comparison must serialize");
        };
        let row: serde_json::Value =
            serde_json::from_slice(&encoded).expect("serialized comparison JSON");
        assert_eq!(row["v2_proposal_created"], false);
        assert_eq!(row["v2_economic_mutation"], false);
    }

    #[test]
    fn entry_amount_precedence_is_explicit() {
        assert_eq!(
            build_entry_value_contract(1_000, Some(0.001), 2_000_000),
            (Some(1_000), EntryValueSourceV1::PersistedEntryAmount, true)
        );
        assert_eq!(
            build_entry_value_contract(0, Some(0.001), 2_000_000),
            (
                Some(2_000_000),
                EntryValueSourceV1::DiagnosticPriceTimesQuantityFallback,
                false
            )
        );
        assert_eq!(
            build_entry_value_contract(0, None, 0),
            (None, EntryValueSourceV1::Unavailable, false)
        );
    }

    #[test]
    fn anchor_comparability_rejects_every_identity_mismatch() {
        let key = ExecutableQuoteKeyV2 {
            position_id: "p".into(),
            position_epoch: 2,
            state_revision: 4,
            remaining_quantity_raw: 100,
            route_id: HET_PM_V2_PUMP_ROUTE_ID.into(),
            quote_model_id: HET_PM_V2_QUOTE_MODEL_ID.into(),
            sample_slot: Some(5),
            sample_timestamp_ms: Some(6),
        };
        let anchor = ExecutablePeakAnchorV1 {
            position_id: "p".into(),
            position_epoch: 2,
            remaining_quantity_raw: 100,
            route_id: HET_PM_V2_PUMP_ROUTE_ID.into(),
            quote_model_id: HET_PM_V2_QUOTE_MODEL_ID.into(),
            policy_config_hash: "h".into(),
            quote_state_revision: 3,
            source_snapshot_id: "s".into(),
            source_sample_slot: Some(4),
            source_sample_timestamp_ms: Some(5),
            peak_mark_price_sol: 2.0,
            executable_value_quote_raw: None,
            executable_value_sol: 1.0,
            executable_gross_return_bps: Some(1_000),
            anchor_seq: 1,
            created_at_ms: 7,
        };
        assert_eq!(comparable_anchor(&anchor, &key, "h"), Ok(()));

        let mut mismatch = anchor.clone();
        mismatch.remaining_quantity_raw = 99;
        assert_eq!(
            comparable_anchor(&mismatch, &key, "h"),
            Err(HetPmUnknownReasonV2::AnchorQuantityMismatch)
        );
        mismatch = anchor.clone();
        mismatch.position_epoch = 3;
        assert_eq!(
            comparable_anchor(&mismatch, &key, "h"),
            Err(HetPmUnknownReasonV2::AnchorEpochMismatch)
        );
        mismatch = anchor.clone();
        mismatch.route_id = "other".into();
        assert_eq!(
            comparable_anchor(&mismatch, &key, "h"),
            Err(HetPmUnknownReasonV2::AnchorRouteMismatch)
        );
        mismatch = anchor.clone();
        mismatch.quote_model_id = "other".into();
        assert_eq!(
            comparable_anchor(&mismatch, &key, "h"),
            Err(HetPmUnknownReasonV2::AnchorQuoteModelMismatch)
        );
    }

    #[test]
    fn anchor_request_requires_a_new_peak_and_respects_step_and_force_interval() {
        let config = effective();

        let mut first_peak_trajectory = usable_trajectory();
        first_peak_trajectory.peak_mark_price_sol = Some(1.5);
        first_peak_trajectory.peak_sample_slot = first_peak_trajectory.newest_sample_slot;
        first_peak_trajectory.peak_sample_timestamp_ms =
            first_peak_trajectory.newest_sample_timestamp_ms;
        let first_peak = bundle(
            false,
            20_000,
            1.5,
            RouteStatusV1::PumpCurveSupported,
            first_peak_trajectory,
            vitality(VitalityStateV1::Alive, 0),
            None,
        );
        assert!(matches!(
            ExitPolicyV2::evaluate_anchor_request(first_peak.view(), 16_000, &config),
            PeakAnchorPreQuoteDecisionV1::QuoteRequired {
                peak_mark_price_sol,
                ..
            } if peak_mark_price_sol == 1.5
        ));

        let old_anchor = anchor(&config);
        let non_peak = bundle(
            false,
            20_000,
            1.3,
            RouteStatusV1::PumpCurveSupported,
            usable_trajectory(),
            vitality(VitalityStateV1::Alive, 0),
            Some(old_anchor.clone()),
        );
        assert!(matches!(
            ExitPolicyV2::evaluate_anchor_request(non_peak.view(), 100_000, &config),
            PeakAnchorPreQuoteDecisionV1::NoChange
        ));

        let mut small_new_high_trajectory = usable_trajectory();
        small_new_high_trajectory.peak_mark_price_sol = Some(1.55);
        small_new_high_trajectory.peak_sample_slot = small_new_high_trajectory.newest_sample_slot;
        small_new_high_trajectory.peak_sample_timestamp_ms =
            small_new_high_trajectory.newest_sample_timestamp_ms;
        let small_new_high = bundle(
            false,
            20_000,
            1.55,
            RouteStatusV1::PumpCurveSupported,
            small_new_high_trajectory,
            vitality(VitalityStateV1::Alive, 0),
            Some(old_anchor),
        );
        assert!(matches!(
            ExitPolicyV2::evaluate_anchor_request(small_new_high.view(), 14_999, &config),
            PeakAnchorPreQuoteDecisionV1::NoChange
        ));
        assert!(matches!(
            ExitPolicyV2::evaluate_anchor_request(small_new_high.view(), 15_000, &config),
            PeakAnchorPreQuoteDecisionV1::QuoteRequired { .. }
        ));
    }

    #[test]
    fn hierarchy_pending_integrity_crash_and_hard_loss_preempt_lower_gates() {
        let config = effective();
        let pending = bundle(
            true,
            20_000,
            1.2,
            RouteStatusV1::PumpCurveSupported,
            usable_trajectory(),
            vitality(VitalityStateV1::Weak, 3),
            Some(anchor(&config)),
        );
        let result = ExitPolicyV2::evaluate_prequote(
            pending.view(),
            &PreQuoteDecision::Hold,
            &CrashGuardPreQuoteDecision::Disabled,
            &config,
        );
        assert_eq!(result.winning_gate, HetPmGateV2::Pending);

        let unsupported = bundle(
            false,
            20_000,
            1.2,
            RouteStatusV1::CurveCompletePumpSwapUnsupported,
            usable_trajectory(),
            vitality(VitalityStateV1::Weak, 3),
            Some(anchor(&config)),
        );
        let result = ExitPolicyV2::evaluate_prequote(
            unsupported.view(),
            &PreQuoteDecision::Hold,
            &CrashGuardPreQuoteDecision::Disabled,
            &config,
        );
        assert_eq!(result.winning_gate, HetPmGateV2::Integrity);
        assert_eq!(
            result.candidate,
            HetPmCandidateV2::Blocked(HetPmUnknownReasonV2::RouteUnsupported)
        );

        let max_hold_lattice = ExitPolicyV2::evaluate_gate_lattice(
            unsupported.view(),
            &PreQuoteDecision::QuoteRequired {
                candidate: ExitCandidate::from_reason(ExitCandidateReason::AbsoluteMaxHold),
            },
            &CrashGuardPreQuoteDecision::Disabled,
            &config,
        );
        assert!(matches!(
            max_hold_lattice.last(),
            Some(HetPmGatePreQuoteEvidenceV2 {
                reason: HetPmExitReasonV2::AbsoluteMaxHold,
                candidate: HetPmCandidateV2::Blocked(HetPmUnknownReasonV2::RouteUnsupported),
            })
        ));

        let normal = bundle(
            false,
            20_000,
            1.2,
            RouteStatusV1::PumpCurveSupported,
            usable_trajectory(),
            vitality(VitalityStateV1::Weak, 3),
            Some(anchor(&config)),
        );
        let crash = ExitPolicyV2::evaluate_prequote(
            normal.view(),
            &PreQuoteDecision::QuoteRequired {
                candidate: ExitCandidate::from_reason(ExitCandidateReason::StopLoss),
            },
            &CrashGuardPreQuoteDecision::QuoteRequired {
                candidate: ExitCandidate::from_reason(ExitCandidateReason::CrashGuard),
            },
            &config,
        );
        assert_eq!(crash.winning_gate, HetPmGateV2::Crash);

        let hard_loss = ExitPolicyV2::evaluate_prequote(
            normal.view(),
            &PreQuoteDecision::QuoteRequired {
                candidate: ExitCandidate::from_reason(ExitCandidateReason::StopLoss),
            },
            &CrashGuardPreQuoteDecision::Disabled,
            &config,
        );
        assert_eq!(hard_loss.winning_gate, HetPmGateV2::HardLoss);
    }

    #[test]
    fn hierarchy_trailing_vitality_max_hold_and_hold_is_deterministic() {
        let config = effective();
        let trailing = bundle(
            false,
            20_000,
            1.3,
            RouteStatusV1::PumpCurveSupported,
            usable_trajectory(),
            vitality(VitalityStateV1::Weak, 3),
            Some(anchor(&config)),
        );
        let first = ExitPolicyV2::evaluate_prequote(
            trailing.view(),
            &PreQuoteDecision::Hold,
            &CrashGuardPreQuoteDecision::Disabled,
            &config,
        );
        let second = ExitPolicyV2::evaluate_prequote(
            trailing.view(),
            &PreQuoteDecision::Hold,
            &CrashGuardPreQuoteDecision::Disabled,
            &config,
        );
        assert_eq!(first, second);
        assert_eq!(first.winning_gate, HetPmGateV2::ExecutableTrailing);

        let mut non_trailing_trajectory = usable_trajectory();
        non_trailing_trajectory.drawdown_from_peak_bps = Some(0);
        let vitality_candidate = bundle(
            false,
            20_000,
            1.1,
            RouteStatusV1::PumpCurveSupported,
            non_trailing_trajectory,
            vitality(VitalityStateV1::HeartbeatOnly, 3),
            Some(anchor(&config)),
        );
        let result = ExitPolicyV2::evaluate_prequote(
            vitality_candidate.view(),
            &PreQuoteDecision::QuoteRequired {
                candidate: ExitCandidate::from_reason(ExitCandidateReason::AbsoluteMaxHold),
            },
            &CrashGuardPreQuoteDecision::Disabled,
            &config,
        );
        assert_eq!(result.winning_gate, HetPmGateV2::VitalityDecay);

        let mut recovered_trajectory = usable_trajectory();
        recovered_trajectory.drawdown_from_peak_bps = Some(0);
        recovered_trajectory.return_5s_bps = Some(300);
        let recovered = bundle(
            false,
            20_000,
            1.1,
            RouteStatusV1::PumpCurveSupported,
            recovered_trajectory,
            vitality(VitalityStateV1::Weak, 3),
            Some(anchor(&config)),
        );
        let max_hold = ExitPolicyV2::evaluate_prequote(
            recovered.view(),
            &PreQuoteDecision::QuoteRequired {
                candidate: ExitCandidate::from_reason(ExitCandidateReason::AbsoluteMaxHold),
            },
            &CrashGuardPreQuoteDecision::Disabled,
            &config,
        );
        assert_eq!(max_hold.winning_gate, HetPmGateV2::AbsoluteMaxHold);

        let hold = ExitPolicyV2::evaluate_prequote(
            recovered.view(),
            &PreQuoteDecision::Hold,
            &CrashGuardPreQuoteDecision::Disabled,
            &config,
        );
        assert_eq!(hold.winning_gate, HetPmGateV2::Hold);
        assert_eq!(hold.candidate, HetPmCandidateV2::Hold);
    }

    #[test]
    fn executable_trailing_arms_from_peak_mark_return_not_current_return_after_giveback() {
        let config = effective();
        let mut trajectory = usable_trajectory();
        trajectory.peak_mark_price_sol = Some(1.5);
        trajectory.drawdown_from_peak_bps = Some(3_000);
        trajectory.time_since_peak_ms = Some(6_000);
        let snapshot = bundle(
            false,
            20_000,
            1.05,
            RouteStatusV1::PumpCurveSupported,
            trajectory,
            vitality(VitalityStateV1::Alive, 0),
            Some(anchor(&config)),
        );

        let result = ExitPolicyV2::evaluate_prequote(
            snapshot.view(),
            &PreQuoteDecision::Hold,
            &CrashGuardPreQuoteDecision::Disabled,
            &config,
        );
        assert_eq!(result.winning_gate, HetPmGateV2::ExecutableTrailing);
        assert_eq!(
            result.candidate,
            HetPmCandidateV2::QuoteRequired(HetPmExitReasonV2::ExecutableTrailing)
        );

        let lattice = ExitPolicyV2::evaluate_gate_lattice(
            snapshot.view(),
            &PreQuoteDecision::Hold,
            &CrashGuardPreQuoteDecision::Disabled,
            &config,
        );
        assert!(matches!(
            lattice
                .iter()
                .find(|gate| gate.reason == HetPmExitReasonV2::ExecutableTrailing),
            Some(HetPmGatePreQuoteEvidenceV2 {
                candidate: HetPmCandidateV2::QuoteRequired(HetPmExitReasonV2::ExecutableTrailing),
                ..
            })
        ));
    }

    #[test]
    fn same_snapshot_gate_lattice_retains_trailing_beneath_observed_crash() {
        let config = effective();
        let v1_config = crash_v1_config(-20.0);
        let crash_vector = CrashVectorV1::new(
            1.5,
            Some(CrashSampleV1::new(1.4, 19, 15_000)),
            Some(CrashSampleV1::new(1.2, 19, 15_500)),
            Some(CrashSampleV1::new(0.9, 20, 16_000)),
            Some(0),
            2,
            Some(0.35),
            Some(0.40),
            true,
            true,
        );
        let snapshot = bundle_with_crash_vector(
            false,
            20_000,
            1.3,
            RouteStatusV1::PumpCurveSupported,
            usable_trajectory(),
            vitality(VitalityStateV1::Alive, 0),
            Some(anchor(&config)),
            crash_vector,
            v1_config.config_hash(),
        );
        let crash = ExitPolicyV1::evaluate_crash_guard_prequote(&snapshot.base, &v1_config);
        assert!(matches!(
            crash,
            CrashGuardPreQuoteDecision::QuoteRequired { .. }
        ));

        // The legacy observed projection remains hierarchy-only: Crash is the
        // single winner.  Promotion evidence must nevertheless retain the
        // independently evaluated lower Trailing candidate from this exact
        // immutable snapshot, rather than fabricate a second monitor row.
        let observed = ExitPolicyV2::evaluate_prequote(
            snapshot.view(),
            &PreQuoteDecision::Hold,
            &crash,
            &config,
        );
        assert_eq!(observed.winning_gate, HetPmGateV2::Crash);

        let lattice = ExitPolicyV2::evaluate_gate_lattice(
            snapshot.view(),
            &PreQuoteDecision::Hold,
            &crash,
            &config,
        );
        assert_eq!(lattice.len(), 5);
        assert!(matches!(
            lattice[0],
            HetPmGatePreQuoteEvidenceV2 {
                reason: HetPmExitReasonV2::Crash,
                candidate: HetPmCandidateV2::QuoteRequired(HetPmExitReasonV2::Crash),
            }
        ));
        assert!(matches!(
            lattice[2],
            HetPmGatePreQuoteEvidenceV2 {
                reason: HetPmExitReasonV2::ExecutableTrailing,
                candidate: HetPmCandidateV2::QuoteRequired(HetPmExitReasonV2::ExecutableTrailing),
            }
        ));

        let quote = ExecutableExitQuote::new(100, 0.75, 0.75, -0.25, -25.0);
        let quote_key = ExecutableQuoteKeyV2::from_view(snapshot.view());
        let quote_evidence = QuoteEvidenceRevisionV1::new(Some(20), Some(16_000), Some(0));
        let crash_requirement = ExitPolicyV1::crash_guard_quote_requirement(&snapshot.base)
            .expect("real Crash prequote must carry a full-position requirement");
        let mut prepared = comparison_record();
        prepared.v2_gate_evaluations = lattice
            .into_iter()
            .map(|evaluation| {
                let prequote = HetPmPreQuoteEvaluationV2 {
                    candidate: evaluation.candidate,
                    winning_gate: match evaluation.reason {
                        HetPmExitReasonV2::Crash => HetPmGateV2::Crash,
                        HetPmExitReasonV2::HardLoss => HetPmGateV2::HardLoss,
                        HetPmExitReasonV2::ExecutableTrailing => HetPmGateV2::ExecutableTrailing,
                        HetPmExitReasonV2::VitalityDecay => HetPmGateV2::VitalityDecay,
                        HetPmExitReasonV2::AbsoluteMaxHold => HetPmGateV2::AbsoluteMaxHold,
                    },
                    suppressed_gates_mask: 0,
                };
                let final_decision = ExitPolicyV2::finalize_with_quote(
                    snapshot.view(),
                    &prequote,
                    HetPmQuoteFinalizationInputV2 {
                        quote: Some(&quote),
                        quote_key: Some(&quote_key),
                        quote_evidence: Some(quote_evidence),
                        crash_requirement: Some(&crash_requirement),
                    },
                    &v1_config,
                    &config,
                );
                let executable_gross_return_bps = match &final_decision {
                    HetPmFinalDecisionV2::ExitAll {
                        executable_gross_return_bps,
                        ..
                    } => Some(*executable_gross_return_bps),
                    _ => None,
                };
                HetPmGateEvaluationV2 {
                    gate: evaluation.reason,
                    quote_status: ExitPolicyV2::gate_quote_status(
                        &prequote.candidate,
                        &final_decision,
                    ),
                    prequote: prequote.candidate,
                    final_decision,
                    executable_gross_return_bps,
                }
            })
            .collect();
        assert_eq!(
            ExitPolicyV2::select_authoritative_exit(
                &prepared.v2_gate_evaluations,
                CrashGuardMode::ObserveOnly,
            ),
            Some(HetPmExitReasonV2::ExecutableTrailing),
            "an observe-only Crash must not suppress the lower active Trailing exit"
        );
        assert_eq!(
            ExitPolicyV2::select_authoritative_exit(
                &prepared.v2_gate_evaluations,
                CrashGuardMode::AuthoritativeShadow,
            ),
            Some(HetPmExitReasonV2::Crash)
        );
        assert!(prepared.validate_and_serialize().is_ok());
    }

    #[test]
    fn crash_mark_candidate_with_mild_executable_loss_is_rejected() {
        let v1_config = crash_v1_config(-20.0);
        let bundle = crash_bundle(&v1_config);
        let mild_loss = ExecutableExitQuote::new(100, 0.90, 0.90, -0.10, -10.0);

        assert_eq!(
            finalize_crash(
                &bundle,
                &v1_config,
                &mild_loss,
                QuoteEvidenceRevisionV1::new(Some(20), Some(16_000), Some(0)),
            ),
            HetPmFinalDecisionV2::CrashRejectedByQuote {
                reason: CrashGuardQuoteRejectionReason::ExecutableReturnNotSevereEnough,
            }
        );
    }

    #[test]
    fn crash_quote_older_than_candidate_is_blocked() {
        let v1_config = crash_v1_config(-20.0);
        let bundle = crash_bundle(&v1_config);
        let severe_loss = ExecutableExitQuote::new(100, 0.75, 0.75, -0.25, -25.0);

        assert_eq!(
            finalize_crash(
                &bundle,
                &v1_config,
                &severe_loss,
                QuoteEvidenceRevisionV1::new(Some(19), Some(15_999), Some(0)),
            ),
            HetPmFinalDecisionV2::CrashBlockedByData
        );
    }

    #[test]
    fn crash_quantity_mismatch_is_rejected() {
        let v1_config = crash_v1_config(-20.0);
        let bundle = crash_bundle(&v1_config);
        let wrong_quantity = ExecutableExitQuote::new(99, 0.75, 0.75, -0.25, -25.0);

        assert_eq!(
            finalize_crash(
                &bundle,
                &v1_config,
                &wrong_quantity,
                QuoteEvidenceRevisionV1::new(Some(20), Some(16_000), Some(0)),
            ),
            HetPmFinalDecisionV2::CrashRejectedByQuote {
                reason: CrashGuardQuoteRejectionReason::QuoteQuantityMismatch,
            }
        );
    }

    #[test]
    fn crash_confirmed_requires_v1_threshold() {
        let strict_v1 = crash_v1_config(-30.0);
        let strict_bundle = crash_bundle(&strict_v1);
        let quote = ExecutableExitQuote::new(100, 0.75, 0.75, -0.25, -25.0);
        let evidence = QuoteEvidenceRevisionV1::new(Some(20), Some(16_000), Some(0));
        assert_eq!(
            finalize_crash(&strict_bundle, &strict_v1, &quote, evidence),
            HetPmFinalDecisionV2::CrashRejectedByQuote {
                reason: CrashGuardQuoteRejectionReason::ExecutableReturnNotSevereEnough,
            }
        );

        let normal_v1 = crash_v1_config(-20.0);
        let normal_bundle = crash_bundle(&normal_v1);
        assert!(matches!(
            finalize_crash(&normal_bundle, &normal_v1, &quote, evidence),
            HetPmFinalDecisionV2::ExitAll {
                reason: HetPmExitReasonV2::Crash,
                ..
            }
        ));
    }

    #[test]
    fn executable_trailing_requires_comparable_anchor_and_breach() {
        let config = effective();
        let v1_config = crash_v1_config(-20.0);
        let bundle = bundle(
            false,
            20_000,
            1.3,
            RouteStatusV1::PumpCurveSupported,
            usable_trajectory(),
            vitality(VitalityStateV1::Alive, 0),
            Some(anchor(&config)),
        );
        let prequote = ExitPolicyV2::evaluate_prequote(
            bundle.view(),
            &PreQuoteDecision::Hold,
            &CrashGuardPreQuoteDecision::Disabled,
            &config,
        );
        let key = ExecutableQuoteKeyV2::from_view(bundle.view());
        let breached = ExecutableExitQuote::new(100, 1.0, 1.0, 0.0, 0.0);
        assert!(matches!(
            ExitPolicyV2::finalize_with_quote(
                bundle.view(),
                &prequote,
                HetPmQuoteFinalizationInputV2 {
                    quote: Some(&breached),
                    quote_key: Some(&key),
                    quote_evidence: None,
                    crash_requirement: None,
                },
                &v1_config,
                &config
            ),
            HetPmFinalDecisionV2::ExitAll {
                reason: HetPmExitReasonV2::ExecutableTrailing,
                ..
            }
        ));

        let not_breached = ExecutableExitQuote::new(100, 1.2, 1.2, 0.2, 20.0);
        assert_eq!(
            ExitPolicyV2::finalize_with_quote(
                bundle.view(),
                &prequote,
                HetPmQuoteFinalizationInputV2 {
                    quote: Some(&not_breached),
                    quote_key: Some(&key),
                    quote_evidence: None,
                    crash_requirement: None,
                },
                &v1_config,
                &config
            ),
            HetPmFinalDecisionV2::Hold
        );
    }
}
