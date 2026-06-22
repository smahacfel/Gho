use ghost_brain::config::GatekeeperV2Config;
use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_core::checkpoint::{CpvMetricSource, EvidenceStatus, MetricEvidenceQuality};
use ghost_core::{CurveFinality, EventSemanticEnvelope};
use ghost_launcher::events::{PoolTransaction, RawBytesMissingReason};
use ghost_launcher::session::{OpenSessionRequest, SessionConfig, SessionManager};
use ghost_launcher::tx_intelligence::FundingSourceConfig;
use seer::early_fingerprint::EarlyFingerprintConfig;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

fn test_candidate(pool_id: Pubkey, base_mint: Pubkey, bonding_curve: Pubkey) -> EnhancedCandidate {
    let mut candidate = EnhancedCandidate::default();
    candidate.pool_amm_id = pool_id;
    candidate.base_mint = base_mint;
    candidate.bonding_curve = bonding_curve;
    candidate.timestamp = 1_000;
    candidate
}

fn test_tx(pool_id: Pubkey, signature: &str, timestamp_ms: u64) -> Arc<PoolTransaction> {
    Arc::new(PoolTransaction {
        semantic: EventSemanticEnvelope::default(),
        pool_amm_id: pool_id.to_string(),
        slot: Some(1),
        event_ordinal: Some(0),
        tx_index: None,
        outer_instruction_index: None,
        inner_group_index: None,
        outer_program_id: None,
        cpi_stack_height: None,
        timestamp_ms,
        event_time: ghost_core::EventTimeMetadata::new(None, Some(timestamp_ms), None),
        arrival_ts_ms: timestamp_ms,
        signer: Pubkey::new_unique().to_string(),
        is_buy: true,
        volume_sol: 0.1,
        sol_amount_lamports: Some(100_000_000),
        token_amount_units: Some(1_000_000),
        reserve_base: None,
        reserve_quote: None,
        price_quote: None,
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: signature.to_string(),
        success: true,
        error_code: None,
        compute_units_consumed: None,
        owner_token_deltas: vec![],
        mpcf_payload: vec![],
        mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
        token_mint: None,
        v_tokens_in_bonding_curve: None,
        v_sol_in_bonding_curve: None,
        market_cap_sol: None,
        global_config: None,
        fee_recipient: None,
        token_program: None,
        buy_variant: None,
        associated_bonding_curve: None,
        creator_vault: None,
        bonding_curve_v2: None,
        bonding_curve_v2_provenance: None,
        buy_remaining_accounts: Vec::new(),
        is_mayhem_mode: None,
        cu_price_micro_lamports: None,
        compute_unit_limit: None,
        inner_ix_count: None,
        cpi_depth: None,
        ata_create_count: None,
        signer_pre_balance_lamports: None,
        signer_post_balance_lamports: None,
        jito_tip_detected: None,
        toolchain_fingerprint: seer::types::ToolchainFingerprintInput::default(),
        curve_data_known: false,
        curve_finality: CurveFinality::Speculative,
    })
}

fn cpv_tx(
    pool_id: Pubkey,
    signer: Pubkey,
    signature: &str,
    timestamp_ms: u64,
    slot: u64,
    is_dev_buy: bool,
    price: f64,
) -> Arc<PoolTransaction> {
    Arc::new(PoolTransaction {
        signer: signer.to_string(),
        signature: signature.to_string(),
        timestamp_ms,
        arrival_ts_ms: timestamp_ms,
        event_time: ghost_core::EventTimeMetadata::new(None, Some(timestamp_ms), None),
        slot: Some(slot),
        event_ordinal: Some(0),
        is_dev_buy,
        signer_pre_balance_lamports: Some(100),
        signer_post_balance_lamports: Some(90),
        toolchain_fingerprint: seer::types::ToolchainFingerprintInput {
            account_keys_len: Some(12),
            outer_instruction_count: Some(3),
            inner_instruction_group_count: Some(2),
            has_set_compute_unit_limit: Some(true),
            has_set_compute_unit_price: Some(true),
            external_fee_transfer_count: Some(0),
            internal_fee_transfer_count: Some(0),
            filtered_wsol_self_transfer_count: Some(0),
        },
        v_sol_in_bonding_curve: Some(price),
        v_tokens_in_bonding_curve: Some(1.0),
        market_cap_sol: Some(price * 1_000_000_000.0),
        curve_data_known: true,
        ..(*test_tx(pool_id, signature, timestamp_ms)).clone()
    })
}

