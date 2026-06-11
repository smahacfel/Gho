use ghost_brain::config::GatekeeperV2Config;
use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_core::account_state_core::types::{AccountStateUpdate, StatePhase, UpdateSource};
use ghost_core::checkpoint::{
    EventCheckpointTrigger, EvidenceDegradedReason, EvidenceStatus, EvidenceUnavailableReason,
};
use ghost_core::session::types::{SessionStatus, VerdictOutcome};
use ghost_core::EventSemanticEnvelope;
use ghost_core::{CurveFinality, CurveFreshnessState};
use ghost_launcher::events::{FundingTransferObserved, PoolTransaction, RawBytesMissingReason};
use ghost_launcher::session::{OpenSessionRequest, SessionConfig, SessionManager};
use ghost_launcher::tx_intelligence::FundingSourceConfig;
use seer::early_fingerprint::EarlyFingerprintConfig;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::Duration;

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

fn ftdi_tx(
    pool_id: Pubkey,
    signer: Pubkey,
    signature: &str,
    timestamp_ms: u64,
    topology: (u32, u32),
) -> Arc<PoolTransaction> {
    Arc::new(PoolTransaction {
        signer: signer.to_string(),
        signature: signature.to_string(),
        timestamp_ms,
        arrival_ts_ms: timestamp_ms,
        event_time: ghost_core::EventTimeMetadata::new(None, Some(timestamp_ms), None),
        signer_pre_balance_lamports: Some(1_000_000_000),
        signer_post_balance_lamports: Some(900_000_000),
        toolchain_fingerprint: seer::types::ToolchainFingerprintInput {
            external_fee_transfer_count: Some(topology.0),
            internal_fee_transfer_count: Some(topology.1),
            ..seer::types::ToolchainFingerprintInput::default()
        },
        ..(*test_tx(pool_id, signature, timestamp_ms)).clone()
    })
}

fn dbia_fingerprint(
    account_keys_len: u32,
    outer_instruction_count: u32,
    has_set_compute_unit_limit: bool,
    has_set_compute_unit_price: bool,
    inner_instruction_group_count: u32,
    fee_topology: (u32, u32),
) -> seer::types::ToolchainFingerprintInput {
    seer::types::ToolchainFingerprintInput {
        account_keys_len: Some(account_keys_len),
        outer_instruction_count: Some(outer_instruction_count),
        inner_instruction_group_count: Some(inner_instruction_group_count),
        has_set_compute_unit_limit: Some(has_set_compute_unit_limit),
        has_set_compute_unit_price: Some(has_set_compute_unit_price),
        external_fee_transfer_count: Some(fee_topology.0),
        internal_fee_transfer_count: Some(fee_topology.1),
        filtered_wsol_self_transfer_count: Some(0),
    }
}

fn dbia_tx(
    pool_id: Pubkey,
    signer: Pubkey,
    signature: &str,
    timestamp_ms: u64,
    is_dev_buy: bool,
    toolchain_fingerprint: seer::types::ToolchainFingerprintInput,
) -> Arc<PoolTransaction> {
    Arc::new(PoolTransaction {
        signer: signer.to_string(),
        signature: signature.to_string(),
        timestamp_ms,
        arrival_ts_ms: timestamp_ms,
        event_time: ghost_core::EventTimeMetadata::new(None, Some(timestamp_ms), None),
        is_dev_buy,
        signer_pre_balance_lamports: Some(1_000_000_000),
        signer_post_balance_lamports: Some(900_000_000),
        toolchain_fingerprint,
        ..(*test_tx(pool_id, signature, timestamp_ms)).clone()
    })
}

fn sfd_tx(
    pool_id: Pubkey,
    signer: Pubkey,
    signature: &str,
    timestamp_ms: u64,
    is_dev_buy: bool,
    pre_balance: u64,
    post_balance: u64,
) -> Arc<PoolTransaction> {
    Arc::new(PoolTransaction {
        signer: signer.to_string(),
        signature: signature.to_string(),
        timestamp_ms,
        arrival_ts_ms: timestamp_ms,
        event_time: ghost_core::EventTimeMetadata::new(None, Some(timestamp_ms), None),
        is_dev_buy,
        signer_pre_balance_lamports: Some(pre_balance),
        signer_post_balance_lamports: Some(post_balance),
        toolchain_fingerprint: dbia_fingerprint(12, 3, true, true, 2, (0, 0)),
        ..(*test_tx(pool_id, signature, timestamp_ms)).clone()
    })
}

fn des_tx(
    pool_id: Pubkey,
    signer: Pubkey,
    signature: &str,
    timestamp_ms: u64,
    slot: u64,
    event_ordinal: Option<u32>,
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
        event_ordinal,
        is_dev_buy,
        signer_pre_balance_lamports: Some(100),
        signer_post_balance_lamports: Some(90),
        toolchain_fingerprint: dbia_fingerprint(12, 3, true, true, 2, (0, 0)),
        v_sol_in_bonding_curve: Some(price),
        v_tokens_in_bonding_curve: Some(1.0),
        market_cap_sol: Some(price * 1_000_000_000.0),
        curve_data_known: true,
        ..(*test_tx(pool_id, signature, timestamp_ms)).clone()
    })
}

fn funding_transfer(
    source_wallet: &str,
    recipient_wallet: &str,
    signature: &str,
    timestamp_ms: u64,
    lamports: u64,
) -> FundingTransferObserved {
    FundingTransferObserved {
        semantic: EventSemanticEnvelope::default(),
        slot: Some(1),
        event_ordinal: None,
        tx_index: None,
        outer_instruction_index: None,
        inner_group_index: None,
        cpi_stack_height: None,
        event_time: ghost_core::EventTimeMetadata::new(None, Some(timestamp_ms), None),
        arrival_ts_ms: timestamp_ms,
        signature: signature.to_string(),
        source_wallet: source_wallet.to_string(),
        recipient_wallet: recipient_wallet.to_string(),
        lamports,
        full_chain_coverage: true,
        provenance: seer::ipc::FundingTransferProvenance::authoritative_full_feed_live(),
        lane_health: seer::ipc::FundingLaneRuntimeHealth::default(),
        detected_at: std::time::SystemTime::now(),
        sequence_number: timestamp_ms,
    }
}

