use ghost_brain::config::{GatekeeperMode, GatekeeperV2Config};
use ghost_core::EventSemanticEnvelope;
use ghost_launcher::components::gatekeeper::GatekeeperBuffer;
use ghost_launcher::components::gatekeeper_trajectory::{build_segment, score_trajectory};
use ghost_launcher::events::PoolTransaction;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

fn tas_enabled_config() -> GatekeeperV2Config {
    let mut cfg = GatekeeperV2Config::default();
    cfg.mode = GatekeeperMode::Long;
    cfg.min_tx_count = 3;
    cfg.min_unique_signers = 2;
    cfg.min_buy_count = 2;
    cfg.tas.enabled = true;
    cfg.tas.tas_min_tx_per_segment = 2;
    cfg.tas.tas_min_total_duration_ms = 1000;
    cfg
}

fn make_tx(signer: &str, ts_ms: u64, is_buy: bool, vol_sol: f64) -> PoolTransaction {
    PoolTransaction {
        semantic: EventSemanticEnvelope::default(),
        pool_amm_id: "pool1".to_string(),
        signer: signer.to_string(),
        token_mint: Some("mint1".to_string()),
        owner_token_deltas: vec![],
        is_buy,
        volume_sol: vol_sol,
        price_quote: Some(0.0001),
        slot: Some(100_000 + (ts_ms / 400) as u64),
        event_ordinal: Some(0),
        outer_instruction_index: None,
        inner_group_index: None,
        outer_program_id: None,
        cpi_stack_height: None,
        timestamp_ms: ts_ms,
        signature: format!("tas_{}_{}", signer, ts_ms),
        success: true,
        error_code: None,
        compute_units_consumed: None,
        sol_amount_lamports: Some((vol_sol * 1e9) as u64),
        token_amount_units: None,
        reserve_base: None,
        reserve_quote: None,
        is_dev_buy: false,
        dev_buy_lamports: 0,
        arrival_ts_ms: ts_ms + 50,
        event_time: ghost_core::EventTimeMetadata::new(None, Some(ts_ms), None),
        mpcf_payload: vec![],
        mpcf_payload_missing_reason: ghost_launcher::events::RawBytesMissingReason::Unknown,
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
        curve_finality: ghost_core::CurveFinality::Speculative,
    }
}

fn ingest(buffer: &mut GatekeeperBuffer, tx: PoolTransaction) {
    use ghost_launcher::components::gatekeeper::GatekeeperIngressOutcome;
    let _ = buffer.ingest_transaction_tracking_only(Arc::new(tx));
}

#[test]
fn test_momentum_acceleration_positive() {
    let cfg = tas_enabled_config();
    let pool = Pubkey::new_unique();
    let mut buf = GatekeeperBuffer::new(pool, &cfg);
    buf.set_registered_wall_t0(1000);

    for i in 0..3 {
        ingest(
            &mut buf,
            make_tx(&format!("a{}", i), 1100 + i * 300, true, 1.0),
        );
    }
    for i in 0..5 {
        ingest(
            &mut buf,
            make_tx(&format!("b{}", i), 2100 + i * 200, true, 1.0),
        );
    }
    for i in 0..8 {
        ingest(
            &mut buf,
            make_tx(&format!("c{}", i), 3100 + i * 110, true, 1.0),
        );
    }

    let traj = buf.materialize_trajectory(&cfg.tas);
    assert!(traj.is_some(), "Should produce trajectory");
    let t = traj.unwrap();
    assert!(
        (t.momentum_score - 1.0).abs() < f64::EPSILON,
        "Accelerating momentum=1.0, got {:.3}",
        t.momentum_score
    );
    assert!(
        t.overall_tas_score > 0.5,
        "Accelerating pool TAS score should be >0.5, got {:.3}",
        t.overall_tas_score
    );
}

#[test]
fn test_momentum_deceleration_negative() {
    let cfg = tas_enabled_config();
    let pool = Pubkey::new_unique();
    let mut buf = GatekeeperBuffer::new(pool, &cfg);
    buf.set_registered_wall_t0(1000);

    for i in 0..8 {
        ingest(
            &mut buf,
            make_tx(&format!("a{}", i), 1100 + i * 110, true, 1.0),
        );
    }
    for i in 0..5 {
        ingest(
            &mut buf,
            make_tx(&format!("b{}", i), 2100 + i * 200, true, 1.0),
        );
    }
    for i in 0..3 {
        ingest(
            &mut buf,
            make_tx(&format!("c{}", i), 3100 + i * 300, true, 1.0),
        );
    }

    let traj = buf.materialize_trajectory(&cfg.tas);
    assert!(traj.is_some());
    let t = traj.unwrap();
    assert!(
        (t.momentum_score - 0.0).abs() < f64::EPSILON,
        "Decelerating momentum=0.0, got {:.3}",
        t.momentum_score
    );
    assert!(
        t.overall_tas_score < 0.5,
        "Decelerating pool TAS <0.5, got {:.3}",
        t.overall_tas_score
    );
}

#[test]
fn test_hhi_decline_during_observation() {
    let cfg = tas_enabled_config();
    let seg0 = build_segment(&[
        &make_tx("whale", 1100, true, 1.0),
        &make_tx("whale", 1200, true, 1.0),
        &make_tx("whale", 1300, true, 1.0),
        &make_tx("whale", 1400, true, 1.0),
    ]);
    let seg1 = build_segment(&[
        &make_tx("w1", 2100, true, 1.0),
        &make_tx("w2", 2200, true, 1.0),
        &make_tx("w3", 2300, true, 1.0),
        &make_tx("w4", 2400, true, 1.0),
    ]);
    let seg2 = build_segment(&[
        &make_tx("a", 3100, true, 1.0),
        &make_tx("b", 3200, true, 1.0),
        &make_tx("c", 3300, true, 1.0),
        &make_tx("d", 3400, true, 1.0),
    ]);

    assert!(
        (seg0.hhi - 1.0).abs() < f64::EPSILON,
        "T0 single signer → HHI=1.0"
    );
    assert!((seg2.hhi - 0.25).abs() < 0.01, "T2 4 signers → HHI=0.25");

    let result = score_trajectory(&seg0, &seg1, &seg2, &cfg.tas);
    assert!(
        (result.hhi_score - 1.0).abs() < f64::EPSILON,
        "Declining HHI → hhi_score=1.0, got {:.3}",
        result.hhi_score
    );
}

#[test]
fn test_volume_spike_detection() {
    let cfg = tas_enabled_config();
    let seg0 = build_segment(&[
        &make_tx("a", 1100, true, 1.0),
        &make_tx("b", 1200, true, 1.0),
    ]);
    let seg1 = build_segment(&[
        &make_tx("c", 2100, true, 10.0),
        &make_tx("d", 2200, true, 10.0),
    ]);
    let seg2 = build_segment(&[
        &make_tx("e", 3100, true, 1.0),
        &make_tx("f", 3200, true, 1.0),
    ]);

    let result = score_trajectory(&seg0, &seg1, &seg2, &cfg.tas);
    assert!(
        result.volume_score < 0.6,
        "Volume spike → low volume_score, got {:.3}",
        result.volume_score
    );
}
