use ghost_brain::config::{
    GatekeeperMode, GatekeeperV2Config, GatekeeperV3Config, MetricContractFoundationConfigV1,
};
use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_core::account_state_core::types::AccountStateReserveVelocitySnapshotV1;
use ghost_core::checkpoint::{
    DecisionTimeSeriesPriceSource, DecisionTimeSeriesRetentionStatus,
    DecisionTimeSeriesSourceCounts, EvidenceStatus, ManipulationContradictionFeatures,
    MaterializedFeatureSet,
};
use ghost_core::metric_contracts::*;
use ghost_launcher::components::gatekeeper::{
    GatekeeperAssessment, GatekeeperBuffer, GatekeeperDevPrimaryCompatibilitySnapshotV1,
    GatekeeperVerdict,
};
use ghost_launcher::components::gatekeeper_policy::{
    build_assessment_from_features, evaluate_policy_from_assessment, PolicyEvaluationContext,
};
use ghost_launcher::components::gatekeeper_v3::{
    evaluate_v3_from_features, v3_actionability_payload, v3_component_scores_payload,
    v3_feature_snapshot_hash_from_payload,
};
use ghost_launcher::metric_contracts::*;
use ghost_launcher::tx_intelligence::{
    compute_ftdi, DevBuyProducerSnapshotV1, FlipV2ProducerSnapshotV1, FscComputation,
    FtdiComputation, FundingSourceConfig, FundingSourceIndex,
    FundingSourceProducerConfigSnapshotV1, TxIntelligenceConfig, TxIntelligenceEngine,
    TxIntelligenceMetricContractSnapshotV1, TxTimingProducerSnapshotV1,
};
use seer::early_fingerprint::EarlyFingerprintConfig;

pub const TEST_GATEKEEPER_CONFIG_HASH: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub const TEST_BRAIN_CONFIG_HASH: &str =
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

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

pub struct CurrentV33UnroutedFixture {
    pub log: ghost_brain::oracle::GatekeeperBuyLog,
    pub assessment: GatekeeperAssessment,
    pub config: GatekeeperV2Config,
}

