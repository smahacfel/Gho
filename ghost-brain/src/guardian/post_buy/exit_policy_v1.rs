//! Pure Position Manager Lite V1 exit policy.
//!
//! This module deliberately owns no runtime state and performs no I/O. The
//! engine materializes an immutable snapshot, evaluates it without locks, and
//! applies the result through guarded mutation methods.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use trigger::PriceTruthSource;

use crate::execution::backend::Lane;

use super::config::{PostBuyGuardianConfig, DEFAULT_WAIT_FOR_TIMESTOP_MS};

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
        Self::new(
            take_profit_fraction,
            stop_loss_fraction,
            guardian
                .wait_for_timestop
                .unwrap_or(DEFAULT_WAIT_FOR_TIMESTOP_MS),
            guardian.exit_policy_v1.quote_recovery_ms,
        )
    }

    pub(super) fn new(
        take_profit_fraction: f64,
        stop_loss_fraction: f64,
        inactivity_timeout_ms: u64,
        quote_recovery_ms: u64,
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

        #[derive(Serialize)]
        struct HashInput {
            take_profit_fraction: f64,
            stop_loss_fraction: f64,
            inactivity_timeout_ms: u64,
            quote_recovery_ms: u64,
            policy_id: &'static str,
            policy_version: u16,
        }

        let hash_input = HashInput {
            take_profit_fraction,
            stop_loss_fraction,
            inactivity_timeout_ms,
            quote_recovery_ms,
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct CrashVectorV1 {
    distinct_slots: u8,
    short_window_return_pct: Option<f64>,
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
        } else {
            PreQuoteDecision::Hold
        }
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

    fn snapshot(mark: Option<f64>, inactivity_ms: u64) -> PostBuyDecisionSnapshot {
        let config = config();
        let guard = PositionSnapshotGuard::new("p1".to_string(), 1, 7, 100, Some(10), Some(20));
        PostBuyDecisionSnapshot::new(
            guard,
            Lane::Shadow,
            Some(1.0),
            100,
            100,
            1,
            10,
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
            CrashVectorV1::default(),
            false,
            config.config_hash().to_string(),
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
