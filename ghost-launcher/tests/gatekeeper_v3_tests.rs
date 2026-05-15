use ghost_brain::config::GatekeeperV3Config;
use ghost_brain::oracle::reason_code::GatekeeperReasonCode;
use ghost_core::checkpoint::{
    EvidenceStatus, EvidenceUnavailableReason, FeatureEvidenceStatus,
    ManipulationContradictionFeatures, MaterializedEvidenceStatus, MaterializedFeatureSet,
    OrganicBroadeningFeatures,
};
use ghost_launcher::components::gatekeeper_v3::{
    evaluate_v3_from_features, v3_actionability_payload, v3_feature_snapshot_hash, V3ShadowVerdict,
    V3_SHADOW_SCHEMA_VERSION,
};

fn test_config() -> GatekeeperV3Config {
    let mut config = GatekeeperV3Config::default();
    config.shadow_emit_enabled = true;
    for profile in [&mut config.early, &mut config.normal, &mut config.extended] {
        profile.min_tx_count = 9;
        profile.min_unique_signers = 6;
        profile.min_buy_count = 5;
        profile.min_buy_ratio = 0.50;
        profile.max_buy_ratio = 0.95;
        profile.max_hhi = 0.25;
        profile.hard_fail_hhi = 0.10;
        profile.hard_fail_same_ms_tx_ratio = 0.60;
        profile.hard_fail_top3_volume_pct = 0.70;
        profile.max_tx_per_signer = 4;
        profile.max_dev_volume_ratio = 0.40;
        profile.reject_on_dev_sell = true;
    }
    config
}

fn clean_status() -> FeatureEvidenceStatus {
    FeatureEvidenceStatus {
        status: EvidenceStatus::Clean,
        degraded_reasons: Vec::new(),
        unavailable_reasons: Vec::new(),
    }
}

fn unavailable_status(reason: EvidenceUnavailableReason) -> FeatureEvidenceStatus {
    FeatureEvidenceStatus {
        status: EvidenceStatus::Unavailable,
        degraded_reasons: Vec::new(),
        unavailable_reasons: vec![reason],
    }
}

fn clean_materialized_evidence() -> MaterializedEvidenceStatus {
    MaterializedEvidenceStatus {
        identity: clean_status(),
        account_state: clean_status(),
        tx_intel: clean_status(),
        tx_segments: clean_status(),
        checkpoints: clean_status(),
        trajectory: clean_status(),
        pdd_sequence: clean_status(),
        curve: clean_status(),
        sybil: clean_status(),
        cpv: clean_status(),
        fsc: clean_status(),
        alpha: clean_status(),
        manipulation: clean_status(),
        organic_broadening: clean_status(),
        manipulation_contradiction: clean_status(),
        execution: unavailable_status(EvidenceUnavailableReason::ExecutionNotRun),
    }
}

fn strong_organic_features() -> MaterializedFeatureSet {
    let mut features = MaterializedFeatureSet::default();
    features.evidence_status = clean_materialized_evidence();
    features.tx_intel_features.tx_count = 12;
    features.tx_intel_features.buy_count = 8;
    features.tx_intel_features.unique_signers = 8;
    features.tx_intel_features.buy_ratio = 0.67;
    features.tx_intel_features.hhi = 0.05;
    features.tx_intel_features.top3_volume_pct = 0.30;
    features.tx_intel_features.same_ms_tx_ratio = 0.05;
    features.tx_intel_features.max_tx_per_signer = 2;
    features.tx_intel_features.dev_has_sold = false;
    features.alpha_fingerprint.jito_tip_intensity = Some(0.10);
    features.sybil_resistance.signer_cross_pool_velocity = Some(0.05);
    features.sybil_resistance.funding_source_concentration = Some(0.10);
    features.organic_broadening = OrganicBroadeningFeatures {
        sequence_available: true,
        total_tx_count: 12,
        total_unique_signers: 8,
        t0_tx_count: 3,
        t1_tx_count: 4,
        t2_tx_count: 5,
        t0_unique_signers: 2,
        t1_unique_signers: 3,
        t2_unique_signers: 4,
        t1_vs_t0_unique_signer_delta: 1,
        t2_vs_t1_unique_signer_delta: 1,
        tx_count_growth_ratio: 1.25,
        unique_signer_growth_ratio: 1.33,
        buy_ratio_mean: 0.67,
        buy_ratio_min: 0.60,
        buy_ratio_max: 0.75,
        max_segment_hhi: 0.08,
        min_segment_hhi: 0.03,
        signer_growth_t2_t0: 2,
        hhi_delta_t2_t0: -0.02,
        tx_count_growth_vs_signer_growth: -0.08,
        new_signer_ratio_t2: 0.50,
        broadening_score: 0.72,
        status: EvidenceStatus::Clean,
        degraded_reasons: Vec::new(),
    };
    features.manipulation_contradictions = ManipulationContradictionFeatures {
        same_ms_tx_ratio: 0.05,
        bundle_suspicion_ratio: 0.05,
        top3_volume_pct: 0.30,
        hhi: 0.05,
        max_tx_per_signer: 2,
        dev_volume_ratio: 0.05,
        dev_has_sold: false,
        signer_cross_pool_velocity: Some(0.05),
        funding_source_concentration: Some(0.10),
        ..ManipulationContradictionFeatures::default()
    };
    features
}

