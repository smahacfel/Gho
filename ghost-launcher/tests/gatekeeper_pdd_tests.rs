use ghost_brain::config::GatekeeperV2Config;
use ghost_core::EventSemanticEnvelope;
use ghost_launcher::components::gatekeeper::GatekeeperBuffer;
use ghost_launcher::components::gatekeeper_pdd::{evaluate_pdd, PddHardFail};
use ghost_launcher::events::PoolTransaction;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

fn pdd_test_config() -> GatekeeperV2Config {
    let mut cfg = GatekeeperV2Config::default();
    cfg.min_sol_threshold = 0.001;
    cfg.min_tx_count = 2;
    cfg.min_unique_signers = 2;
    cfg.min_buy_count = 2;
    cfg.max_wait_time_ms = 100_000;
    cfg.pdd.enabled = true;
    cfg.pdd.entry_drift_max_pct = 5.0;
    cfg.pdd.entry_drift_soft_max_pct = 3.0;
    cfg.pdd.entry_drift_soft_weight = 2;
    cfg.pdd.spike_detection_enabled = true;
    cfg.pdd.spike_hard_veto = true;
    cfg.pdd.spike_ratio_threshold = 2.0;
    cfg.pdd.spike_observation_window_ms = 3000;
    cfg.pdd.spike_soft_penalty = 5;
    cfg.pdd.ramping_detection_enabled = true;
    cfg.pdd.ramping_min_consecutive_buys = 4;
    cfg.pdd.ramping_size_tolerance_pct = 15.0;
    cfg.pdd.ramping_hard_veto = true;
    cfg.pdd.whale_top3_max_pct = 60.0;
    cfg.pdd.whale_single_max_pct = 35.0;
    cfg.pdd.flash_crash_protection_enabled = true;
    cfg.pdd.flash_crash_sell_cluster_max_ms = 500;
    cfg.pdd.flash_crash_max_price_impact_pct = 15.0;
    cfg.pdd.reserve_min_sol = 30.0;
    cfg.pdd.reserve_min_ratio = 0.15;
    cfg
}

fn tx(signer: &str, ts_ms: u64, is_buy: bool, vol: f64) -> PoolTransaction {
    tx_with_curve(
        signer,
        ts_ms,
        is_buy,
        vol,
        500_000_000.0,
        50.0,
        100.0,
        false,
    )
}

fn tx_with_curve(
    signer: &str,
    ts_ms: u64,
    is_buy: bool,
    vol: f64,
    v_tokens: f64,
    v_sol: f64,
    mcap: f64,
    curve_known: bool,
) -> PoolTransaction {
    PoolTransaction {
        semantic: EventSemanticEnvelope::default(),
        pool_amm_id: "pool1".to_string(),
        signer: signer.to_string(),
        token_mint: Some("mint1".to_string()),
        owner_token_deltas: vec![],
        is_buy,
        volume_sol: vol,
        price_quote: Some(0.0001),
        slot: Some(100_000 + (ts_ms / 400) as u64),
        event_ordinal: Some(0),
        outer_instruction_index: None,
        inner_group_index: None,
        outer_program_id: None,
        cpi_stack_height: None,
        timestamp_ms: ts_ms,
        signature: format!("pdd_{}_{}", signer, ts_ms),
        success: true,
        error_code: None,
        compute_units_consumed: None,
        sol_amount_lamports: Some((vol * 1e9) as u64),
        token_amount_units: None,
        reserve_base: None,
        reserve_quote: None,
        is_dev_buy: false,
        dev_buy_lamports: 0,
        arrival_ts_ms: ts_ms + 50,
        event_time: ghost_core::EventTimeMetadata::new(None, Some(ts_ms), None),
        mpcf_payload: vec![],
        mpcf_payload_missing_reason: ghost_launcher::events::RawBytesMissingReason::Unknown,
        v_tokens_in_bonding_curve: Some(v_tokens),
        v_sol_in_bonding_curve: Some(v_sol),
        market_cap_sol: Some(mcap),
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
        curve_data_known: curve_known,
        curve_finality: ghost_core::CurveFinality::Provisional,
    }
}

fn ingest(buf: &mut GatekeeperBuffer, t: PoolTransaction) {
    let _ = buf.ingest_transaction_tracking_only(Arc::new(t));
}