fn test_curve_tx(
    pool_id: Pubkey,
    signature: &str,
    timestamp_ms: u64,
    v_sol: f64,
    v_tokens: f64,
) -> Arc<PoolTransaction> {
    Arc::new(PoolTransaction {
        v_sol_in_bonding_curve: Some(v_sol),
        v_tokens_in_bonding_curve: Some(v_tokens),
        market_cap_sol: Some((v_sol / v_tokens) * 1_000_000_000.0),
        curve_data_known: true,
        ..(*test_tx(pool_id, signature, timestamp_ms)).clone()
    })
}

fn test_account_update(
    pool_id: Pubkey,
    base_mint: Pubkey,
    bonding_curve: Pubkey,
    receive_ts_ms: u64,
    sol_reserves: u64,
    token_reserves: u64,
) -> AccountStateUpdate {
    AccountStateUpdate {
        pool_amm_id: pool_id,
        base_mint,
        bonding_curve,
        sol_reserves,
        token_reserves,
        is_complete: 0,
        slot: 1,
        write_version: Some(receive_ts_ms),
        receive_ts_ms,
        receive_seq: receive_ts_ms,
        curve_finality: CurveFinality::Finalized,
        source: UpdateSource::GeyserAccountUpdate,
    }
}

fn open_session(
    manager: &SessionManager,
    pool_id: Pubkey,
    base_mint: Pubkey,
    bonding_curve: Pubkey,
    created_at_wall_ms: u64,
) -> ghost_launcher::session::SharedSession {
    open_session_with_deadline_and_gatekeeper_config(
        manager,
        pool_id,
        base_mint,
        bonding_curve,
        created_at_wall_ms,
        created_at_wall_ms + 100,
        GatekeeperV2Config::default(),
    )
}

fn open_session_with_deadline_and_gatekeeper_config(
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

#[test]
fn materialize_features_populates_ftdi_from_session_tx_buffer() {
    let manager = SessionManager::new(SessionConfig {
        default_observation_duration_ms: 100,
        max_sessions: 8,
        ..SessionConfig::default()
    });
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let session = open_session(&manager, pool_id, base_mint, bonding_curve, 10_000);

    let features = {
        let mut guard = session.write();
        let _ = guard.ingest_transaction(ftdi_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-ftdi-a",
            10_010,
            (0, 0),
        ));
        let _ = guard.ingest_transaction(ftdi_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-ftdi-b",
            10_020,
            (1, 0),
        ));
        let _ = guard.ingest_transaction(ftdi_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-ftdi-c",
            10_030,
            (2, 0),
        ));
        guard.materialize_features()
    };

    assert_eq!(
        features.sybil_resistance.fee_topology_diversity_index,
        Some(1.0)
    );
    assert_eq!(
        features.sybil_resistance.signer_cross_pool_velocity,
        Some(0.0)
    );
    assert_eq!(features.sybil_resistance.buy_sample_count, 3);
    assert_eq!(features.sybil_resistance.signer_sample_count, 3);
    assert_eq!(
        features.sybil_resistance.degraded_reasons,
        vec![
            ghost_core::tx_intelligence::types::DBIA_NO_DEV_BUY_REASON.to_string(),
            ghost_core::tx_intelligence::types::DES_INSUFFICIENT_BUYS_REASON.to_string(),
            ghost_core::tx_intelligence::types::FSC_FUNDING_STREAM_UNAVAILABLE_REASON.to_string(),
        ]
    );
    assert_eq!(
        features.evidence_status.sybil.status,
        EvidenceStatus::Degraded
    );
    assert_eq!(
        features.evidence_status.sybil.degraded_reasons,
        vec![EvidenceDegradedReason::SybilEvidencePartial]
    );
}

#[test]
fn materialize_features_populates_dbia_from_session_tx_buffer() {
    let manager = SessionManager::new(SessionConfig {
        default_observation_duration_ms: 100,
        max_sessions: 8,
        ..SessionConfig::default()
    });
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let session = open_session(&manager, pool_id, base_mint, bonding_curve, 20_000);
    let dev_wallet = session
        .read()
        .dev_wallet
        .expect("session should know dev wallet");

    let features = {
        let mut guard = session.write();
        let _ = guard.ingest_transaction(dbia_tx(
            pool_id,
            dev_wallet,
            "sig-dev",
            20_010,
            true,
            dbia_fingerprint(12, 3, true, true, 2, (0, 0)),
        ));
        let _ = guard.ingest_transaction(dbia_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-buyer-a",
            20_020,
            false,
            dbia_fingerprint(12, 3, true, true, 2, (0, 0)),
        ));
        let _ = guard.ingest_transaction(dbia_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-buyer-b",
            20_030,
            false,
            dbia_fingerprint(30, 10, false, false, 8, (3, 3)),
        ));
        guard.materialize_features()
    };

    assert_eq!(
        features.sybil_resistance.dev_buyer_infrastructure_affinity,
        Some(0.5)
    );
    assert_eq!(
        features.sybil_resistance.signer_cross_pool_velocity,
        Some(0.0)
    );
    assert_eq!(features.sybil_resistance.buy_sample_count, 3);
    assert_eq!(features.sybil_resistance.signer_sample_count, 3);
    assert_eq!(
        features.sybil_resistance.degraded_reasons,
        vec![
            ghost_core::tx_intelligence::types::DES_INSUFFICIENT_BUYS_REASON.to_string(),
            ghost_core::tx_intelligence::types::FSC_FUNDING_STREAM_UNAVAILABLE_REASON.to_string(),
        ]
    );
}