#[test]
fn gatekeeper_v3_hard_risk_wins_over_organic_opportunity() {
    let mut features = strong_organic_features();
    features.manipulation_contradictions.dev_has_sold = true;

    let decision = evaluate_v3_from_features(&features, &test_config(), false);

    assert_eq!(decision.schema_version, V3_SHADOW_SCHEMA_VERSION);
    assert_eq!(decision.verdict, V3ShadowVerdict::Reject);
    assert_eq!(
        decision.reason_code,
        GatekeeperReasonCode::RejectV3ManipulationContradiction
    );
    assert_eq!(
        decision.reason_chain[0],
        GatekeeperReasonCode::V3ManipulationContradiction
    );
}

#[test]
fn gatekeeper_v3_missing_critical_evidence_produces_pending_not_buy() {
    let mut features = strong_organic_features();
    features.evidence_status.tx_segments =
        unavailable_status(EvidenceUnavailableReason::SegmentSequenceMissing);

    let decision = evaluate_v3_from_features(&features, &test_config(), false);

    assert_eq!(decision.verdict, V3ShadowVerdict::Pending);
    assert_eq!(
        decision.reason_code,
        GatekeeperReasonCode::PendingV3WaitEvidence
    );
    assert_eq!(
        decision.reason_chain[0],
        GatekeeperReasonCode::V3EvidenceUnavailable
    );
    assert_eq!(decision.confidence, 0.0);
}

#[test]
fn gatekeeper_v3_deadline_elapsed_maps_insufficient_evidence_to_timeout() {
    let mut features = strong_organic_features();
    features.evidence_status.curve =
        unavailable_status(EvidenceUnavailableReason::CurveDataMissing);

    let decision = evaluate_v3_from_features(&features, &test_config(), true);

    assert_eq!(decision.verdict, V3ShadowVerdict::Timeout);
    assert_eq!(
        decision.reason_code,
        GatekeeperReasonCode::TimeoutV3DegradedEvidence
    );
    assert_eq!(
        decision.reason_chain[0],
        GatekeeperReasonCode::V3EvidenceUnavailable
    );
}

#[test]
fn gatekeeper_v3_strong_organic_broadening_can_produce_buy_candidate() {
    let features = strong_organic_features();

    let decision = evaluate_v3_from_features(&features, &test_config(), false);

    assert_eq!(decision.verdict, V3ShadowVerdict::BuyCandidate);
    assert_eq!(
        decision.reason_code,
        GatekeeperReasonCode::BuyV3NormalConfirmedOpportunity
    );
    assert!(decision.confidence > 0.0);
}

#[test]
fn gatekeeper_v3_uses_early_profile_before_early_window() {
    let mut config = test_config();
    config.early_window_ms = 2_000;
    config.normal.min_tx_count = 99;

    let mut early_features = strong_organic_features();
    early_features.session_metadata.observation_duration_ms = 1_999;
    let early_decision = evaluate_v3_from_features(&early_features, &config, false);
    let early_actionability = v3_actionability_payload(&early_features, &config, false);

    let mut normal_features = early_features.clone();
    normal_features.session_metadata.observation_duration_ms = 2_000;
    let normal_decision = evaluate_v3_from_features(&normal_features, &config, false);
    let normal_actionability = v3_actionability_payload(&normal_features, &config, false);

    assert_eq!(early_decision.verdict, V3ShadowVerdict::BuyCandidate);
    assert_eq!(early_actionability["profile"], "early");
    assert_eq!(early_actionability["stages"]["opportunity"], "actionable");
    assert_eq!(normal_decision.verdict, V3ShadowVerdict::Pending);
    assert_eq!(normal_actionability["profile"], "normal");
    assert_eq!(
        normal_actionability["stages"]["opportunity"],
        "not_actionable"
    );
}

#[test]
fn gatekeeper_v3_is_deterministic_for_same_snapshot() {
    let features = strong_organic_features();
    let config = test_config();

    let first = evaluate_v3_from_features(&features, &config, false);
    let second = evaluate_v3_from_features(&features, &config, false);

    assert_eq!(first, second);
}

#[test]
fn gatekeeper_v3_feature_snapshot_hash_is_stable_and_feature_sensitive() {
    let features = strong_organic_features();
    let mut changed = features.clone();
    changed.tx_intel_features.tx_count += 1;

    let first = v3_feature_snapshot_hash(&features, 1);
    let second = v3_feature_snapshot_hash(&features, 1);
    let changed_hash = v3_feature_snapshot_hash(&changed, 1);

    assert_eq!(first, second);
    assert_ne!(first, changed_hash);
}

#[test]
fn gatekeeper_v3_evaluator_only_needs_snapshot_config_and_deadline() {
    let features = MaterializedFeatureSet::default();
    let config = test_config();

    let decision = evaluate_v3_from_features(&features, &config, false);

    assert_eq!(decision.verdict, V3ShadowVerdict::Pending);
    assert_eq!(
        decision.reason_code,
        GatekeeperReasonCode::PendingV3WaitEvidence
    );
}

#[test]
fn gatekeeper_v3_actionability_is_local_to_v3_sidecar() {
    let features = strong_organic_features();
    let config = test_config();

    let payload = v3_actionability_payload(&features, &config, false);

    assert_eq!(payload["stages"]["evidence"], "actionable");
    assert_eq!(payload["stages"]["opportunity"], "actionable");
    assert_eq!(payload["groups"]["tx_intel"]["actionability"], "actionable");
}
