//! Pure Position Manager Lite V1 exit policy.
//!
//! This module deliberately owns no runtime state and performs no I/O. The
//! engine materializes an immutable snapshot, evaluates it without locks, and
//! applies the result through guarded mutation methods.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use trigger::PriceTruthSource;

use crate::execution::backend::Lane;

use super::config::{
    CrashGuardMode, ExitPolicyV1Config, PostBuyGuardianConfig, DEFAULT_WAIT_FOR_TIMESTOP_MS,
};

pub(super) const EXIT_POLICY_V1_ID: &str = "position_manager_lite_exit_policy_v1";
pub(super) const EXIT_POLICY_V1_VERSION: u16 = 1;
pub(super) const EXECUTION_COST_COVERAGE_UNMODELED: &str = "unmodeled";
pub(super) const EXECUTABLE_QUOTE_GRADE: &str =
    "position_sized_curve_executable_gross_costs_unmodeled";

#[derive(Debug, Clone, Error, PartialEq)]
pub enum ExitPolicyConfigError {
    #[error("take-profit threshold is required for shadow Position Manager")]
    MissingTakeProfit,
    #[error("take-profit threshold must be finite and non-negative")]
    InvalidTakeProfit,
    #[error("stop-loss threshold is required for shadow Position Manager")]
    MissingStopLoss,
    #[error("stop-loss threshold must be finite and within 0..=1")]
    InvalidStopLoss,
    #[error("inactivity timeout must be greater than zero")]
    InvalidInactivityTimeout,
    #[error("quote recovery timeout must be greater than zero")]
    InvalidQuoteRecovery,
    #[error("absolute max-hold timeout must be greater than zero when enabled")]
    InvalidAbsoluteMaxHold,
    #[error("CrashGuard window must be greater than zero when enabled")]
    InvalidCrashWindow,
    #[error("CrashGuard short-window drop must be finite and greater than zero")]
    InvalidCrashShortWindowDrop,
    #[error("CrashGuard peak drawdown must be finite and greater than zero")]
    InvalidCrashPeakDrawdown,
    #[error("CrashGuard requires at least two distinct slots")]
    InvalidCrashDistinctSlots,
    #[error("CrashGuard maximum sample age must be greater than zero")]
    InvalidCrashMaxSampleAge,
    #[error("CrashGuard executable return threshold must be finite and non-positive")]
    InvalidCrashExecutableReturn,
    #[error("effective exit policy config could not be serialized for hashing")]
    ConfigHashSerialization,
}

/// Immutable, validated policy config used by every shadow position.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(super) struct EffectiveExitPolicyV1Config {
    take_profit_fraction: f64,
    stop_loss_fraction: f64,
    inactivity_timeout_ms: u64,
    quote_recovery_ms: u64,
    absolute_max_hold_enabled: bool,
    absolute_max_hold_ms: u64,
    crash_guard_mode: CrashGuardMode,
    crash_window_ms: u64,
    crash_min_short_window_drop_fraction: f64,
    crash_min_peak_drawdown_fraction: f64,
    crash_min_distinct_slots: u8,
    crash_max_sample_age_ms: u64,
    crash_max_executable_return_pct: f64,
    policy_id: &'static str,
    policy_version: u16,
    config_hash: String,
}

impl EffectiveExitPolicyV1Config {
    pub(super) fn from_guardian(
        guardian: &PostBuyGuardianConfig,
    ) -> Result<Self, ExitPolicyConfigError> {
        let take_profit_fraction = guardian
            .target_threshold
            .ok_or(ExitPolicyConfigError::MissingTakeProfit)?
            / 100.0;
        let stop_loss_fraction = guardian
            .stoploss_threshold
            .ok_or(ExitPolicyConfigError::MissingStopLoss)?
            / 100.0;
        Self::new_with_features(
            take_profit_fraction,
            stop_loss_fraction,
            guardian
                .wait_for_timestop
                .unwrap_or(DEFAULT_WAIT_FOR_TIMESTOP_MS),
            guardian.exit_policy_v1.quote_recovery_ms,
            guardian.exit_policy_v1.absolute_max_hold_enabled,
            guardian.exit_policy_v1.absolute_max_hold_ms,
            guardian.exit_policy_v1.crash_guard_mode,
            guardian.exit_policy_v1.crash_window_ms,
            guardian.exit_policy_v1.crash_min_short_window_drop_pct,
            guardian.exit_policy_v1.crash_min_peak_drawdown_pct,
            guardian.exit_policy_v1.crash_min_distinct_slots,
            guardian.exit_policy_v1.crash_max_sample_age_ms,
            guardian.exit_policy_v1.crash_max_executable_return_pct,
        )
    }