#[test]
fn materialize_features_preserves_toolchain_partial_coverage_reasons() {
    let manager = SessionManager::new(SessionConfig {
        default_observation_duration_ms: 100,
        max_sessions: 8,
        ..SessionConfig::default()
    });
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let session = open_session(&manager, pool_id, base_mint, bonding_curve, 21_000);
    let dev_wallet = session
        .read()
        .dev_wallet
        .expect("session should know dev wallet");
    let shared = dbia_fingerprint(12, 3, true, true, 2, (0, 0));

    let features = {
        let mut guard = session.write();
        let _ = guard.ingest_transaction(dbia_tx(
            pool_id,
            dev_wallet,
            "sig-partial-dev",
            21_010,
            true,
            shared.clone(),
        ));
        let _ = guard.ingest_transaction(dbia_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-partial-a",
            21_020,
            false,
            shared.clone(),
        ));
        let _ = guard.ingest_transaction(dbia_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-partial-b",
            21_030,
            false,
            shared.clone(),
        ));
        let _ = guard.ingest_transaction(dbia_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-partial-c",
            21_040,
            false,
            shared,
        ));
        let _ = guard.ingest_transaction(dbia_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-partial-missing",
            21_050,
            false,
            seer::types::ToolchainFingerprintInput::default(),
        ));
        guard.materialize_features()
    };

    assert_eq!(
        features.sybil_resistance.dev_buyer_infrastructure_affinity,
        Some(1.0)
    );
    assert_eq!(
        features.sybil_resistance.toolchain_fingerprint_coverage,
        Some(0.8)
    );
    assert!(features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::FTDI_PARTIAL_FEE_TOPOLOGY_COVERAGE.to_string()
    ));
    assert!(features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::DBIA_PARTIAL_FINGERPRINT_COVERAGE.to_string()
    ));
    assert!(!features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::FTDI_RAW_FEE_TOPOLOGY_UNAVAILABLE_REASON.to_string()
    ));
    assert!(!features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::DBIA_RAW_FINGERPRINT_UNAVAILABLE_REASON.to_string()
    ));
}

#[test]
fn materialize_features_populates_sfd_from_session_tx_buffer() {
    let manager = SessionManager::new(SessionConfig {
        default_observation_duration_ms: 100,
        max_sessions: 8,
        ..SessionConfig::default()
    });
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let session = open_session(&manager, pool_id, base_mint, bonding_curve, 30_000);
    let dev_wallet = session
        .read()
        .dev_wallet
        .expect("session should know dev wallet");

    let features = {
        let mut guard = session.write();
        let _ = guard.ingest_transaction(sfd_tx(
            pool_id,
            dev_wallet,
            "sig-sfd-dev",
            30_010,
            true,
            100,
            99,
        ));
        let _ = guard.ingest_transaction(sfd_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-sfd-a",
            30_020,
            false,
            100,
            17,
        ));
        let _ = guard.ingest_transaction(sfd_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-sfd-b",
            30_030,
            false,
            100,
            80,
        ));
        let _ = guard.ingest_transaction(sfd_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-sfd-c",
            30_040,
            false,
            100,
            55,
        ));
        let _ = guard.ingest_transaction(sfd_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-sfd-d",
            30_050,
            false,
            100,
            38,
        ));
        guard.materialize_features()
    };

    assert_eq!(
        features.sybil_resistance.spend_fraction_divergence,
        Some(0.25)
    );
    assert_eq!(
        features.sybil_resistance.signer_cross_pool_velocity,
        Some(0.0)
    );
    assert_eq!(features.sybil_resistance.buy_sample_count, 5);
    assert_eq!(features.sybil_resistance.signer_sample_count, 5);
    assert_eq!(
        features.sybil_resistance.degraded_reasons,
        vec![
            ghost_core::tx_intelligence::types::DES_CURVE_DATA_UNAVAILABLE_REASON.to_string(),
            ghost_core::tx_intelligence::types::FSC_FUNDING_STREAM_UNAVAILABLE_REASON.to_string(),
        ]
    );
}

#[test]
fn materialize_features_keeps_sfd_when_partial_balance_coverage_still_has_three_usable_signers() {
    let manager = SessionManager::new(SessionConfig {
        default_observation_duration_ms: 100,
        max_sessions: 8,
        ..SessionConfig::default()
    });
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let session = open_session(&manager, pool_id, base_mint, bonding_curve, 31_000);
    let dev_wallet = session
        .read()
        .dev_wallet
        .expect("session should know dev wallet");

    let features = {
        let mut guard = session.write();
        let _ = guard.ingest_transaction(sfd_tx(
            pool_id,
            dev_wallet,
            "sig-sfd-partial-dev",
            31_010,
            true,
            100,
            10,
        ));
        let _ = guard.ingest_transaction(sfd_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-sfd-partial-a",
            31_020,
            false,
            100,
            10,
        ));
        let _ = guard.ingest_transaction(sfd_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-sfd-partial-b",
            31_030,
            false,
            100,
            10,
        ));
        let _ = guard.ingest_transaction(Arc::new(PoolTransaction {
            signer: Pubkey::new_unique().to_string(),
            signature: "sig-sfd-partial-missing".to_string(),
            timestamp_ms: 31_040,
            arrival_ts_ms: 31_040,
            event_time: ghost_core::EventTimeMetadata::new(None, Some(31_040), None),
            is_dev_buy: false,
            signer_pre_balance_lamports: Some(100),
            signer_post_balance_lamports: None,
            toolchain_fingerprint: dbia_fingerprint(12, 3, true, true, 2, (0, 0)),
            ..(*test_tx(pool_id, "sig-sfd-partial-missing", 31_040)).clone()
        }));
        guard.materialize_features()
    };

    assert_eq!(
        features.sybil_resistance.spend_fraction_divergence,
        Some(0.0)
    );
    assert!(features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::SFD_PARTIAL_BALANCE_COVERAGE_REASON.to_string()
    ));
}

#[test]
fn materialize_features_populates_des_from_session_tx_buffer() {
    let manager = SessionManager::new(SessionConfig {
        default_observation_duration_ms: 100,
        max_sessions: 8,
        ..SessionConfig::default()
    });
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let session = open_session(&manager, pool_id, base_mint, bonding_curve, 40_000);
    let dev_wallet = session
        .read()
        .dev_wallet
        .expect("session should know dev wallet");

    let features = {
        let mut guard = session.write();
        let _ = guard.ingest_transaction(des_tx(
            pool_id,
            dev_wallet,
            "sig-des-dev",
            40_010,
            1,
            Some(0),
            true,
            10.0,
        ));
        let _ = guard.ingest_transaction(des_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-des-a",
            40_020,
            2,
            Some(0),
            false,
            11.0,
        ));
        let _ = guard.ingest_transaction(des_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-des-b",
            40_030,
            4,
            Some(0),
            false,
            13.2,
        ));
        let _ = guard.ingest_transaction(des_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-des-c",
            40_040,
            7,
            Some(0),
            false,
            17.16,
        ));
        guard.materialize_features()
    };

    assert_eq!(features.sybil_resistance.demand_elasticity_score, Some(1.0));
    assert_eq!(features.sybil_resistance.buy_sample_count, 4);
    assert_eq!(features.sybil_resistance.signer_sample_count, 4);
    assert_eq!(
        features.sybil_resistance.degraded_reasons,
        vec![ghost_core::tx_intelligence::types::FSC_FUNDING_STREAM_UNAVAILABLE_REASON.to_string()]
    );
}

