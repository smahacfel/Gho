use ghost_brain::config::{
    GatekeeperV3ComponentWeights, GatekeeperV3Config, GatekeeperV3StageProfile,
};
use ghost_brain::oracle::reason_code::GatekeeperReasonCode;
use ghost_core::checkpoint::{
    EvidenceStatus, FeatureEvidenceStatus, MaterializedFeatureSet, OrganicBroadeningFeatures,
    TrajectorySegmentSnapshot,
};
use ghost_core::tx_intelligence::types::FundingSourceDiagnostics;
use serde::Serialize;
use serde_json::{json, Value};

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

pub fn v3_actionability_payload(
    features: &MaterializedFeatureSet,
    config: &GatekeeperV3Config,
    deadline_elapsed: bool,
) -> serde_json::Value {
    let evidence = &features.evidence_status;
    let profile = config.profile_for_context(
        deadline_elapsed,
        features.session_metadata.observation_duration_ms,
    );
    let profile_name = config.profile_name_for_context(
        deadline_elapsed,
        features.session_metadata.observation_duration_ms,
    );
    let groups = [
        ("identity", &evidence.identity),
        ("account_state", &evidence.account_state),
        ("tx_intel", &evidence.tx_intel),
        ("tx_segments", &evidence.tx_segments),
        ("checkpoints", &evidence.checkpoints),
        ("trajectory", &evidence.trajectory),
        ("pdd_sequence", &evidence.pdd_sequence),
        ("curve", &evidence.curve),
        ("sybil", &evidence.sybil),
        ("cpv", &evidence.cpv),
        ("fsc", &evidence.fsc),
        ("alpha", &evidence.alpha),
        ("manipulation", &evidence.manipulation),
        ("organic_broadening", &evidence.organic_broadening),
        (
            "manipulation_contradiction",
            &evidence.manipulation_contradiction,
        ),
        ("execution", &evidence.execution),
    ];

    let group_statuses: serde_json::Map<String, serde_json::Value> = groups
        .iter()
        .map(|(name, status)| {
            let stage = default_stage_for_evidence_group(name);
            (
                (*name).to_string(),
                json!({
                    "status": format!("{:?}", status.status),
                    "stage": format!("{:?}", stage),
                    "actionability": evaluate_feature_actionability(
                        name,
                        status.status,
                        stage,
                        config,
                    ),
                }),
            )
        })
        .collect();

    let evidence_actionable = non_clean_evidence_issue(features, config).is_none();
    let risk_actionable = evaluate_feature_actionability(
        "manipulation_contradiction",
        evidence.manipulation_contradiction.status,
        DecisionStage::Risk,
        config,
    ) == "actionable";
    let opportunity_actionable =
        features.organic_broadening.sequence_available && has_sufficient_sample(features, profile);
    let confidence_actionable = evidence_actionable && opportunity_actionable;

    json!({
        "profile": profile_name,
        "stages": {
            "evidence": stage_actionability(evidence_actionable),
            "risk": stage_actionability(risk_actionable),
            "opportunity": stage_actionability(opportunity_actionable),
            "confidence": stage_actionability(confidence_actionable)
        },
        "groups": group_statuses
    })
}

pub fn v3_component_scores_payload(decision: &V3ShadowDecision) -> serde_json::Value {
    json!({
        "risk": {
            "status": decision.risk_status.as_log_str(),
            "penalty": decision.risk_penalty
        },
        "opportunity": {
            "status": decision.opportunity_status.as_log_str(),
            "score": decision.opportunity_score
        },
        "confidence": {
            "raw": decision.confidence_breakdown.raw,
            "after_risk": decision.confidence_breakdown.after_risk,
            "after_stage": decision.confidence_breakdown.after_stage,
            "cap": decision.confidence_breakdown.cap,
            "cap_reasons": decision.confidence_breakdown.cap_reasons.clone(),
            "final": decision.confidence_breakdown.final_confidence
        },
        "final_confidence": decision.confidence
    })
}