    /// PR1-compatible constructor used by focused tests and local adapters.
    /// Feature additions remain disabled unless the full config constructor is
    /// used, preserving the old policy contract exactly.
    #[allow(dead_code)] // Retained for test-only PR1 compatibility adapters.
    pub(super) fn new(
        take_profit_fraction: f64,
        stop_loss_fraction: f64,
        inactivity_timeout_ms: u64,
        quote_recovery_ms: u64,
    ) -> Result<Self, ExitPolicyConfigError> {
        let defaults = ExitPolicyV1Config::default();
        Self::new_with_features(
            take_profit_fraction,
            stop_loss_fraction,
            inactivity_timeout_ms,
            quote_recovery_ms,
            defaults.absolute_max_hold_enabled,
            defaults.absolute_max_hold_ms,
            defaults.crash_guard_mode,
            defaults.crash_window_ms,
            defaults.crash_min_short_window_drop_pct,
            defaults.crash_min_peak_drawdown_pct,
            defaults.crash_min_distinct_slots,
            defaults.crash_max_sample_age_ms,
            defaults.crash_max_executable_return_pct,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn new_with_features(
        take_profit_fraction: f64,
        stop_loss_fraction: f64,
        inactivity_timeout_ms: u64,
        quote_recovery_ms: u64,
        absolute_max_hold_enabled: bool,
        absolute_max_hold_ms: u64,
        crash_guard_mode: CrashGuardMode,
        crash_window_ms: u64,
        crash_min_short_window_drop_pct: f64,
        crash_min_peak_drawdown_pct: f64,
        crash_min_distinct_slots: u8,
        crash_max_sample_age_ms: u64,
        crash_max_executable_return_pct: f64,
    ) -> Result<Self, ExitPolicyConfigError> {
        if !take_profit_fraction.is_finite() || take_profit_fraction < 0.0 {
            return Err(ExitPolicyConfigError::InvalidTakeProfit);
        }
        if !stop_loss_fraction.is_finite() || !(0.0..=1.0).contains(&stop_loss_fraction) {
            return Err(ExitPolicyConfigError::InvalidStopLoss);
        }
        if inactivity_timeout_ms == 0 {
            return Err(ExitPolicyConfigError::InvalidInactivityTimeout);
        }
        if quote_recovery_ms == 0 {
            return Err(ExitPolicyConfigError::InvalidQuoteRecovery);
        }
        if absolute_max_hold_enabled && absolute_max_hold_ms == 0 {
            return Err(ExitPolicyConfigError::InvalidAbsoluteMaxHold);
        }
        if !matches!(crash_guard_mode, CrashGuardMode::Disabled) {
            if crash_window_ms == 0 {
                return Err(ExitPolicyConfigError::InvalidCrashWindow);
            }
            if !crash_min_short_window_drop_pct.is_finite()
                || crash_min_short_window_drop_pct <= 0.0
            {
                return Err(ExitPolicyConfigError::InvalidCrashShortWindowDrop);
            }
            if !crash_min_peak_drawdown_pct.is_finite() || crash_min_peak_drawdown_pct <= 0.0 {
                return Err(ExitPolicyConfigError::InvalidCrashPeakDrawdown);
            }
            if crash_min_distinct_slots < 2 {
                return Err(ExitPolicyConfigError::InvalidCrashDistinctSlots);
            }
            if crash_max_sample_age_ms == 0 {
                return Err(ExitPolicyConfigError::InvalidCrashMaxSampleAge);
            }
            if !crash_max_executable_return_pct.is_finite() || crash_max_executable_return_pct > 0.0
            {
                return Err(ExitPolicyConfigError::InvalidCrashExecutableReturn);
            }
        }

        #[derive(Serialize)]
        struct HashInput {
            take_profit_fraction: f64,
            stop_loss_fraction: f64,
            inactivity_timeout_ms: u64,
            quote_recovery_ms: u64,
            absolute_max_hold_enabled: bool,
            absolute_max_hold_ms: u64,
            crash_guard_mode: CrashGuardMode,
            crash_window_ms: u64,
            crash_min_short_window_drop_pct: f64,
            crash_min_peak_drawdown_pct: f64,
            crash_min_distinct_slots: u8,
            crash_max_sample_age_ms: u64,
            crash_max_executable_return_pct: f64,
            policy_id: &'static str,
            policy_version: u16,
        }

        let hash_input = HashInput {
            take_profit_fraction,
            stop_loss_fraction,
            inactivity_timeout_ms,
            quote_recovery_ms,
            absolute_max_hold_enabled,
            absolute_max_hold_ms,
            crash_guard_mode,
            crash_window_ms,
            crash_min_short_window_drop_pct,
            crash_min_peak_drawdown_pct,
            crash_min_distinct_slots,
            crash_max_sample_age_ms,
            crash_max_executable_return_pct,
            policy_id: EXIT_POLICY_V1_ID,
            policy_version: EXIT_POLICY_V1_VERSION,
        };
        let encoded = serde_json::to_vec(&hash_input)
            .map_err(|_| ExitPolicyConfigError::ConfigHashSerialization)?;
        let config_hash = blake3::hash(&encoded).to_hex().to_string();

        Ok(Self {
            take_profit_fraction,
            stop_loss_fraction,
            inactivity_timeout_ms,
            quote_recovery_ms,
            absolute_max_hold_enabled,
            absolute_max_hold_ms,
            crash_guard_mode,
            crash_window_ms,
            crash_min_short_window_drop_fraction: crash_min_short_window_drop_pct / 100.0,
            crash_min_peak_drawdown_fraction: crash_min_peak_drawdown_pct / 100.0,
            crash_min_distinct_slots,
            crash_max_sample_age_ms,
            crash_max_executable_return_pct,
            policy_id: EXIT_POLICY_V1_ID,
            policy_version: EXIT_POLICY_V1_VERSION,
            config_hash,
        })
    }

    pub(super) fn take_profit_fraction(&self) -> f64 {
        self.take_profit_fraction
    }

    pub(super) fn stop_loss_fraction(&self) -> f64 {
        self.stop_loss_fraction
    }

    pub(super) fn inactivity_timeout_ms(&self) -> u64 {
        self.inactivity_timeout_ms
    }

    pub(super) fn quote_recovery_ms(&self) -> u64 {
        self.quote_recovery_ms
    }

    pub(super) fn absolute_max_hold_enabled(&self) -> bool {
        self.absolute_max_hold_enabled
    }

    pub(super) fn absolute_max_hold_ms(&self) -> u64 {
        self.absolute_max_hold_ms
    }

    pub(super) fn crash_guard_mode(&self) -> CrashGuardMode {
        self.crash_guard_mode
    }

    pub(super) fn crash_window_ms(&self) -> u64 {
        self.crash_window_ms
    }

    pub(super) fn crash_min_short_window_drop_fraction(&self) -> f64 {
        self.crash_min_short_window_drop_fraction
    }

    pub(super) fn crash_min_peak_drawdown_fraction(&self) -> f64 {
        self.crash_min_peak_drawdown_fraction
    }

    pub(super) fn crash_min_distinct_slots(&self) -> u8 {
        self.crash_min_distinct_slots
    }

    pub(super) fn crash_max_sample_age_ms(&self) -> u64 {
        self.crash_max_sample_age_ms
    }

    pub(super) fn crash_max_executable_return_pct(&self) -> f64 {
        self.crash_max_executable_return_pct
    }

    pub(super) fn policy_id(&self) -> &'static str {
        self.policy_id
    }

    pub(super) fn policy_version(&self) -> u16 {
        self.policy_version
    }

    pub(super) fn config_hash(&self) -> &str {
        &self.config_hash
    }

    pub(super) fn status(&self) -> ExitPolicyV1Status {
        ExitPolicyV1Status {
            policy_id: self.policy_id.to_string(),
            policy_version: self.policy_version,
            config_hash: self.config_hash.clone(),
            take_profit_fraction: self.take_profit_fraction,
            stop_loss_fraction: self.stop_loss_fraction,
            inactivity_timeout_ms: self.inactivity_timeout_ms,
            quote_recovery_ms: self.quote_recovery_ms,
            absolute_max_hold_enabled: self.absolute_max_hold_enabled,
            absolute_max_hold_ms: self.absolute_max_hold_ms,
            crash_guard_mode: self.crash_guard_mode,
            crash_window_ms: self.crash_window_ms,
            crash_min_short_window_drop_pct: self.crash_min_short_window_drop_fraction * 100.0,
            crash_min_peak_drawdown_pct: self.crash_min_peak_drawdown_fraction * 100.0,
            crash_min_distinct_slots: self.crash_min_distinct_slots,
            crash_max_sample_age_ms: self.crash_max_sample_age_ms,
            crash_max_executable_return_pct: self.crash_max_executable_return_pct,
        }
    }
}

/// Public, immutable projection of the validated effective policy. It exposes
/// startup evidence without exposing the mutable position state or policy
/// internals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExitPolicyV1Status {
    pub policy_id: String,
    pub policy_version: u16,
    pub config_hash: String,
    pub take_profit_fraction: f64,
    pub stop_loss_fraction: f64,
    pub inactivity_timeout_ms: u64,
    pub quote_recovery_ms: u64,
    pub absolute_max_hold_enabled: bool,
    pub absolute_max_hold_ms: u64,
    pub crash_guard_mode: CrashGuardMode,
    pub crash_window_ms: u64,
    pub crash_min_short_window_drop_pct: f64,
    pub crash_min_peak_drawdown_pct: f64,
    pub crash_min_distinct_slots: u8,
    pub crash_max_sample_age_ms: u64,
    pub crash_max_executable_return_pct: f64,
}