#[test]
fn materialize_features_populates_cpv_from_shared_session_index() {
    let manager = SessionManager::new(SessionConfig {
        default_observation_duration_ms: 100,
        max_sessions: 8,
        ..SessionConfig::default()
    });
    let pool_a = Pubkey::new_unique();
    let pool_b = Pubkey::new_unique();
    let base_mint_a = Pubkey::new_unique();
    let base_mint_b = Pubkey::new_unique();
    let bonding_curve_a = Pubkey::new_unique();
    let bonding_curve_b = Pubkey::new_unique();
    let session_a = open_session(&manager, pool_a, base_mint_a, bonding_curve_a, 49_000);
    let session_b = open_session(&manager, pool_b, base_mint_b, bonding_curve_b, 50_000);
    let shared_signer = Pubkey::new_unique();
    let session_b_dev_wallet = session_b
        .read()
        .dev_wallet
        .expect("session should know dev wallet");

    {
        let mut guard = session_a.write();
        let _ = guard.ingest_transaction(des_tx(
            pool_a,
            shared_signer,
            "sig-cpv-pool-a",
            49_010,
            1,
            Some(0),
            false,
            9.0,
        ));
    }

    let features = {
        let mut guard = session_b.write();
        let _ = guard.ingest_transaction(des_tx(
            pool_b,
            session_b_dev_wallet,
            "sig-cpv-dev",
            50_010,
            2,
            Some(0),
            true,
            10.0,
        ));
        let _ = guard.ingest_transaction(des_tx(
            pool_b,
            shared_signer,
            "sig-cpv-shared",
            50_020,
            3,
            Some(0),
            false,
            11.0,
        ));
        let _ = guard.ingest_transaction(des_tx(
            pool_b,
            Pubkey::new_unique(),
            "sig-cpv-local",
            50_030,
            4,
            Some(0),
            false,
            12.0,
        ));
        guard.materialize_features()
    };

    assert_eq!(
        features.sybil_resistance.signer_cross_pool_velocity,
        Some(1.0 / 3.0)
    );
    assert_eq!(features.sybil_resistance.funding_source_concentration, None);
    assert!(!features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::CPV_ROLLING_STATE_UNAVAILABLE_REASON.to_string()
    ));
    assert!(!features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::CPV_INSUFFICIENT_SIGNERS_REASON.to_string()
    ));
    assert!(features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::FSC_FUNDING_STREAM_UNAVAILABLE_REASON.to_string()
    ));
}

#[test]
fn materialize_features_populates_fsc_from_shared_funding_source_index() {
    let manager = SessionManager::new(SessionConfig {
        default_observation_duration_ms: 100,
        max_sessions: 8,
        ..SessionConfig::default()
    });
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let session = open_session(&manager, pool_id, base_mint, bonding_curve, 60_000);
    let dev_wallet = session
        .read()
        .dev_wallet
        .expect("session should know dev wallet");
    let buyer_a = Pubkey::new_unique();
    let buyer_b = Pubkey::new_unique();
    let buyer_c = Pubkey::new_unique();

    let funding_config =
        FundingSourceConfig::from_gatekeeper_config(&GatekeeperV2Config::default());
    let decision_wall_ms = 60_500;
    manager
        .funding_source_index()
        .set_stream_available_at(true, 59_500);
    manager.funding_source_index().observe_transfer(
        &funding_transfer(
            "shared-funder",
            &dev_wallet.to_string(),
            "fund-dev",
            59_900,
            50_000_000,
        ),
        &funding_config,
    );
    manager.funding_source_index().observe_transfer(
        &funding_transfer(
            "shared-funder",
            &buyer_a.to_string(),
            "fund-a",
            59_910,
            50_000_000,
        ),
        &funding_config,
    );
    manager.funding_source_index().observe_transfer(
        &funding_transfer(
            "shared-funder",
            &buyer_b.to_string(),
            "fund-b",
            59_920,
            50_000_000,
        ),
        &funding_config,
    );
    manager.funding_source_index().observe_transfer(
        &funding_transfer(
            "distinct-funder",
            &buyer_c.to_string(),
            "fund-c",
            59_930,
            50_000_000,
        ),
        &funding_config,
    );

    let features = {
        let mut guard = session.write();
        let _ = guard.ingest_transaction(des_tx(
            pool_id,
            dev_wallet,
            "sig-fsc-dev",
            60_010,
            1,
            Some(0),
            true,
            10.0,
        ));
        let _ = guard.ingest_transaction(des_tx(
            pool_id,
            buyer_a,
            "sig-fsc-a",
            60_020,
            2,
            Some(0),
            false,
            11.0,
        ));
        let _ = guard.ingest_transaction(des_tx(
            pool_id,
            buyer_b,
            "sig-fsc-b",
            60_030,
            3,
            Some(0),
            false,
            12.0,
        ));
        let _ = guard.ingest_transaction(des_tx(
            pool_id,
            buyer_c,
            "sig-fsc-c",
            60_040,
            4,
            Some(0),
            false,
            13.0,
        ));
        guard.materialize_features_at(decision_wall_ms)
    };
    let expected_coverage = manager
        .funding_source_index()
        .coverage_window_status(&funding_config, decision_wall_ms);

    assert_eq!(
        features.sybil_resistance.funding_source_concentration, None,
        "primary FSC should stay unactionable while FSC v2 is not clean"
    );
    let fsc_v2 = features
        .sybil_resistance
        .funding_source_v2
        .as_ref()
        .expect("FSC v2 evidence should be materialized additively");
    assert!(matches!(
        fsc_v2.status,
        ghost_core::tx_intelligence::types::FscEvidenceStatus::Clean
            | ghost_core::tx_intelligence::types::FscEvidenceStatus::Degraded
    ));
    assert!(
        (fsc_v2
            .hhi_norm_count
            .expect("FSC v2 HHI should be materialized")
            - (1.0 / 3.0))
            .abs()
            < f64::EPSILON
    );
    assert_eq!(
        fsc_v2.coverage_window_ready,
        expected_coverage.coverage_window_ready
    );
    assert_eq!(
        fsc_v2.coverage_window_remaining_ms,
        expected_coverage.coverage_window_remaining_ms
    );
    assert_eq!(
        fsc_v2.authoritative_buy_ready,
        expected_coverage.authoritative_buy_ready
    );
    assert!(!fsc_v2.authoritative_buy_ready);
    assert!(!features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::FSC_FUNDING_STREAM_UNAVAILABLE_REASON.to_string()
    ));
    assert!(!features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::FSC_ROLLING_STATE_UNAVAILABLE_REASON.to_string()
    ));
    assert!(features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::FSC_COVERAGE_WINDOW_UNAVAILABLE.to_string()
    ));
}

