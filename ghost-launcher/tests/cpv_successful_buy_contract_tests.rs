use ghost_brain::config::GatekeeperV2Config;
use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_core::checkpoint::{CpvMetricSource, EvidenceStatus, MetricEvidenceQuality};
use ghost_core::{CurveFinality, EventSemanticEnvelope};
use ghost_launcher::events::{PoolTransaction, RawBytesMissingReason};
use ghost_launcher::session::{OpenSessionRequest, SessionConfig, SessionManager};
use ghost_launcher::tx_intelligence::{CpvQueryWindow, FundingSourceConfig};
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
        virtual_sol_reserves: None,
        virtual_token_reserves: None,
        real_sol_reserves: None,
        real_token_reserves: None,
        complete: None,
        metadata_availability: seer::types::TransactionMetadataAvailability {
            status_known: true,
            inner_instructions_known: true,
        },
        semantic: EventSemanticEnvelope::default(),
        pool_amm_id: pool_id.to_string(),
        slot: Some(1),
        event_ordinal: Some(0),
        tx_index: None,
        outer_instruction_index: None,
        inner_instruction_path: None,
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

fn m4_fixture_feed_then_session(
    session: &mut ghost_launcher::session::observation::PoolObservationSession,
    tx: Arc<PoolTransaction>,
) {
    use solana_sdk::signature::Signature;
    let mut tx = (*tx).clone();
    if tx.signature.parse::<Signature>().is_err() {
        let digest = solana_sdk::hash::hash(tx.signature.as_bytes()).to_bytes();
        let mut bytes = [0u8; 64];
        bytes[..32].copy_from_slice(&digest);
        bytes[32..].copy_from_slice(&digest);
        tx.signature = Signature::from(bytes).to_string();
    }
    session.cross_pool_velocity_index.observe_transaction(
        &tx.pool_amm_id,
        &tx,
        &session.cross_pool_velocity_config,
    );
    session.ingest_transaction(Arc::new(tx));
}