pub fn v3_feature_snapshot_hash(
    features: &MaterializedFeatureSet,
    materialization_version: u32,
) -> String {
    let account = &features.account_features;
    let tx = &features.tx_intel_features;
    let checkpoints = &features.checkpoint_features;
    let session = &features.session_metadata;
    let curve = &features.curve_readiness;
    let sybil = &features.sybil_resistance;
    let alpha = &features.alpha_fingerprint;
    let organic = &features.organic_broadening;
    let manipulation = &features.manipulation_contradictions;

    let payload = json!({
        "materialization_version": materialization_version,
        "account_features": {
            "bonding_progress_bits": f64_bits(account.bonding_progress),
            "current_reserves": [account.current_reserves.0, account.current_reserves.1],
            "curve_finality": account.curve_finality.as_str(),
            "is_bootstrap": account.is_bootstrap,
            "market_cap_sol_bits": f64_bits(account.market_cap_sol),
            "price_change_since_t0_pct_bits": f64_bits(account.price_change_since_t0_pct),
            "price_sol_bits": f64_bits(account.price_sol),
            "reserve_velocity_sol_per_sec_bits": f64_bits(account.reserve_velocity_sol_per_sec),
            "state_phase": serde_value(&account.state_phase),
            "update_count": account.update_count
        },
        "tx_intel_features": {
            "avg_interval_ms_bits": f64_bits(tx.avg_interval_ms),
            "avg_tx_per_signer_bits": f64_bits(tx.avg_tx_per_signer),
            "avg_tx_sol_bits": f64_bits(tx.avg_tx_sol),
            "bundle_suspicion_ratio_bits": f64_bits(tx.bundle_suspicion_ratio),
            "burst_ratio_bits": f64_bits(tx.burst_ratio),
            "buy_count": tx.buy_count,
            "buy_ratio_bits": f64_bits(tx.buy_ratio),
            "dev_buy_sol_bits": f64_bits(tx.dev_buy_sol),
            "dev_has_sold": tx.dev_has_sold,
            "dev_initial_buy_tokens_bits": opt_f64_bits(tx.dev_initial_buy_tokens),
            "dev_is_first_buyer": tx.dev_is_first_buyer,
            "dev_tx_count": tx.dev_tx_count,
            "dev_tx_ratio_bits": f64_bits(tx.dev_tx_ratio),
            "dev_volume_ratio_bits": f64_bits(tx.dev_volume_ratio),
            "dev_wallet_known": tx.dev_wallet_known,
            "dust_ratio_bits": f64_bits(tx.dust_ratio),
            "dust_tx_count": tx.dust_tx_count,
            "failed_tx_count": tx.failed_tx_count,
            "hhi_bits": f64_bits(tx.hhi),
            "interval_cv_bits": f64_bits(tx.interval_cv),
            "max_consecutive_buys": tx.max_consecutive_buys,
            "max_tx_per_signer": tx.max_tx_per_signer,
            "max_tx_sol_bits": f64_bits(tx.max_tx_sol),
            "min_tx_sol_bits": f64_bits(tx.min_tx_sol),
            "same_ms_tx_ratio_bits": f64_bits(tx.same_ms_tx_ratio),
            "sell_count": tx.sell_count,
            "sol_buy_ratio_bits": f64_bits(tx.sol_buy_ratio),
            "timing_entropy_bits": f64_bits(tx.timing_entropy),
            "top3_volume_pct_bits": f64_bits(tx.top3_volume_pct),
            "total_volume_sol_bits": f64_bits(tx.total_volume_sol),
            "tx_count": tx.tx_count,
            "unique_signer_ratio_bits": f64_bits(tx.unique_signer_ratio),
            "unique_signers": tx.unique_signers,
            "volume_cv_bits": f64_bits(tx.volume_cv),
            "volume_gini_bits": f64_bits(tx.volume_gini)
        },
        "checkpoint_features": {
            "bonding_progress_bits": f64_bits(checkpoints.bonding_progress),
            "buy_pressure_trend": serde_value(&checkpoints.buy_pressure_trend),
            "price_change_from_first_checkpoint_pct_bits": f64_bits(checkpoints.price_change_from_first_checkpoint_pct),
            "price_trajectory_bits": checkpoints.price_trajectory
                .iter()
                .map(|value| f64_bits(*value))
                .collect::<Vec<_>>(),
            "reserve_trajectory": checkpoints.reserve_trajectory
                .iter()
                .map(|(sol, token)| json!([*sol, *token]))
                .collect::<Vec<_>>(),
            "risk_flag_count_trend": serde_value(&checkpoints.risk_flag_count_trend),
            "signer_diversity_trend": serde_value(&checkpoints.signer_diversity_trend),
            "single_tx_max_price_impact_pct_bits": f64_bits(checkpoints.single_tx_max_price_impact_pct),
            "max_single_sell_impact_pct_bits": f64_bits(checkpoints.max_single_sell_impact_pct),
            "trajectory_assessment": checkpoints.trajectory_assessment.as_ref().map_or(Value::Null, |assessment| json!({
                "buy_ratio_score_bits": f64_bits(assessment.buy_ratio_score),
                "hhi_score_bits": f64_bits(assessment.hhi_score),
                "interval_score_bits": f64_bits(assessment.interval_score),
                "momentum_score_bits": f64_bits(assessment.momentum_score),
                "overall_tas_score_bits": f64_bits(assessment.overall_tas_score),
                "segment_count": assessment.segment_count,
                "t0_tx_count": assessment.t0_tx_count,
                "t1_tx_count": assessment.t1_tx_count,
                "t2_tx_count": assessment.t2_tx_count,
                "volume_score_bits": f64_bits(assessment.volume_score)
            })),
            "trajectory_checkpoint_count": checkpoints.trajectory_checkpoint_count
        },
        "risk_flags": features.risk_flags
            .iter()
            .map(|flag| json!({
                "detected_at_ms": flag.detected_at_ms,
                "detail": flag.detail.as_str(),
                "flag_id": flag.flag_id.as_ref(),
                "severity": serde_value(&flag.severity)
            }))
            .collect::<Vec<_>>(),
        "session_metadata": {
            "base_mint": session.base_mint.to_string(),
            "is_dev_known": session.is_dev_known,
            "observation_duration_ms": session.observation_duration_ms,
            "pool_amm_id": session.pool_amm_id.to_string()
        },
        "curve_readiness": {
            "curve_data_known": curve.curve_data_known,
            "finality": curve.finality.as_str(),
            "freshness": curve.freshness.as_str(),
            "is_ready": curve.is_ready,
            "price_sample_count": curve.price_sample_count,
            "t0_event_ts_ms": opt_u64_value(curve.t0_event_ts_ms),
            "wait_elapsed_ms": opt_u64_value(curve.wait_elapsed_ms)
        },
        "sybil_resistance": {
            "buy_sample_count": sybil.buy_sample_count,
            "degraded_reasons": sybil.degraded_reasons.clone(),
            "demand_elasticity_score_bits": opt_f64_bits(sybil.demand_elasticity_score),
            "dev_buyer_infrastructure_affinity_bits": opt_f64_bits(sybil.dev_buyer_infrastructure_affinity),
            "fee_topology_diversity_index_bits": opt_f64_bits(sybil.fee_topology_diversity_index),
            "funding_source_concentration_bits": opt_f64_bits(sybil.funding_source_concentration),
            "funding_source_diagnostics": sybil
                .funding_source_diagnostics
                .as_ref()
                .map_or(Value::Null, funding_source_diagnostics_payload),
            "signer_cross_pool_velocity_bits": opt_f64_bits(sybil.signer_cross_pool_velocity),
            "signer_sample_count": sybil.signer_sample_count,
            "spend_fraction_divergence_bits": opt_f64_bits(sybil.spend_fraction_divergence)
        },
        "alpha_fingerprint": {
            "avg_inner_ix_count_50tx_bits": opt_f64_bits(alpha.avg_inner_ix_count_50tx),
            "compute_unit_cluster_dominance_bits": opt_f64_bits(alpha.compute_unit_cluster_dominance),
            "early_slot_volume_dominance_buy_bits": opt_f64_bits(alpha.early_slot_volume_dominance_buy),
            "early_top3_buy_volume_pct_3s_bits": opt_f64_bits(alpha.early_top3_buy_volume_pct_3s),
            "fixed_size_buy_ratio_bits": opt_f64_bits(alpha.fixed_size_buy_ratio),
            "flipper_presence_ratio_bits": opt_f64_bits(alpha.flipper_presence_ratio),
            "jito_tip_intensity_bits": opt_f64_bits(alpha.jito_tip_intensity),
            "sell_buy_ratio_bits": opt_f64_bits(alpha.sell_buy_ratio),
            "static_fee_profile_ratio_bits": opt_f64_bits(alpha.static_fee_profile_ratio)
        },
        "tx_segment_sequence": features.tx_segment_sequence.as_ref().map_or(Value::Null, |sequence| json!({
            "min_tx_per_segment_satisfied": sequence.min_tx_per_segment_satisfied,
            "t0_segment": trajectory_segment_payload(&sequence.t0_segment),
            "t1_segment": trajectory_segment_payload(&sequence.t1_segment),
            "t2_segment": trajectory_segment_payload(&sequence.t2_segment),
            "total_duration_ms": sequence.total_duration_ms
        })),
        "evidence_status": {
            "account_state": evidence_status_payload(&features.evidence_status.account_state),
            "alpha": evidence_status_payload(&features.evidence_status.alpha),
            "checkpoints": evidence_status_payload(&features.evidence_status.checkpoints),
            "cpv": evidence_status_payload(&features.evidence_status.cpv),
            "curve": evidence_status_payload(&features.evidence_status.curve),
            "execution": evidence_status_payload(&features.evidence_status.execution),
            "fsc": evidence_status_payload(&features.evidence_status.fsc),
            "identity": evidence_status_payload(&features.evidence_status.identity),
            "manipulation": evidence_status_payload(&features.evidence_status.manipulation),
            "manipulation_contradiction": evidence_status_payload(&features.evidence_status.manipulation_contradiction),
            "organic_broadening": evidence_status_payload(&features.evidence_status.organic_broadening),
            "pdd_sequence": evidence_status_payload(&features.evidence_status.pdd_sequence),
            "sybil": evidence_status_payload(&features.evidence_status.sybil),
            "trajectory": evidence_status_payload(&features.evidence_status.trajectory),
            "tx_intel": evidence_status_payload(&features.evidence_status.tx_intel),
            "tx_segments": evidence_status_payload(&features.evidence_status.tx_segments)
        },
        "organic_broadening": {
            "broadening_score_bits": f64_bits(organic.broadening_score),
            "buy_ratio_max_bits": f64_bits(organic.buy_ratio_max),
            "buy_ratio_mean_bits": f64_bits(organic.buy_ratio_mean),
            "buy_ratio_min_bits": f64_bits(organic.buy_ratio_min),
            "degraded_reasons": organic.degraded_reasons
                .iter()
                .map(serde_value)
                .collect::<Vec<_>>(),
            "hhi_delta_t2_t0_bits": f64_bits(organic.hhi_delta_t2_t0),
            "max_segment_hhi_bits": f64_bits(organic.max_segment_hhi),
            "min_segment_hhi_bits": f64_bits(organic.min_segment_hhi),
            "new_signer_ratio_t2_bits": f64_bits(organic.new_signer_ratio_t2),
            "sequence_available": organic.sequence_available,
            "signer_growth_t2_t0": organic.signer_growth_t2_t0,
            "status": serde_value(&organic.status),
            "t0_tx_count": organic.t0_tx_count,
            "t0_unique_signers": organic.t0_unique_signers,
            "t1_tx_count": organic.t1_tx_count,
            "t1_unique_signers": organic.t1_unique_signers,
            "t1_vs_t0_unique_signer_delta": organic.t1_vs_t0_unique_signer_delta,
            "t2_tx_count": organic.t2_tx_count,
            "t2_unique_signers": organic.t2_unique_signers,
            "t2_vs_t1_unique_signer_delta": organic.t2_vs_t1_unique_signer_delta,
            "total_tx_count": organic.total_tx_count,
            "total_unique_signers": organic.total_unique_signers,
            "tx_count_growth_ratio_bits": f64_bits(organic.tx_count_growth_ratio),
            "tx_count_growth_vs_signer_growth_bits": f64_bits(organic.tx_count_growth_vs_signer_growth),
            "unique_signer_growth_ratio_bits": f64_bits(organic.unique_signer_growth_ratio)
        },
        "manipulation_contradictions": {
            "bundle_suspicion_ratio_bits": f64_bits(manipulation.bundle_suspicion_ratio),
            "contradiction_score_bits": f64_bits(manipulation.contradiction_score),
            "dev_has_sold": manipulation.dev_has_sold,
            "dev_volume_ratio_bits": f64_bits(manipulation.dev_volume_ratio),
            "early_top3_concentration": manipulation.early_top3_concentration,
            "fee_topology_diversity_index_bits": opt_f64_bits(manipulation.fee_topology_diversity_index),
            "fixed_size_or_ramping_pattern": manipulation.fixed_size_or_ramping_pattern,
            "funding_source_concentration_bits": opt_f64_bits(manipulation.funding_source_concentration),
            "hhi_bits": f64_bits(manipulation.hhi),
            "high_bundle_suspicion_ratio": manipulation.high_bundle_suspicion_ratio,
            "high_buy_pressure_with_high_top3": manipulation.high_buy_pressure_with_high_top3,
            "high_dev_concentration": manipulation.high_dev_concentration,
            "high_hhi": manipulation.high_hhi,
            "high_same_ms_tx_ratio": manipulation.high_same_ms_tx_ratio,
            "high_signer_concentration": manipulation.high_signer_concentration,
            "high_top3_volume_pct": manipulation.high_top3_volume_pct,
            "max_tx_per_signer": manipulation.max_tx_per_signer,
            "momentum_without_broadening": manipulation.momentum_without_broadening,
            "reasons": manipulation.reasons.clone(),
            "same_ms_tx_ratio_bits": f64_bits(manipulation.same_ms_tx_ratio),
            "signer_cross_pool_velocity_bits": opt_f64_bits(manipulation.signer_cross_pool_velocity),
            "spend_fraction_divergence_bits": opt_f64_bits(manipulation.spend_fraction_divergence),
            "status": serde_value(&manipulation.status),
            "sybil_evidence_degraded": manipulation.sybil_evidence_degraded,
            "timing_bundle_concentration": manipulation.timing_bundle_concentration,
            "top3_volume_pct_bits": f64_bits(manipulation.top3_volume_pct),
            "volume_spike_without_new_signers": manipulation.volume_spike_without_new_signers
        }
    });

    let bytes = serde_json::to_vec(&payload).expect("canonical V3 feature snapshot serializes");
    blake3::hash(&bytes).to_hex().to_string()
}