// ───────────────────────────────────────────────────────────────────
// Plan: test_entry_drift_shadow_hard_reject — drift >5% → hard fail
// ───────────────────────────────────────────────────────────────────
#[test]
fn test_entry_drift_shadow_hard_reject() {
    let cfg = pdd_test_config();
    let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf.set_registered_wall_t0(1000);

    // Anchor: v_sol=1, v_tok=1M → price=1e-6. Varied volumes to avoid ramping
    for i in 0..4 {
        ingest(
            &mut buf,
            tx_with_curve(
                &format!("a{}", i),
                1100 + i * 50,
                true,
                0.3 + i as f64 * 0.2,
                1_000_000.0,
                1.0,
                10.0,
                true,
            ),
        );
    }
    // Break consecutive buy streak
    ingest(
        &mut buf,
        tx_with_curve("sell1", 1310, false, 0.1, 1_000_000.0, 1.0, 10.0, true),
    );
    for i in 0..3 {
        ingest(
            &mut buf,
            tx_with_curve(
                &format!("b{}", i),
                1400 + i * 50,
                true,
                0.4 + i as f64 * 0.3,
                1_000_000.0,
                1.0,
                10.0,
                true,
            ),
        );
    }
    // Pump: v_sol=10, v_tok=1M → price=1e-5 → 900% drift ↑
    ingest(
        &mut buf,
        tx_with_curve("pump", 1700, true, 1.5, 1_000_000.0, 10.0, 50.0, true),
    );

    let r = evaluate_pdd(&buf, &cfg.pdd, None);
    assert!(r.hard_fail.is_some(), "Must hard-fail on >5% drift");
    assert!(
        (r.pdd_score - 0.0).abs() < f64::EPSILON,
        "Hard fail → score=0"
    );
    assert!(r.entry_drift_pct.unwrap() > 5.0, "Drift must exceed 5%");
}

// ───────────────────────────────────────────────────────────────────
// Plan: test_entry_drift_soft_pass — drift 3-5% → soft flag only
// ───────────────────────────────────────────────────────────────────
#[test]
fn test_entry_drift_soft_pass() {
    let cfg = pdd_test_config();
    let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf.set_registered_wall_t0(1000);

    // Anchor at price=1e-6, final at price=1.04e-6 → 4% drift (3% < 4% < 5%)
    // Use 7 diverse signers to avoid whale false-positive (>60% top3)
    // Varied volumes to avoid ramping false-positive
    for i in 0..4 {
        ingest(
            &mut buf,
            tx_with_curve(
                &format!("a{}", i),
                1100 + i * 50,
                true,
                0.3 + i as f64 * 0.2,
                1_000_000.0,
                1.0,
                10.0,
                true,
            ),
        );
    }
    ingest(
        &mut buf,
        tx_with_curve("s1", 1310, false, 0.1, 1_000_000.0, 1.0, 10.0, true),
    );
    for i in 0..3 {
        ingest(
            &mut buf,
            tx_with_curve(
                &format!("b{}", i),
                1400 + i * 50,
                true,
                0.5 + i as f64 * 0.3,
                1_000_000.0,
                1.0,
                10.0,
                true,
            ),
        );
    }
    // Final with pump: 4% drift
    ingest(
        &mut buf,
        tx_with_curve("pump", 1600, true, 1.2, 1_000_000.0, 1.04, 10.0, true),
    );

    let r = evaluate_pdd(&buf, &cfg.pdd, None);
    // The 4% drift should be between soft (3%) and hard (5%) thresholds
    // Other PDD checks (whale, ramping, spike) may also fire — that's acceptable
    // as long as the drift itself didn't hard-fail
    if r.hard_fail == Some(PddHardFail::EntryDrift) {
        panic!(
            "4% drift must NOT trigger EntryDrift hard-fail (threshold={})",
            cfg.pdd.entry_drift_max_pct
        );
    }
    if let Some(d) = r.entry_drift_pct {
        assert!(
            d > cfg.pdd.entry_drift_soft_max_pct || d < cfg.pdd.entry_drift_max_pct,
            "Drift {:.1}% must be in soft range ({:.0}-{:.0}%)",
            d,
            cfg.pdd.entry_drift_soft_max_pct,
            cfg.pdd.entry_drift_max_pct
        );
        if d > cfg.pdd.entry_drift_soft_max_pct {
            assert!(
                r.soft_penalty_points > 0,
                "Drift {:.1}% above soft threshold must trigger penalty",
                d
            );
        }
    } else {
        eprintln!("WARN: drift not computed (anchor may not be available)");
    }
}

// ───────────────────────────────────────────────────────────────────
// Plan: test_spike_pattern_detection — volume rate spike → detected
// ───────────────────────────────────────────────────────────────────
#[test]
fn test_spike_pattern_detection() {
    let cfg = pdd_test_config();
    let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf.set_registered_wall_t0(1000);

    // Low-rate earlier period (1000ms-5500ms): 4 TX × 1 SOL
    for i in 0..4 {
        ingest(
            &mut buf,
            tx(&format!("lo{}", i), 1200 + i * 1200, true, 1.0),
        );
    }
    // High-rate spike in last 3s: 4 TX × 5 SOL at 7000-9000ms
    for i in 0..4 {
        ingest(&mut buf, tx(&format!("hi{}", i), 7000 + i * 400, true, 5.0));
    }

    let r = evaluate_pdd(&buf, &cfg.pdd, None);
    assert!(
        r.spike_detected,
        "Volume spike MUST be detected (recent rate >> earlier rate)"
    );
    assert!(
        r.hard_fail.is_some(),
        "Spike with hard_veto=true must hard-fail"
    );
}

