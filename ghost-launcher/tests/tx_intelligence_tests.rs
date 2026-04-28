use ghost_brain::config::GatekeeperV2Config;
use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_core::session::types::SessionStatus;
use ghost_core::{CurveFinality, EventSemanticEnvelope};
use ghost_launcher::events::{PoolTransaction, RawBytesMissingReason};
use ghost_launcher::session::{OpenSessionRequest, SessionConfig, SessionManager};
use ghost_launcher::tx_intelligence::{
    TxIntelligenceConfig, TxIntelligenceEngine, DEFAULT_SESSION_TX_RING_CAPACITY,
};
use seer::early_fingerprint::EarlyFingerprintConfig;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn candidate(pool_id: Pubkey, base_mint: Pubkey, bonding_curve: Pubkey) -> EnhancedCandidate {
    let mut candidate = EnhancedCandidate::default();
    candidate.pool_amm_id = pool_id;
    candidate.base_mint = base_mint;
    candidate.bonding_curve = bonding_curve;
    candidate.slot = Some(1);
    candidate.timestamp = 1_000;
    candidate
}

#[allow(clippy::too_many_arguments)]
fn test_tx(
    pool_id: Pubkey,
    signer: Pubkey,
    signature: &str,
    ordinal: u32,
    timestamp_ms: u64,
    is_buy: bool,
    volume_sol: f64,
    is_dev_buy: bool,
) -> PoolTransaction {
    PoolTransaction {
        semantic: EventSemanticEnvelope::default(),
        pool_amm_id: pool_id.to_string(),
        slot: Some(1),
        event_ordinal: Some(ordinal),
        outer_instruction_index: None,
        inner_group_index: None,
        outer_program_id: None,
        cpi_stack_height: None,
        timestamp_ms,
        event_time: ghost_core::EventTimeMetadata::new(None, Some(timestamp_ms), None),
        arrival_ts_ms: timestamp_ms,
        signer: signer.to_string(),
        is_buy,
        volume_sol,
        sol_amount_lamports: Some((volume_sol * 1_000_000_000.0) as u64),
        token_amount_units: Some((volume_sol * 1_000_000.0) as u64),
        reserve_base: None,
        reserve_quote: None,
        price_quote: None,
        is_dev_buy,
        dev_buy_lamports: if is_dev_buy {
            (volume_sol * 1_000_000_000.0) as u64
        } else {
            0
        },
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
    }
}

fn make_engine(config: GatekeeperV2Config, dev_wallet: Option<Pubkey>) -> TxIntelligenceEngine {
    let pool_id = Pubkey::new_unique();
    TxIntelligenceEngine::new(
        TxIntelligenceConfig::from_gatekeeper_config(&config, EarlyFingerprintConfig::default()),
        &candidate(pool_id, Pubkey::new_unique(), Pubkey::new_unique()),
        dev_wallet,
    )
}

#[test]
fn engine_computes_hhi_gini_and_volume_cv() {
    let pool_id = Pubkey::new_unique();
    let signer_a = Pubkey::new_unique();
    let signer_b = Pubkey::new_unique();
    let mut engine = make_engine(GatekeeperV2Config::default(), None);

    engine.on_transaction(&test_tx(
        pool_id, signer_a, "sig-a", 0, 1_000, true, 1.0, false,
    ));
    engine.on_transaction(&test_tx(
        pool_id, signer_b, "sig-b", 1, 1_300, true, 1.0, false,
    ));
    engine.on_transaction(&test_tx(
        pool_id, signer_b, "sig-c", 2, 1_900, false, 2.0, false,
    ));

    let features = engine.compute_features();

    assert_eq!(features.tx_count, 3);
    assert_eq!(features.unique_signers, 2);
    assert!((features.hhi - (5.0 / 9.0)).abs() < 1e-9);
    assert!((features.volume_gini - 0.25).abs() < 1e-9);
    assert!((features.volume_cv - 0.3535533905).abs() < 1e-6);
    assert!((features.interval_cv - (1.0 / 3.0)).abs() < 1e-9);
}

#[test]
fn engine_sets_dev_sell_hard_flag() {
    let pool_id = Pubkey::new_unique();
    let dev_wallet = Pubkey::new_unique();
    let mut engine = make_engine(GatekeeperV2Config::default(), Some(dev_wallet));

    engine.on_transaction(&test_tx(
        pool_id,
        dev_wallet,
        "sig-dev-buy",
        0,
        1_000,
        true,
        1.5,
        true,
    ));
    engine.on_transaction(&test_tx(
        pool_id,
        dev_wallet,
        "sig-dev-sell",
        1,
        1_600,
        false,
        0.5,
        false,
    ));

    let (features, flags) = engine.snapshot();

    assert!(features.dev_has_sold);
    assert!(
        flags.iter().any(|flag| flag.flag_id == "dev_has_sold"),
        "expected dev_has_sold hard flag"
    );
}