fn m4_fixture_source_complete(
    session: &ghost_launcher::session::observation::PoolObservationSession,
    end: u64,
) {
    let config = &session.cross_pool_velocity_config;
    session
        .cross_pool_velocity_index
        .observe_source_progress(1, 0, 1, 1, config);
    session
        .cross_pool_velocity_index
        .observe_source_progress(1, end, end, end, config);
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
        m4_fixture_feed_then_session(
            &mut guard,
            cpv_tx(
                pool_a,
                shared_signer,
                "sig-pr2-cpv-seed-clean",
                49_010,
                1,
                false,
                9.0,
            ),
        );
    }

    let features = {
        let mut guard = session_b.write();
        m4_fixture_feed_then_session(
            &mut guard,
            cpv_tx(
                pool_b,
                session_b_dev_wallet,
                "sig-pr2-cpv-dev-clean",
                50_010,
                2,
                true,
                10.0,
            ),
        );
        m4_fixture_feed_then_session(
            &mut guard,
            cpv_tx(
                pool_b,
                shared_signer,
                "sig-pr2-cpv-shared-clean",
                50_020,
                3,
                false,
                11.0,
            ),
        );
        m4_fixture_feed_then_session(
            &mut guard,
            cpv_tx(
                pool_b,
                Pubkey::new_unique(),
                "sig-pr2-cpv-local-clean",
                50_030,
                4,
                false,
                12.0,
            ),
        );
        m4_fixture_source_complete(&guard, guard.highest_seen_ts_ms);
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
        m4_fixture_feed_then_session(
            &mut guard,
            cpv_tx(
                pool_a,
                shared_signer,
                "sig-pr2-cpv-low-seed",
                59_010,
                1,
                false,
                9.0,
            ),
        );
    }

    let features = {
        let mut guard = session_b.write();
        m4_fixture_feed_then_session(
            &mut guard,
            cpv_tx(
                pool_b,
                shared_signer,
                "sig-pr2-cpv-low-shared",
                60_010,
                2,
                false,
                10.0,
            ),
        );
        m4_fixture_feed_then_session(
            &mut guard,
            cpv_tx(
                pool_b,
                local_signer,
                "sig-pr2-cpv-low-local",
                60_020,
                3,
                false,
                11.0,
            ),
        );
        m4_fixture_source_complete(&guard, guard.highest_seen_ts_ms);
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

fn m4_raw_primary_buy(
    pool: Pubkey,
    mint: Pubkey,
    user: Pubkey,
    time: u64,
    index: u64,
) -> (seer::types::TradeEvent, ghost_core::ObservedPumpMutationV1) {
    let source = m6_raw_primary_source(pool, mint, user, time, index);
    let mut bundle = seer::binary_parser::BinaryParser::new(false)
        .parse_transaction_bundle(&source)
        .unwrap();
    assert_eq!(bundle.trades.len(), 1);
    (
        bundle.trades.remove(0),
        bundle.trade_observations.remove(0).unwrap(),
    )
}

fn m6_raw_primary_source(
    pool: Pubkey,
    mint: Pubkey,
    user: Pubkey,
    time: u64,
    index: u64,
) -> seer::types::GeyserEvent {
    use ghost_core::{ObservationProvenanceV1, ObservationSourceFamilyV1, RawProviderRoleV1};
    use solana_sdk::signature::Signature;
    use std::str::FromStr;

    use seer::{
        binary_parser::DISC_BUY,
        types::{GeyserEvent, RawInstruction, TransactionMetadataAvailability},
    };
    let signature = Signature::new_unique();
    let mut accounts = vec![Pubkey::new_unique(); 12];
    accounts[2] = mint;
    accounts[3] = pool;
    accounts[6] = user;
    accounts[8] = Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap();
    let mut data = DISC_BUY.to_vec();
    data.extend_from_slice(&1_000_000u64.to_le_bytes());
    data.extend_from_slice(&200_000_000u64.to_le_bytes());
    let source = GeyserEvent::Transaction {
        metadata_availability: TransactionMetadataAvailability {
            status_known: true,
            inner_instructions_known: true,
        },
        provider_id: Some("primary".into()),
        provider_role: Some(RawProviderRoleV1::PrimaryAuthority),
        observation_provenance: Some(ObservationProvenanceV1 {
            source_family: ObservationSourceFamilyV1::RawYellowstone,
            source_id: "grpc_global_stream".into(),
            provider_id: "primary".into(),
            schema_id: "m4_raw_fixture".into(),
            payload_hash_blake3:
                ObservationProvenanceV1::payload_hash_for_captured_provider_payload(
                    signature.as_ref(),
                ),
            received_at_monotonic_ns: index + 1,
        }),
        slot: Some(42 + index),
        tx_index: Some(index as u32),
        event_ts_ms: Some(time),
        arrival_ts_ms: Some(time),
        event_time: ghost_core::EventTimeMetadata::new(None, Some(time), Some(time)),
        signature,
        accounts,
        instructions: vec![RawInstruction {
            program_id: Pubkey::from_str(seer::grpc_connection::PUMP_FUN_PROGRAM_ID).unwrap(),
            account_indices: (0..12).collect(),
            data,
        }],
        logs: vec![],
        block_time: None,
        account_data: std::collections::HashMap::new(),
        pre_balances: vec![1_500_000_000; 12],
        post_balances: {
            let mut balances = vec![1_500_000_000; 12];
            // Raw execution truth for a 0.2 SOL Pump.fun BUY:
            // bonding curve receives 0.2 SOL; the recognized user pays 0.2 SOL.
            // The instruction's sol_bound is only a limit and must not stand in for fill.
            balances[3] = 1_700_000_000;
            balances[6] = 1_300_000_000;
            balances
        },
        success: true,
        error_code: None,
        compute_units_consumed: None,
        synthetic: false,
        source: "grpc_global_stream".into(),
        mpcf_payload_bytes: None,
        mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
        inner_instructions: vec![],
        pre_token_balances: vec![],
        post_token_balances: vec![],
    };
    source
}

#[test]
fn m4_raw_ingest_mfs_evidence_policy_and_recovery_preserve_saved_snapshots() {
    use ghost_core::PumpObservationLedgerV1;
    use ghost_launcher::components::{
        gatekeeper_policy::evaluate_policy, seer::trade_event_to_pool_transaction,
    };
    use ghost_launcher::tx_intelligence::CrossPoolVelocityConfig;
    let manager = SessionManager::default();
    let mut config = GatekeeperV2Config::default();
    config.cpv_lookback_window_s = 1;
    config.cpv_per_signer_cap = 8;
    config.cpv_global_signer_cap = 32;
    config.max_signer_cross_pool_velocity = 0.2;
    config.soft_penalty_high_cpv = 2;
    let cpv = CrossPoolVelocityConfig::from_gatekeeper_config(&config);
    let index = manager.cross_pool_velocity_index();
    let mut ledger = PumpObservationLedgerV1::default();
    let pool = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let users = [
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    ];
    let session = open_session_with_gatekeeper_config(
        &manager,
        pool,
        mint,
        pool,
        1500,
        10000,
        config.clone(),
    );
    let feed = |ledger: &mut PumpObservationLedgerV1,
                pool: Pubkey,
                mint: Pubkey,
                user: Pubkey,
                time: u64,
                ordinal: u64| {
        let (trade, observation) = m4_raw_primary_buy(pool, mint, user, time, ordinal);
        let result = ledger.observe(observation, ordinal + 1);
        assert!(result.observation_decision.canonical_mutation.is_some());
        let tx = trade_event_to_pool_transaction(&trade);
        assert_eq!(tx.sol_amount_lamports, Some(200_000_000));
        assert_eq!(tx.signer_pre_balance_lamports, Some(1_500_000_000));
        assert_eq!(tx.signer_post_balance_lamports, Some(1_300_000_000));
        assert!(
            tx.volume_sol >= config.min_sol_threshold,
            "fixture musi kwalifikować się do okna sesji"
        );
        index.observe_transaction_at(&tx.pool_amm_id, &tx, time, &cpv);
        Arc::new(tx)
    };
    // Inny pool nigdy nie miał sesji; jego zwalidowany BUY jest w historii.
    let ignored_pool = Pubkey::new_unique();
    let ignored_mint = Pubkey::new_unique();
    let _ = feed(&mut ledger, ignored_pool, ignored_mint, users[0], 1500, 0);
    assert!(manager.get_session(&ignored_pool).is_none());
    for (i, user) in users.into_iter().enumerate() {
        let tx = feed(&mut ledger, pool, mint, user, 1900 + i as u64, 1 + i as u64);
        session.write().ingest_transaction(tx);
    }
    index.observe_source_progress(1, 0, 1, 1, &cpv);
    index.observe_source_progress(1, 2000, 2000, 2000, &cpv);
    let full = session.read().try_materialize_features().unwrap();
    assert_eq!(
        full.sybil_resistance.signer_cross_pool_velocity,
        Some(1.0 / 3.0)
    );
    assert_eq!(
        full.sybil_resistance.cpv_evidence.quality,
        MetricEvidenceQuality::Clean
    );
    assert_eq!(full.sybil_resistance.cpv_evidence.sample_count, Some(3));
    assert!(
        evaluate_policy(&full, &config)
            .sybil_policy
            .soft_signals
            .high_cpv
    );
    let saved = serde_json::to_value(&full.sybil_resistance.cpv_evidence).unwrap();
    // Spóźniony rekord zmieniłby niezapisane obliczenie, ale nie zapisany anchor.
    let (trade, observation) = m4_raw_primary_buy(ignored_pool, ignored_mint, users[1], 1600, 4);
    assert!(ledger
        .observe(observation, 5)
        .observation_decision
        .canonical_mutation
        .is_some());
    let late = trade_event_to_pool_transaction(&trade);
    index.observe_transaction_at(&late.pool_amm_id, &late, 2100, &cpv);
    let reread = session.read().try_materialize_features().unwrap();
    assert_eq!(
        reread.sybil_resistance.signer_cross_pool_velocity,
        Some(2.0 / 3.0)
    );
    assert_eq!(
        serde_json::to_value(&full.sybil_resistance.cpv_evidence).unwrap(),
        saved
    );
    index.mark_stream_gap(2101);
    let tx = feed(&mut ledger, pool, mint, users[0], 2200, 5);
    session.write().ingest_transaction(tx);
    let missing = session.read().try_materialize_features().unwrap();
    assert_eq!(missing.sybil_resistance.signer_cross_pool_velocity, None);
    assert_eq!(
        missing.sybil_resistance.cpv_evidence.quality,
        MetricEvidenceQuality::UnavailableSource
    );
    assert!(
        !evaluate_policy(&missing, &config)
            .sybil_policy
            .soft_signals
            .high_cpv
    );
    assert!(missing.sybil_resistance.spend_fraction_divergence.is_some());
    let missing_saved = serde_json::to_vec(&missing.sybil_resistance.cpv_evidence).unwrap();
    // Odbudowany cały lookback i nowy obserwowany pool odzyskują pełną jakość.
    let recovery_pool = Pubkey::new_unique();
    let recovery_mint = Pubkey::new_unique();
    let recovered_session = open_session_with_gatekeeper_config(
        &manager,
        recovery_pool,
        recovery_mint,
        recovery_pool,
        4000,
        12000,
        config.clone(),
    );
    index.observe_source_progress(1, 3000, 3000, 3000, &cpv);
    let _ = feed(&mut ledger, ignored_pool, ignored_mint, users[0], 4200, 6);
    for (i, user) in users.into_iter().enumerate() {
        let tx = feed(
            &mut ledger,
            recovery_pool,
            recovery_mint,
            user,
            4800 + i as u64,
            7 + i as u64,
        );
        recovered_session.write().ingest_transaction(tx);
    }
    index.observe_source_progress(1, 5000, 5000, 5000, &cpv);
    let recovered = recovered_session.read().try_materialize_features().unwrap();
    assert_eq!(
        recovered.sybil_resistance.signer_cross_pool_velocity,
        Some(1.0 / 3.0)
    );
    assert_eq!(
        recovered.sybil_resistance.cpv_evidence.quality,
        MetricEvidenceQuality::Clean
    );
    assert!(
        evaluate_policy(&recovered, &config)
            .sybil_policy
            .soft_signals
            .high_cpv
    );
    assert_eq!(
        serde_json::to_vec(&missing.sybil_resistance.cpv_evidence).unwrap(),
        missing_saved
    );
    let refreshed = session.read().try_materialize_features().unwrap();
    assert_eq!(refreshed.sybil_resistance.signer_cross_pool_velocity, None);
    assert_eq!(
        refreshed.sybil_resistance.cpv_evidence.quality,
        MetricEvidenceQuality::UnavailableSource
    );
    assert!(refreshed
        .sybil_resistance
        .cpv_evidence
        .degraded_reasons
        .iter()
        .any(|reason| reason == "CPV_HISTORY_NOT_RETAINED"));
    assert_eq!(
        serde_json::to_value(&full.sybil_resistance.cpv_evidence).unwrap(),
        saved
    );
}

fn m4_cache_regression_session() -> (
    SessionManager,
    ghost_launcher::session::SharedSession,
    Pubkey,
    Vec<Arc<PoolTransaction>>,
) {
    let manager = SessionManager::default();
    let pool = Pubkey::new_unique();
    let mut config = GatekeeperV2Config::default();
    config.cpv_lookback_window_s = 1;
    let session = open_session_with_gatekeeper_config(
        &manager,
        pool,
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        1_000,
        60_000,
        config,
    );
    let txs: Vec<_> = [1_800, 1_900, 2_000]
        .into_iter()
        .enumerate()
        .map(|(index, timestamp_ms)| {
            cpv_tx(
                pool,
                Pubkey::new_unique(),
                &solana_sdk::signature::Signature::new_unique().to_string(),
                timestamp_ms,
                100 + index as u64,
                false,
                1.0,
            )
        })
        .collect();
    {
        let mut guard = session.write();
        for tx in &txs {
            m4_fixture_feed_then_session(&mut guard, tx.clone());
        }
        let other_pool = cpv_tx(
            Pubkey::new_unique(),
            txs[0].signer.parse().unwrap(),
            &solana_sdk::signature::Signature::new_unique().to_string(),
            1_700,
            99,
            false,
            1.0,
        );
        guard.cross_pool_velocity_index.observe_transaction(
            &other_pool.pool_amm_id,
            &other_pool,
            &guard.cross_pool_velocity_config,
        );
        guard.cross_pool_velocity_index.observe_source_progress(
            1,
            0,
            1,
            1,
            &guard.cross_pool_velocity_config,
        );
    }
    (manager, session, pool, txs)
}

fn m4_cache_regression_source_progress(session: &ghost_launcher::session::SharedSession, end: u64) {
    let guard = session.read();
    guard.cross_pool_velocity_index.observe_source_progress(
        1,
        end,
        end,
        end,
        &guard.cross_pool_velocity_config,
    );
}

#[test]
fn m4_current_materialization_complete_snapshot_is_one_third() {
    let (_manager, session, _pool, _txs) = m4_cache_regression_session();
    m4_cache_regression_source_progress(&session, 2_000);

    let features = session.read().try_materialize_features().unwrap();
    assert_eq!(
        features.sybil_resistance.signer_cross_pool_velocity,
        Some(1.0 / 3.0)
    );
}

#[test]
fn m4_current_materialization_recovers_when_source_completeness_changes() {
    let (_manager, session, _pool, _txs) = m4_cache_regression_session();

    let before = session.read().try_materialize_features().unwrap();
    assert_eq!(before.sybil_resistance.signer_cross_pool_velocity, None);

    m4_cache_regression_source_progress(&session, 2_000);
    let after = session.read().try_materialize_features().unwrap();

    assert_eq!(before.sybil_resistance.signer_cross_pool_velocity, None);
    assert_eq!(
        after.sybil_resistance.signer_cross_pool_velocity,
        Some(1.0 / 3.0)
    );
}

#[test]
fn m4_current_materialization_recomputes_population_inside_unchanged_event_bounds() {
    let (_manager, session, pool, mut txs) = m4_cache_regression_session();
    m4_cache_regression_source_progress(&session, 2_000);

    let before = session.read().try_materialize_features().unwrap();
    assert_eq!(
        before.sybil_resistance.signer_cross_pool_velocity,
        Some(1.0 / 3.0)
    );

    let added = cpv_tx(
        pool,
        Pubkey::new_unique(),
        &solana_sdk::signature::Signature::new_unique().to_string(),
        1_850,
        103,
        false,
        1.0,
    );
    {
        let mut guard = session.write();
        m4_fixture_feed_then_session(&mut guard, added.clone());
    }
    txs.push(added);

    let guard = session.read();
    assert_eq!(guard.tx_buffer.len(), 4);
    assert_eq!(guard.diagnostics.total_tx_seen, 4);
    let direct = guard.cross_pool_velocity_index.compute_for_transactions_at(
        &pool.to_string(),
        txs.iter().map(AsRef::as_ref),
        CpvQueryWindow {
            signer_window_start_ms: 1_800,
            anchor_ms: 2_000,
            cutoff_received_ms: seer::types::ingress_epoch_ms(),
        },
        &guard.cross_pool_velocity_config,
    );
    let after = guard.try_materialize_features().unwrap();

    assert_eq!(direct.signer_cross_pool_velocity, Some(0.25));
    assert_eq!(
        after.sybil_resistance.signer_cross_pool_velocity,
        direct.signer_cross_pool_velocity
    );
}

#[test]
fn m4_current_materialization_recomputes_population_at_same_anchor_timestamp() {
    let (_manager, session, pool, mut txs) = m4_cache_regression_session();
    m4_cache_regression_source_progress(&session, 2_000);

    let before = session.read().try_materialize_features().unwrap();
    assert_eq!(
        before.sybil_resistance.signer_cross_pool_velocity,
        Some(1.0 / 3.0)
    );

    let added = cpv_tx(
        pool,
        Pubkey::new_unique(),
        &solana_sdk::signature::Signature::new_unique().to_string(),
        2_000,
        103,
        false,
        1.0,
    );
    {
        let mut guard = session.write();
        m4_fixture_feed_then_session(&mut guard, added.clone());
    }
    txs.push(added);

    let guard = session.read();
    assert_eq!(guard.tx_buffer.len(), 4);
    assert_eq!(guard.diagnostics.total_tx_seen, 4);
    let direct = guard.cross_pool_velocity_index.compute_for_transactions_at(
        &pool.to_string(),
        txs.iter().map(AsRef::as_ref),
        CpvQueryWindow {
            signer_window_start_ms: 1_800,
            anchor_ms: 2_000,
            cutoff_received_ms: seer::types::ingress_epoch_ms(),
        },
        &guard.cross_pool_velocity_config,
    );
    let after = guard.try_materialize_features().unwrap();

    assert_eq!(direct.signer_sample_count, 4);
    assert_eq!(direct.signer_cross_pool_velocity, Some(0.25));
    assert_eq!(
        after.sybil_resistance.signer_cross_pool_velocity,
        Some(0.25)
    );
}

#[test]
fn m4_session_ingest_alone_never_warms_or_populates_the_shared_feed_index() {
    let manager = SessionManager::default();
    let pool = Pubkey::new_unique();
    let session = open_default_session(&manager, pool, Pubkey::new_unique(), pool, 1000);
    for i in 0..3 {
        let tx = cpv_tx(
            pool,
            Pubkey::new_unique(),
            &format!("session-only-{i}"),
            1010 + i,
            10 + i,
            false,
            10.0,
        );
        session.write().ingest_transaction(tx);
    }
    assert_eq!(manager.cross_pool_velocity_index().entry_count(), 0);
    assert!(!manager.cross_pool_velocity_index().is_ready());
    let features = session.read().try_materialize_features().unwrap();
    assert_eq!(features.sybil_resistance.signer_cross_pool_velocity, None);
}

mod m6_integration {
    use super::*;
    use ghost_core::checkpoint::MaterializedFeatureSet;
    use ghost_core::metric_contracts::MetricContractDecisionProjectionWireV1;
    use ghost_core::tx_intelligence::types::{FtdiDefinitionV2, SybilResistanceFeatures};
    use ghost_launcher::components::{
        gatekeeper_policy::{
            build_assessment_from_features, evaluate_policy, PolicyEvaluationContext,
        },
        gatekeeper_v3::v3_feature_snapshot_hash,
        seer::trade_event_to_pool_transaction,
    };
    use seer::types::{GeyserEvent, InnerInstructionGroup, InnerIx};

    fn raw_sources(pool: Pubkey, mint: Pubkey, users: &[Pubkey; 5]) -> Vec<GeyserEvent> {
        let slots = [43, 44, 48, 51, 53];
        let reserves = [
            10_000_000_000u64,
            11_000_000_000,
            13_200_000_000,
            17_160_000_000,
            24_024_000_000,
        ];
        users
            .iter()
            .enumerate()
            .map(|(i, user)| {
                let mut source =
                    m6_raw_primary_source(pool, mint, *user, 1_900 + i as u64 * 10, i as u64 + 1);
                if let GeyserEvent::Transaction {
                    accounts,
                    slot,
                    inner_instructions,
                    ..
                } = &mut source
                {
                    let program_index = accounts.len() as u8;
                    accounts.push(seer::grpc_connection::PUMP_FUN_PROGRAM_ID.parse().unwrap());
                    *slot = Some(slots[i]);
                    let mut data = seer::binary_parser::DISC_EVENT_TRADE.to_vec();
                    data.extend_from_slice(mint.as_ref());
                    data.extend_from_slice(&200_000_000u64.to_le_bytes());
                    data.extend_from_slice(&1_000_000u64.to_le_bytes());
                    data.push(1);
                    data.extend_from_slice(user.as_ref());
                    data.extend_from_slice(&1i64.to_le_bytes());
                    data.extend_from_slice(&reserves[i].to_le_bytes());
                    data.extend_from_slice(&1_000_000_000_000_000u64.to_le_bytes());
                    *inner_instructions = vec![InnerInstructionGroup {
                        index: 0,
                        instructions: vec![InnerIx {
                            program_id_index: program_index,
                            accounts: vec![],
                            data,
                            stack_height: Some(2),
                        }],
                    }];
                }
                source
            })
            .collect()
    }

    fn materialize_raw(missing_balance: bool) -> (MaterializedFeatureSet, GatekeeperV2Config) {
        let manager = SessionManager::default();
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let users = std::array::from_fn(|_| Pubkey::new_unique());
        let mut config = GatekeeperV2Config::default();
        config.cpv_lookback_window_s = 1;
        config.enable_prosperity_filter = false;
        config.enable_prosperity_overlay = false;
        // Kontrolowane progi testowe, nie kalibracja ani konfiguracja produkcyjna.
        config.sybil_thresholds_v2.ftdi_gini_simpson_min = Some(0.1);
        config.sybil_thresholds_v2.des_next_buy_slot_tau_b_min = Some(0.0);
        config.max_dev_buyer_infrastructure_affinity = 0.5;
        config.min_spend_fraction_divergence = 0.1;
        config.max_signer_cross_pool_velocity = 0.1;
        config.soft_penalty_low_ftdi = 1;
        config.soft_penalty_high_dbia = 1;
        config.soft_penalty_low_sfd = 1;
        config.soft_penalty_inelastic_demand = 1;
        config.soft_penalty_high_cpv = 1;
        let session = open_session_with_gatekeeper_config(
            &manager,
            pool,
            mint,
            pool,
            1_000,
            60_000,
            config.clone(),
        );
        let index = manager.cross_pool_velocity_index();
        let cpv_config = session.read().cross_pool_velocity_config;
        let mut ledger = ghost_core::PumpObservationLedgerV1::default();
        let (other, observation) = m4_raw_primary_buy(
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            users[0],
            1_500,
            0,
        );
        assert!(ledger
            .observe(observation, 1)
            .observation_decision
            .canonical_mutation
            .is_some());
        let other = trade_event_to_pool_transaction(&other);
        index.observe_transaction_at(&other.pool_amm_id, &other, 1_500, &cpv_config);
        let mut guard = session.write();
        guard.update_tx_intelligence_dev_wallet(Some(users[0]));
        guard.set_pr2c_snapshot_capture_enabled(true);
        for (i, mut source) in raw_sources(pool, mint, &users).into_iter().enumerate() {
            if missing_balance && i == 3 {
                if let GeyserEvent::Transaction { post_balances, .. } = &mut source {
                    // User ma indeks 6: zachowujemy raw fill poola, usuwamy tylko saldo użytkownika.
                    post_balances.truncate(6);
                }
            }
            let mut bundle = seer::binary_parser::BinaryParser::new(false)
                .parse_transaction_bundle(&source)
                .unwrap();
            assert_eq!(bundle.trades.len(), 1);
            let observation = bundle.trade_observations.remove(0).unwrap();
            assert!(ledger
                .observe(observation, i as u64 + 2)
                .observation_decision
                .canonical_mutation
                .is_some());
            let tx = trade_event_to_pool_transaction(&bundle.trades.remove(0));
            assert!(tx.virtual_sol_reserves.is_some());
            assert!(tx.metadata_availability.status_known);
            if missing_balance && i == 3 {
                assert_eq!(tx.signer_post_balance_lamports, None);
            }
            index.observe_transaction_at(&tx.pool_amm_id, &tx, 1_900 + i as u64 * 10, &cpv_config);
            guard.ingest_transaction(Arc::new(tx));
        }
        index.observe_source_progress(1, 0, 1, 1, &cpv_config);
        index.observe_source_progress(1, 2_000, 2_000, 2_000, &cpv_config);
        let features = guard.try_materialize_features().unwrap();
        assert_eq!(guard.diagnostics.total_tx_seen, 5);
        let frozen = guard.take_pr2c_complete_metric_contract_snapshot().unwrap();
        let full_evidence = &frozen.snapshot().full_evidence;
        full_evidence.validate_semantics().unwrap();
        assert_eq!(
            full_evidence.fee_topology_diversity_index.gini_simpson_v2,
            features.sybil_resistance.fee_topology_diversity_v2
        );
        let projection = features
            .metric_contract_decision_projection_v1
            .as_ref()
            .unwrap();
        assert_eq!(
            projection.fee_topology_diversity_index.gini_simpson_v2,
            features.sybil_resistance.fee_topology_diversity_v2
        );
        let wire = MetricContractDecisionProjectionWireV1::try_from_domain(projection).unwrap();
        assert_eq!(wire.w, 2);
        assert_eq!(&wire.try_into_domain().unwrap(), projection);
        let stored = serde_json::to_vec(&features).unwrap();
        let restored: MaterializedFeatureSet = serde_json::from_slice(&stored).unwrap();
        assert_eq!(restored.sybil_resistance, features.sybil_resistance);
        assert_eq!(
            restored.metric_contract_decision_projection_v1,
            features.metric_contract_decision_projection_v1
        );
        (restored, config)
    }

    #[test]
    fn m6_raw_ingest_mfs_evidence_policy_full_and_missing_balance() {
        for missing in [false, true] {
            let (features, config) = materialize_raw(missing);
            let sybil = &features.sybil_resistance;
            let ftdi = sybil.fee_topology_diversity_v2.as_ref().unwrap();
            assert_eq!(ftdi.definition, FtdiDefinitionV2::GiniSimpson);
            assert_eq!(ftdi.fee_topology_diversity_index, Some(0.0));
            assert_eq!(sybil.fee_topology_diversity_index, Some(0.2)); // tylko historyczny K/N
            assert!(ftdi.has_full_quality());
            assert_eq!(ftdi.represented_signer_count, 5);
            let dbia = sybil.dbia_evidence_v1.as_ref().unwrap();
            assert_eq!(dbia.dev_buyer_infrastructure_affinity, Some(1.0));
            assert!(dbia.has_full_quality());
            let sfd = sybil.sfd_evidence_v1.as_ref().unwrap();
            assert_eq!(sfd.spend_fraction_divergence, Some(0.0));
            assert_eq!(sfd.signer_sample_count, 5);
            assert_eq!(sfd.represented_signer_count, if missing { 4 } else { 5 });
            assert_eq!(sfd.has_full_quality(), !missing);
            let des = sybil.demand_elasticity_v2.as_ref().unwrap();
            assert_eq!(sybil.demand_elasticity_score, None);
            assert_eq!(des.demand_elasticity_score, Some(-1.0));
            assert_eq!(des.closed_triple_count, 3);
            assert!(des.has_full_quality());
            assert_eq!(sybil.signer_cross_pool_velocity, Some(0.2));
            assert_eq!(sybil.cpv_evidence.quality, MetricEvidenceQuality::Clean);
            assert_eq!(sybil.cpv_evidence.sample_count, Some(5));
            let decision = evaluate_policy(&features, &config);
            let signals = &decision.sybil_policy.soft_signals;
            assert!(signals.low_ftdi && signals.high_dbia && signals.low_des && signals.high_cpv);
            assert_eq!(signals.low_sfd, !missing);
            assert_eq!(
                decision.sybil_policy.soft_points,
                if missing { 4 } else { 5 }
            );
            let assessment = build_assessment_from_features(
                features.clone(),
                &config,
                PolicyEvaluationContext::default(),
            );
            let log = assessment.to_buy_log(&features.session_metadata.pool_amm_id, &config);
            let bytes = serde_json::to_vec(&log).unwrap();
            let read: ghost_brain::oracle::GatekeeperBuyLog =
                serde_json::from_slice(&bytes).unwrap();
            let measured = read.sybil_measurements_v2.unwrap();
            assert_eq!(
                measured.fee_topology_diversity_v2,
                sybil.fee_topology_diversity_v2
            );
            assert_eq!(measured.sfd_evidence_v1, sybil.sfd_evidence_v1);
            assert_eq!(measured.demand_elasticity_v2, sybil.demand_elasticity_v2);
            assert_eq!(measured.cpv_evidence, sybil.cpv_evidence);
            assert_eq!(measured.thresholds, config.sybil_thresholds_v2);
            assert!(!measured
                .comparison_reasons
                .iter()
                .any(|r| r.contains("DEFINITION_MISMATCH")));
            assert_eq!(read.demand_elasticity_score, None);
        }
    }

    #[test]
    fn m6_explicit_thresholds_no_legacy_alias_and_independent_signals() {
        let (mut features, mut config) = materialize_raw(false);
        features.sybil_resistance.demand_elasticity_score = Some(1.0); // mieszany zapis V1/V2
        config.min_fee_topology_diversity_index = 1.0;
        config.min_demand_elasticity_score = -1.0;
        let saved = serde_json::to_vec(&features.sybil_resistance).unwrap();
        assert!(
            evaluate_policy(&features, &config)
                .sybil_policy
                .soft_signals
                .low_des
        );
        config.sybil_thresholds_v2 = Default::default();
        let diagnostics = evaluate_policy(&features, &config).sybil_policy;
        assert!(!diagnostics.soft_signals.low_ftdi && !diagnostics.soft_signals.low_des);
        assert!(
            diagnostics.soft_signals.high_dbia
                && diagnostics.soft_signals.low_sfd
                && diagnostics.soft_signals.high_cpv
        );
        assert_eq!(diagnostics.soft_points, 3);
        for reason in [
            "FTDI_COMPARISON_DEFINITION_MISMATCH",
            "DES_COMPARISON_DEFINITION_MISMATCH",
        ] {
            assert!(diagnostics
                .metric_degraded_reasons
                .iter()
                .any(|r| r == reason));
        }
        assert_eq!(
            serde_json::to_vec(&features.sybil_resistance).unwrap(),
            saved
        );
        config.sybil_thresholds_v2.ftdi_gini_simpson_min = Some(0.0);
        config.sybil_thresholds_v2.des_next_buy_slot_tau_b_min = Some(-1.0);
        let diagnostics = evaluate_policy(&features, &config).sybil_policy;
        assert!(!diagnostics.soft_signals.low_ftdi && !diagnostics.soft_signals.low_des); // równość na granicy
        assert!(!diagnostics
            .metric_degraded_reasons
            .iter()
            .any(|r| r.contains("DEFINITION_MISMATCH")));
    }

    #[test]
    fn m6_replay_hash_covers_definition_quality_and_receiver_cutoff() {
        let (features, _) = materialize_raw(false);
        let hash = v3_feature_snapshot_hash(&features, 2);
        for field in 0..4 {
            let mut changed = features.clone();
            match field {
                0 => changed
                    .sybil_resistance
                    .fee_topology_diversity_v2
                    .as_mut()
                    .unwrap()
                    .degraded_reasons
                    .push("FTDI_INPUT_STATUS_UNAVAILABLE".into()),
                1 => {
                    changed
                        .sybil_resistance
                        .dbia_evidence_v1
                        .as_mut()
                        .unwrap()
                        .represented_signer_count -= 1
                }
                2 => {
                    changed
                        .sybil_resistance
                        .demand_elasticity_v2
                        .as_mut()
                        .unwrap()
                        .closed_triple_count -= 1
                }
                _ => changed.sybil_resistance.measurement_cutoff_received_ms = Some(1),
            }
            assert_ne!(v3_feature_snapshot_hash(&changed, 2), hash);
        }
        let legacy: SybilResistanceFeatures = serde_json::from_str(
            r#"{"fee_topology_diversity_index":0.2,"demand_elasticity_score":0.3}"#,
        )
        .unwrap();
        assert!(
            legacy.fee_topology_diversity_v2.is_none() && legacy.demand_elasticity_v2.is_none()
        );
        assert_eq!(legacy.fee_topology_diversity_index, Some(0.2));
        assert_eq!(legacy.demand_elasticity_score, Some(0.3));
    }

    #[test]
    fn m6_threshold_config_defaults_unknown_and_validates_each_definition_range() {
        let config = GatekeeperV2Config::default();
        let stored = serde_json::to_value(&config).unwrap();
        assert!(stored.get("sybil_thresholds_v2").is_none());
        assert!(stored["aps"].get("ftdi_gini_simpson_v2_min").is_none());
        for invalid in [-0.01, 1.01, f64::NAN, f64::INFINITY] {
            let mut config = config.clone();
            config.sybil_thresholds_v2.ftdi_gini_simpson_min = Some(invalid);
            assert!(config.validate().is_err());
        }
        for invalid in [-1.01, 1.01, f64::NAN, f64::NEG_INFINITY] {
            let mut config = config.clone();
            config.sybil_thresholds_v2.des_next_buy_slot_tau_b_min = Some(invalid);
            assert!(config.validate().is_err());
        }
        let mut configured = config;
        configured.sybil_thresholds_v2.ftdi_gini_simpson_min = Some(0.2);
        configured.sybil_thresholds_v2.des_next_buy_slot_tau_b_min = Some(-0.3);
        let restored: GatekeeperV2Config =
            serde_json::from_value(serde_json::to_value(&configured).unwrap()).unwrap();
        assert_eq!(restored.sybil_thresholds_v2, configured.sybil_thresholds_v2);
        let mut unknown = serde_json::to_value(&configured.sybil_thresholds_v2).unwrap();
        unknown["ftdi_min"] = 0.5.into();
        assert!(serde_json::from_value::<
            ghost_brain::config::ghost_brain_config::SybilThresholdsV2Config,
        >(unknown)
        .is_err());
    }
}