fn open_session_with_gatekeeper_config(
    manager: &SessionManager,
    pool_id: Pubkey,
    base_mint: Pubkey,
    bonding_curve: Pubkey,
    created_at_wall_ms: u64,
    deadline_wall_ms: u64,
    gatekeeper_config: GatekeeperV2Config,
) -> ghost_launcher::session::SharedSession {
    let funding_source_config = FundingSourceConfig::from_gatekeeper_config(&gatekeeper_config);
    manager
        .open_session(OpenSessionRequest {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            dev_wallet: Some(Pubkey::new_unique()),
            candidate_snapshot: test_candidate(pool_id, base_mint, bonding_curve),
            created_at_wall_ms,
            deadline_wall_ms: Some(deadline_wall_ms),
            gatekeeper_config,
            funding_source_config,
            fingerprint_config: EarlyFingerprintConfig::default(),
        })
        .expect("session open should succeed");
    manager
        .get_session(&pool_id)
        .expect("session must be retrievable after open")
}

fn open_default_session(
    manager: &SessionManager,
    pool_id: Pubkey,
    base_mint: Pubkey,
    bonding_curve: Pubkey,
    created_at_wall_ms: u64,
) -> ghost_launcher::session::SharedSession {
    open_session_with_gatekeeper_config(
        manager,
        pool_id,
        base_mint,
        bonding_curve,
        created_at_wall_ms,
        created_at_wall_ms + 100,
        GatekeeperV2Config::default(),
    )
}

#[test]
fn clean_cpv_materializes_to_policy_fields_and_evidence_context() {
    let manager = SessionManager::new(SessionConfig {
        default_observation_duration_ms: 100,
        max_sessions: 8,
        ..SessionConfig::default()
    });
    let pool_a = Pubkey::new_unique();
    let pool_b = Pubkey::new_unique();
    let session_a = open_default_session(
        &manager,
        pool_a,
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        49_000,
    );
    let mut gatekeeper_config = GatekeeperV2Config::default();
    gatekeeper_config.max_wait_time_ms = 5_000;
    let session_b = open_session_with_gatekeeper_config(
        &manager,
        pool_b,
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        50_000,
        55_000,
        gatekeeper_config,
    );
    let shared_signer = Pubkey::new_unique();
    let session_b_dev_wallet = session_b
        .read()
        .dev_wallet
        .expect("session should know dev wallet");

    {
        let mut guard = session_a.write();
        let _ = guard.ingest_transaction(cpv_tx(
            pool_a,
            shared_signer,
            "sig-pr2-cpv-seed-clean",
            49_010,
            1,
            false,
            9.0,
        ));
    }

    let features = {
        let mut guard = session_b.write();
        let _ = guard.ingest_transaction(cpv_tx(
            pool_b,
            session_b_dev_wallet,
            "sig-pr2-cpv-dev-clean",
            50_010,
            2,
            true,
            10.0,
        ));
        let _ = guard.ingest_transaction(cpv_tx(
            pool_b,
            shared_signer,
            "sig-pr2-cpv-shared-clean",
            50_020,
            3,
            false,
            11.0,
        ));
        let _ = guard.ingest_transaction(cpv_tx(
            pool_b,
            Pubkey::new_unique(),
            "sig-pr2-cpv-local-clean",
            50_030,
            4,
            false,
            12.0,
        ));
        guard.materialize_features()
    };

    assert_eq!(
        features.sybil_resistance.signer_cross_pool_velocity,
        Some(1.0 / 3.0)
    );
    assert_eq!(
        features.sybil_resistance.cpv_other_pool_activity,
        Some(1.0 / 3.0)
    );
    assert_eq!(
        features.sybil_resistance.cpv_evidence.quality,
        MetricEvidenceQuality::Clean
    );
    assert_eq!(
        features.sybil_resistance.cpv_evidence.source,
        CpvMetricSource::SuccessfulBuyRollingIndex
    );
    assert_eq!(
        features
            .sybil_resistance
            .cpv_evidence
            .signer_cross_pool_velocity,
        Some(1.0 / 3.0)
    );
    assert_eq!(features.sybil_resistance.cpv_evidence.sample_count, Some(3));
    assert_eq!(features.evidence_status.cpv.status, EvidenceStatus::Clean);

    let snapshot_json = serde_json::to_value(&features).expect("serialize materialized features");
    let cpv_evidence_json = &snapshot_json["sybil_resistance"]["cpv_evidence"];
    assert_eq!(cpv_evidence_json["quality"], "clean");
    assert_eq!(cpv_evidence_json["source"], "successful_buy_rolling_index");
    assert_eq!(cpv_evidence_json["sample_count"], 3);
    assert_eq!(cpv_evidence_json["signer_cross_pool_velocity"], 1.0 / 3.0);
}

