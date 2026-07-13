use ghost_brain::config::{GatekeeperV2Config, MetricContractFoundationConfigV1};
use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_core::account_state_core::types::AccountStateReserveVelocitySnapshotV1;
use ghost_core::checkpoint::{ManipulationContradictionFeatures, MaterializedFeatureSet};
use ghost_core::metric_contracts::*;
use ghost_launcher::components::gatekeeper::GatekeeperDevPrimaryCompatibilitySnapshotV1;
use ghost_launcher::metric_contracts::*;
use ghost_launcher::tx_intelligence::{
    compute_ftdi, FlipV2ProducerSnapshotV1, FscComputation, FtdiComputation, FundingSourceConfig,
    FundingSourceIndex, FundingSourceProducerConfigSnapshotV1, TxIntelligenceConfig,
    TxIntelligenceEngine, TxIntelligenceMetricContractSnapshotV1, TxTimingProducerSnapshotV1,
};
use seer::early_fingerprint::EarlyFingerprintConfig;

pub struct Pr2cFixture {
    pub complete: Pr2bCompleteMetricContractSnapshotV1,
    pub profile: MetricContractProfileV1,
    pub effective: ResolvedMetricContractEffectiveConfigV1,
}

pub struct Pr2cFrozenInputsFixture {
    ftdi: FtdiComputation,
    tx_snapshot: TxIntelligenceMetricContractSnapshotV1,
    gatekeeper_dev: GatekeeperDevPrimaryCompatibilitySnapshotV1,
    recent_exact: TxTimingProducerSnapshotV1,
    fsc: FscComputation,
    funding: FundingSourceConfig,
    funding_producer_config: FundingSourceProducerConfigSnapshotV1,
    flip: FlipV2ProducerSnapshotV1,
    manipulation: ManipulationFrozenSnapshotV2,
    reserve: AccountStateReserveVelocitySnapshotV1,
    recent: RecentBuySellProducerSnapshotV1,
    profile: MetricContractProfileV1,
    effective: ResolvedMetricContractEffectiveConfigV1,
    source_cutoff: MetricContractDecisionSourceCutoffV1,
}

impl Pr2cFrozenInputsFixture {
    pub fn build_timed(&self) -> Pr2bTimedCompleteMetricContractSnapshotV1 {
        build_pr2b_timed_complete_metric_contract_snapshot_v1(
            Pr2bFrozenProducerInputsV1 {
                pr2a: Pr2aFrozenProducerInputsV1 {
                    ftdi: &self.ftdi,
                    tx_intelligence: &self.tx_snapshot,
                    gatekeeper_dev_primary: &self.gatekeeper_dev,
                    recent_exact_timing: &self.recent_exact,
                    fsc: &self.fsc,
                    funding_source_config: &self.funding,
                    funding_source_producer_config: &self.funding_producer_config,
                },
                legacy_flip_ratio: None,
                flip_v2: &self.flip,
                manipulation: &self.manipulation,
                reserve_velocity: &self.reserve,
                recent_buy_sell: &self.recent,
            },
            &Pr2bBuildContextV1 {
                rollout_mode: MetricContractRolloutMode::Legacy,
                profile: &self.profile,
                effective_config: &self.effective,
                source_cutoff: self.source_cutoff.clone(),
            },
        )
        .unwrap()
    }

    pub fn profile(&self) -> &MetricContractProfileV1 {
        &self.profile
    }

    pub fn effective_config(&self) -> &ResolvedMetricContractEffectiveConfigV1 {
        &self.effective
    }
}

fn frozen_manipulation() -> ManipulationFrozenSnapshotV2 {
    freeze_manipulation_producer_snapshot_v2(
        &MaterializedFeatureSet::default(),
        ManipulationContradictionFeatures::default(),
    )
}