pub fn evaluate_v3_from_features(
    features: &MaterializedFeatureSet,
    config: &GatekeeperV3Config,
    deadline_elapsed: bool,
) -> V3ShadowDecision {
    let profile = config.profile_for_context(
        deadline_elapsed,
        features.session_metadata.observation_duration_ms,
    );
    let opportunity_score = organic_confidence(
        &features.organic_broadening,
        features,
        profile,
        &config.component_weights,
    );
    let risk_penalty = v3_risk_penalty(features, profile, &config.component_weights);

    if has_hard_risk_contradiction(features, profile) {
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
            ConfidenceBreakdown::capped(
                opportunity_score,
                1.0,
                0.0,
                config.confidence_caps.hard_risk,
                "hard_risk",
            ),
        );
    }

    if let Some(issue) = non_clean_evidence_issue(features, config) {
        return insufficient_evidence_decision(deadline_elapsed, issue, risk_penalty, config);
    }

    if organic_broadening_passes(&features.organic_broadening, features, profile) {
        let confidence = ConfidenceBreakdown::capped(
            opportunity_score,
            risk_penalty,
            1.0,
            config.confidence_caps.execution_not_run,
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

    let reason = if has_sufficient_sample(features, profile) {
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
            config.confidence_caps.organic_broadening_insufficient,
            "organic_broadening_insufficient",
        ),
    )
}