#[test]
fn degraded_low_sample_cpv_emits_value_with_degraded_evidence() {
    let manager = SessionManager::new(SessionConfig {
        default_observation_duration_ms: 100,
        max_sessions: 8,
        ..SessionConfig::default()
    });
    let pool_a = Pubkey::new_unique();
    let pool_b = Pubkey::new_unique();
    let session_a = open_default_session(
        &manager,
        pool_a,
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        59_000,
    );
    let mut gatekeeper_config = GatekeeperV2Config::default();
    gatekeeper_config.max_wait_time_ms = 5_000;
    gatekeeper_config.cpv_emit_degraded_low_sample = true;
    gatekeeper_config.cpv_min_successful_buy_signers_clean = 3;
    gatekeeper_config.cpv_min_successful_buy_signers_degraded = 2;
    let session_b = open_session_with_gatekeeper_config(
        &manager,
        pool_b,
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        60_000,
        65_000,
        gatekeeper_config,
    );
    let shared_signer = Pubkey::new_unique();
    let local_signer = Pubkey::new_unique();

    {
        let mut guard = session_a.write();
        let _ = guard.ingest_transaction(cpv_tx(
            pool_a,
            shared_signer,
            "sig-pr2-cpv-low-seed",
            59_010,
            1,
            false,
            9.0,
        ));
    }

    let features = {
        let mut guard = session_b.write();
        let _ = guard.ingest_transaction(cpv_tx(
            pool_b,
            shared_signer,
            "sig-pr2-cpv-low-shared",
            60_010,
            2,
            false,
            10.0,
        ));
        let _ = guard.ingest_transaction(cpv_tx(
            pool_b,
            local_signer,
            "sig-pr2-cpv-low-local",
            60_020,
            3,
            false,
            11.0,
        ));
        guard.materialize_features()
    };

    assert_eq!(
        features.sybil_resistance.signer_cross_pool_velocity,
        Some(0.5)
    );
    assert_eq!(features.sybil_resistance.cpv_other_pool_activity, Some(0.5));
    assert_eq!(
        features.sybil_resistance.cpv_evidence.quality,
        MetricEvidenceQuality::DegradedLowSample
    );
    assert_eq!(
        features.sybil_resistance.cpv_evidence.source,
        CpvMetricSource::SuccessfulBuyRollingIndex
    );
    assert_eq!(
        features
            .sybil_resistance
            .cpv_evidence
            .signer_cross_pool_velocity,
        Some(0.5)
    );
    assert_eq!(
        features
            .sybil_resistance
            .cpv_evidence
            .cpv_other_pool_activity,
        Some(0.5)
    );
    assert_eq!(features.sybil_resistance.cpv_evidence.sample_count, Some(2));
    assert_eq!(
        features
            .sybil_resistance
            .cpv_evidence
            .required_clean_sample_count,
        Some(3)
    );
    assert_eq!(
        features
            .sybil_resistance
            .cpv_evidence
            .required_degraded_sample_count,
        Some(2)
    );
    assert_eq!(
        features.evidence_status.cpv.status,
        EvidenceStatus::Degraded
    );
    assert!(features
        .sybil_resistance
        .degraded_reasons
        .contains(&ghost_core::tx_intelligence::types::CPV_LOW_SAMPLE_DEGRADED_REASON.to_string()));
    assert!(!features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::CPV_INSUFFICIENT_SIGNERS_REASON.to_string()
    ));

    let snapshot_json = serde_json::to_value(&features).expect("serialize materialized features");
    let sybil_json = &snapshot_json["sybil_resistance"];
    assert_eq!(sybil_json["signer_cross_pool_velocity"], 0.5);
    assert_eq!(sybil_json["cpv_other_pool_activity"], 0.5);
    let cpv_evidence_json = &sybil_json["cpv_evidence"];
    assert_eq!(cpv_evidence_json["quality"], "degraded_low_sample");
    assert_eq!(cpv_evidence_json["sample_count"], 2);
    assert_eq!(cpv_evidence_json["required_clean_sample_count"], 3);
    assert_eq!(cpv_evidence_json["signer_cross_pool_velocity"], 0.5);
    assert_eq!(
        cpv_evidence_json["degraded_reasons"][0],
        "CPV_LOW_SAMPLE_DEGRADED"
    );
}