pub fn validate_exit_policy_v1_config(
    guardian: &PostBuyGuardianConfig,
) -> Result<ExitPolicyV1Status, ExitPolicyConfigError> {
    EffectiveExitPolicyV1Config::from_guardian(guardian).map(|config| config.status())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum MarkEvidenceStatus {
    Available,
    Stale,
    Unavailable,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ExitCandidateReason {
    StopLoss,
    TakeProfit,
    Inactivity,
    AbsoluteMaxHold,
    CrashGuard,
}

impl ExitCandidateReason {
    #[allow(dead_code)] // Reserved for structured candidate diagnostics in PR2.
    pub(super) const fn as_label(self) -> &'static str {
        match self {
            Self::StopLoss => "stop_loss",
            Self::TakeProfit => "take_profit",
            Self::Inactivity => "inactivity",
            Self::AbsoluteMaxHold => "absolute_max_hold",
            Self::CrashGuard => "crash_guard",
        }
    }

    pub(super) const fn reason_code(self) -> &'static str {
        match self {
            Self::StopLoss => "stop_loss",
            Self::TakeProfit => "target",
            Self::Inactivity => "time_stop",
            Self::AbsoluteMaxHold => "absolute_max_hold",
            Self::CrashGuard => "crash_guard",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum UnknownEvidenceReason {
    PolicyConfigMismatch,
    MarkUnavailable,
    MarkStale,
    MarkInvalid,
    InvalidEntryPrice,
    InvalidEntryQuantity,
    InvalidRemainingQuantity,
    QuoteUnavailable,
    QuoteStale,
    QuoteSemanticViolation,
    QuoteNoFill,
    QuoteQuantityMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExitCandidate {
    reason: ExitCandidateReason,
}

impl ExitCandidate {
    fn new(reason: ExitCandidateReason) -> Self {
        Self { reason }
    }

    pub(super) fn reason(&self) -> ExitCandidateReason {
        self.reason
    }

    pub(super) fn from_reason(reason: ExitCandidateReason) -> Self {
        Self::new(reason)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PreQuoteDecision {
    Hold,
    UnknownEvidence { reason: UnknownEvidenceReason },
    QuoteRequired { candidate: ExitCandidate },
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ExecutableExitQuote {
    quantity_raw: u64,
    exit_price_sol: f64,
    exit_value_sol: f64,
    gross_pnl_sol: f64,
    gross_return_pct: f64,
}

impl ExecutableExitQuote {
    pub(super) fn new(
        quantity_raw: u64,
        exit_price_sol: f64,
        exit_value_sol: f64,
        gross_pnl_sol: f64,
        gross_return_pct: f64,
    ) -> Self {
        Self {
            quantity_raw,
            exit_price_sol,
            exit_value_sol,
            gross_pnl_sol,
            gross_return_pct,
        }
    }

    pub(super) fn quantity_raw(&self) -> u64 {
        self.quantity_raw
    }

    #[allow(dead_code)]
    pub(super) fn exit_price_sol(&self) -> f64 {
        self.exit_price_sol
    }

    #[allow(dead_code)]
    pub(super) fn exit_value_sol(&self) -> f64 {
        self.exit_value_sol
    }

    #[allow(dead_code)]
    pub(super) fn gross_pnl_sol(&self) -> f64 {
        self.gross_pnl_sol
    }

    #[allow(dead_code)]
    pub(super) fn gross_return_pct(&self) -> f64 {
        self.gross_return_pct
    }

    fn is_resolved(&self) -> bool {
        self.quantity_raw > 0
            && self.exit_price_sol.is_finite()
            && self.exit_price_sol > 0.0
            && self.exit_value_sol.is_finite()
            && self.exit_value_sol > 0.0
            && self.gross_pnl_sol.is_finite()
            && self.gross_return_pct.is_finite()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ExitIntent {
    reason: ExitCandidateReason,
    quantity_raw: u64,
}

impl ExitIntent {
    pub(super) fn reason(&self) -> ExitCandidateReason {
        self.reason
    }

    pub(super) fn quantity_raw(&self) -> u64 {
        self.quantity_raw
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum FinalPolicyDecision {
    #[allow(dead_code)] // Kept in the stable policy contract for future quote-side holds.
    Hold,
    Exit {
        intent: ExitIntent,
    },
    UnknownEvidence {
        reason: UnknownEvidenceReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PositionSnapshotGuard {
    position_id: String,
    position_epoch: u64,
    state_revision: u64,
    remaining_token_amount_raw: u64,
    latest_sample_slot: Option<u64>,
    latest_sample_timestamp_ms: Option<u64>,
}

impl PositionSnapshotGuard {
    pub(super) fn new(
        position_id: String,
        position_epoch: u64,
        state_revision: u64,
        remaining_token_amount_raw: u64,
        latest_sample_slot: Option<u64>,
        latest_sample_timestamp_ms: Option<u64>,
    ) -> Self {
        Self {
            position_id,
            position_epoch,
            state_revision,
            remaining_token_amount_raw,
            latest_sample_slot,
            latest_sample_timestamp_ms,
        }
    }

    pub(super) fn position_id(&self) -> &str {
        &self.position_id
    }

    pub(super) fn position_epoch(&self) -> u64 {
        self.position_epoch
    }

    pub(super) fn state_revision(&self) -> u64 {
        self.state_revision
    }

    pub(super) fn remaining_token_amount_raw(&self) -> u64 {
        self.remaining_token_amount_raw
    }

    #[allow(dead_code)]
    pub(super) fn latest_sample_slot(&self) -> Option<u64> {
        self.latest_sample_slot
    }

    #[allow(dead_code)]
    pub(super) fn latest_sample_timestamp_ms(&self) -> Option<u64> {
        self.latest_sample_timestamp_ms
    }
}

/// One canonical mark sample retained in the compact CrashGuard projection.
/// It deliberately excludes all mutable engine state and all unbounded path
/// history. The engine is responsible for selecting it from its bounded
/// canonical timeline before the pure policy sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct CrashSampleV1 {
    price_sol: f64,
    slot: u64,
    timestamp_ms: u64,
}

impl CrashSampleV1 {
    pub(super) fn new(price_sol: f64, slot: u64, timestamp_ms: u64) -> Self {
        Self {
            price_sol,
            slot,
            timestamp_ms,
        }
    }

    pub(super) fn price_sol(&self) -> f64 {
        self.price_sol
    }

    pub(super) fn slot(&self) -> u64 {
        self.slot
    }

    pub(super) fn timestamp_ms(&self) -> u64 {
        self.timestamp_ms
    }
}

/// Compact immutable CrashGuard evidence. It contains exactly the points that
/// are necessary to prove or reject the V1 predicate: oldest/window start,
/// previous distinct slot, latest sample, canonical peak and provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct CrashVectorV1 {
    peak_price_sol: f64,
    oldest_in_window: Option<CrashSampleV1>,
    previous_distinct_slot: Option<CrashSampleV1>,
    latest: Option<CrashSampleV1>,
    /// Age is materialized by the engine from the canonical sample timestamp.
    /// It is deliberately separate from the normal mark-age field: runtime
    /// time-stop compatibility may observe a current account-state projection,
    /// whereas CrashGuard must never make an old pool write look fresh.
    latest_sample_age_ms: Option<u64>,
    distinct_slots: u8,
    short_window_drop_fraction: Option<f64>,
    peak_drawdown_fraction: Option<f64>,
    monotonic_decrease: bool,
    ordering_valid: bool,
}

impl Default for CrashVectorV1 {
    fn default() -> Self {
        Self {
            peak_price_sol: 0.0,
            oldest_in_window: None,
            previous_distinct_slot: None,
            latest: None,
            latest_sample_age_ms: None,
            distinct_slots: 0,
            short_window_drop_fraction: None,
            peak_drawdown_fraction: None,
            monotonic_decrease: false,
            ordering_valid: false,
        }
    }
}

impl CrashVectorV1 {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        peak_price_sol: f64,
        oldest_in_window: Option<CrashSampleV1>,
        previous_distinct_slot: Option<CrashSampleV1>,
        latest: Option<CrashSampleV1>,
        latest_sample_age_ms: Option<u64>,
        distinct_slots: u8,
        short_window_drop_fraction: Option<f64>,
        peak_drawdown_fraction: Option<f64>,
        monotonic_decrease: bool,
        ordering_valid: bool,
    ) -> Self {
        Self {
            peak_price_sol,
            oldest_in_window,
            previous_distinct_slot,
            latest,
            latest_sample_age_ms,
            distinct_slots,
            short_window_drop_fraction,
            peak_drawdown_fraction,
            monotonic_decrease,
            ordering_valid,
        }
    }

    pub(super) fn oldest_in_window(&self) -> Option<&CrashSampleV1> {
        self.oldest_in_window.as_ref()
    }

    pub(super) fn previous_distinct_slot(&self) -> Option<&CrashSampleV1> {
        self.previous_distinct_slot.as_ref()
    }

    pub(super) fn latest(&self) -> Option<&CrashSampleV1> {
        self.latest.as_ref()
    }

    pub(super) fn latest_sample_age_ms(&self) -> Option<u64> {
        self.latest_sample_age_ms
    }

    pub(super) fn distinct_slots(&self) -> u8 {
        self.distinct_slots
    }

    pub(super) fn short_window_drop_fraction(&self) -> Option<f64> {
        self.short_window_drop_fraction
    }

    pub(super) fn peak_drawdown_fraction(&self) -> Option<f64> {
        self.peak_drawdown_fraction
    }

    pub(super) fn monotonic_decrease(&self) -> bool {
        self.monotonic_decrease
    }

    pub(super) fn ordering_valid(&self) -> bool {
        self.ordering_valid
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CrashGuardObservationState {
    NotTriggered,
    Candidate,
    Confirmed,
    RejectedByQuote,
    BlockedByData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CrashGuardNotTriggeredReason {
    Disabled,
    NotShadowLane,
    InvalidPositionContract,
    PendingProposal,
    MissingSample,
    StaleSample,
    InsufficientDistinctSlots,
    InvalidOrdering,
    NonDescendingPath,
    ShortWindowDropTooSmall,
    PeakDrawdownTooSmall,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CrashGuardPreQuoteDecision {
    Disabled,
    NotTriggered {
        reason: CrashGuardNotTriggeredReason,
    },
    QuoteRequired {
        candidate: ExitCandidate,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CrashGuardQuoteRejectionReason {
    QuoteNotExecutable,
    QuoteQuantityMismatch,
    ExecutableReturnNotSevereEnough,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CrashGuardQuoteDecision {
    Confirmed,
    RejectedByQuote {
        reason: CrashGuardQuoteRejectionReason,
    },
    BlockedByData,
}

/// Provenance of the one lazy executable quote evaluated by CrashGuard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct QuoteEvidenceRevisionV1 {
    slot: Option<u64>,
    timestamp_ms: Option<u64>,
    age_ms: Option<u64>,
}

impl QuoteEvidenceRevisionV1 {
    pub(super) fn new(slot: Option<u64>, timestamp_ms: Option<u64>, age_ms: Option<u64>) -> Self {
        Self {
            slot,
            timestamp_ms,
            age_ms,
        }
    }

    fn is_same_or_newer_and_fresh_than(self, candidate: Self, max_sample_age_ms: u64) -> bool {
        let Some(quote_slot) = self.slot else {
            return false;
        };
        let Some(quote_timestamp_ms) = self.timestamp_ms else {
            return false;
        };
        let Some(quote_age_ms) = self.age_ms else {
            return false;
        };
        if quote_age_ms > max_sample_age_ms {
            return false;
        }
        let (Some(candidate_slot), Some(candidate_timestamp_ms)) =
            (candidate.slot, candidate.timestamp_ms)
        else {
            return false;
        };
        quote_slot >= candidate_slot && quote_timestamp_ms >= candidate_timestamp_ms
    }
}

/// Immutable evidence captured exactly when a CrashGuard action becomes
/// sticky. Retries must prove their fresh executable quote against this
/// original candidate, not against a later prequote result that may only say
/// `PendingProposal`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CrashGuardQuoteRequirementV1 {
    candidate_snapshot_id: String,
    candidate_evidence: QuoteEvidenceRevisionV1,
}

impl CrashGuardQuoteRequirementV1 {
    fn from_snapshot(snapshot: &PostBuyDecisionSnapshot) -> Option<Self> {
        let latest = snapshot.crash_vector().latest()?;
        Some(Self {
            candidate_snapshot_id: snapshot.snapshot_id().to_string(),
            candidate_evidence: QuoteEvidenceRevisionV1::new(
                Some(latest.slot()),
                Some(latest.timestamp_ms()),
                snapshot.crash_vector().latest_sample_age_ms(),
            ),
        })
    }

    fn accepts_quote(
        &self,
        quote_evidence: QuoteEvidenceRevisionV1,
        max_sample_age_ms: u64,
    ) -> bool {
        quote_evidence.is_same_or_newer_and_fresh_than(self.candidate_evidence, max_sample_age_ms)
    }

    pub(super) fn candidate_snapshot_id(&self) -> &str {
        &self.candidate_snapshot_id
    }

    #[cfg(test)]
    fn candidate_evidence(&self) -> QuoteEvidenceRevisionV1 {
        self.candidate_evidence
    }
}

/// Immutable decision boundary. Fields stay private so runtime state can only
/// be observed through this materialized contract.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(super) struct PostBuyDecisionSnapshot {
    guard: PositionSnapshotGuard,
    lane: Lane,
    entry_price_sol: Option<f64>,
    entry_token_amount_raw: u64,
    remaining_token_amount_raw: u64,
    entry_unix_ms: u64,
    absolute_age_ms: u64,
    inactivity_age_ms: u64,
    mark_price_sol: Option<f64>,
    mark_evidence_status: MarkEvidenceStatus,
    mark_source: PriceTruthSource,
    latest_sample_slot: Option<u64>,
    latest_sample_timestamp_ms: Option<u64>,
    latest_sample_age_ms: Option<u64>,
    quote_reserve_base_raw: Option<f64>,
    quote_reserve_quote_sol: Option<f64>,
    mfe_mark_pct: Option<f64>,
    mae_mark_pct: Option<f64>,
    peak_price_sol: f64,
    drawdown_pct: Option<f64>,
    crash_vector: CrashVectorV1,
    has_pending_proposal: bool,
    policy_id: &'static str,
    effective_config_hash: String,
    snapshot_id: String,
}

impl PostBuyDecisionSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        guard: PositionSnapshotGuard,
        lane: Lane,
        entry_price_sol: Option<f64>,
        entry_token_amount_raw: u64,
        remaining_token_amount_raw: u64,
        entry_unix_ms: u64,
        absolute_age_ms: u64,
        inactivity_age_ms: u64,
        mark_price_sol: Option<f64>,
        mark_evidence_status: MarkEvidenceStatus,
        mark_source: PriceTruthSource,
        latest_sample_slot: Option<u64>,
        latest_sample_timestamp_ms: Option<u64>,
        latest_sample_age_ms: Option<u64>,
        quote_reserve_base_raw: Option<f64>,
        quote_reserve_quote_sol: Option<f64>,
        mfe_mark_pct: Option<f64>,
        mae_mark_pct: Option<f64>,
        peak_price_sol: f64,
        drawdown_pct: Option<f64>,
        crash_vector: CrashVectorV1,
        has_pending_proposal: bool,
        effective_config_hash: String,
    ) -> Self {
        let snapshot_id = format!(
            "{}:{}:{}:{}:{}:{}",
            guard.position_id,
            guard.position_epoch,
            guard.state_revision,
            guard.remaining_token_amount_raw,
            latest_sample_slot
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            latest_sample_timestamp_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        );
        Self {
            guard,
            lane,
            entry_price_sol,
            entry_token_amount_raw,
            remaining_token_amount_raw,
            entry_unix_ms,
            absolute_age_ms,
            inactivity_age_ms,
            mark_price_sol,
            mark_evidence_status,
            mark_source,
            latest_sample_slot,
            latest_sample_timestamp_ms,
            latest_sample_age_ms,
            quote_reserve_base_raw,
            quote_reserve_quote_sol,
            mfe_mark_pct,
            mae_mark_pct,
            peak_price_sol,
            drawdown_pct,
            crash_vector,
            has_pending_proposal,
            policy_id: EXIT_POLICY_V1_ID,
            effective_config_hash,
            snapshot_id,
        }
    }

    pub(super) fn guard(&self) -> &PositionSnapshotGuard {
        &self.guard
    }

    pub(super) fn lane(&self) -> Lane {
        self.lane
    }

    pub(super) fn entry_price_sol(&self) -> Option<f64> {
        self.entry_price_sol
    }

    pub(super) fn entry_token_amount_raw(&self) -> u64 {
        self.entry_token_amount_raw
    }

    pub(super) fn remaining_token_amount_raw(&self) -> u64 {
        self.remaining_token_amount_raw
    }

    pub(super) fn absolute_age_ms(&self) -> u64 {
        self.absolute_age_ms
    }

    pub(super) fn inactivity_age_ms(&self) -> u64 {
        self.inactivity_age_ms
    }

    pub(super) fn mark_price_sol(&self) -> Option<f64> {
        self.mark_price_sol
    }

    pub(super) fn mark_evidence_status(&self) -> MarkEvidenceStatus {
        self.mark_evidence_status
    }

    pub(super) fn mark_source(&self) -> PriceTruthSource {
        self.mark_source
    }

    pub(super) fn latest_sample_slot(&self) -> Option<u64> {
        self.latest_sample_slot
    }

    pub(super) fn latest_sample_timestamp_ms(&self) -> Option<u64> {
        self.latest_sample_timestamp_ms
    }

    pub(super) fn latest_sample_age_ms(&self) -> Option<u64> {
        self.latest_sample_age_ms
    }

    pub(super) fn quote_reserve_base_raw(&self) -> Option<f64> {
        self.quote_reserve_base_raw
    }

    pub(super) fn quote_reserve_quote_sol(&self) -> Option<f64> {
        self.quote_reserve_quote_sol
    }

    pub(super) fn mfe_mark_pct(&self) -> Option<f64> {
        self.mfe_mark_pct
    }

    pub(super) fn mae_mark_pct(&self) -> Option<f64> {
        self.mae_mark_pct
    }

    pub(super) fn crash_vector(&self) -> &CrashVectorV1 {
        &self.crash_vector
    }

    pub(super) fn has_pending_proposal(&self) -> bool {
        self.has_pending_proposal
    }

    pub(super) fn snapshot_id(&self) -> &str {
        &self.snapshot_id
    }

    pub(super) fn effective_config_hash(&self) -> &str {
        &self.effective_config_hash
    }

    pub(super) fn policy_id(&self) -> &'static str {
        self.policy_id
    }
}

pub(super) struct ExitPolicyV1;

impl ExitPolicyV1 {
    fn validate_snapshot_contract(
        snapshot: &PostBuyDecisionSnapshot,
        config: &EffectiveExitPolicyV1Config,
    ) -> Result<(), UnknownEvidenceReason> {
        if snapshot.policy_id() != config.policy_id()
            || snapshot.effective_config_hash() != config.config_hash()
        {
            return Err(UnknownEvidenceReason::PolicyConfigMismatch);
        }
        if snapshot.entry_token_amount_raw() == 0 {
            return Err(UnknownEvidenceReason::InvalidEntryQuantity);
        }
        if snapshot.remaining_token_amount_raw() == 0 {
            return Err(UnknownEvidenceReason::InvalidRemainingQuantity);
        }
        if snapshot
            .entry_price_sol()
            .is_none_or(|value| !value.is_finite() || value <= 0.0)
        {
            return Err(UnknownEvidenceReason::InvalidEntryPrice);
        }
        Ok(())
    }

    pub(super) fn evaluate_prequote(
        snapshot: &PostBuyDecisionSnapshot,
        config: &EffectiveExitPolicyV1Config,
    ) -> PreQuoteDecision {
        if !matches!(snapshot.lane(), Lane::Shadow) {
            return PreQuoteDecision::Hold;
        }
        if let Err(reason) = Self::validate_snapshot_contract(snapshot, config) {
            return PreQuoteDecision::UnknownEvidence { reason };
        }
        let Some(entry_price) = snapshot.entry_price_sol() else {
            return PreQuoteDecision::UnknownEvidence {
                reason: UnknownEvidenceReason::InvalidEntryPrice,
            };
        };

        if snapshot.has_pending_proposal() {
            // The engine supplies the sticky proposal's original reason. It
            // never asks the pure policy to manufacture a second candidate.
            return PreQuoteDecision::Hold;
        }

        let inactivity_due = snapshot.inactivity_age_ms() >= config.inactivity_timeout_ms();
        let absolute_max_hold_due = config.absolute_max_hold_enabled()
            && snapshot.absolute_age_ms() >= config.absolute_max_hold_ms();
        let mark_price = match snapshot.mark_evidence_status() {
            MarkEvidenceStatus::Available => snapshot
                .mark_price_sol()
                .filter(|value| value.is_finite() && *value > 0.0),
            MarkEvidenceStatus::Stale
            | MarkEvidenceStatus::Unavailable
            | MarkEvidenceStatus::Invalid => None,
        };
        let Some(mark_price) = mark_price else {
            if inactivity_due {
                return PreQuoteDecision::QuoteRequired {
                    candidate: ExitCandidate::new(ExitCandidateReason::Inactivity),
                };
            }
            if absolute_max_hold_due {
                return PreQuoteDecision::QuoteRequired {
                    candidate: ExitCandidate::new(ExitCandidateReason::AbsoluteMaxHold),
                };
            }
            return PreQuoteDecision::UnknownEvidence {
                reason: match snapshot.mark_evidence_status() {
                    MarkEvidenceStatus::Invalid | MarkEvidenceStatus::Available => {
                        UnknownEvidenceReason::MarkInvalid
                    }
                    MarkEvidenceStatus::Stale => UnknownEvidenceReason::MarkStale,
                    MarkEvidenceStatus::Unavailable => UnknownEvidenceReason::MarkUnavailable,
                },
            };
        };

        let lower = entry_price * (1.0 - config.stop_loss_fraction());
        let upper = entry_price * (1.0 + config.take_profit_fraction());
        if mark_price <= lower {
            PreQuoteDecision::QuoteRequired {
                candidate: ExitCandidate::new(ExitCandidateReason::StopLoss),
            }
        } else if mark_price >= upper {
            PreQuoteDecision::QuoteRequired {
                candidate: ExitCandidate::new(ExitCandidateReason::TakeProfit),
            }
        } else if inactivity_due {
            PreQuoteDecision::QuoteRequired {
                candidate: ExitCandidate::new(ExitCandidateReason::Inactivity),
            }
        } else if absolute_max_hold_due {
            PreQuoteDecision::QuoteRequired {
                candidate: ExitCandidate::new(ExitCandidateReason::AbsoluteMaxHold),
            }
        } else {
            PreQuoteDecision::Hold
        }
    }

    /// Evaluate the cheap, deterministic CrashGuard predicate. This performs
    /// no quote calculation and never mutates a position. A caller may use
    /// `QuoteRequired` either as observation-only evidence or, in the strictly
    /// gated shadow mode, as a pre-emptive exit candidate.
    pub(super) fn evaluate_crash_guard_prequote(
        snapshot: &PostBuyDecisionSnapshot,
        config: &EffectiveExitPolicyV1Config,
    ) -> CrashGuardPreQuoteDecision {
        if matches!(config.crash_guard_mode(), CrashGuardMode::Disabled) {
            return CrashGuardPreQuoteDecision::Disabled;
        }
        if !matches!(snapshot.lane(), Lane::Shadow) {
            return CrashGuardPreQuoteDecision::NotTriggered {
                reason: CrashGuardNotTriggeredReason::NotShadowLane,
            };
        }
        if Self::validate_snapshot_contract(snapshot, config).is_err() {
            return CrashGuardPreQuoteDecision::NotTriggered {
                reason: CrashGuardNotTriggeredReason::InvalidPositionContract,
            };
        }
        if snapshot.has_pending_proposal() {
            return CrashGuardPreQuoteDecision::NotTriggered {
                reason: CrashGuardNotTriggeredReason::PendingProposal,
            };
        }

        let vector = snapshot.crash_vector();
        let Some(_latest) = vector.latest() else {
            return CrashGuardPreQuoteDecision::NotTriggered {
                reason: CrashGuardNotTriggeredReason::MissingSample,
            };
        };
        if vector
            .latest_sample_age_ms()
            .is_none_or(|age_ms| age_ms > config.crash_max_sample_age_ms())
        {
            return CrashGuardPreQuoteDecision::NotTriggered {
                reason: CrashGuardNotTriggeredReason::StaleSample,
            };
        }
        if vector.distinct_slots() < config.crash_min_distinct_slots() {
            return CrashGuardPreQuoteDecision::NotTriggered {
                reason: CrashGuardNotTriggeredReason::InsufficientDistinctSlots,
            };
        }
        if !vector.ordering_valid() {
            return CrashGuardPreQuoteDecision::NotTriggered {
                reason: CrashGuardNotTriggeredReason::InvalidOrdering,
            };
        }
        if !vector.monotonic_decrease() {
            return CrashGuardPreQuoteDecision::NotTriggered {
                reason: CrashGuardNotTriggeredReason::NonDescendingPath,
            };
        }
        if vector
            .short_window_drop_fraction()
            .is_none_or(|drop| drop < config.crash_min_short_window_drop_fraction())
        {
            return CrashGuardPreQuoteDecision::NotTriggered {
                reason: CrashGuardNotTriggeredReason::ShortWindowDropTooSmall,
            };
        }
        if vector
            .peak_drawdown_fraction()
            .is_none_or(|drawdown| drawdown < config.crash_min_peak_drawdown_fraction())
        {
            return CrashGuardPreQuoteDecision::NotTriggered {
                reason: CrashGuardNotTriggeredReason::PeakDrawdownTooSmall,
            };
        }

        CrashGuardPreQuoteDecision::QuoteRequired {
            candidate: ExitCandidate::new(ExitCandidateReason::CrashGuard),
        }
    }

    /// Confirm a cheap CrashGuard candidate with the exact lazy, full-position
    /// executable quote used by the normal exit path. A mark-only crash is
    /// never sufficient for `Confirmed`.
    pub(super) fn evaluate_crash_guard_quote(
        snapshot: &PostBuyDecisionSnapshot,
        quote: &ExecutableExitQuote,
        quote_evidence: QuoteEvidenceRevisionV1,
        requirement: &CrashGuardQuoteRequirementV1,
        config: &EffectiveExitPolicyV1Config,
    ) -> CrashGuardQuoteDecision {
        if Self::validate_snapshot_contract(snapshot, config).is_err() {
            return CrashGuardQuoteDecision::BlockedByData;
        }
        if quote.quantity_raw() != snapshot.remaining_token_amount_raw() {
            return CrashGuardQuoteDecision::RejectedByQuote {
                reason: CrashGuardQuoteRejectionReason::QuoteQuantityMismatch,
            };
        }
        if !quote.is_resolved() {
            return CrashGuardQuoteDecision::RejectedByQuote {
                reason: CrashGuardQuoteRejectionReason::QuoteNotExecutable,
            };
        }
        if !requirement.accepts_quote(quote_evidence, config.crash_max_sample_age_ms()) {
            return CrashGuardQuoteDecision::BlockedByData;
        }
        if quote.gross_return_pct() > config.crash_max_executable_return_pct() {
            return CrashGuardQuoteDecision::RejectedByQuote {
                reason: CrashGuardQuoteRejectionReason::ExecutableReturnNotSevereEnough,
            };
        }
        CrashGuardQuoteDecision::Confirmed
    }

    /// Capture the raw canonical evidence that proved a CrashGuard candidate.
    /// The engine persists this compact contract inside a pending action so a
    /// retry cannot silently inherit a later `PendingProposal` prequote.
    pub(super) fn crash_guard_quote_requirement(
        snapshot: &PostBuyDecisionSnapshot,
    ) -> Option<CrashGuardQuoteRequirementV1> {
        CrashGuardQuoteRequirementV1::from_snapshot(snapshot)
    }

    pub(super) fn finalize_with_quote(
        snapshot: &PostBuyDecisionSnapshot,
        candidate: &ExitCandidate,
        quote: &ExecutableExitQuote,
        config: &EffectiveExitPolicyV1Config,
    ) -> FinalPolicyDecision {
        if let Err(reason) = Self::validate_snapshot_contract(snapshot, config) {
            return FinalPolicyDecision::UnknownEvidence { reason };
        }
        if quote.quantity_raw() != snapshot.remaining_token_amount_raw() {
            return FinalPolicyDecision::UnknownEvidence {
                reason: UnknownEvidenceReason::QuoteQuantityMismatch,
            };
        }
        if !quote.is_resolved() {
            return FinalPolicyDecision::UnknownEvidence {
                reason: UnknownEvidenceReason::QuoteNoFill,
            };
        }
        FinalPolicyDecision::Exit {
            intent: ExitIntent {
                reason: candidate.reason(),
                quantity_raw: quote.quantity_raw(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> EffectiveExitPolicyV1Config {
        EffectiveExitPolicyV1Config::new(0.50, 0.50, 30_000, 5_000).unwrap()
    }

    fn feature_config(
        absolute_max_hold_enabled: bool,
        crash_guard_mode: CrashGuardMode,
    ) -> EffectiveExitPolicyV1Config {
        EffectiveExitPolicyV1Config::new_with_features(
            0.50,
            0.50,
            30_000,
            5_000,
            absolute_max_hold_enabled,
            120_000,
            crash_guard_mode,
            1_500,
            25.0,
            30.0,
            2,
            1_500,
            -20.0,
        )
        .unwrap()
    }

    fn snapshot_for_config(
        config: &EffectiveExitPolicyV1Config,
        mark: Option<f64>,
        absolute_age_ms: u64,
        inactivity_ms: u64,
        crash_vector: CrashVectorV1,
    ) -> PostBuyDecisionSnapshot {
        let guard = PositionSnapshotGuard::new("p1".to_string(), 1, 7, 100, Some(10), Some(20));
        PostBuyDecisionSnapshot::new(
            guard,
            Lane::Shadow,
            Some(1.0),
            100,
            100,
            1,
            absolute_age_ms,
            inactivity_ms,
            mark,
            if mark.is_some() {
                MarkEvidenceStatus::Available
            } else {
                MarkEvidenceStatus::Unavailable
            },
            PriceTruthSource::CanonicalAccountStateSnapshot,
            Some(10),
            Some(20),
            Some(0),
            Some(1_000_000.0),
            Some(10.0),
            Some(50.0),
            Some(-50.0),
            1.0,
            Some(0.0),
            crash_vector,
            false,
            config.config_hash().to_string(),
        )
    }

    fn snapshot(mark: Option<f64>, inactivity_ms: u64) -> PostBuyDecisionSnapshot {
        let config = config();
        snapshot_for_config(&config, mark, 10, inactivity_ms, CrashVectorV1::default())
    }

    fn crash_vector(
        oldest_price: f64,
        previous_price: f64,
        latest_price: f64,
        latest_age_ms: u64,
        distinct_slots: u8,
        monotonic_decrease: bool,
        ordering_valid: bool,
    ) -> CrashVectorV1 {
        CrashVectorV1::new(
            1.0,
            Some(CrashSampleV1::new(oldest_price, 100, 10_000)),
            Some(CrashSampleV1::new(previous_price, 101, 10_700)),
            Some(CrashSampleV1::new(latest_price, 102, 11_000)),
            Some(latest_age_ms),
            distinct_slots,
            Some(((oldest_price - latest_price) / oldest_price).max(0.0)),
            Some(((1.0 - latest_price) / 1.0).max(0.0)),
            monotonic_decrease,
            ordering_valid,
        )
    }

    fn reason(decision: PreQuoteDecision) -> Option<ExitCandidateReason> {
        match decision {
            PreQuoteDecision::QuoteRequired { candidate } => Some(candidate.reason()),
            _ => None,
        }
    }

    #[test]
    fn exact_stop_loss_boundary_and_just_above() {
        assert_eq!(
            reason(ExitPolicyV1::evaluate_prequote(
                &snapshot(Some(0.5), 0),
                &config()
            )),
            Some(ExitCandidateReason::StopLoss)
        );
        assert_eq!(
            ExitPolicyV1::evaluate_prequote(&snapshot(Some(0.500_001), 0), &config()),
            PreQuoteDecision::Hold
        );
    }

    #[test]
    fn exact_take_profit_boundary_and_just_below() {
        assert_eq!(
            reason(ExitPolicyV1::evaluate_prequote(
                &snapshot(Some(1.5), 0),
                &config()
            )),
            Some(ExitCandidateReason::TakeProfit)
        );
        assert_eq!(
            ExitPolicyV1::evaluate_prequote(&snapshot(Some(1.499_999), 0), &config()),
            PreQuoteDecision::Hold
        );
    }

    #[test]
    fn inactivity_boundary_preserves_priority() {
        assert_eq!(
            ExitPolicyV1::evaluate_prequote(&snapshot(Some(1.0), 29_999), &config()),
            PreQuoteDecision::Hold
        );
        assert_eq!(
            reason(ExitPolicyV1::evaluate_prequote(
                &snapshot(Some(0.5), 30_000),
                &config()
            )),
            Some(ExitCandidateReason::StopLoss)
        );
        assert_eq!(
            reason(ExitPolicyV1::evaluate_prequote(
                &snapshot(Some(1.5), 30_000),
                &config()
            )),
            Some(ExitCandidateReason::TakeProfit)
        );
        assert_eq!(
            reason(ExitPolicyV1::evaluate_prequote(
                &snapshot(Some(1.0), 30_000),
                &config()
            )),
            Some(ExitCandidateReason::Inactivity)
        );
    }

    #[test]
    fn missing_mark_is_diagnostic_until_time_condition() {
        assert_eq!(
            ExitPolicyV1::evaluate_prequote(&snapshot(None, 29_999), &config()),
            PreQuoteDecision::UnknownEvidence {
                reason: UnknownEvidenceReason::MarkUnavailable
            }
        );
        assert_eq!(
            reason(ExitPolicyV1::evaluate_prequote(
                &snapshot(None, 30_000),
                &config()
            )),
            Some(ExitCandidateReason::Inactivity)
        );
    }

    #[test]
    fn stale_mark_cannot_create_price_exit_but_time_condition_still_can() {
        let mut before_time_stop = snapshot(Some(1.5), 29_999);
        before_time_stop.mark_evidence_status = MarkEvidenceStatus::Stale;
        assert_eq!(
            ExitPolicyV1::evaluate_prequote(&before_time_stop, &config()),
            PreQuoteDecision::UnknownEvidence {
                reason: UnknownEvidenceReason::MarkStale
            }
        );

        let mut at_time_stop = snapshot(Some(1.5), 30_000);
        at_time_stop.mark_evidence_status = MarkEvidenceStatus::Stale;
        assert_eq!(
            reason(ExitPolicyV1::evaluate_prequote(&at_time_stop, &config())),
            Some(ExitCandidateReason::Inactivity)
        );
    }

    #[test]
    fn resolved_quote_requires_full_remaining_quantity() {
        let snapshot = snapshot(Some(1.5), 0);
        let candidate = ExitCandidate::new(ExitCandidateReason::TakeProfit);
        let wrong = ExecutableExitQuote::new(99, 1.4, 1.4, 0.4, 40.0);
        assert_eq!(
            ExitPolicyV1::finalize_with_quote(&snapshot, &candidate, &wrong, &config()),
            FinalPolicyDecision::UnknownEvidence {
                reason: UnknownEvidenceReason::QuoteQuantityMismatch
            }
        );
        let quote = ExecutableExitQuote::new(100, 1.4, 1.4, 0.4, 40.0);
        assert!(matches!(
            ExitPolicyV1::finalize_with_quote(&snapshot, &candidate, &quote, &config()),
            FinalPolicyDecision::Exit { .. }
        ));
    }

    #[test]
    fn config_hash_is_deterministic_and_sensitive() {
        let base = config();
        let same = config();
        let changed = EffectiveExitPolicyV1Config::new(0.51, 0.50, 30_000, 5_000).unwrap();
        assert_eq!(base.config_hash(), same.config_hash());
        assert_ne!(base.config_hash(), changed.config_hash());
    }

    #[test]
    fn config_hash_is_sensitive_to_pr2_features() {
        let disabled = feature_config(false, CrashGuardMode::Disabled);
        let max_hold = feature_config(true, CrashGuardMode::Disabled);
        let crash_observe = feature_config(true, CrashGuardMode::ObserveOnly);
        assert_ne!(disabled.config_hash(), max_hold.config_hash());
        assert_ne!(max_hold.config_hash(), crash_observe.config_hash());
    }

    #[test]
    fn absolute_max_hold_has_exact_boundary_and_can_be_disabled() {
        let enabled = feature_config(true, CrashGuardMode::Disabled);
        let disabled = feature_config(false, CrashGuardMode::Disabled);
        assert_eq!(
            ExitPolicyV1::evaluate_prequote(
                &snapshot_for_config(&enabled, Some(1.0), 119_999, 1, CrashVectorV1::default()),
                &enabled,
            ),
            PreQuoteDecision::Hold
        );
        assert_eq!(
            reason(ExitPolicyV1::evaluate_prequote(
                &snapshot_for_config(&enabled, Some(1.0), 120_000, 1, CrashVectorV1::default()),
                &enabled,
            )),
            Some(ExitCandidateReason::AbsoluteMaxHold)
        );
        assert_eq!(
            ExitPolicyV1::evaluate_prequote(
                &snapshot_for_config(&disabled, Some(1.0), 120_000, 1, CrashVectorV1::default()),
                &disabled,
            ),
            PreQuoteDecision::Hold
        );
    }

    #[test]
    fn inactivity_precedes_absolute_max_hold() {
        let config = feature_config(true, CrashGuardMode::Disabled);
        assert_eq!(
            reason(ExitPolicyV1::evaluate_prequote(
                &snapshot_for_config(
                    &config,
                    Some(1.0),
                    120_000,
                    30_000,
                    CrashVectorV1::default(),
                ),
                &config,
            )),
            Some(ExitCandidateReason::Inactivity)
        );
    }

    #[test]
    fn crash_guard_candidate_requires_every_mark_evidence_predicate() {
        let config = feature_config(true, CrashGuardMode::ObserveOnly);
        let candidate_snapshot = snapshot_for_config(
            &config,
            Some(0.70),
            1,
            1,
            crash_vector(1.0, 0.80, 0.70, 1_500, 3, true, true),
        );
        assert!(matches!(
            ExitPolicyV1::evaluate_crash_guard_prequote(&candidate_snapshot, &config),
            CrashGuardPreQuoteDecision::QuoteRequired { .. }
        ));

        let same_slot = snapshot_for_config(
            &config,
            Some(0.70),
            1,
            1,
            crash_vector(1.0, 0.80, 0.70, 1_500, 1, true, true),
        );
        assert!(matches!(
            ExitPolicyV1::evaluate_crash_guard_prequote(&same_slot, &config),
            CrashGuardPreQuoteDecision::NotTriggered {
                reason: CrashGuardNotTriggeredReason::InsufficientDistinctSlots
            }
        ));

        let stale = snapshot_for_config(
            &config,
            Some(0.70),
            1,
            1,
            crash_vector(1.0, 0.80, 0.70, 1_501, 3, true, true),
        );
        assert!(matches!(
            ExitPolicyV1::evaluate_crash_guard_prequote(&stale, &config),
            CrashGuardPreQuoteDecision::NotTriggered {
                reason: CrashGuardNotTriggeredReason::StaleSample
            }
        ));

        let reversed = snapshot_for_config(
            &config,
            Some(0.70),
            1,
            1,
            crash_vector(1.0, 0.80, 0.70, 1_500, 3, true, false),
        );
        assert!(matches!(
            ExitPolicyV1::evaluate_crash_guard_prequote(&reversed, &config),
            CrashGuardPreQuoteDecision::NotTriggered {
                reason: CrashGuardNotTriggeredReason::InvalidOrdering
            }
        ));

        let non_descending = snapshot_for_config(
            &config,
            Some(0.70),
            1,
            1,
            crash_vector(1.0, 0.80, 0.70, 1_500, 3, false, true),
        );
        assert!(matches!(
            ExitPolicyV1::evaluate_crash_guard_prequote(&non_descending, &config),
            CrashGuardPreQuoteDecision::NotTriggered {
                reason: CrashGuardNotTriggeredReason::NonDescendingPath
            }
        ));

        let short_drop = snapshot_for_config(
            &config,
            Some(0.90),
            1,
            1,
            crash_vector(1.0, 0.95, 0.90, 1_500, 3, true, true),
        );
        assert!(matches!(
            ExitPolicyV1::evaluate_crash_guard_prequote(&short_drop, &config),
            CrashGuardPreQuoteDecision::NotTriggered {
                reason: CrashGuardNotTriggeredReason::ShortWindowDropTooSmall
            }
        ));

        let peak_drawdown = snapshot_for_config(
            &config,
            Some(0.80),
            1,
            1,
            // The short-window drop must pass here; only the drawdown from
            // the independently tracked peak is intentionally insufficient.
            CrashVectorV1::new(
                1.10,
                Some(CrashSampleV1::new(1.10, 100, 10_000)),
                Some(CrashSampleV1::new(0.90, 101, 10_700)),
                Some(CrashSampleV1::new(0.80, 102, 11_000)),
                Some(1_500),
                3,
                Some((1.10 - 0.80) / 1.10),
                Some((1.10 - 0.80) / 1.10),
                true,
                true,
            ),
        );
        assert!(matches!(
            ExitPolicyV1::evaluate_crash_guard_prequote(&peak_drawdown, &config),
            CrashGuardPreQuoteDecision::NotTriggered {
                reason: CrashGuardNotTriggeredReason::PeakDrawdownTooSmall
            }
        ));
    }

    #[test]
    fn crash_guard_quote_requires_full_fresh_and_severe_executable_truth() {
        let config = feature_config(true, CrashGuardMode::ObserveOnly);
        let snapshot = snapshot_for_config(
            &config,
            Some(0.70),
            1,
            1,
            crash_vector(1.0, 0.80, 0.70, 100, 3, true, true),
        );
        let requirement = ExitPolicyV1::crash_guard_quote_requirement(&snapshot)
            .expect("CrashGuard candidate provenance");
        assert_eq!(requirement.candidate_snapshot_id(), snapshot.snapshot_id());
        assert_eq!(
            requirement.candidate_evidence(),
            QuoteEvidenceRevisionV1::new(Some(102), Some(11_000), Some(100))
        );
        let fresh = QuoteEvidenceRevisionV1::new(Some(102), Some(11_000), Some(100));
        let confirmed_quote = ExecutableExitQuote::new(100, 0.70, 0.70, -0.30, -30.0);
        assert_eq!(
            ExitPolicyV1::evaluate_crash_guard_quote(
                &snapshot,
                &confirmed_quote,
                fresh,
                &requirement,
                &config,
            ),
            CrashGuardQuoteDecision::Confirmed
        );

        let wrong_quantity = ExecutableExitQuote::new(99, 0.70, 0.70, -0.30, -30.0);
        assert!(matches!(
            ExitPolicyV1::evaluate_crash_guard_quote(
                &snapshot,
                &wrong_quantity,
                fresh,
                &requirement,
                &config,
            ),
            CrashGuardQuoteDecision::RejectedByQuote {
                reason: CrashGuardQuoteRejectionReason::QuoteQuantityMismatch
            }
        ));

        let no_fill = ExecutableExitQuote::new(100, 0.0, 0.0, 0.0, 0.0);
        assert!(matches!(
            ExitPolicyV1::evaluate_crash_guard_quote(
                &snapshot,
                &no_fill,
                fresh,
                &requirement,
                &config,
            ),
            CrashGuardQuoteDecision::RejectedByQuote {
                reason: CrashGuardQuoteRejectionReason::QuoteNotExecutable
            }
        ));

        let not_severe = ExecutableExitQuote::new(100, 0.90, 0.90, -0.10, -10.0);
        assert!(matches!(
            ExitPolicyV1::evaluate_crash_guard_quote(
                &snapshot,
                &not_severe,
                fresh,
                &requirement,
                &config,
            ),
            CrashGuardQuoteDecision::RejectedByQuote {
                reason: CrashGuardQuoteRejectionReason::ExecutableReturnNotSevereEnough
            }
        ));

        let stale_or_older = QuoteEvidenceRevisionV1::new(Some(101), Some(10_999), Some(100));
        assert_eq!(
            ExitPolicyV1::evaluate_crash_guard_quote(
                &snapshot,
                &confirmed_quote,
                stale_or_older,
                &requirement,
                &config,
            ),
            CrashGuardQuoteDecision::BlockedByData
        );
    }

    #[test]
    fn identical_snapshot_and_config_are_deterministic() {
        let snapshot = snapshot(Some(1.5), 30_000);
        let config = config();
        assert_eq!(
            ExitPolicyV1::evaluate_prequote(&snapshot, &config),
            ExitPolicyV1::evaluate_prequote(&snapshot, &config)
        );
    }

    #[test]
    fn config_hash_mismatch_fails_closed_before_quote() {
        let snapshot = snapshot(Some(1.5), 30_000);
        let changed = EffectiveExitPolicyV1Config::new(0.51, 0.50, 30_000, 5_000).unwrap();
        assert_eq!(
            ExitPolicyV1::evaluate_prequote(&snapshot, &changed),
            PreQuoteDecision::UnknownEvidence {
                reason: UnknownEvidenceReason::PolicyConfigMismatch
            }
        );
    }

    #[test]
    fn production_policy_source_has_no_runtime_or_io_dependencies() {
        let source = include_str!("exit_policy_v1.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production source");
        for forbidden in [
            concat!("Rw", "Lock"),
            concat!("Mutex"),
            concat!("Rpc", "Client"),
            concat!("Instant"),
            concat!("Account", "StateReducer"),
            concat!("Shadow", "PositionBook"),
            concat!("tokio", "::"),
        ] {
            assert!(
                !production.contains(forbidden),
                "pure policy production source contains forbidden dependency: {forbidden}"
            );
        }
    }
}