impl Pr2cFrozenInputsFixture {
    pub fn build_timed(&self) -> Pr2bTimedCompleteMetricContractSnapshotV1 {
        let context = Pr2bBuildContextV1 {
            rollout_mode: MetricContractRolloutMode::Legacy,
            profile: &self.profile,
            effective_config: &self.effective,
            source_cutoff: self.source_cutoff.clone(),
        };
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
            &context,
        )
        .unwrap()
    }

    pub fn build_timed_from(
        &self,
        full_path_started: std::time::Instant,
    ) -> Pr2bTimedCompleteMetricContractSnapshotV1 {
        let context = Pr2bBuildContextV1 {
            rollout_mode: MetricContractRolloutMode::Legacy,
            profile: &self.profile,
            effective_config: &self.effective,
            source_cutoff: self.source_cutoff.clone(),
        };
        build_pr2b_timed_complete_metric_contract_snapshot_from_started_v1(
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
            &context,
            full_path_started,
        )
        .unwrap()
    }

    pub fn profile(&self) -> &MetricContractProfileV1 {
        &self.profile
    }

    pub fn effective_config(&self) -> &ResolvedMetricContractEffectiveConfigV1 {
        &self.effective
    }

    pub fn with_dev_counterfactual(mut self, legacy_amount: f64, primary_amount: f64) -> Self {
        self.tx_snapshot.dev_first_observed = DevBuyProducerSnapshotV1 {
            amount_sol: Some(legacy_amount),
            creator_known: true,
            create_signature: Some("create-signature".to_string()),
            create_signature_matched: false,
            selection_mode: DevBuySelectionModeV1::LegacyFirstObserved,
            selected_signature: Some("legacy-signature".to_string()),
            selected_slot: Some(100),
            selected_transaction_index: Some(0),
            eligible_buy_count: 1,
            selected_success: Some(true),
            selection_complete: true,
        };
        self.tx_snapshot.dev_primary_v1 = DevBuyProducerSnapshotV1 {
            amount_sol: Some(primary_amount),
            creator_known: true,
            create_signature: Some("create-signature".to_string()),
            create_signature_matched: true,
            selection_mode: DevBuySelectionModeV1::CreateSignatureMatch,
            selected_signature: Some("create-signature".to_string()),
            selected_slot: Some(100),
            selected_transaction_index: Some(1),
            eligible_buy_count: 1,
            selected_success: Some(true),
            selection_complete: true,
        };
        self.gatekeeper_dev = GatekeeperDevPrimaryCompatibilitySnapshotV1 {
            amount_sol: Some(primary_amount),
            creator_known: true,
            create_signature: Some("create-signature".to_string()),
            create_signature_matched: true,
            selection_mode: DevBuySelectionModeV1::CreateSignatureMatch,
            selected_signature: Some("create-signature".to_string()),
            selected_slot: Some(100),
            selected_transaction_index: Some(1),
            eligible_buy_count: 1,
            selected_success: Some(true),
        };
        self
    }

    pub fn with_ftdi_counterfactual(mut self) -> Self {
        // Three accepted BUY transactions satisfy the frozen legacy gate, while
        // two unique buyers remain below the corrected three-buyer gate. The
        // ratio and HHI are exact outputs for two singleton fee topologies.
        self.ftdi = FtdiComputation {
            fee_topology_diversity_index: Some(1.0),
            unique_topology_count: 2,
            coordination_hhi: Some(0.5),
            legacy_buy_tx_actionable: true,
            unique_buyer_actionable_v2: false,
            degraded_reasons: Vec::new(),
            buy_sample_count: 3,
            signer_sample_count: 2,
        };
        self
    }

    pub fn with_clean_flip_evaluable(mut self) -> Self {
        let anchor_identity = StableEventIdentityV1::try_from_signature("seer", "flip-anchor")
            .expect("test stable identity");
        self.flip = FlipV2ProducerSnapshotV1 {
            ratio: Some(0.0),
            eligible_buyer_count: 1,
            flipper_count: 0,
            owners: vec![FlipOwnerEvidenceV2 {
                owner_id: "owner-a".to_string(),
                status: FlipOwnerStatusV2::Tracking,
                anchor_event_identity: CanonicalNullableV1::Value(anchor_identity),
                anchor_slot: CanonicalNullableV1::Value(CanonicalU64StringV1::new(100)),
                anchor_timestamp_ms: CanonicalNullableV1::Value(CanonicalU64StringV1::new(9_000)),
                pre_anchor_sell_count: 0,
                cumulative_eligible_buy_tokens: CanonicalU128StringV1::new(100),
                cumulative_eligible_sell_tokens: CanonicalU128StringV1::new(0),
                qualifying_sell_event_identity: CanonicalNullableV1::Null,
                qualifying_sell_slot: CanonicalNullableV1::Null,
                qualifying_sell_timestamp_ms: CanonicalNullableV1::Null,
            }],
            config: self.flip.config.clone(),
            evaluable: true,
            reasons: Vec::new(),
            dedupe_eviction_count: 0,
            wallet_eviction_count: 0,
        };
        self
    }

    pub fn with_dev_primary_only(mut self, primary_amount: f64) -> Self {
        self.tx_snapshot.dev_primary_v1 = DevBuyProducerSnapshotV1 {
            amount_sol: Some(primary_amount),
            creator_known: true,
            create_signature: Some("create-signature".to_string()),
            create_signature_matched: true,
            selection_mode: DevBuySelectionModeV1::CreateSignatureMatch,
            selected_signature: Some("create-signature".to_string()),
            selected_slot: Some(100),
            selected_transaction_index: Some(1),
            eligible_buy_count: 1,
            selected_success: Some(true),
            selection_complete: true,
        };
        self.gatekeeper_dev = GatekeeperDevPrimaryCompatibilitySnapshotV1 {
            amount_sol: Some(primary_amount),
            creator_known: true,
            create_signature: Some("create-signature".to_string()),
            create_signature_matched: true,
            selection_mode: DevBuySelectionModeV1::CreateSignatureMatch,
            selected_signature: Some("create-signature".to_string()),
            selected_slot: Some(100),
            selected_transaction_index: Some(1),
            eligible_buy_count: 1,
            selected_success: Some(true),
        };
        self
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
            comparator_evaluable: true,
            comparator_elapsed_us: 10,
            metric_contract_serialize_us: 20,
            metric_contract_build_and_serialize_us: 0,
            projection_build_and_validate_us: 20,
            gatekeeper_config_hash: TEST_GATEKEEPER_CONFIG_HASH,
            brain_config_hash: Some(TEST_BRAIN_CONFIG_HASH),
        },
    )
    .unwrap()
}