#[test]
fn filtered_transfer_does_not_unlock_fsc_when_stream_is_only_health_ready() {
    let manager = SessionManager::new(SessionConfig {
        default_observation_duration_ms: 100,
        max_sessions: 8,
        ..SessionConfig::default()
    });
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let session = open_session(&manager, pool_id, base_mint, bonding_curve, 60_000);
    let dev_wallet = session
        .read()
        .dev_wallet
        .expect("session should know dev wallet");
    let buyer_a = Pubkey::new_unique();
    let buyer_b = Pubkey::new_unique();
    let buyer_c = Pubkey::new_unique();
    let funding_config =
        ghost_launcher::tx_intelligence::FundingSourceConfig::from_gatekeeper_config(
            &GatekeeperV2Config::default(),
        );
    let stream_available_since_ms = 59_500;
    let decision_wall_ms = 60_050;
    let mut filtered_transfer = funding_transfer(
        "filtered-funder",
        &buyer_a.to_string(),
        "filtered-a",
        59_910,
        50_000_000,
    );
    filtered_transfer.full_chain_coverage = false;
    filtered_transfer.provenance =
        seer::ipc::FundingTransferProvenance::funding_lane_pump_filtered_live();

    manager
        .funding_source_index()
        .set_stream_available_at(true, stream_available_since_ms);
    manager
        .funding_source_index()
        .observe_transfer(&filtered_transfer, &funding_config);

    let features = {
        let mut guard = session.write();
        let _ = guard.ingest_transaction(des_tx(
            pool_id,
            dev_wallet,
            "sig-fsc-filtered-dev",
            60_010,
            1,
            Some(0),
            true,
            10.0,
        ));
        let _ = guard.ingest_transaction(des_tx(
            pool_id,
            buyer_a,
            "sig-fsc-filtered-a",
            60_020,
            2,
            Some(0),
            false,
            11.0,
        ));
        let _ = guard.ingest_transaction(des_tx(
            pool_id,
            buyer_b,
            "sig-fsc-filtered-b",
            60_030,
            3,
            Some(0),
            false,
            12.0,
        ));
        let _ = guard.ingest_transaction(des_tx(
            pool_id,
            buyer_c,
            "sig-fsc-filtered-c",
            60_040,
            4,
            Some(0),
            false,
            13.0,
        ));
        guard.materialize_features_at(decision_wall_ms)
    };

    assert_eq!(features.sybil_resistance.funding_source_concentration, None);
    let fsc_v2 = features
        .sybil_resistance
        .funding_source_v2
        .as_ref()
        .expect("FSC v2 evidence should be materialized");
    assert!(fsc_v2.index_warm);
    assert!(!fsc_v2.coverage_window_ready);
    assert!(!fsc_v2.authoritative_buy_ready);
    assert_eq!(
        fsc_v2.coverage_window_remaining_ms,
        funding_config
            .lookback_window_ms
            .saturating_sub(decision_wall_ms.saturating_sub(stream_available_since_ms))
    );
    assert!(!features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::FSC_FUNDING_STREAM_UNAVAILABLE_REASON.to_string()
    ));
    assert!(!features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::FSC_ROLLING_STATE_UNAVAILABLE_REASON.to_string()
    ));
    assert!(features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::FSC_INSUFFICIENT_KNOWN_SOURCES_REASON.to_string()
    ));
    assert!(features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::FSC_COVERAGE_WINDOW_UNAVAILABLE.to_string()
    ));
}

#[test]
fn session_lifecycle_transitions_created_to_closed() {
    let manager = SessionManager::new(SessionConfig {
        default_observation_duration_ms: 100,
        max_sessions: 1_000,
        ..SessionConfig::default()
    });
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let session = open_session(&manager, pool_id, base_mint, bonding_curve, 1_000);

    assert!(matches!(
        session.read().get_status(),
        SessionStatus::Created
    ));

    {
        let tx = test_tx(pool_id, "sig-lifecycle", 1_010);
        let mut guard = session.write();
        let _ = guard.ingest_transaction(tx);
        assert!(matches!(guard.get_status(), SessionStatus::Accumulating));
        guard.begin_evaluation();
        assert!(matches!(guard.get_status(), SessionStatus::Evaluating));
    }

    assert!(manager.close_session(
        &pool_id,
        VerdictOutcome::Pass {
            reason: "passed".to_string(),
        },
    ));
    {
        let guard = session.read();
        assert!(matches!(
            guard.get_status(),
            SessionStatus::Decided(VerdictOutcome::Pass { reason }) if reason == "passed"
        ));
    }

    assert!(manager.remove_session(&pool_id));
    assert!(manager.get_session(&pool_id).is_none());
    assert!(matches!(session.read().get_status(), SessionStatus::Closed));
}

#[test]
fn session_dedup_does_not_double_count_duplicates() {
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let session = open_session(
        &manager,
        pool_id,
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        2_000,
    );
    let tx = test_tx(pool_id, "sig-dup", 2_010);

    {
        let mut guard = session.write();
        let _ = guard.ingest_transaction(tx.clone());
        let _ = guard.ingest_transaction(tx);
    }

    let guard = session.read();
    assert_eq!(guard.diagnostics.total_tx_seen, 1);
    assert_eq!(guard.tx_buffer.len(), 1);
    assert_eq!(guard.tx_keys_seen.len(), 1);
}

#[test]
fn session_timing_reports_elapsed_and_expiry() {
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let session = open_session(
        &manager,
        pool_id,
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        3_000,
    );

    std::thread::sleep(Duration::from_millis(5));

    let guard = session.read();
    assert!(guard.elapsed_ms() >= 5);
    assert!(!guard.is_expired(3_099));
    assert!(guard.is_expired(3_100));
}