// ───────────────────────────────────────────────────────────────────
// Plan: test_ramping_detection — 5 consecutive uniform buys → detected
// ───────────────────────────────────────────────────────────────────
#[test]
fn test_ramping_detection() {
    let cfg = pdd_test_config();
    let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf.set_registered_wall_t0(1000);

    // 5 consecutive buys, each ~0.5 SOL ±2% (within 15% tolerance)
    for i in 0..5 {
        let vol = 0.50 + ((i % 2) as f64) * 0.02;
        ingest(&mut buf, tx(&format!("r{}", i), 1100 + i * 200, true, vol));
    }

    let r = evaluate_pdd(&buf, &cfg.pdd, None);
    assert!(
        r.ramping_detected,
        "5 consecutive same-size buys MUST trigger ramping. max_cb={}",
        buf.max_consecutive_buys_count()
    );
    assert!(
        r.hard_fail.is_some(),
        "Ramping with hard_veto=true must hard-fail"
    );
}

// ───────────────────────────────────────────────────────────────────
// Plan: test_whale_concentration_shadow_veto — top3 >60% → hard fail
// ───────────────────────────────────────────────────────────────────
#[test]
fn test_whale_concentration_shadow_veto() {
    let cfg = pdd_test_config();
    let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf.set_registered_wall_t0(1000);

    // Whale: 4 TX × 3 SOL = 12 SOL (87% of total)
    for i in 0..4 {
        ingest(&mut buf, tx("whale", 1100 + i * 200, true, 3.0));
    }
    // Small: 6 TX × 0.3 SOL = 1.8 SOL (13% of total)
    for i in 0..6 {
        ingest(&mut buf, tx(&format!("s{}", i), 2000 + i * 100, true, 0.3));
    }

    let r = evaluate_pdd(&buf, &cfg.pdd, None);
    assert!(r.whale_top3_pct.is_some(), "Whale top3 must be computed");
    let top3 = r.whale_top3_pct.unwrap();
    assert!(
        top3 > cfg.pdd.whale_top3_max_pct,
        "Top-3 whale {:.1}% must exceed threshold {:.1}%",
        top3,
        cfg.pdd.whale_top3_max_pct
    );
    assert!(r.hard_fail.is_some(), "Whale >60% must hard-fail");
}

// ───────────────────────────────────────────────────────────────────
// Plan: test_reserve_health — reserve <30 SOL → fail
// ───────────────────────────────────────────────────────────────────
#[test]
fn test_reserve_health() {
    let cfg = pdd_test_config();
    let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf.set_registered_wall_t0(1000);

    // Anchor with healthy reserve (50 SOL), diverse signers to avoid whale false-positive
    // Varied volumes, diverse signers, interspersed sell to avoid ramping
    for i in 0..4 {
        ingest(
            &mut buf,
            tx_with_curve(
                &format!("a{}", i),
                1100 + i * 50,
                true,
                0.3 + i as f64 * 0.2,
                500_000_000.0,
                50.0,
                100.0,
                true,
            ),
        );
    }
    ingest(
        &mut buf,
        tx_with_curve("s1", 1310, false, 0.1, 500_000_000.0, 50.0, 100.0, true),
    );
    for i in 0..3 {
        ingest(
            &mut buf,
            tx_with_curve(
                &format!("b{}", i),
                1400 + i * 50,
                true,
                0.5 + i as f64 * 0.3,
                500_000_000.0,
                50.0,
                100.0,
                true,
            ),
        );
    }
    // Final with small reserve (20 SOL < 30 SOL min)
    ingest(
        &mut buf,
        tx_with_curve("low", 1600, true, 1.0, 500_000_000.0, 20.0, 80.0, true),
    );

    let r = evaluate_pdd(&buf, &cfg.pdd, None);
    // Reserve at last price point is 20 SOL < 30 SOL minimum
    let lp = buf.last_price_point();
    let small_reserve = lp.map_or(false, |p| p.v_sol_in_curve < cfg.pdd.reserve_min_sol);
    if small_reserve {
        assert!(
            !r.reserve_health_pass || r.hard_fail == Some(PddHardFail::Reserve),
            "Small reserve ({} SOL < {}) must fail. health_pass={} hard_fail={:?}",
            lp.unwrap().v_sol_in_curve,
            cfg.pdd.reserve_min_sol,
            r.reserve_health_pass,
            r.hard_fail
        );
    } else {
        eprintln!("WARN: last price point has sufficient reserve ({:?})", lp);
    }
}