fn insufficient_evidence_decision(
    deadline_elapsed: bool,
    issue: EvidenceIssue,
    risk_penalty: f64,
    config: &GatekeeperV3Config,
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
        ConfidenceBreakdown::capped(
            0.0,
            risk_penalty,
            0.0,
            evidence_issue_confidence_cap(issue, config),
            "insufficient_evidence",
        ),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvidenceIssue {
    Unavailable,
    InsufficientSample,
    Degraded,
}

fn non_clean_evidence_issue(
    features: &MaterializedFeatureSet,
    config: &GatekeeperV3Config,
) -> Option<EvidenceIssue> {
    let evidence = &features.evidence_status;
    let relevant = [
        ("identity", &evidence.identity),
        ("account_state", &evidence.account_state),
        ("tx_intel", &evidence.tx_intel),
        ("tx_segments", &evidence.tx_segments),
        ("checkpoints", &evidence.checkpoints),
        ("trajectory", &evidence.trajectory),
        ("pdd_sequence", &evidence.pdd_sequence),
        ("curve", &evidence.curve),
        ("sybil", &evidence.sybil),
        ("cpv", &evidence.cpv),
        ("fsc", &evidence.fsc),
        ("alpha", &evidence.alpha),
        ("manipulation", &evidence.manipulation),
        ("organic_broadening", &evidence.organic_broadening),
        (
            "manipulation_contradiction",
            &evidence.manipulation_contradiction,
        ),
    ];

    if relevant
        .iter()
        .filter(|(group, _)| config.evidence_requirements.required(group))
        .any(|(_, status)| matches!(status.status, EvidenceStatus::Unavailable))
    {
        return Some(EvidenceIssue::Unavailable);
    }

    if relevant
        .iter()
        .filter(|(group, _)| config.evidence_requirements.required(group))
        .any(|(_, status)| matches!(status.status, EvidenceStatus::InsufficientSample))
    {
        return Some(EvidenceIssue::InsufficientSample);
    }

    if relevant
        .iter()
        .filter(|(group, _)| config.evidence_requirements.required(group))
        .any(|(_, status)| {
            matches!(
                status.status,
                EvidenceStatus::Degraded
                    | EvidenceStatus::Stale
                    | EvidenceStatus::Fallback
                    | EvidenceStatus::ShadowOnly
                    | EvidenceStatus::NotConfigured
            )
        })
    {
        return Some(EvidenceIssue::Degraded);
    }

    None
}

fn has_hard_risk_contradiction(
    features: &MaterializedFeatureSet,
    config: &GatekeeperV3StageProfile,
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
        || risk.max_tx_per_signer > config.max_tx_per_signer
        || risk.dev_volume_ratio > config.max_dev_volume_ratio
        || risk
            .signer_cross_pool_velocity
            .is_some_and(|value| value > config.max_signer_cross_pool_velocity)
        || risk
            .funding_source_concentration
            .is_some_and(|value| value > config.max_funding_source_concentration)
}

fn v3_risk_penalty(
    features: &MaterializedFeatureSet,
    config: &GatekeeperV3StageProfile,
    weights: &GatekeeperV3ComponentWeights,
) -> f64 {
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
        .clamp(0.0, weights.max_risk_penalty)
}

fn has_sufficient_sample(
    features: &MaterializedFeatureSet,
    config: &GatekeeperV3StageProfile,
) -> bool {
    features.organic_broadening.sequence_available
        && features.tx_intel_features.tx_count >= config.min_tx_count
        && features.tx_intel_features.unique_signers >= config.min_unique_signers
        && features.tx_intel_features.buy_count >= config.min_buy_count
}

fn organic_broadening_passes(
    organic: &OrganicBroadeningFeatures,
    features: &MaterializedFeatureSet,
    config: &GatekeeperV3StageProfile,
) -> bool {
    organic.sequence_available
        && organic.total_tx_count >= config.min_tx_count
        && organic.total_unique_signers >= config.min_unique_signers
        && features.tx_intel_features.buy_count >= config.min_buy_count
        && organic.buy_ratio_min >= config.min_buy_ratio
        && organic.buy_ratio_max <= config.max_buy_ratio
        && organic.t1_vs_t0_unique_signer_delta >= 0
        && organic.t2_vs_t1_unique_signer_delta >= 0
        && organic.tx_count_growth_ratio >= config.organic_min_tx_count_growth_ratio
        && organic.unique_signer_growth_ratio >= config.organic_min_unique_signer_growth_ratio
        && organic.max_segment_hhi <= config.max_hhi
}

fn organic_confidence(
    organic: &OrganicBroadeningFeatures,
    features: &MaterializedFeatureSet,
    config: &GatekeeperV3StageProfile,
    weights: &GatekeeperV3ComponentWeights,
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

    let total_weight = weights.total_opportunity_weight();
    clamp01(
        (weights.tx_count * tx_score
            + weights.unique_signers * signer_score
            + weights.buy_count * buy_count_score
            + weights.buy_ratio * buy_ratio_score
            + weights.growth * growth_score)
            / total_weight,
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

fn f64_bits(value: f64) -> String {
    let bits = if value == 0.0 { 0 } else { value.to_bits() };
    format!("{bits:016x}")
}

fn opt_f64_bits(value: Option<f64>) -> Value {
    value.map_or(Value::Null, |value| json!(f64_bits(value)))
}

fn opt_u64_value(value: Option<u64>) -> Value {
    value.map_or(Value::Null, |value| json!(value))
}

fn serde_value<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or_else(|_| Value::String("serialization_error".to_string()))
}

fn funding_source_diagnostics_payload(diagnostics: &FundingSourceDiagnostics) -> Value {
    json!({
        "buyer_sample_count": diagnostics.buyer_sample_count,
        "indeterminate_unknown_buyer_count": diagnostics.indeterminate_unknown_buyer_count,
        "known_source_count": diagnostics.known_source_count,
        "miss_reason_counts": diagnostics.miss_reason_counts
            .iter()
            .map(|count| json!({
                "class": count.class.as_str(),
                "count": count.count,
                "reason": count.reason.as_str()
            }))
            .collect::<Vec<_>>(),
        "operational_unknown_buyer_count": diagnostics.operational_unknown_buyer_count,
        "structural_unknown_buyer_count": diagnostics.structural_unknown_buyer_count,
        "unknown_buyer_count": diagnostics.unknown_buyer_count
    })
}

fn trajectory_segment_payload(segment: &TrajectorySegmentSnapshot) -> Value {
    json!({
        "avg_interval_ms_bits": f64_bits(segment.avg_interval_ms),
        "buy_ratio_bits": f64_bits(segment.buy_ratio),
        "hhi_bits": f64_bits(segment.hhi),
        "max_single_tx_sol_bits": f64_bits(segment.max_single_tx_sol),
        "same_size_streak": segment.same_size_streak,
        "total_volume_sol_bits": f64_bits(segment.total_volume_sol),
        "tx_count": segment.tx_count
    })
}

fn evidence_status_payload(status: &FeatureEvidenceStatus) -> Value {
    json!({
        "degraded_reasons": status.degraded_reasons
            .iter()
            .map(serde_value)
            .collect::<Vec<_>>(),
        "status": serde_value(&status.status),
        "unavailable_reasons": status.unavailable_reasons
            .iter()
            .map(serde_value)
            .collect::<Vec<_>>()
    })
}

fn evidence_issue_confidence_cap(issue: EvidenceIssue, config: &GatekeeperV3Config) -> f64 {
    match issue {
        EvidenceIssue::Unavailable => config.confidence_caps.unavailable,
        EvidenceIssue::InsufficientSample => config.confidence_caps.insufficient_sample,
        EvidenceIssue::Degraded => config.confidence_caps.degraded,
    }
}

fn default_stage_for_evidence_group(group: &str) -> DecisionStage {
    match group {
        "manipulation" | "manipulation_contradiction" => DecisionStage::Risk,
        "organic_broadening" => DecisionStage::Opportunity,
        "execution" => DecisionStage::Confidence,
        _ => DecisionStage::Evidence,
    }
}

fn evaluate_feature_actionability(
    group: &str,
    status: EvidenceStatus,
    stage: DecisionStage,
    config: &GatekeeperV3Config,
) -> &'static str {
    if !config.evidence_requirements.required(group) {
        return match status {
            EvidenceStatus::Clean => "optional_clean",
            EvidenceStatus::InsufficientSample => "optional_wait_sample",
            EvidenceStatus::Unavailable => "optional_unavailable",
            EvidenceStatus::Degraded
            | EvidenceStatus::Stale
            | EvidenceStatus::Fallback
            | EvidenceStatus::ShadowOnly
            | EvidenceStatus::NotConfigured => "optional_degraded",
        };
    }

    match (stage, status) {
        (_, EvidenceStatus::Clean) => "actionable",
        (_, EvidenceStatus::InsufficientSample) => "wait_sample",
        (DecisionStage::Risk, EvidenceStatus::Degraded | EvidenceStatus::Stale) => "not_actionable",
        (DecisionStage::Risk, EvidenceStatus::Fallback | EvidenceStatus::ShadowOnly) => {
            "not_actionable"
        }
        (_, EvidenceStatus::Degraded | EvidenceStatus::Stale) => "degraded",
        (_, EvidenceStatus::Fallback | EvidenceStatus::ShadowOnly) => "degraded",
        (_, EvidenceStatus::Unavailable | EvidenceStatus::NotConfigured) => "not_actionable",
    }
}

fn stage_actionability(actionable: bool) -> &'static str {
    if actionable {
        "actionable"
    } else {
        "not_actionable"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_core::session::types::SessionId;

    #[test]
    fn feature_snapshot_hash_includes_materialization_version_not_session_id() {
        let mut features = MaterializedFeatureSet::default();
        features.session_metadata.session_id = SessionId(1);

        let original = v3_feature_snapshot_hash(&features, 1);
        assert_ne!(original, v3_feature_snapshot_hash(&features, 2));

        features.session_metadata.session_id = SessionId(99);
        assert_eq!(original, v3_feature_snapshot_hash(&features, 1));
    }

    #[test]
    fn component_weights_change_opportunity_score_without_code_changes() {
        let mut features = MaterializedFeatureSet::default();
        features.tx_intel_features.buy_count = 5;
        features.organic_broadening.total_tx_count = 10;
        features.organic_broadening.total_unique_signers = 10;
        features.organic_broadening.buy_ratio_min = 0.80;
        features.organic_broadening.buy_ratio_mean = 0.90;
        features.organic_broadening.buy_ratio_max = 1.0;
        features.organic_broadening.tx_count_growth_ratio = 1.20;
        features.organic_broadening.unique_signer_growth_ratio = 1.20;

        let mut profile = GatekeeperV3StageProfile::default();
        profile.min_tx_count = 10;
        profile.min_unique_signers = 10;
        profile.min_buy_count = 10;
        profile.min_buy_ratio = 0.80;
        profile.max_buy_ratio = 1.0;

        let default_score = organic_confidence(
            &features.organic_broadening,
            &features,
            &profile,
            &GatekeeperV3ComponentWeights::default(),
        );

        let weights = GatekeeperV3ComponentWeights {
            tx_count: 0.0,
            unique_signers: 0.0,
            buy_count: 1.0,
            buy_ratio: 0.0,
            growth: 0.0,
            max_risk_penalty: 0.85,
        };
        let buy_count_only_score =
            organic_confidence(&features.organic_broadening, &features, &profile, &weights);

        assert_ne!(default_score, buy_count_only_score);
        assert_eq!(buy_count_only_score, 0.5);
    }

    #[test]
    fn degraded_manipulation_contradiction_blocks_risk_actionability() {
        let mut features = MaterializedFeatureSet::default();
        features.evidence_status.manipulation_contradiction.status = EvidenceStatus::Degraded;
        let config = GatekeeperV3Config::default();

        let payload = v3_actionability_payload(&features, &config, false);

        assert_eq!(
            payload["groups"]["manipulation_contradiction"]["actionability"],
            json!("not_actionable")
        );
        assert_eq!(payload["stages"]["risk"], json!("not_actionable"));
    }
}
