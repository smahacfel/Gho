use ghost_brain::config::GatekeeperV2Config;
use ghost_brain::oracle::reason_code::GatekeeperReasonCode;
use ghost_core::checkpoint::{EvidenceStatus, MaterializedFeatureSet, OrganicBroadeningFeatures};

pub const V3_SHADOW_SCHEMA_VERSION: u32 = 1;

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

#[derive(Debug, Clone, PartialEq)]
pub struct V3ShadowDecision {
    pub schema_version: u32,
    pub verdict: V3ShadowVerdict,
    pub reason_code: GatekeeperReasonCode,
    pub reason_chain: Vec<GatekeeperReasonCode>,
    pub confidence: f64,
}

impl V3ShadowDecision {
    fn new(
        verdict: V3ShadowVerdict,
        reason_code: GatekeeperReasonCode,
        reason_chain: Vec<GatekeeperReasonCode>,
        confidence: f64,
    ) -> Self {
        Self {
            schema_version: V3_SHADOW_SCHEMA_VERSION,
            verdict,
            reason_code,
            reason_chain,
            confidence: clamp01(confidence),
        }
    }
}

pub fn evaluate_v3_from_features(
    features: &MaterializedFeatureSet,
    config: &GatekeeperV2Config,
    deadline_elapsed: bool,
) -> V3ShadowDecision {
    if has_hard_risk_contradiction(features, config) {
        return V3ShadowDecision::new(
            V3ShadowVerdict::Reject,
            GatekeeperReasonCode::V3HardRiskReject,
            vec![
                GatekeeperReasonCode::V3ManipulationContradiction,
                GatekeeperReasonCode::V3HardRiskReject,
            ],
            0.0,
        );
    }

    if let Some(evidence_reason) = non_clean_evidence_reason(features) {
        return insufficient_evidence_decision(deadline_elapsed, evidence_reason);
    }

    if organic_broadening_passes(&features.organic_broadening, features, config) {
        return V3ShadowDecision::new(
            V3ShadowVerdict::BuyCandidate,
            GatekeeperReasonCode::V3ShadowBuyCandidate,
            vec![GatekeeperReasonCode::V3ShadowBuyCandidate],
            organic_confidence(&features.organic_broadening, features, config),
        );
    }

    insufficient_evidence_decision(
        deadline_elapsed,
        GatekeeperReasonCode::V3OrganicBroadeningInsufficient,
    )
}

fn insufficient_evidence_decision(
    deadline_elapsed: bool,
    primary_reason: GatekeeperReasonCode,
) -> V3ShadowDecision {
    let terminal_reason = if deadline_elapsed {
        GatekeeperReasonCode::V3ShadowTimeoutEvidence
    } else {
        GatekeeperReasonCode::V3ShadowPendingInsufficientEvidence
    };
    let verdict = if deadline_elapsed {
        V3ShadowVerdict::Timeout
    } else {
        V3ShadowVerdict::Pending
    };

    V3ShadowDecision::new(
        verdict,
        terminal_reason,
        vec![primary_reason, terminal_reason],
        0.0,
    )
}

fn non_clean_evidence_reason(features: &MaterializedFeatureSet) -> Option<GatekeeperReasonCode> {
    let evidence = &features.evidence_status;
    let relevant = [
        &evidence.account_state,
        &evidence.tx_intel,
        &evidence.tx_segments,
        &evidence.checkpoints,
        &evidence.curve,
        &evidence.sybil,
        &evidence.alpha,
        &evidence.manipulation,
    ];

    if relevant
        .iter()
        .any(|status| matches!(status.status, EvidenceStatus::Unavailable))
    {
        return Some(GatekeeperReasonCode::V3EvidenceUnavailable);
    }

    if relevant
        .iter()
        .any(|status| matches!(status.status, EvidenceStatus::Degraded))
    {
        return Some(GatekeeperReasonCode::V3EvidenceDegraded);
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