#[test]
fn sessions_are_isolated_per_pool() {
    let manager = SessionManager::default();
    let pool_a = Pubkey::new_unique();
    let pool_b = Pubkey::new_unique();
    let session_a = open_session(
        &manager,
        pool_a,
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        4_000,
    );
    let session_b = open_session(
        &manager,
        pool_b,
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        4_000,
    );

    {
        let mut guard_a = session_a.write();
        let _ = guard_a.ingest_transaction(test_tx(pool_a, "sig-a", 4_010));
    }

    {
        let guard_a = session_a.read();
        let guard_b = session_b.read();
        assert_eq!(guard_a.diagnostics.total_tx_seen, 1);
        assert_eq!(guard_b.diagnostics.total_tx_seen, 0);
        assert_ne!(guard_a.session_id, guard_b.session_id);
    }
}

#[test]
fn remove_session_releases_manager_entry() {
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let _session = open_session(
        &manager,
        pool_id,
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        5_000,
    );

    assert_eq!(manager.active_session_count(), 1);
    assert!(manager.remove_session(&pool_id));
    assert_eq!(manager.active_session_count(), 0);
    assert!(manager.get_session(&pool_id).is_none());
}

#[test]
fn open_session_same_pool_reuses_existing_session() {
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();

    let first = open_session(&manager, pool_id, base_mint, bonding_curve, 6_000);
    let first_id = first.read().session_id;

    let second_id = manager
        .open_session(OpenSessionRequest {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            dev_wallet: Some(Pubkey::new_unique()),
            candidate_snapshot: test_candidate(pool_id, base_mint, bonding_curve),
            created_at_wall_ms: 6_500,
            deadline_wall_ms: Some(6_999),
            gatekeeper_config: GatekeeperV2Config::default(),
            funding_source_config: FundingSourceConfig::from_gatekeeper_config(
                &GatekeeperV2Config::default(),
            ),
            fingerprint_config: EarlyFingerprintConfig::default(),
        })
        .expect("reopening same pool should reuse existing session");
    let second = manager
        .get_session(&pool_id)
        .expect("existing session should still be present");

    assert_eq!(first_id, second_id);
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(manager.active_session_count(), 1);
}

#[test]
fn session_limit_rejects_new_pool_without_replacing_existing_session() {
    let manager = SessionManager::new(SessionConfig {
        default_observation_duration_ms: 100,
        max_sessions: 1,
        ..SessionConfig::default()
    });
    let first_pool = Pubkey::new_unique();
    let first_base_mint = Pubkey::new_unique();
    let first_curve = Pubkey::new_unique();
    let first = open_session(&manager, first_pool, first_base_mint, first_curve, 7_000);
    let first_id = first.read().session_id;

    let second_pool = Pubkey::new_unique();
    let second_base_mint = Pubkey::new_unique();
    let second_curve = Pubkey::new_unique();
    let second_result = manager.open_session(OpenSessionRequest {
        pool_amm_id: second_pool,
        base_mint: second_base_mint,
        bonding_curve: second_curve,
        dev_wallet: Some(Pubkey::new_unique()),
        candidate_snapshot: test_candidate(second_pool, second_base_mint, second_curve),
        created_at_wall_ms: 7_100,
        deadline_wall_ms: Some(7_200),
        gatekeeper_config: GatekeeperV2Config::default(),
        funding_source_config: FundingSourceConfig::from_gatekeeper_config(
            &GatekeeperV2Config::default(),
        ),
        fingerprint_config: EarlyFingerprintConfig::default(),
    });
    assert!(matches!(
        second_result,
        Err(ghost_launcher::session::SessionManagerError::SessionLimitExceeded { max_sessions: 1 })
    ));

    let reused_id = manager
        .open_session(OpenSessionRequest {
            pool_amm_id: first_pool,
            base_mint: first_base_mint,
            bonding_curve: first_curve,
            dev_wallet: Some(Pubkey::new_unique()),
            candidate_snapshot: test_candidate(first_pool, first_base_mint, first_curve),
            created_at_wall_ms: 7_150,
            deadline_wall_ms: Some(7_250),
            gatekeeper_config: GatekeeperV2Config::default(),
            funding_source_config: FundingSourceConfig::from_gatekeeper_config(
                &GatekeeperV2Config::default(),
            ),
            fingerprint_config: EarlyFingerprintConfig::default(),
        })
        .expect("existing pool should still be reusable at capacity");
    assert_eq!(reused_id, first_id);
    assert_eq!(manager.active_session_count(), 1);
}

#[test]
fn refresh_from_gatekeeper_preserves_session_owned_timing() {
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let session = open_session(
        &manager,
        pool_id,
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        7_000,
    );

    {
        let mut guard = session.write();
        guard.gatekeeper_buffer_mut().set_registered_wall_t0(0);
        guard.gatekeeper_buffer_mut().set_deadline_wall_ts_ms(0);
        guard.refresh_from_gatekeeper();

        assert_eq!(guard.created_at_wall_ms, 7_000);
        assert_eq!(guard.deadline_wall_ms, 7_100);
    }
}

#[test]
fn session_materializes_checkpoint_features_from_runtime_observation() {
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let session = open_session(&manager, pool_id, base_mint, bonding_curve, 8_000);

    {
        let mut guard = session.write();
        guard.checkpoint_engine.config.interval_ms = 1;
        guard.checkpoint_engine.config.min_tx_between_checkpoints = 1;
        guard.checkpoint_engine.config.enable_event_checkpoints = true;
        guard.checkpoint_engine.config.event_triggers =
            vec![EventCheckpointTrigger::LargeTradeImpact(10.0)];

        guard.on_account_update(&test_account_update(
            pool_id,
            base_mint,
            bonding_curve,
            8_001,
            31_000_000_000,
            1_000_000_000,
        ));
        let _ = guard.ingest_transaction(test_curve_tx(pool_id, "sig-1", 8_010, 30.0, 1_000_000.0));
        guard.try_checkpoint(8_010);

        guard.on_account_update(&test_account_update(
            pool_id,
            base_mint,
            bonding_curve,
            8_020,
            33_000_000_000,
            900_000_000,
        ));
        let _ = guard.ingest_transaction(test_curve_tx(pool_id, "sig-2", 8_020, 36.0, 900_000.0));
        guard.try_checkpoint(8_020);

        let features = guard.materialize_features();
        assert!(guard.checkpoints.len() >= 2);
        assert_eq!(
            guard.diagnostics.checkpoint_count,
            guard.checkpoints.len() as u32
        );
        assert_eq!(features.account_features.state_phase, StatePhase::Canonical);
        assert_eq!(
            features.checkpoint_features.trajectory_checkpoint_count,
            guard.checkpoints.len() as u32
        );
        assert_eq!(
            features.checkpoint_features.price_trajectory.len(),
            guard.checkpoints.len()
        );
        assert!(features.checkpoint_features.single_tx_max_price_impact_pct > 0.0);
        assert!(
            features
                .checkpoint_features
                .price_change_from_first_checkpoint_pct
                > 0.0
        );
        assert!(features.checkpoint_features.bonding_progress > 0.0);
    }
}