#[test]
fn engine_detects_burst_ratio_and_flag() {
    let pool_id = Pubkey::new_unique();
    let signer_a = Pubkey::new_unique();
    let signer_b = Pubkey::new_unique();
    let signer_c = Pubkey::new_unique();
    let signer_d = Pubkey::new_unique();
    let signer_e = Pubkey::new_unique();

    let mut config = GatekeeperV2Config::default();
    config.max_burst_ratio = 0.5;
    let mut engine = make_engine(config, None);

    engine.on_transaction(&test_tx(
        pool_id, signer_a, "sig-1", 0, 1_000, true, 0.2, false,
    ));
    engine.on_transaction(&test_tx(
        pool_id, signer_b, "sig-2", 1, 1_100, true, 0.2, false,
    ));
    engine.on_transaction(&test_tx(
        pool_id, signer_c, "sig-3", 2, 1_200, true, 0.2, false,
    ));
    engine.on_transaction(&test_tx(
        pool_id, signer_d, "sig-4", 3, 4_000, true, 0.2, false,
    ));
    engine.on_transaction(&test_tx(
        pool_id, signer_e, "sig-5", 4, 8_000, true, 0.2, false,
    ));

    let (features, flags) = engine.snapshot();

    assert!((features.burst_ratio - 0.6).abs() < 1e-9);
    assert_eq!(engine.state().burst_windows.len(), 1);
    assert!(
        flags.iter().any(|flag| flag.flag_id == "high_burst_ratio"),
        "expected high_burst_ratio flag"
    );
}

#[test]
fn engine_emits_concentration_and_timing_hard_flags() {
    let pool_id = Pubkey::new_unique();
    let signer = Pubkey::new_unique();
    let mut engine = make_engine(GatekeeperV2Config::default(), None);

    engine.on_transaction(&test_tx(
        pool_id, signer, "sig-1", 0, 1_000, true, 0.3, false,
    ));
    engine.on_transaction(&test_tx(
        pool_id, signer, "sig-2", 1, 1_010, true, 0.3, false,
    ));
    engine.on_transaction(&test_tx(
        pool_id, signer, "sig-3", 2, 1_020, true, 0.3, false,
    ));

    let flags = engine.get_risk_flags();

    assert!(flags
        .iter()
        .any(|flag| flag.flag_id == "extreme_bot_timing"));
    assert!(flags
        .iter()
        .any(|flag| flag.flag_id == "extreme_signer_concentration"));
}

#[test]
fn engine_handles_100tx_stream_consistently() {
    let pool_id = Pubkey::new_unique();
    let signers: Vec<Pubkey> = (0..10).map(|_| Pubkey::new_unique()).collect();
    let mut engine = make_engine(GatekeeperV2Config::default(), None);

    for i in 0..100u32 {
        let signer = signers[(i as usize) % signers.len()];
        let is_buy = i % 5 != 0;
        engine.on_transaction(&test_tx(
            pool_id,
            signer,
            &format!("sig-{i}"),
            i,
            1_000 + u64::from(i) * 100,
            is_buy,
            0.25,
            false,
        ));
    }

    let features = engine.compute_features();

    assert_eq!(features.tx_count, 100);
    assert_eq!(features.unique_signers, 10);
    assert_eq!(features.buy_count, 80);
    assert_eq!(features.sell_count, 20);
    assert!((features.buy_ratio - 0.8).abs() < 1e-9);
    assert!((features.avg_interval_ms - 100.0).abs() < 1e-9);
    assert!((features.avg_tx_per_signer - 10.0).abs() < 1e-9);
}

#[test]
fn session_tx_buffer_is_bounded() {
    let manager = SessionManager::new(SessionConfig {
        default_observation_duration_ms: 100,
        max_sessions: 8,
        ..SessionConfig::default()
    });
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let mut gatekeeper_config = GatekeeperV2Config::default();
    gatekeeper_config.min_tx_count = DEFAULT_SESSION_TX_RING_CAPACITY * 2;
    gatekeeper_config.min_unique_signers = DEFAULT_SESSION_TX_RING_CAPACITY * 2;
    gatekeeper_config.min_buy_count = DEFAULT_SESSION_TX_RING_CAPACITY * 2;
    gatekeeper_config.max_wait_time_ms = 10_000;

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_millis() as u64;

    manager
        .open_session(OpenSessionRequest {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            dev_wallet: Some(Pubkey::new_unique()),
            candidate_snapshot: candidate(pool_id, base_mint, bonding_curve),
            created_at_wall_ms: now_ms,
            deadline_wall_ms: Some(now_ms + 10_000),
            gatekeeper_config,
            fingerprint_config: EarlyFingerprintConfig::default(),
        })
        .expect("session should open");
    let session = manager
        .get_session(&pool_id)
        .expect("session should be retrievable");

    for i in 0..(DEFAULT_SESSION_TX_RING_CAPACITY + 5) as u32 {
        let signer = Pubkey::new_unique();
        let tx = Arc::new(test_tx(
            pool_id,
            signer,
            &format!("sig-buffer-{i}"),
            i,
            now_ms + u64::from(i),
            true,
            0.2,
            false,
        ));
        let _ = session.write().ingest_transaction(tx);
    }

    let guard = session.read();
    assert!(matches!(
        guard.get_status(),
        SessionStatus::Accumulating | SessionStatus::Evaluating
    ));
    assert_eq!(guard.tx_buffer.len(), DEFAULT_SESSION_TX_RING_CAPACITY);
    assert_eq!(
        guard.tx_buffer.front().map(|tx| tx.signature.as_str()),
        Some("sig-buffer-5")
    );
}

#[test]
fn tx_intelligence_engine_has_no_account_state_imports() {
    let source = include_str!("../src/tx_intelligence/engine.rs");
    assert!(!source.contains("AccountStateCore"));
    assert!(!source.contains("AccountStateReducer"));
    assert!(!source.contains("account_state_core"));
}