pub fn paired_fixture_with_comparator(
    run_id: &str,
    join_key: &str,
    comparator_policy: &MetricContractPolicyEquivalenceSnapshotV1,
    comparator_evaluable: bool,
) -> MetricContractPairedRecordV1 {
    let fixture = complete_snapshot_fixture();
    let authoritative_policy = equal_policy();
    build_pr2c_paired_record_v1(
        &fixture.complete,
        &Pr2cDecisionRecordContextV1 {
            record_identity: MetricEvidenceRecordIdentityV1::try_new(
                run_id,
                join_key,
                "legacy_live",
            )
            .unwrap(),
            stable_event_identity: Some(
                StableEventIdentityV1::try_from_signature("seer", format!("sig-{join_key}"))
                    .unwrap(),
            ),
            rollout_mode: MetricContractRolloutMode::Legacy,
            profile: &fixture.profile,
            effective_config: &fixture.effective,
            authoritative_policy: &authoritative_policy,
            comparator_policy,
            comparator_evaluable,
            comparator_elapsed_us: 10,
            metric_contract_serialize_us: 0,
            metric_contract_build_and_serialize_us: 0,
            projection_build_and_validate_us: 20,
            gatekeeper_config_hash: TEST_GATEKEEPER_CONFIG_HASH,
            brain_config_hash: Some(TEST_BRAIN_CONFIG_HASH),
        },
    )
    .unwrap()
}

pub fn paired_fixture_with_policies(
    run_id: &str,
    join_key: &str,
    authoritative_policy: &MetricContractPolicyEquivalenceSnapshotV1,
    comparator_policy: &MetricContractPolicyEquivalenceSnapshotV1,
) -> MetricContractPairedRecordV1 {
    let fixture = complete_snapshot_fixture();
    build_pr2c_paired_record_v1(
        &fixture.complete,
        &Pr2cDecisionRecordContextV1 {
            record_identity: MetricEvidenceRecordIdentityV1::try_new(
                run_id,
                join_key,
                "legacy_live",
            )
            .unwrap(),
            stable_event_identity: Some(
                StableEventIdentityV1::try_from_signature("seer", format!("sig-{join_key}"))
                    .unwrap(),
            ),
            rollout_mode: MetricContractRolloutMode::Legacy,
            profile: &fixture.profile,
            effective_config: &fixture.effective,
            authoritative_policy,
            comparator_policy,
            comparator_evaluable: true,
            comparator_elapsed_us: 10,
            metric_contract_serialize_us: 0,
            metric_contract_build_and_serialize_us: 0,
            projection_build_and_validate_us: 20,
            gatekeeper_config_hash: TEST_GATEKEEPER_CONFIG_HASH,
            brain_config_hash: Some(TEST_BRAIN_CONFIG_HASH),
        },
    )
    .unwrap()
}