#[test]
fn session_curve_readiness_prefers_account_state_core_canonical_state() {
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let session = open_session(&manager, pool_id, base_mint, bonding_curve, 9_000);

    {
        let mut guard = session.write();
        guard
            .gatekeeper_buffer_mut()
            .record_curve_state(CurveFreshnessState::Unknown, CurveFinality::Speculative);
        guard.on_account_update(&test_account_update(
            pool_id,
            base_mint,
            bonding_curve,
            9_001,
            31_000_000_000,
            1_000_000_000,
        ));

        let features = guard.materialize_features();
        assert!(features.curve_readiness.is_ready);
        assert_eq!(
            features.curve_readiness.freshness,
            CurveFreshnessState::Committed
        );
        assert_eq!(features.curve_readiness.finality, CurveFinality::Finalized);
        assert!(features.curve_readiness.curve_data_known);
    }
}

#[test]
fn open_session_syncs_preexisting_canonical_state_from_shared_core() {
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();

    let apply_result = manager
        .account_state_core()
        .apply_account_update(test_account_update(
            pool_id,
            base_mint,
            bonding_curve,
            9_995,
            31_000_000_000,
            1_000_000_000,
        ));
    assert!(matches!(
        apply_result,
        ghost_core::account_state_core::types::AccountUpdateResult::Applied
            | ghost_core::account_state_core::types::AccountUpdateResult::PromotedFromBootstrap
    ));

    let session = open_session(&manager, pool_id, base_mint, bonding_curve, 10_000);

    let guard = session.read();
    let features = guard.materialize_features();
    assert_eq!(guard.canonical_update_count(), 1);
    assert_eq!(features.account_features.update_count, 1);
    assert_eq!(features.account_features.state_phase, StatePhase::Canonical);
    assert!(matches!(guard.get_status(), SessionStatus::Accumulating));
    assert_eq!(guard.diagnostics.total_account_updates, 0);
}

#[test]
fn session_materializes_curve_market_cap_without_account_updates() {
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let session = open_session(&manager, pool_id, base_mint, bonding_curve, 10_000);
    let signers: Vec<Pubkey> = (0..3).map(|_| Pubkey::new_unique()).collect();

    let features = {
        let mut guard = session.write();
        let _ = guard.ingest_transaction(test_curve_tx(
            pool_id,
            "curve-sig-1",
            10_010,
            30.0,
            1_000_000.0,
        ));
        let _ = guard.ingest_transaction(test_curve_tx(
            pool_id,
            "curve-sig-2",
            10_120,
            31.0,
            950_000.0,
        ));
        let _ = guard.ingest_transaction(Arc::new(PoolTransaction {
            signer: signers[2].to_string(),
            ..(*test_curve_tx(pool_id, "curve-sig-3", 10_240, 32.0, 900_000.0)).clone()
        }));

        guard.materialize_features()
    };

    assert_eq!(features.account_features.update_count, 0);
    assert_eq!(features.account_features.state_phase, StatePhase::Bootstrap);
    assert!(features.curve_readiness.curve_data_known);
    assert!(features.account_features.price_sol > 0.0);
    assert!(
        features.account_features.market_cap_sol > 0.0,
        "expected positive curve-derived market cap, got {}",
        features.account_features.market_cap_sol
    );
}

#[test]
fn session_materializes_segment_sequence_and_path_b_keeps_flash_fail_closed() {
    use ghost_launcher::components::gatekeeper_policy::{
        build_assessment_from_features, evaluate_policy_from_assessment, PolicyEvaluationContext,
    };

    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();

    let mut gatekeeper_config = GatekeeperV2Config::default();
    gatekeeper_config.v25.shadow_enabled = true;
    gatekeeper_config.pdd.enabled = true;
    gatekeeper_config.pdd.spike_detection_enabled = true;
    gatekeeper_config.pdd.ramping_detection_enabled = true;
    gatekeeper_config.pdd.flash_crash_protection_enabled = true;
    gatekeeper_config.tas.enabled = false;
    gatekeeper_config.tas.tas_min_tx_per_segment = 0;
    gatekeeper_config.tas.tas_min_total_duration_ms = 2_000;

    let session = open_session_with_deadline_and_gatekeeper_config(
        &manager,
        pool_id,
        base_mint,
        bonding_curve,
        10_000,
        20_000,
        gatekeeper_config.clone(),
    );

    let features = {
        let mut guard = session.write();
        guard.gatekeeper_buffer_mut().set_registered_wall_t0(10_000);
        guard
            .gatekeeper_buffer_mut()
            .set_deadline_wall_ts_ms(20_000);
        for i in 0..9 {
            let _ = guard
                .gatekeeper_buffer_mut()
                .ingest_transaction_tracking_only(test_tx(
                    pool_id,
                    &format!("sig-seq-{}", i),
                    10_100 + i as u64 * 500,
                ));
        }
        guard.materialize_features()
    };

    assert!(
        features.tx_segment_sequence.is_some(),
        "session materialization must carry tx_segment_sequence for Path B"
    );
    let seq = features
        .tx_segment_sequence
        .as_ref()
        .expect("segment sequence must be present");
    assert!(seq.total_duration_ms >= gatekeeper_config.tas.tas_min_total_duration_ms);

    let mut assessment = build_assessment_from_features(
        features,
        &gatekeeper_config,
        PolicyEvaluationContext::default(),
    );
    assert_eq!(
        assessment.pdd_sequence_signals_availability(&gatekeeper_config),
        (Some(false), Some("pdd_flash_crash_unavailable".to_string()))
    );

    assessment.decision = Some(evaluate_policy_from_assessment(
        &assessment,
        &gatekeeper_config,
    ));
    assessment.cache_v25_confidence(&gatekeeper_config);
    assert_eq!(
        assessment.v25_confidence_availability(&gatekeeper_config),
        (Some(false), Some("pdd_flash_crash_unavailable".to_string()))
    );
}

