use ghost_brain::config::GatekeeperV2Config;
use ghost_brain::oracle::reason_code::GatekeeperReasonCode;
use ghost_core::checkpoint::{EvidenceStatus, MaterializedFeatureSet, OrganicBroadeningFeatures};

pub const V3_SHADOW_SCHEMA_VERSION: u32 = 1;
const V3_P0_EXECUTION_NOT_RUN_CAP: f64 = 0.80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V3ShadowVerdict {
    BuyCandidate,
    Reject,
    Pending,
    Timeout,
}

impl V3ShadowVerdict {
    pub const fn as_log_str(self) -> &'static str {
        match self {
            Self::BuyCandidate => "BUY_CANDIDATE",
            Self::Reject => "REJECT",
            Self::Pending => "PENDING",
            Self::Timeout => "TIMEOUT",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionStage {
    Evidence,
    Risk,
    Opportunity,
    Confidence,
}

impl DecisionStage {
    pub const fn as_log_str(self) -> &'static str {
        match self {
            Self::Evidence => "EVIDENCE",
            Self::Risk => "RISK",
            Self::Opportunity => "OPPORTUNITY",
            Self::Confidence => "CONFIDENCE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskVerdictStatus {
    Clean,
    Actionable,
    Degraded,
    Unavailable,
}

impl RiskVerdictStatus {
    pub const fn as_log_str(self) -> &'static str {
        match self {
            Self::Clean => "CLEAN",
            Self::Actionable => "ACTIONABLE",
            Self::Degraded => "DEGRADED",
            Self::Unavailable => "UNAVAILABLE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpportunityVerdictStatus {
    Sufficient,
    Insufficient,
    Degraded,
    Unavailable,
}

impl OpportunityVerdictStatus {
    pub const fn as_log_str(self) -> &'static str {
        match self {
            Self::Sufficient => "SUFFICIENT",
            Self::Insufficient => "INSUFFICIENT",
            Self::Degraded => "DEGRADED",
            Self::Unavailable => "UNAVAILABLE",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfidenceBreakdown {
    pub raw: f64,
    pub after_risk: f64,
    pub after_stage: f64,
    pub cap: f64,
    pub cap_reasons: Vec<String>,
    pub final_confidence: f64,
}

impl ConfidenceBreakdown {
    fn capped(raw: f64, risk_penalty: f64, stage_multiplier: f64, cap: f64, reason: &str) -> Self {
        let raw = clamp01(raw);
        let after_risk = clamp01(raw * (1.0 - risk_penalty));
        let after_stage = clamp01(after_risk * stage_multiplier);
        let cap = clamp01(cap);
        Self {
            raw,
            after_risk,
            after_stage,
            cap,
            cap_reasons: vec![reason.to_string()],
            final_confidence: after_stage.min(cap),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct V3ShadowDecision {
    pub schema_version: u32,
    pub verdict: V3ShadowVerdict,
    pub stage: DecisionStage,
    pub reason_code: GatekeeperReasonCode,
    pub reason_chain: Vec<GatekeeperReasonCode>,
    pub risk_status: RiskVerdictStatus,
    pub risk_primary_reason: Option<GatekeeperReasonCode>,
    pub risk_penalty: f64,
    pub opportunity_status: OpportunityVerdictStatus,
    pub opportunity_score: f64,
    pub confidence_breakdown: ConfidenceBreakdown,
    pub confidence: f64,
}

impl V3ShadowDecision {
    fn new(
        verdict: V3ShadowVerdict,
        stage: DecisionStage,
        reason_code: GatekeeperReasonCode,
        reason_chain: Vec<GatekeeperReasonCode>,
        risk_status: RiskVerdictStatus,
        risk_primary_reason: Option<GatekeeperReasonCode>,
        risk_penalty: f64,
        opportunity_status: OpportunityVerdictStatus,
        opportunity_score: f64,
        confidence_breakdown: ConfidenceBreakdown,
    ) -> Self {
        let confidence = confidence_breakdown.final_confidence;
        Self {
            schema_version: V3_SHADOW_SCHEMA_VERSION,
            verdict,
            stage,
            reason_code,
            reason_chain,
            risk_status,
            risk_primary_reason,
            risk_penalty: clamp01(risk_penalty),
            opportunity_status,
            opportunity_score: clamp01(opportunity_score),
            confidence_breakdown,
            confidence,
        }
    }
}

pub fn evaluate_v3_from_features(
    features: &MaterializedFeatureSet,
    config: &GatekeeperV2Config,
    deadline_elapsed: bool,
) -> V3ShadowDecision {
    let opportunity_score = organic_confidence(&features.organic_broadening, features, config);
    let risk_penalty = v3_risk_penalty(features, config);

    if has_hard_risk_contradiction(features, config) {
        return V3ShadowDecision::new(
            V3ShadowVerdict::Reject,
            DecisionStage::Risk,
            GatekeeperReasonCode::RejectV3ManipulationContradiction,
            vec![
                GatekeeperReasonCode::V3ManipulationContradiction,
                GatekeeperReasonCode::RejectV3ManipulationContradiction,
            ],
            RiskVerdictStatus::Actionable,
            Some(GatekeeperReasonCode::RejectV3ManipulationContradiction),
            1.0,
            OpportunityVerdictStatus::Unavailable,
            opportunity_score,
            ConfidenceBreakdown::capped(opportunity_score, 1.0, 0.0, 0.0, "hard_risk"),
        );
    }

    if let Some(issue) = non_clean_evidence_issue(features) {
        return insufficient_evidence_decision(deadline_elapsed, issue, risk_penalty);
    }

    if organic_broadening_passes(&features.organic_broadening, features, config) {
        let confidence = ConfidenceBreakdown::capped(
            opportunity_score,
            risk_penalty,
            1.0,
            V3_P0_EXECUTION_NOT_RUN_CAP,
            "execution_not_run",
        );
        return V3ShadowDecision::new(
            V3ShadowVerdict::BuyCandidate,
            DecisionStage::Opportunity,
            GatekeeperReasonCode::BuyV3NormalConfirmedOpportunity,
            vec![GatekeeperReasonCode::BuyV3NormalConfirmedOpportunity],
            RiskVerdictStatus::Clean,
            None,
            risk_penalty,
            OpportunityVerdictStatus::Sufficient,
            opportunity_score,
            confidence,
        );
    }

    let reason = if has_sufficient_sample(features, config) {
        GatekeeperReasonCode::RejectV3LowOrganicBroadening
    } else if deadline_elapsed {
        GatekeeperReasonCode::TimeoutV3UnresolvedConfidence
    } else {
        GatekeeperReasonCode::PendingV3WaitSample
    };
    let verdict = if matches!(reason, GatekeeperReasonCode::RejectV3LowOrganicBroadening) {
        V3ShadowVerdict::Reject
    } else if deadline_elapsed {
        V3ShadowVerdict::Timeout
    } else {
        V3ShadowVerdict::Pending
    };
    let stage = if matches!(reason, GatekeeperReasonCode::RejectV3LowOrganicBroadening) {
        DecisionStage::Opportunity
    } else {
        DecisionStage::Evidence
    };

    V3ShadowDecision::new(
        verdict,
        stage,
        reason,
        vec![
            GatekeeperReasonCode::V3OrganicBroadeningInsufficient,
            reason,
        ],
        RiskVerdictStatus::Clean,
        None,
        risk_penalty,
        OpportunityVerdictStatus::Insufficient,
        opportunity_score,
        ConfidenceBreakdown::capped(
            opportunity_score,
            risk_penalty,
            if verdict == V3ShadowVerdict::Reject {
                1.0
            } else {
                0.0
            },
            0.0,
            "organic_broadening_insufficient",
        ),
    )
}

fn insufficient_evidence_decision(
    deadline_elapsed: bool,
    issue: EvidenceIssue,
    risk_penalty: f64,
) -> V3ShadowDecision {
    let terminal_reason = match (deadline_elapsed, issue) {
        (true, EvidenceIssue::Unavailable | EvidenceIssue::Degraded) => {
            GatekeeperReasonCode::TimeoutV3DegradedEvidence
        }
        (true, EvidenceIssue::InsufficientSample) => {
            GatekeeperReasonCode::TimeoutV3UnresolvedConfidence
        }
        (false, EvidenceIssue::InsufficientSample) => GatekeeperReasonCode::PendingV3WaitSample,
        (false, EvidenceIssue::Unavailable | EvidenceIssue::Degraded) => {
            GatekeeperReasonCode::PendingV3WaitEvidence
        }
    };
    let primary_reason = if issue == EvidenceIssue::Unavailable {
        GatekeeperReasonCode::V3EvidenceUnavailable
    } else {
        GatekeeperReasonCode::V3EvidenceDegraded
    };
    let verdict = if deadline_elapsed {
        V3ShadowVerdict::Timeout
    } else {
        V3ShadowVerdict::Pending
    };

    V3ShadowDecision::new(
        verdict,
        DecisionStage::Evidence,
        terminal_reason,
        vec![primary_reason, terminal_reason],
        if issue == EvidenceIssue::Unavailable {
            RiskVerdictStatus::Unavailable
        } else {
            RiskVerdictStatus::Degraded
        },
        None,
        risk_penalty,
        if issue == EvidenceIssue::Unavailable {
            OpportunityVerdictStatus::Unavailable
        } else {
            OpportunityVerdictStatus::Degraded
        },
        0.0,
        ConfidenceBreakdown::capped(0.0, risk_penalty, 0.0, 0.0, "insufficient_evidence"),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvidenceIssue {
    Unavailable,
    InsufficientSample,
    Degraded,
}

fn non_clean_evidence_issue(features: &MaterializedFeatureSet) -> Option<EvidenceIssue> {
    let evidence = &features.evidence_status;
    let relevant = [
        &evidence.identity,
        &evidence.account_state,
        &evidence.tx_intel,
        &evidence.tx_segments,
        &evidence.checkpoints,
        &evidence.trajectory,
        &evidence.pdd_sequence,
        &evidence.curve,
        &evidence.sybil,
        &evidence.cpv,
        &evidence.fsc,
        &evidence.alpha,
        &evidence.manipulation,
        &evidence.organic_broadening,
        &evidence.manipulation_contradiction,
    ];

    if relevant
        .iter()
        .any(|status| matches!(status.status, EvidenceStatus::Unavailable))
    {
        return Some(EvidenceIssue::Unavailable);
    }

    if relevant
        .iter()
        .any(|status| matches!(status.status, EvidenceStatus::InsufficientSample))
    {
        return Some(EvidenceIssue::InsufficientSample);
    }

    if relevant.iter().any(|status| {
        matches!(
            status.status,
            EvidenceStatus::Degraded
                | EvidenceStatus::Stale
                | EvidenceStatus::Fallback
                | EvidenceStatus::ShadowOnly
                | EvidenceStatus::NotConfigured
        )
    }) {
        return Some(EvidenceIssue::Degraded);
    }

    None
}

fn has_hard_risk_contradiction(
    features: &MaterializedFeatureSet,
    config: &GatekeeperV2Config,
) -> bool {
    let risk = &features.manipulation_contradictions;
    let tx = &features.tx_intel_features;

    (config.reject_on_dev_sell && (risk.dev_has_sold || tx.dev_has_sold))
        || risk.high_same_ms_tx_ratio
        || risk.high_bundle_suspicion_ratio
        || risk.high_top3_volume_pct
        || risk.high_hhi
        || risk.high_signer_concentration
        || risk.high_dev_concentration
        || risk.same_ms_tx_ratio > config.hard_fail_same_ms_tx_ratio
        || risk.bundle_suspicion_ratio > config.hard_fail_same_ms_tx_ratio
        || risk.top3_volume_pct > config.hard_fail_top3_volume_pct
        || risk.hhi > config.hard_fail_hhi
        || risk.max_tx_per_signer as usize > config.max_tx_per_signer
        || risk.dev_volume_ratio > config.max_dev_volume_ratio
        || risk
            .signer_cross_pool_velocity
            .is_some_and(|value| value > config.max_signer_cross_pool_velocity)
        || risk
            .funding_source_concentration
            .is_some_and(|value| value > config.max_funding_source_concentration)
}

fn v3_risk_penalty(features: &MaterializedFeatureSet, config: &GatekeeperV2Config) -> f64 {
    if has_hard_risk_contradiction(features, config) {
        return 1.0;
    }

    let risk = &features.manipulation_contradictions;
    let boolean_penalty = [
        risk.momentum_without_broadening,
        risk.volume_spike_without_new_signers,
        risk.high_buy_pressure_with_high_top3,
        risk.fixed_size_or_ramping_pattern,
        risk.timing_bundle_concentration,
        risk.early_top3_concentration,
        risk.sybil_evidence_degraded,
    ]
    .into_iter()
    .filter(|flag| *flag)
    .count() as f64
        / 7.0;

    risk.contradiction_score
        .max(boolean_penalty)
        .clamp(0.0, 0.85)
}

fn has_sufficient_sample(features: &MaterializedFeatureSet, config: &GatekeeperV2Config) -> bool {
    features.organic_broadening.sequence_available
        && features.tx_intel_features.tx_count >= config.min_tx_count as u64
        && features.tx_intel_features.unique_signers >= config.min_unique_signers as u64
        && features.tx_intel_features.buy_count >= config.min_buy_count as u64
}

fn organic_broadening_passes(
    organic: &OrganicBroadeningFeatures,
    features: &MaterializedFeatureSet,
    config: &GatekeeperV2Config,
) -> bool {
    organic.sequence_available
        && organic.total_tx_count >= config.min_tx_count as u64
        && organic.total_unique_signers >= config.min_unique_signers as u64
        && features.tx_intel_features.buy_count >= config.min_buy_count as u64
        && organic.buy_ratio_min >= config.min_buy_ratio
        && organic.buy_ratio_max <= config.max_buy_ratio
        && organic.t1_vs_t0_unique_signer_delta >= 0
        && organic.t2_vs_t1_unique_signer_delta >= 0
        && organic.tx_count_growth_ratio >= 1.0
        && organic.unique_signer_growth_ratio >= 1.0
        && organic.max_segment_hhi <= config.max_hhi
}

fn organic_confidence(
    organic: &OrganicBroadeningFeatures,
    features: &MaterializedFeatureSet,
    config: &GatekeeperV2Config,
) -> f64 {
    let tx_score = ratio_score(organic.total_tx_count as f64, config.min_tx_count as f64);
    let signer_score = ratio_score(
        organic.total_unique_signers as f64,
        config.min_unique_signers as f64,
    );
    let buy_count_score = ratio_score(
        features.tx_intel_features.buy_count as f64,
        config.min_buy_count as f64,
    );
    let buy_ratio_score = if config.max_buy_ratio <= config.min_buy_ratio {
        1.0
    } else {
        let center = (config.min_buy_ratio + config.max_buy_ratio) / 2.0;
        let half_range = (config.max_buy_ratio - config.min_buy_ratio) / 2.0;
        1.0 - ((organic.buy_ratio_mean - center).abs() / half_range).min(1.0)
    };
    let growth_score = ((organic.tx_count_growth_ratio.min(2.0) - 1.0)
        + (organic.unique_signer_growth_ratio.min(2.0) - 1.0))
        / 2.0;

    clamp01(
        0.25 * tx_score
            + 0.25 * signer_score
            + 0.20 * buy_count_score
            + 0.15 * buy_ratio_score
            + 0.15 * growth_score,
    )
}

fn ratio_score(value: f64, required: f64) -> f64 {
    if required <= 0.0 {
        return 1.0;
    }
    clamp01(value / required)
}

fn clamp01(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}