pub fn paired_fixture_with_dev_counterfactual(
    run_id: &str,
    join_key: &str,
) -> MetricContractPairedRecordV1 {
    let frozen = frozen_inputs_fixture().with_dev_counterfactual(0.5, 0.75);
    let timed = frozen.build_timed();
    let policy = equal_policy();
    build_pr2c_paired_record_v1(
        timed.snapshot(),
        &Pr2cDecisionRecordContextV1 {
            record_identity: MetricEvidenceRecordIdentityV1::try_new(
                run_id,
                join_key,
                "legacy_live",
            )
            .unwrap(),
            stable_event_identity: Some(
                StableEventIdentityV1::try_from_signature("seer", format!("sig-{join_key}"))
                    .unwrap(),
            ),
            rollout_mode: MetricContractRolloutMode::Legacy,
            profile: frozen.profile(),
            effective_config: frozen.effective_config(),
            authoritative_policy: &policy,
            comparator_policy: &policy,
            comparator_evaluable: true,
            comparator_elapsed_us: 10,
            metric_contract_serialize_us: 0,
            metric_contract_build_and_serialize_us: 0,
            projection_build_and_validate_us: timed.timings().projection_build_and_validate_us,
            gatekeeper_config_hash: TEST_GATEKEEPER_CONFIG_HASH,
            brain_config_hash: Some(TEST_BRAIN_CONFIG_HASH),
        },
    )
    .unwrap()
}

pub fn paired_fixture_with_ftdi_counterfactual(
    run_id: &str,
    join_key: &str,
) -> MetricContractPairedRecordV1 {
    let frozen = frozen_inputs_fixture().with_ftdi_counterfactual();
    let timed = frozen.build_timed();
    let policy = equal_policy();
    build_pr2c_paired_record_v1(
        timed.snapshot(),
        &Pr2cDecisionRecordContextV1 {
            record_identity: MetricEvidenceRecordIdentityV1::try_new(
                run_id,
                join_key,
                "legacy_live",
            )
            .unwrap(),
            stable_event_identity: Some(
                StableEventIdentityV1::try_from_signature("seer", format!("sig-{join_key}"))
                    .unwrap(),
            ),
            rollout_mode: MetricContractRolloutMode::Legacy,
            profile: frozen.profile(),
            effective_config: frozen.effective_config(),
            authoritative_policy: &policy,
            comparator_policy: &policy,
            comparator_evaluable: true,
            comparator_elapsed_us: 10,
            metric_contract_serialize_us: 0,
            metric_contract_build_and_serialize_us: 0,
            projection_build_and_validate_us: timed.timings().projection_build_and_validate_us,
            gatekeeper_config_hash: TEST_GATEKEEPER_CONFIG_HASH,
            brain_config_hash: Some(TEST_BRAIN_CONFIG_HASH),
        },
    )
    .unwrap()
}

pub fn paired_fixture_with_degraded_flip(
    run_id: &str,
    join_key: &str,
) -> MetricContractPairedRecordV1 {
    let frozen = frozen_inputs_fixture().with_clean_flip_evaluable();
    let timed = frozen.build_timed();
    let policy = equal_policy();
    let mut pair = build_pr2c_paired_record_v1(
        timed.snapshot(),
        &Pr2cDecisionRecordContextV1 {
            record_identity: MetricEvidenceRecordIdentityV1::try_new(
                run_id,
                join_key,
                "legacy_live",
            )
            .unwrap(),
            stable_event_identity: Some(
                StableEventIdentityV1::try_from_signature("seer", format!("sig-{join_key}"))
                    .unwrap(),
            ),
            rollout_mode: MetricContractRolloutMode::Legacy,
            profile: frozen.profile(),
            effective_config: frozen.effective_config(),
            authoritative_policy: &policy,
            comparator_policy: &policy,
            comparator_evaluable: true,
            comparator_elapsed_us: 10,
            metric_contract_serialize_us: 0,
            metric_contract_build_and_serialize_us: 0,
            projection_build_and_validate_us: timed.timings().projection_build_and_validate_us,
            gatekeeper_config_hash: TEST_GATEKEEPER_CONFIG_HASH,
            brain_config_hash: Some(TEST_BRAIN_CONFIG_HASH),
        },
    )
    .unwrap();

    // This fixture models a durable, semantically valid degraded producer
    // result. It must remain evaluable for replay, but it is not a clean Flip
    // sample for offline audit aggregation.
    pair.evidence
        .payload
        .contracts
        .flip_ratio
        .hybrid_v2
        .envelope
        .measurement_quality = MetricMeasurementQualityV1::Degraded;
    pair.decision_time_projection
        .flip_ratio
        .hybrid_v2_ratio
        .envelope
        .measurement_quality = MetricMeasurementQualityV1::Degraded;
    pair.evidence = MetricContractEvidenceTransportV1::try_new(
        pair.evidence.payload.clone(),
        pair.evidence.writer_timestamp_ms,
        pair.evidence.rotation_part_index,
    )
    .unwrap();
    pair.decision_v34.evidence_sha256 = pair.evidence.evidence_sha256.clone();
    pair.decision_time_projection_hash = pair
        .decision_time_projection
        .validated_canonical_hash(&MetricDecisionProjectionBuildContextV1 {
            rollout_mode: MetricContractRolloutMode::Legacy,
            profile: frozen.profile(),
            effective_config: frozen.effective_config(),
            source_cutoff: pair.evidence.payload.source_cutoff.clone(),
        })
        .unwrap();
    pair
}