pub fn frozen_inputs_fixture() -> Pr2cFrozenInputsFixture {
    let gatekeeper = GatekeeperV2Config::default();
    let fingerprint = EarlyFingerprintConfig::default();
    let tx_config = TxIntelligenceConfig::from_gatekeeper_config(&gatekeeper, fingerprint.clone());
    let funding = FundingSourceConfig::from_gatekeeper_config(&gatekeeper);
    let effective = resolve_metric_contract_effective_config_v1(
        MetricContractFoundationConfigV1::default(),
        &gatekeeper,
        &tx_config,
        &fingerprint,
        &funding,
        None,
    )
    .unwrap();
    let profile = MetricContractProfileV1::profile_a().unwrap();
    let source_cutoff = MetricContractDecisionSourceCutoffV1::try_new(10_000, Some(100)).unwrap();
    let candidate = EnhancedCandidate {
        timestamp: 1_000,
        ..EnhancedCandidate::default()
    };
    let engine = TxIntelligenceEngine::new(tx_config, &candidate, None);
    let tx_snapshot = engine
        .metric_contract_snapshot(&ghost_core::tx_intelligence::types::TxIntelFeatures::default());
    let flip = engine.flip_v2_snapshot(10_000, Some(100));
    let ftdi = compute_ftdi(std::iter::empty::<&ghost_launcher::events::PoolTransaction>());
    let gatekeeper_dev = GatekeeperDevPrimaryCompatibilitySnapshotV1 {
        amount_sol: None,
        creator_known: false,
        create_signature: None,
        create_signature_matched: false,
        selection_mode: DevBuySelectionModeV1::NoEligibleBuy,
        selected_signature: None,
        selected_slot: None,
        selected_transaction_index: None,
        eligible_buy_count: 0,
        selected_success: None,
    };
    let recent_exact = TxTimingProducerSnapshotV1 {
        numerator: 0,
        denominator: 0,
        ratio: None,
        canonical_dedupe_applied: true,
        dust_filter_sol: None,
        window_ms: Some(10_000),
        fallback_timestamp_count: 0,
        fallback_ordering_count: 0,
        source_complete: true,
        source_state_capacity: Some(
            u64::try_from(gatekeeper.decision_time_series_tx_capacity).unwrap(),
        ),
    };
    let fsc = FundingSourceIndex::new().compute_for_transactions(
        std::iter::empty::<&ghost_launcher::events::PoolTransaction>(),
        &funding,
    );
    let funding_producer_config = funding
        .metric_contract_producer_config_snapshot(None)
        .unwrap();
    let reserve = AccountStateReserveVelocitySnapshotV1::default();
    let recent = RecentBuySellProducerSnapshotV1 {
        window_ms: 10_000,
        buy_count: 0,
        sell_count: 0,
        transaction_count: 0,
        failed_transaction_count: 0,
        source_complete: true,
    };
    let manipulation = frozen_manipulation();
    Pr2cFrozenInputsFixture {
        ftdi,
        tx_snapshot,
        gatekeeper_dev,
        recent_exact,
        fsc,
        funding,
        funding_producer_config,
        flip,
        manipulation,
        reserve,
        recent,
        profile,
        effective,
        source_cutoff,
    }
}

pub fn complete_snapshot_fixture() -> Pr2cFixture {
    let frozen = frozen_inputs_fixture();
    let complete = frozen.build_timed().into_snapshot();
    Pr2cFixture {
        complete,
        profile: frozen.profile,
        effective: frozen.effective,
    }
}

pub fn equal_policy() -> MetricContractPolicyEquivalenceSnapshotV1 {
    MetricContractPolicyEquivalenceSnapshotV1 {
        verdict: "REJECT".to_string(),
        primary_reason_code: "PHASE1_INSUFFICIENT".to_string(),
        ordered_reason_chain: vec!["PHASE1_INSUFFICIENT".to_string()],
        phase_pass_vector: vec![false, false, false, false, false, false],
        soft_points: 0,
        selector_soft_score_bits: 0,
        hard_fail_classification: "none".to_string(),
    }
}

pub fn paired_fixture(run_id: &str, join_key: &str) -> MetricContractPairedRecordV1 {
    paired_fixture_with_stable_identity(
        run_id,
        join_key,
        Some(StableEventIdentityV1::try_from_signature("seer", format!("sig-{join_key}")).unwrap()),
    )
}

pub fn paired_fixture_with_stable_identity(
    run_id: &str,
    join_key: &str,
    stable_event_identity: Option<StableEventIdentityV1>,
) -> MetricContractPairedRecordV1 {
    let fixture = complete_snapshot_fixture();
    let policy = equal_policy();
    build_pr2c_paired_record_v1(
        &fixture.complete,
        &Pr2cDecisionRecordContextV1 {
            record_identity: MetricEvidenceRecordIdentityV1::try_new(
                run_id,
                join_key,
                "legacy_live",
            )
            .unwrap(),
            stable_event_identity,
            rollout_mode: MetricContractRolloutMode::Legacy,
            profile: &fixture.profile,
            effective_config: &fixture.effective,
            authoritative_policy: &policy,
            comparator_policy: &policy,
            counterfactual_delta_present: false,
            comparator_elapsed_us: 10,
            metric_contract_serialize_us: 20,
            metric_contract_build_and_serialize_us: 30,
            projection_build_and_validate_us: 20,
            gatekeeper_config_hash: "gatekeeper-config-a",
            brain_config_hash: Some("brain-config-a"),
            writer_timestamp_ms: 20_000,
        },
    )
    .unwrap()
}

pub fn mfs_with_projection(pair: &MetricContractPairedRecordV1) -> MaterializedFeatureSet {
    MaterializedFeatureSet {
        metric_contract_decision_projection_v1: Some(pair.decision_time_projection.clone()),
        ..MaterializedFeatureSet::default()
    }
}