#[test]
fn session_materializes_v3_organic_broadening_from_segment_sequence() {
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();

    let mut gatekeeper_config = GatekeeperV2Config::default();
    gatekeeper_config.mode = ghost_brain::config::GatekeeperMode::Long;
    gatekeeper_config.tas.tas_min_tx_per_segment = 3;
    gatekeeper_config.tas.tas_min_total_duration_ms = 2_000;

    let session = open_session_with_deadline_and_gatekeeper_config(
        &manager,
        pool_id,
        base_mint,
        bonding_curve,
        10_000,
        20_000,
        gatekeeper_config,
    );

    let features = {
        let mut guard = session.write();
        for i in 0..9 {
            let _ = guard.ingest_transaction(test_tx(
                pool_id,
                &format!("sig-v3-organic-{}", i),
                10_100 + i as u64 * 500,
            ));
        }
        guard.materialize_features()
    };

    assert!(features.organic_broadening.sequence_available);
    assert_eq!(features.organic_broadening.t0_tx_count, 3);
    assert_eq!(features.organic_broadening.t1_tx_count, 3);
    assert_eq!(features.organic_broadening.t2_tx_count, 3);
    assert_eq!(features.organic_broadening.t0_unique_signers, 3);
    assert_eq!(features.organic_broadening.t1_unique_signers, 3);
    assert_eq!(features.organic_broadening.t2_unique_signers, 3);
    assert_eq!(
        features.evidence_status.tx_segments.status,
        EvidenceStatus::Clean
    );
    assert_eq!(
        features.evidence_status.pdd_sequence.status,
        EvidenceStatus::Clean
    );
    assert_eq!(
        features.evidence_status.organic_broadening.status,
        EvidenceStatus::Clean
    );
    assert_eq!(features.organic_broadening.status, EvidenceStatus::Clean);
    assert!(features.organic_broadening.broadening_score > 0.0);
}

#[test]
fn session_materializes_v3_missing_inputs_as_non_clean_evidence() {
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let session = open_session(&manager, pool_id, base_mint, bonding_curve, 11_000);

    let features = session.read().materialize_features();

    assert_eq!(
        features.evidence_status.tx_segments.status,
        EvidenceStatus::Unavailable
    );
    assert_eq!(
        features.evidence_status.tx_segments.unavailable_reasons,
        vec![EvidenceUnavailableReason::SegmentSequenceMissing]
    );
    assert_eq!(
        features.evidence_status.pdd_sequence.unavailable_reasons,
        vec![EvidenceUnavailableReason::PddSequenceMissing]
    );
    assert_eq!(
        features.evidence_status.organic_broadening.status,
        EvidenceStatus::Unavailable
    );
    assert_eq!(
        features.evidence_status.execution.status,
        EvidenceStatus::ShadowOnly
    );
    assert_eq!(
        features.evidence_status.alpha.status,
        EvidenceStatus::Unavailable
    );
    assert_eq!(
        features.evidence_status.alpha.unavailable_reasons,
        vec![EvidenceUnavailableReason::AlphaFingerprintMissing]
    );
    assert_eq!(
        features.evidence_status.execution.unavailable_reasons,
        vec![EvidenceUnavailableReason::ExecutionNotRun]
    );
}

#[test]
fn session_metadata_observation_duration_does_not_mix_event_and_wall_time() {
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let session = open_session(&manager, pool_id, base_mint, bonding_curve, 11_000);

    let features = {
        let mut guard = session.write();
        // Force a huge wall-clock elapsed interval while feeding a tiny event-time
        // tx timestamp. The old implementation mixed these two domains and could
        // collapse observation duration to zero.
        guard.gatekeeper_buffer_mut().set_registered_wall_t0(0);
        let _ = guard.ingest_transaction(test_tx(pool_id, "sig-mixed-time", 10));
        guard.materialize_features()
    };

    assert_eq!(
        features.session_metadata.observation_duration_ms,
        GatekeeperV2Config::default().max_wait_time_ms,
        "observation duration should be sourced from the buffer's capped wall-clock window"
    );
}

#[test]
fn session_materialization_keeps_curve_known_after_trailing_unknown_curve_point() {
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let session = open_session(&manager, pool_id, base_mint, bonding_curve, 12_000);

    let features = {
        let mut guard = session.write();
        let _ = guard.ingest_transaction(test_curve_tx(
            pool_id,
            "curve-known-1",
            12_010,
            30.0,
            1_000_000_000.0,
        ));
        let mut authoritative_curve_tx =
            (*test_curve_tx(pool_id, "curve-known-2", 12_100, 31.0, 966_700_000.0)).clone();
        authoritative_curve_tx.curve_finality = CurveFinality::Finalized;
        let _ = guard.ingest_transaction(Arc::new(authoritative_curve_tx));

        let mut trailing_unknown = (*test_curve_tx(
            pool_id,
            "curve-unknown-after-known",
            12_180,
            32.0,
            950_000_000.0,
        ))
        .clone();
        trailing_unknown.curve_data_known = false;
        trailing_unknown.curve_finality = CurveFinality::Speculative;
        let _ = guard.ingest_transaction(Arc::new(trailing_unknown));

        let curve_dynamics = guard.gatekeeper_buffer().current_curve_dynamics();
        assert!(curve_dynamics.curve_data_known);
        assert!(
            curve_dynamics.bonding_progress_pct > 0.0,
            "gatekeeper buffer should retain authoritative bonding progress before materialization, got {}",
            curve_dynamics.bonding_progress_pct
        );

        guard.materialize_features()
    };

    assert!(
        features.curve_readiness.curve_data_known,
        "a later unknown sample must not erase previously known curve readiness"
    );
    assert!(
        features.account_features.market_cap_sol > 0.0,
        "curve-derived market cap should remain populated after a trailing unknown point"
    );
    assert!(
        features.account_features.bonding_progress > 0.0,
        "bonding progress should remain available after a trailing unknown point, got {}",
        features.account_features.bonding_progress
    );
}