pub fn paired_fixture_with_dev_primary_only(
    run_id: &str,
    join_key: &str,
) -> MetricContractPairedRecordV1 {
    let frozen = frozen_inputs_fixture().with_dev_primary_only(0.75);
    let timed = frozen.build_timed();
    let policy = equal_policy();
    build_pr2c_paired_record_v1(
        timed.snapshot(),
        &Pr2cDecisionRecordContextV1 {
            record_identity: MetricEvidenceRecordIdentityV1::try_new(
                run_id,
                join_key,
                "legacy_live",
            )
            .unwrap(),
            stable_event_identity: Some(
                StableEventIdentityV1::try_from_signature("seer", format!("sig-{join_key}"))
                    .unwrap(),
            ),
            rollout_mode: MetricContractRolloutMode::Legacy,
            profile: frozen.profile(),
            effective_config: frozen.effective_config(),
            authoritative_policy: &policy,
            comparator_policy: &policy,
            comparator_evaluable: true,
            comparator_elapsed_us: 10,
            metric_contract_serialize_us: 0,
            metric_contract_build_and_serialize_us: 0,
            projection_build_and_validate_us: timed.timings().projection_build_and_validate_us,
            gatekeeper_config_hash: TEST_GATEKEEPER_CONFIG_HASH,
            brain_config_hash: Some(TEST_BRAIN_CONFIG_HASH),
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

pub fn current_v33_unrouted_fixture(
    projection: &MetricContractDecisionEvidenceProjectionV1,
) -> CurrentV33UnroutedFixture {
    // A current v33 decision row normally carries the same bounded decision
    // series both in its top-level replay vectors and in the materialized MFS
    // snapshot. Populate the configured 128-sample bound instead of using an
    // arbitrary padding field as the storage denominator.
    const SAMPLE_COUNT: usize = 128;
    let mut features = MaterializedFeatureSet {
        metric_contract_decision_projection_v1: Some(projection.clone()),
        ..MaterializedFeatureSet::default()
    };
    let ts_offsets_ms = (0..SAMPLE_COUNT)
        .map(|index| i64::try_from(index).unwrap() * 79)
        .collect::<Vec<_>>();
    let sol_amounts = (0..SAMPLE_COUNT)
        .map(|index| 0.123_456_789_012_345_f64 + index as f64 / 10_000.0)
        .collect::<Vec<_>>();
    let prices = (0..SAMPLE_COUNT)
        .map(|index| Some(0.000_000_123_456_789_f64 + index as f64 / 1_000_000_000.0))
        .collect::<Vec<_>>();
    let interval_ms = vec![79.123_456_789_012_34_f64; SAMPLE_COUNT - 1];
    let d_price = vec![Some(0.000_000_001_234_567_89_f64); SAMPLE_COUNT - 1];
    features.decision_time_series.status = EvidenceStatus::Clean;
    features.decision_time_series.retention_status = DecisionTimeSeriesRetentionStatus::Clean;
    features.decision_time_series.retention_capacity = SAMPLE_COUNT as u64;
    features.decision_time_series.retained_sample_count = SAMPLE_COUNT as u64;
    features.decision_time_series.total_tx_count = SAMPLE_COUNT as u64;
    features.decision_time_series.sample_count = SAMPLE_COUNT as u64;
    features.decision_time_series.finite_price_count = SAMPLE_COUNT as u64;
    features.decision_time_series.price_coverage_ratio = Some(1.0);
    features.decision_time_series.ts_offsets_ms = ts_offsets_ms;
    features.decision_time_series.sol_amounts = sol_amounts;
    features.decision_time_series.prices = prices;
    features.decision_time_series.price_sources =
        vec![DecisionTimeSeriesPriceSource::Reserve; SAMPLE_COUNT];
    features.decision_time_series.interval_ms = interval_ms;
    features.decision_time_series.d_price = d_price;
    features.decision_time_series.source_counts = DecisionTimeSeriesSourceCounts {
        reserve: SAMPLE_COUNT as u64,
        ..DecisionTimeSeriesSourceCounts::default()
    };
    let mut config = GatekeeperV2Config::default();
    config.mode = GatekeeperMode::Long;
    config.max_wait_time_ms = 10_000;
    config.v25.shadow_enabled = true;
    config.v25.emit_shadow_decisions = true;
    config.dow.enabled = true;
    config.dow.early_entry_enabled = true;
    let mut assessment = build_assessment_from_features(
        features.clone(),
        &config,
        PolicyEvaluationContext::default(),
    );
    // A real terminal Gatekeeper assessment carries the authoritative policy
    // decision before `to_buy_log()`. Keep routing/provenance fields untouched
    // (`None`) so PR2C still proves that DecisionLogger owns their hydration.
    let authoritative_decision = evaluate_policy_from_assessment(&assessment, &config);
    assessment.hard_reject_reason = authoritative_decision.hard_fail_reason.clone();
    assessment.decision = Some(authoritative_decision);
    let mut log = assessment.to_buy_log(&solana_sdk::pubkey::Pubkey::new_unique(), &config);
    let mut temporal_buffer =
        GatekeeperBuffer::new(solana_sdk::pubkey::Pubkey::new_unique(), &config);
    temporal_buffer.set_registered_wall_t0(1_000);
    temporal_buffer.maybe_fire_shadow_checkpoint(4_000);
    temporal_buffer.maybe_fire_shadow_checkpoint(7_001);
    temporal_buffer.maybe_fire_shadow_checkpoint(9_000);
    let temporal_assessment = match temporal_buffer.force_check_deadline(
        1_000_u64
            .saturating_add(config.max_wait_time_ms)
            .saturating_add(1),
    ) {
        GatekeeperVerdict::Reject { assessment, .. }
        | GatekeeperVerdict::Timeout { assessment }
        | GatekeeperVerdict::Buy { assessment, .. } => assessment,
        _ => panic!("terminal current-v33 fixture did not reach a terminal assessment"),
    };
    log.decision_eval_snapshots = temporal_assessment
        .to_buy_log(&solana_sdk::pubkey::Pubkey::new_unique(), &config)
        .decision_eval_snapshots;
    let v3_config = GatekeeperV3Config {
        shadow_emit_enabled: true,
        replay_payload_enabled: true,
        ..GatekeeperV3Config::default()
    };
    let v3_payload = v3_config.v3_policy_config_payload();
    log.gatekeeper_v3_config_payload = Some(v3_payload.clone());
    log.v3_policy_config_payload = Some(v3_payload);
    log.v3_policy_config_hash = Some(v3_config.v3_policy_config_hash());
    log.v3_materialization_version = Some(v3_config.materialization_version);
    log.v3_policy_version = Some(v3_config.policy_version);
    log.v3_stage_thresholds = Some(v3_config.stage_thresholds_payload());
    log.v3_replay_payload_schema_version = Some(1);
    let v3_evidence_status = serde_json::to_value(&features.evidence_status).unwrap();
    let v3_organic_broadening = serde_json::to_value(&features.organic_broadening).unwrap();
    let v3_manipulation = serde_json::to_value(&features.manipulation_contradictions).unwrap();
    log.v3_shadow_evidence_status = Some(v3_evidence_status.clone());
    log.v3_shadow_organic_broadening = Some(v3_organic_broadening.clone());
    log.v3_shadow_manipulation_contradictions = Some(v3_manipulation.clone());
    log.v3_evidence_status = Some(v3_evidence_status);
    log.v3_organic_broadening = Some(v3_organic_broadening);
    log.v3_manipulation_contradictions = Some(v3_manipulation);
    log.v3_shadow_notes = Some(serde_json::json!({
        "p1": "calibrated_shadow_funnel",
        "source": "MaterializedFeatureSet",
        "deadline_elapsed": false,
        "execution": "execution_not_run"
    }));
    let decision = evaluate_v3_from_features(&features, &v3_config, false);
    log.v3_shadow_schema_version = Some(decision.schema_version);
    log.v3_shadow_verdict = Some(decision.verdict.as_log_str().to_string());
    log.v3_shadow_stage = Some(decision.stage.as_log_str().to_string());
    log.v3_shadow_reason_code = Some(decision.reason_code.as_log_str());
    log.v3_shadow_reason_chain = Some(
        decision
            .reason_chain
            .iter()
            .map(|reason| reason.as_log_str())
            .collect(),
    );
    log.v3_shadow_secondary_reason_codes = Some(
        decision
            .reason_chain
            .iter()
            .skip(1)
            .map(|reason| reason.as_log_str())
            .collect(),
    );
    log.v3_shadow_risk_status = Some(decision.risk_status.as_log_str().to_string());
    log.v3_shadow_risk_primary_reason = decision
        .risk_primary_reason
        .map(|reason| reason.as_log_str());
    log.v3_shadow_risk_penalty = Some(decision.risk_penalty);
    log.v3_shadow_opportunity_status = Some(decision.opportunity_status.as_log_str().to_string());
    log.v3_shadow_opportunity_score = Some(decision.opportunity_score);
    log.v3_shadow_confidence_raw = Some(decision.confidence_breakdown.raw);
    log.v3_shadow_confidence_after_risk = Some(decision.confidence_breakdown.after_risk);
    log.v3_shadow_confidence_after_stage = Some(decision.confidence_breakdown.after_stage);
    log.v3_shadow_confidence_cap = Some(decision.confidence_breakdown.cap);
    log.v3_shadow_confidence_cap_reasons = Some(decision.confidence_breakdown.cap_reasons.clone());
    log.v3_shadow_confidence_final = Some(decision.confidence_breakdown.final_confidence);
    log.v3_shadow_confidence = Some(decision.confidence);
    log.v3_component_scores = Some(v3_component_scores_payload(&decision));
    log.v3_actionability = Some(v3_actionability_payload(&features, &v3_config, false));
    let snapshot = serde_json::to_value(features).unwrap();
    log.v3_feature_snapshot_hash = Some(v3_feature_snapshot_hash_from_payload(
        &snapshot,
        v3_config.materialization_version,
    ));
    log.materialized_feature_snapshot = Some(snapshot.clone());
    log.v3_materialized_feature_snapshot = Some(snapshot);
    CurrentV33UnroutedFixture {
        log,
        assessment,
        config,
    }
}

pub fn current_v33_unrouted_log(
    pair: &MetricContractPairedRecordV1,
) -> ghost_brain::oracle::GatekeeperBuyLog {
    current_v33_unrouted_fixture(&pair.decision_time_projection).log
}

pub fn current_v33_log(
    pair: &MetricContractPairedRecordV1,
) -> ghost_brain::oracle::GatekeeperBuyLog {
    let mut log = current_v33_unrouted_log(pair);
    let identity = pair.record_identity();
    log.run_id = Some(identity.run_id.clone());
    log.join_key = Some(identity.join_key.clone());
    log.decision_plane = Some(identity.decision_plane.clone());
    log.config_hash = Some(pair.gatekeeper_config_hash.clone());
    log.brain_config_hash = pair.brain_config_hash.clone();
    log
}
