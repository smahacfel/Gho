use ghost_brain::config::{GatekeeperMode, GatekeeperV2Config};
use ghost_core::EventSemanticEnvelope;
use ghost_launcher::components::gatekeeper::{
    AlphaGateDiagnostics, GatekeeperAssessment, GatekeeperBuffer, GatekeeperDecision,
    GatekeeperVerdict, GatekeeperVerdictType, ObservationStage, ProsperityFilterDiagnostics,
    ShadowCheckpointSource, ShadowDecisionKind, SoftSignals, SybilPolicyDiagnostics,
};
use ghost_launcher::components::gatekeeper_pdd::evaluate_pdd;
use ghost_launcher::events::PoolTransaction;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::Duration;

fn v25_enabled_config() -> GatekeeperV2Config {
    let mut cfg = GatekeeperV2Config::default();
    cfg.mode = GatekeeperMode::Long;
    cfg.min_sol_threshold = 0.001;
    cfg.min_tx_count = 3;
    cfg.min_unique_signers = 3;
    cfg.min_buy_count = 3;
    cfg.max_wait_time_ms = 100_000;
    cfg.v25.shadow_enabled = true;
    cfg.dow.enabled = true;
    cfg.pdd.enabled = true;
    cfg.pdd.entry_drift_max_pct = 5.0;
    cfg.pdd.spike_detection_enabled = true;
    cfg.pdd.spike_hard_veto = true;
    cfg.pdd.spike_ratio_threshold = 2.0;
    cfg.pdd.spike_observation_window_ms = 3000;
    cfg.pdd.ramping_detection_enabled = true;
    cfg.pdd.ramping_min_consecutive_buys = 4;
    cfg.pdd.ramping_hard_veto = true;
    cfg.pdd.whale_top3_max_pct = 60.0;
    cfg.pdd.flash_crash_protection_enabled = true;
    cfg.pdd.flash_crash_sell_cluster_max_ms = 500;
    cfg.pdd.reserve_min_sol = 30.0;
    cfg.tas.enabled = true;
    cfg.tas.tas_min_tx_per_segment = 2;
    cfg.tas.tas_min_total_duration_ms = 2000;
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
        tx_index: None,
        outer_instruction_index: None,
        inner_group_index: None,
        outer_program_id: None,
        cpi_stack_height: None,
        timestamp_ms: ts_ms,
        signature: format!("reg_{}_{}", signer, ts_ms),
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
        curve_data_known: curve_known,
        curve_finality: ghost_core::CurveFinality::Provisional,
    }
}

fn ingest(buf: &mut GatekeeperBuffer, t: PoolTransaction) {
    let _ = buf.ingest_transaction_tracking_only(Arc::new(t));
}

fn final_assessment(verdict: GatekeeperVerdict) -> GatekeeperAssessment {
    match verdict {
        GatekeeperVerdict::Buy { assessment, .. }
        | GatekeeperVerdict::Reject { assessment, .. }
        | GatekeeperVerdict::Timeout { assessment } => assessment,
        _other => panic!("expected terminal verdict, got non-terminal variant"),
    }
}

fn stage_decision<'a>(
    buf: &'a GatekeeperBuffer,
    stage: ObservationStage,
) -> &'a ghost_launcher::components::gatekeeper::ShadowV25Decision {
    buf.v25_shadow_decisions()
        .iter()
        .find(|decision| decision.window == stage)
        .unwrap_or_else(|| panic!("expected {stage:?} shadow decision"))
}

// ───────────────────────────────────────────────────────────────────
// test_v25_vs_historical_losing_pools
//
// Plan: "26.27% drift -> shadow_reject_entry_drift_extreme"
// Simulates a extreme pump pool (26%+ drift) found in historical
// losing pools and verifies PDD hard-fails it.
// ───────────────────────────────────────────────────────────────────
#[test]
fn test_v25_vs_historical_losing_pools() {
    let cfg = v25_enabled_config();
    let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf.set_registered_wall_t0(1000);

    // Anchor: v_sol=1.0, v_tokens=1M → price=1e-6. Many small signers for diversity.
    for i in 0..4 {
        ingest(
            &mut buf,
            tx_with_curve(
                &format!("a{}", i),
                1100 + i as u64 * 50,
                true,
                0.3 + i as f64 * 0.1,
                1_000_000.0,
                1.0,
                10.0,
                true,
            ),
        );
    }
    // Sell to break streak
    ingest(
        &mut buf,
        tx_with_curve("sell", 1310, false, 0.1, 1_000_000.0, 1.0, 10.0, true),
    );
    for i in 0..4 {
        ingest(
            &mut buf,
            tx_with_curve(
                &format!("b{}", i),
                1400 + i as u64 * 50,
                true,
                0.4 + i as f64 * 0.1,
                1_000_000.0,
                1.0,
                10.0,
                true,
            ),
        );
    }
    // ── EXTREME PUMP: v_sol=30 → price=3e-5 → 2900% drift! ──
    // This is the losing-pool signature: price pumped way up before entry.
    ingest(
        &mut buf,
        tx_with_curve("PUMP", 1800, true, 2.0, 1_000_000.0, 30.0, 300.0, true),
    );
    ingest(
        &mut buf,
        tx_with_curve("PUMP2", 1900, true, 3.0, 1_000_000.0, 31.0, 310.0, true),
    );

    let r = evaluate_pdd(&buf, &cfg.pdd, None);

    // Hard assertion: losing pool with 2900% drift MUST be rejected by PDD
    assert!(
        r.hard_fail.is_some(),
        "LOSING POOL: PDD must hard-fail on 2900% entry drift. hard_fail={:?} drift={:?}",
        r.hard_fail,
        r.entry_drift_pct
    );
    assert!(
        r.entry_drift_pct.unwrap_or(0.0) > cfg.pdd.entry_drift_max_pct,
        "LOSING POOL: drift must exceed {}% threshold. actual={:.1}%",
        cfg.pdd.entry_drift_max_pct,
        r.entry_drift_pct.unwrap_or(0.0)
    );
    assert!(
        (r.pdd_score - 0.0).abs() < f64::EPSILON,
        "LOSING POOL: score must be 0.0 on hard fail"
    );
}

// ───────────────────────────────────────────────────────────────────
// test_v25_vs_historical_winning_pools
//
// Plan: "Raportuje false-positive: ilu winnerów PDD/TAS odrzuciłby"
// Simulates an organic, healthy winning pool with gradual growth
// and verifies V2.5 does NOT false-positive hard-fail it.
// ───────────────────────────────────────────────────────────────────
#[test]
fn test_v25_vs_historical_winning_pools() {
    let mut cfg = v25_enabled_config();
    cfg.pdd.spike_detection_enabled = false; // organic pools naturally have volume variations
    let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf.set_registered_wall_t0(1000);

    // Organic winning pool: 12 diverse signers, very gradual growth (<2% drift),
    // sells interspersed early and evenly to avoid any spike-like volume patterns.
    // TX distribution: buy, sell, buy, buy, sell, buy, buy, buy, sell, buy, buy, buy
    let pattern = [
        true, false, true, true, false, true, true, true, false, true, true, true,
    ];
    for i in 0..12 {
        ingest(
            &mut buf,
            tx_with_curve(
                &format!("s{}", i),
                1100 + i as u64 * 500,
                pattern[i],
                0.5 + i as f64 * 0.1,
                1_000_000.0,
                50.0 + i as f64 * 0.05,
                100.0 + i as f64 * 0.3,
                true,
            ),
        );
    }

    let r = evaluate_pdd(&buf, &cfg.pdd, None);

    // Hard assertion: organic pool with gradual growth MUST NOT trigger any PDD hard-fail
    assert!(
        r.hard_fail.is_none(),
        "WINNING POOL FALSE-POSITIVE: organic pool must NOT hard-fail PDD. \
        hard_fail={:?} drift={:.1?}% spike={} ramping={} whale={:.1?}%",
        r.hard_fail,
        r.entry_drift_pct,
        r.spike_detected,
        r.ramping_detected,
        r.whale_top3_pct
    );
    assert!(
        r.pdd_score >= 0.85,
        "WINNING POOL: PDD score must be >= 0.85 for organic pool, got {:.3}",
        r.pdd_score
    );
}

// ───────────────────────────────────────────────────────────────────
// test_v25_shadow_does_not_change_live_verdict
//
// Builds two identical GatekeeperBuffers — one with shadow disabled,
// one with shadow enabled (live_execution_enabled=false) — runs
// compute_decision() on both and asserts the live verdict is identical.
// ───────────────────────────────────────────────────────────────────
#[test]
fn test_v25_shadow_does_not_change_live_verdict() {
    // Baseline: V2 config with shadow DISABLED
    let mut cfg_v2 = v25_enabled_config();
    cfg_v2.v25.shadow_enabled = false;
    cfg_v2.min_sol_threshold = 0.001;

    let pool = Pubkey::new_unique();
    let mut buf_v2 = GatekeeperBuffer::new(pool, &cfg_v2);
    buf_v2.set_registered_wall_t0(1000);

    // Ingest identical TXs into both buffers
    let txs: Vec<_> = (0..5)
        .map(|i| {
            tx(
                &format!("buyer{}", i),
                1100 + i as u64 * 300,
                true,
                0.5 + i as f64 * 0.2,
            )
        })
        .collect();

    for t in &txs {
        ingest(&mut buf_v2, t.clone());
    }

    let assessment_v2 = buf_v2.run_assessment();
    let decision_v2 = buf_v2.compute_decision(&assessment_v2);

    // ── V2.5: shadow enabled, live_execution_enabled=false ──
    let mut cfg_v25 = cfg_v2.clone();
    cfg_v25.v25.shadow_enabled = true;
    cfg_v25.v25.live_execution_enabled = false;
    cfg_v25.dow.enabled = true;

    let mut buf_v25 = GatekeeperBuffer::new(pool, &cfg_v25);
    buf_v25.set_registered_wall_t0(1000);

    for t in &txs {
        ingest(&mut buf_v25, t.clone());
    }

    let assessment_v25 = buf_v25.run_assessment();
    let decision_v25 = buf_v25.compute_decision(&assessment_v25);

    // ── THE INVARIANT: live verdict MUST be identical ──
    assert_eq!(
        decision_v2.verdict_buy,
        decision_v25.verdict_buy,
        "V2={} vs V2.5={}: shadow must not change live BUY/REJECT verdict",
        decision_v2.verdict_type.tag(),
        decision_v25.verdict_type.tag()
    );
    assert_eq!(
        decision_v2.verdict_type, decision_v25.verdict_type,
        "Verdict type must be identical regardless of shadow state"
    );

    // Shadow decisions MAY be recorded, but live path is untouched
    if cfg_v25.v25.shadow_enabled {
        // Shadow decisions existing is expected; what matters is verdict equality above
        let _ = &assessment_v25.v25_shadow_decisions;
    }
}

// ───────────────────────────────────────────────────────────────────
// test_v25_all_modules_disabled_by_default
// ───────────────────────────────────────────────────────────────────
#[test]
fn test_v25_all_modules_disabled_by_default() {
    let mut cfg = GatekeeperV2Config::default();
    cfg.min_tx_count = 3;
    cfg.min_unique_signers = 2;
    cfg.min_buy_count = 2;
    cfg.min_sol_threshold = 0.001;

    let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf.set_registered_wall_t0(1000);

    for i in 0..5 {
        ingest(
            &mut buf,
            tx(&format!("s{}", i), 1100 + i as u64 * 300, true, 1.0),
        );
    }

    let assessment = buf.run_assessment();
    assert!(assessment.trajectory.is_none(), "TAS disabled by default");
    assert!(
        assessment.v25_shadow_decisions.is_empty(),
        "No shadow decisions by default"
    );

    let pdd = evaluate_pdd(&buf, &cfg.pdd, None);
    assert!(!pdd.enabled, "PDD disabled by default");
    assert!(pdd.hard_fail.is_none(), "No PDD hard fail when disabled");
    assert!(
        (pdd.pdd_score - 1.0).abs() < f64::EPSILON,
        "Score must be 1.0 when disabled"
    );
    assert!(
        assessment.phase1_passed,
        "V2 Phase 1 must work with V2.5 disabled"
    );
}

#[test]
fn test_v25_shadow_pdd_hard_fail_forces_extended_reject() {
    let cfg = v25_enabled_config();
    let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf.set_registered_wall_t0(1000);

    for i in 0..4 {
        ingest(
            &mut buf,
            tx_with_curve(
                &format!("anchor{}", i),
                1100 + i as u64 * 50,
                true,
                0.3 + i as f64 * 0.1,
                1_000_000.0,
                1.0,
                10.0,
                true,
            ),
        );
    }
    ingest(
        &mut buf,
        tx_with_curve("sell", 1310, false, 0.1, 1_000_000.0, 1.0, 10.0, true),
    );
    for i in 0..4 {
        ingest(
            &mut buf,
            tx_with_curve(
                &format!("buyer{}", i),
                1400 + i as u64 * 50,
                true,
                0.4 + i as f64 * 0.1,
                1_000_000.0,
                1.0,
                10.0,
                true,
            ),
        );
    }
    ingest(
        &mut buf,
        tx_with_curve("pump", 1800, true, 2.0, 1_000_000.0, 30.0, 300.0, true),
    );
    ingest(
        &mut buf,
        tx_with_curve("pump2", 1900, true, 3.0, 1_000_000.0, 31.0, 310.0, true),
    );

    let assessment = final_assessment(buf.force_check_deadline(101_001));
    let extended = assessment
        .v25_shadow_decisions
        .iter()
        .find(|decision| decision.window == ObservationStage::Extended)
        .expect("expected terminal extended shadow decision");

    // P1: v25_confidence is computed only when ALL inputs are available.
    // This test's TX span (800ms) is below tas_min_total_duration_ms (2000ms),
    // so TAS is unavailable → confidence is None even though PDD hard-fails.
    assert_eq!(assessment.v25_confidence, None);
    assert_eq!(extended.kind, ShadowDecisionKind::RejectPumpAndDump);
    assert_eq!(extended.confidence, 0.0);
    assert_ne!(extended.kind.verdict_str(), "BUY");
    // The deadline fallback produces EXTENDED_SHADOW_DEADLINE_FALLBACK_REJECT_PDD_*.
    // The timer/TX path produces PDD_* or EXTENDED_REJECT_PDD_*.
    let reason_valid = extended.reason.contains("REJECT_PDD_")
        || extended.reason.contains("PDD_ENTRY_DRIFT")
        || extended.reason.contains("PDD_SPIKE");
    assert!(
        reason_valid,
        "expected explicit extended PDD reject reason, got {}",
        extended.reason
    );
}

#[test]
fn test_v25_extended_shadow_stays_reject_when_clean_toggle_is_disabled() {
    let mut cfg = v25_enabled_config();
    cfg.dow.extended_require_pdd_clean = false;

    let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf.set_registered_wall_t0(1000);

    for i in 0..4 {
        ingest(
            &mut buf,
            tx_with_curve(
                &format!("base{}", i),
                1100 + i as u64 * 50,
                true,
                0.3 + i as f64 * 0.1,
                1_000_000.0,
                1.0,
                10.0,
                true,
            ),
        );
    }
    ingest(
        &mut buf,
        tx_with_curve("sell", 1310, false, 0.1, 1_000_000.0, 1.0, 10.0, true),
    );
    for i in 0..4 {
        ingest(
            &mut buf,
            tx_with_curve(
                &format!("step{}", i),
                1400 + i as u64 * 50,
                true,
                0.4 + i as f64 * 0.1,
                1_000_000.0,
                1.0,
                10.0,
                true,
            ),
        );
    }
    ingest(
        &mut buf,
        tx_with_curve("pump", 1800, true, 2.0, 1_000_000.0, 30.0, 300.0, true),
    );

    let assessment = final_assessment(buf.force_check_deadline(101_001));
    let extended = assessment
        .v25_shadow_decisions
        .iter()
        .find(|decision| decision.window == ObservationStage::Extended)
        .expect("expected terminal extended shadow decision");

    assert_eq!(extended.kind, ShadowDecisionKind::RejectPumpAndDump);
    assert_eq!(extended.confidence, 0.0);
    assert!(
        assessment
            .v25_shadow_decisions
            .iter()
            .all(|decision| decision.kind.verdict_str() != "BUY"),
        "PDD-hard-failed shadow plane must never emit BUY"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// P0 — DOW timing reliability tests
// ══════════════════════════════════════════════════════════════════════════════

/// P0: DOW timer fires all three stages without TX pressure.
///
/// Pool with 0 TX should still get Early/Normal/Extended InsufficientData
/// decisions when `maybe_fire_shadow_checkpoint` is called at the right times.
#[test]
fn p0_dow_timer_fires_all_three_stages_without_tx_pressure() {
    let mut cfg = v25_enabled_config();
    cfg.max_wait_time_ms = 12_000;
    cfg.dow.early_entry_enabled = true;
    cfg.dow.early_entry_min_ms = 2_000;
    cfg.dow.early_entry_max_ms = 5_000;
    cfg.dow.normal_window_ms = 7_000;
    cfg.dow.extended_window_ms = 10_000;
    cfg.dow.tick_interval_ms = 250;
    // Keep Phase 1 thresholds above zero so we get InsufficientData for 0 TX.
    cfg.min_tx_count = 8;
    cfg.min_unique_signers = 5;
    cfg.min_buy_count = 5;

    let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf.set_registered_wall_t0(1000);

    // Plan windows: Early [2-5s], Normal (5-7s), Extended [7-10s].
    // t=1500ms (elapsed=500): no window open yet
    let fired = buf.maybe_fire_shadow_checkpoint(1000 + 1500);
    assert!(!fired, "no checkpoint should fire before 2s");

    // t=3000ms (elapsed=2000): Early [2-5s]
    let fired = buf.maybe_fire_shadow_checkpoint(1000 + 3000);
    assert!(fired, "Early checkpoint should fire at 3s (in 2-5s window)");

    // t=6100ms (elapsed=5100): Normal (5-7s)
    let fired = buf.maybe_fire_shadow_checkpoint(1000 + 6100);
    assert!(
        fired,
        "Normal checkpoint should fire at 6.1s (in 5-7s window)"
    );

    // t=8000ms (elapsed=7000): Extended [7-10s]
    let fired = buf.maybe_fire_shadow_checkpoint(1000 + 8000);
    assert!(
        fired,
        "Extended checkpoint should fire at 8s (in 7-10s window)"
    );

    // t=11001ms (elapsed=10001): Extended window has closed; its provisional
    // insufficient-data record is now final.
    let fired = buf.maybe_fire_shadow_checkpoint(1000 + 11001);
    assert!(
        fired,
        "Extended insufficient-data checkpoint should finalize after the window closes"
    );

    // Verify each stage produced exactly one final decision record.
    let early_count = buf
        .v25_shadow_decisions()
        .iter()
        .filter(|d| d.window == ObservationStage::Early)
        .count();
    let normal_count = buf
        .v25_shadow_decisions()
        .iter()
        .filter(|d| d.window == ObservationStage::Normal)
        .count();
    let extended_count = buf
        .v25_shadow_decisions()
        .iter()
        .filter(|d| d.window == ObservationStage::Extended)
        .count();

    // Single-owner invariant: *_shadow_fired is set on the first checkpoint
    // (including InsufficientData). Exactly one decision per stage.
    assert_eq!(early_count, 1, "exactly 1 Early checkpoint");
    assert_eq!(normal_count, 1, "exactly 1 Normal checkpoint");
    assert_eq!(extended_count, 1, "exactly 1 Extended checkpoint");

    // All should be InsufficientData (0 TX ingested).
    let decisions = buf.v25_shadow_decisions();
    for d in decisions.iter() {
        assert_eq!(
            d.kind,
            ShadowDecisionKind::InsufficientData,
            "all stages should be InsufficientData with 0 TX, got {:?} at {:?}",
            d.kind,
            d.window,
        );
        assert!(
            d.reason.starts_with("TIMER_FIRED_INSUFFICIENT_DATA: tx=0/"),
            "timer insufficient-data reason should preserve timer source: {}",
            d.reason
        );
    }
}

/// P0: programmatic configs must fail closed when DOW is enabled with a zero tick interval.
#[test]
#[should_panic(expected = "dow.tick_interval_ms must be > 0")]
fn p0_dow_runtime_rejects_zero_tick_interval() {
    let mut cfg = v25_enabled_config();
    cfg.dow.enabled = true;
    cfg.dow.tick_interval_ms = 0;

    let _ = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
}

/// P0: an empty Early timer tick is provisional; TXs arriving inside the
/// same Early window replace it with one final typed verdict for the stage.
#[tokio::test(start_paused = true)]
async fn p0_dow_runtime_early_empty_tick_then_tx_finalizes_real_verdict() {
    let mut cfg = v25_enabled_config();
    cfg.max_wait_time_ms = 12_000;
    cfg.dow.early_entry_enabled = true;
    cfg.dow.early_entry_min_ms = 2_000;
    cfg.dow.early_entry_max_ms = 5_000;
    cfg.dow.normal_window_ms = 7_000;
    cfg.dow.extended_window_ms = 10_000;
    cfg.dow.tick_interval_ms = 250;
    cfg.dow.early_entry_min_tx_count = 3;
    cfg.min_tx_count = 3;
    cfg.min_unique_signers = 3;
    cfg.min_buy_count = 3;

    let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf.set_registered_wall_t0(1_000);

    let mut dow_tick = ghost_launcher::components::gatekeeper_dow_timer::dow_timer_interval(
        cfg.dow.tick_interval_ms,
    );
    dow_tick.tick().await;
    tokio::time::advance(Duration::from_millis(2_000)).await;
    dow_tick.tick().await;

    assert!(buf.maybe_fire_shadow_checkpoint_from(3_000, ShadowCheckpointSource::Timer));
    let provisional = stage_decision(&buf, ObservationStage::Early);
    assert_eq!(provisional.kind, ShadowDecisionKind::InsufficientData);
    assert!(provisional
        .reason
        .starts_with("TIMER_FIRED_INSUFFICIENT_DATA: tx=0/3 elapsed_ms=2000"));

    ingest(&mut buf, tx("early_runtime_signer_0", 5_400, true, 0.5));
    let tx_provisional = stage_decision(&buf, ObservationStage::Early);
    assert!(tx_provisional
        .reason
        .starts_with("TX_FIRED_INSUFFICIENT_DATA: tx=1/3 elapsed_ms=4400"));
    assert!(!tx_provisional
        .reason
        .starts_with("TIMER_FIRED_INSUFFICIENT_DATA"));

    for i in 1..3 {
        ingest(
            &mut buf,
            tx(
                &format!("early_runtime_signer_{i}"),
                5_400 + i as u64 * 50,
                true,
                0.5,
            ),
        );
    }

    let early = stage_decision(&buf, ObservationStage::Early);
    assert_ne!(early.kind, ShadowDecisionKind::InsufficientData);
    assert!(!early.reason.contains("TIMER_FIRED_INSUFFICIENT_DATA"));
    assert_eq!(
        buf.v25_shadow_decisions()
            .iter()
            .filter(|decision| decision.window == ObservationStage::Early)
            .count(),
        1,
        "Early must have one final record, not provisional plus final duplicate"
    );
}

/// P0: Normal has the same provisional semantics as Early.
#[tokio::test(start_paused = true)]
async fn p0_dow_runtime_normal_empty_tick_then_tx_finalizes_real_verdict() {
    let mut cfg = v25_enabled_config();
    cfg.max_wait_time_ms = 12_000;
    cfg.dow.early_entry_enabled = true;
    cfg.dow.early_entry_min_ms = 2_000;
    cfg.dow.early_entry_max_ms = 5_000;
    cfg.dow.normal_window_ms = 7_000;
    cfg.dow.extended_window_ms = 10_000;
    cfg.dow.tick_interval_ms = 250;
    cfg.min_tx_count = 3;
    cfg.min_unique_signers = 3;
    cfg.min_buy_count = 3;

    let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf.set_registered_wall_t0(1_000);

    let mut dow_tick = ghost_launcher::components::gatekeeper_dow_timer::dow_timer_interval(
        cfg.dow.tick_interval_ms,
    );
    dow_tick.tick().await;
    tokio::time::advance(Duration::from_millis(5_100)).await;
    dow_tick.tick().await;

    assert!(buf.maybe_fire_shadow_checkpoint_from(6_100, ShadowCheckpointSource::Timer));
    let normal_provisional = stage_decision(&buf, ObservationStage::Normal);
    assert_eq!(
        normal_provisional.kind,
        ShadowDecisionKind::InsufficientData
    );
    assert!(normal_provisional
        .reason
        .starts_with("TIMER_FIRED_INSUFFICIENT_DATA: tx=0/3 elapsed_ms=5100"));

    for i in 0..3 {
        ingest(
            &mut buf,
            tx(
                &format!("normal_runtime_signer_{i}"),
                7_450 + i as u64 * 50,
                true,
                0.5,
            ),
        );
    }

    let normal = stage_decision(&buf, ObservationStage::Normal);
    assert_ne!(normal.kind, ShadowDecisionKind::InsufficientData);
    assert!(!normal.reason.contains("TIMER_FIRED_INSUFFICIENT_DATA"));
    assert_eq!(
        buf.v25_shadow_decisions()
            .iter()
            .filter(|decision| decision.window == ObservationStage::Normal)
            .count(),
        1,
        "Normal must have one final record, not provisional plus final duplicate"
    );
}

/// P0: runtime configs with Extended after hard deadline fail fast.
#[test]
#[should_panic(expected = "P0 invariant violated")]
fn p0_dow_runtime_rejects_extended_window_beyond_deadline() {
    let mut cfg = v25_enabled_config();
    cfg.max_wait_time_ms = 5_000;
    cfg.dow.enabled = true;
    cfg.dow.extended_window_ms = 10_000;

    let _ = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
}

/// P0: Extended stage has a typed verdict — not unreachable!().
///
/// When a pool accumulates enough TX data (Phase 1 met) and enters the
/// Extended window (>=10s), `try_shadow_evaluate(Extended)` must produce
/// a real verdict (NormalBuyCandidate, ShadowReject, RejectPumpAndDump, or
/// RejectLowTrajectory). `unreachable!()` must never be hit.
#[test]
fn p0_extended_stage_has_typed_verdict_not_unreachable() {
    let mut cfg = v25_enabled_config();
    cfg.max_wait_time_ms = 12_000;
    cfg.dow.extended_window_ms = 10_000;
    cfg.dow.extended_window_min_confidence = 0.0;

    let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf.set_registered_wall_t0(1000);

    // Feed enough TXs to satisfy Phase 1.
    for i in 0..10 {
        let t = tx(
            &format!("signer_{}", i),
            1500 + i as u64 * 800,
            true,
            0.5 + i as f64 * 0.1,
        );
        ingest(&mut buf, t);
    }

    // Fire Extended checkpoint at t=8000ms (elapsed=7000, in 7-10s window).
    buf.maybe_fire_shadow_checkpoint(1000 + 8000);

    // Find the Extended shadow decision — it must NOT be InsufficientData.
    let extended = buf
        .v25_shadow_decisions()
        .iter()
        .filter(|d| {
            d.window == ObservationStage::Extended && d.kind != ShadowDecisionKind::InsufficientData
        })
        .last()
        .expect("expected a meaningful Extended shadow decision (not InsufficientData)");

    assert_ne!(
        extended.kind,
        ShadowDecisionKind::InsufficientData,
        "Extended must produce a typed verdict, not InsufficientData"
    );

    // Verify the verdict is one of the valid typed kinds.
    let valid_kinds = [
        ShadowDecisionKind::NormalBuyCandidate,
        ShadowDecisionKind::ShadowReject,
        ShadowDecisionKind::RejectPumpAndDump,
        ShadowDecisionKind::RejectLowTrajectory,
    ];
    assert!(
        valid_kinds.contains(&extended.kind),
        "Extended verdict kind must be one of the valid typed kinds, got {:?}",
        extended.kind
    );

    // Verify it is NOT the old unreachable path.
    assert_ne!(
        extended.reason.as_str(),
        "",
        "Extended must have a non-empty reason"
    );
}

/// P0: Extended timer and deadline fallback use the same V2.5 confidence model.
#[test]
fn p0_extended_timer_and_deadline_fallback_confidence_match() {
    let mut cfg = v25_enabled_config();
    cfg.max_wait_time_ms = 10_000;
    cfg.use_three_layer_decision = true;
    cfg.dow.extended_window_ms = 10_000;
    cfg.dow.extended_window_min_confidence = 0.0;
    cfg.min_tx_count = 6;
    cfg.min_unique_signers = 4;
    cfg.min_buy_count = 4;
    cfg.pdd.enabled = true;

    fn seed(buf: &mut GatekeeperBuffer) {
        for i in 0..10 {
            ingest(
                buf,
                tx(
                    &format!("conf_signer_{}", i),
                    1_200 + i as u64 * 650,
                    true,
                    0.35 + i as f64 * 0.03,
                ),
            );
        }
    }

    let mut timer_buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    timer_buf.set_registered_wall_t0(1_000);
    seed(&mut timer_buf);
    assert!(timer_buf.maybe_fire_shadow_checkpoint(9_000));
    let timer_confidence = timer_buf
        .v25_shadow_decisions()
        .iter()
        .filter(|d| {
            d.window == ObservationStage::Extended && d.kind != ShadowDecisionKind::InsufficientData
        })
        .last()
        .expect("timer path must emit Extended")
        .confidence;

    let mut deadline_buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    deadline_buf.set_registered_wall_t0(1_000);
    seed(&mut deadline_buf);
    let assessment = final_assessment(deadline_buf.force_check_deadline(11_100));
    let deadline_confidence = assessment
        .v25_shadow_decisions
        .iter()
        .filter(|d| {
            d.window == ObservationStage::Extended && d.kind != ShadowDecisionKind::InsufficientData
        })
        .last()
        .expect("deadline fallback must emit Extended")
        .confidence;

    assert!(
        (timer_confidence - deadline_confidence).abs() < f64::EPSILON,
        "Extended confidence must match timer vs deadline fallback: timer={timer_confidence:.6} deadline={deadline_confidence:.6}"
    );
}

/// P0: DOW checkpoint owner is serialized per pool — no duplicate checkpoints.
///
/// Multiple calls to `maybe_fire_shadow_checkpoint` within the same window
/// must not produce duplicate shadow decisions for the same stage. The
/// `*_shadow_fired` flags ensure single-owner serialization.
#[test]
fn p0_dow_checkpoint_owner_is_serialized_per_pool() {
    let mut cfg = v25_enabled_config();
    cfg.max_wait_time_ms = 12_000;
    cfg.dow.early_entry_enabled = true;
    cfg.dow.early_entry_min_ms = 2_000;
    cfg.dow.early_entry_max_ms = 5_000;
    cfg.dow.normal_window_ms = 7_000;
    cfg.dow.extended_window_ms = 10_000;
    cfg.dow.tick_interval_ms = 250;
    cfg.min_tx_count = 8;
    cfg.min_unique_signers = 5;
    cfg.min_buy_count = 5;

    let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf.set_registered_wall_t0(1000);

    // Plan windows: Early [2-5s], Normal (5-7s), Extended [7-10s].
    for _ in 0..10 {
        buf.maybe_fire_shadow_checkpoint(1000 + 3000); // Early (elapsed=2000)
    }
    for _ in 0..10 {
        buf.maybe_fire_shadow_checkpoint(1000 + 6100); // Normal (elapsed=5100, in 5-7s)
    }
    for _ in 0..10 {
        buf.maybe_fire_shadow_checkpoint(1000 + 8000); // Extended (elapsed=7000, in 7-10s)
    }

    // Exactly one decision per stage, even after 10 calls per window.
    let early_count = buf
        .v25_shadow_decisions()
        .iter()
        .filter(|d| d.window == ObservationStage::Early)
        .count();
    let normal_count = buf
        .v25_shadow_decisions()
        .iter()
        .filter(|d| d.window == ObservationStage::Normal)
        .count();
    let extended_count = buf
        .v25_shadow_decisions()
        .iter()
        .filter(|d| d.window == ObservationStage::Extended)
        .count();

    // Single-owner invariant: *_shadow_fired is set on the first checkpoint
    // (including InsufficientData). Exactly one decision per stage, even with
    // 30 rapid calls across all windows.
    assert_eq!(early_count, 1, "Early fires exactly once");
    assert_eq!(normal_count, 1, "Normal fires exactly once");
    assert_eq!(extended_count, 1, "Extended fires exactly once");
    assert_eq!(buf.v25_shadow_decisions().len(), 3);

    let real_any = buf
        .v25_shadow_decisions()
        .iter()
        .filter(|d| d.kind != ShadowDecisionKind::InsufficientData)
        .count();
    assert_eq!(real_any, 0, "no real decisions with 0 TX");
}

/// P0: With sufficient data, each stage fires exactly once.
///
/// When Phase 1 is met and data is sufficient, `*_shadow_fired` flags are set
/// and subsequent calls within the same window must NOT produce duplicates.
#[test]
fn p0_dow_single_owner_with_sufficient_data() {
    let mut cfg = v25_enabled_config();
    cfg.max_wait_time_ms = 15_000;
    cfg.dow.early_entry_enabled = true;
    cfg.dow.early_entry_min_ms = 2_000;
    cfg.dow.early_entry_max_ms = 5_000;
    cfg.dow.normal_window_ms = 7_000;
    cfg.dow.extended_window_ms = 10_000;
    cfg.dow.tick_interval_ms = 250;
    cfg.min_tx_count = 5;
    cfg.min_unique_signers = 3;
    cfg.min_buy_count = 3;

    let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf.set_registered_wall_t0(1000);

    // Feed enough TX to meet Phase 1.
    for i in 0..6 {
        let t = tx(
            &format!("signer_{}", i),
            1100 + i as u64 * 200,
            true,
            0.5 + i as f64 * 0.1,
        );
        ingest(&mut buf, t);
    }

    // Fire at 3s (Early window — but Early requires elapsed <= 5000).
    // elapsed = 3000 - 1000 = 2000, which is in [2000, 5000].
    for _ in 0..5 {
        buf.maybe_fire_shadow_checkpoint(1000 + 3000);
    }

    // Normal window (5-7s): elapsed=5100 at t=6100.
    for _ in 0..5 {
        buf.maybe_fire_shadow_checkpoint(1000 + 6100);
    }

    // Extended window (7-10s): elapsed=7000 at t=8000.
    for _ in 0..5 {
        buf.maybe_fire_shadow_checkpoint(1000 + 8000);
    }

    // With sufficient data, real decisions set the flags.
    // Early: exactly 1 non-InsufficientData
    let early_real = buf
        .v25_shadow_decisions()
        .iter()
        .filter(|d| {
            d.window == ObservationStage::Early && d.kind != ShadowDecisionKind::InsufficientData
        })
        .count();
    // Early has stricter criteria (confidence ≥0.85, 6/6 phases, drift <3%,
    // momentum >0.40). With synthetic test TXs these may not be met, so
    // Early's real decision count is ≤1. Each stage still fires exactly 1
    // TOTAL checkpoint (InsufficientData or real) per the single-owner invariant.
    assert!(
        early_real <= 1,
        "Early real decisions must be ≤1, got {}",
        early_real
    );

    // Normal: exactly 1 non-InsufficientData
    let normal_real = buf
        .v25_shadow_decisions()
        .iter()
        .filter(|d| {
            d.window == ObservationStage::Normal && d.kind != ShadowDecisionKind::InsufficientData
        })
        .count();
    assert_eq!(normal_real, 1, "Normal must have exactly 1 real decision");

    // Extended: exactly 1 non-InsufficientData
    let extended_real = buf
        .v25_shadow_decisions()
        .iter()
        .filter(|d| {
            d.window == ObservationStage::Extended && d.kind != ShadowDecisionKind::InsufficientData
        })
        .count();
    assert_eq!(
        extended_real, 1,
        "Extended must have exactly 1 real decision"
    );

    // No duplicate real decisions across any stage.
    for stage in [
        ObservationStage::Early,
        ObservationStage::Normal,
        ObservationStage::Extended,
    ] {
        let real_count = buf
            .v25_shadow_decisions()
            .iter()
            .filter(|d| d.window == stage && d.kind != ShadowDecisionKind::InsufficientData)
            .count();
        assert!(
            real_count <= 1,
            "stage {:?} must not have duplicate real decisions, got {}",
            stage,
            real_count,
        );
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// P1 — SSOT parity + segment_sequence tests
// ══════════════════════════════════════════════════════════════════════════════

use ghost_core::checkpoint::{TrajectorySegmentSnapshot, TxSegmentSequence};

/// P1: `MaterializedFeatureSet` carries optional `tx_segment_sequence`
/// with `#[serde(default)]` — N1 preserved.
#[test]
fn p1_materialized_feature_set_carries_optional_segment_sequence() {
    let mfs = ghost_core::checkpoint::MaterializedFeatureSet::default();
    assert!(mfs.tx_segment_sequence.is_none(), "default must be None");

    // New field is additive only: existing code constructing MaterializedFeatureSet
    // without tx_segment_sequence compiles and defaults to None.
    let built = ghost_launcher::components::gatekeeper_policy::build_assessment_from_features(
        ghost_core::checkpoint::MaterializedFeatureSet::default(),
        &v25_enabled_config(),
        ghost_launcher::components::gatekeeper_policy::PolicyEvaluationContext::default(),
    );
    // Verification: the constructed MaterializedFeatureSet in the assessment
    // has tx_segment_sequence = None when not explicitly set.
    assert!(
        built.feature_snapshot.tx_segment_sequence.is_none(),
        "tx_segment_sequence must be None in default-constructed feature set"
    );
}

/// P1: Path B marks unavailable instead of guessing when sequence missing.
#[test]
fn p1_path_b_marks_unavailable_instead_of_guessing_sequence_features() {
    use ghost_brain::config::GatekeeperMode;
    use ghost_launcher::components::gatekeeper_policy::{
        build_assessment_from_features, PolicyEvaluationContext,
    };

    let mut config = v25_enabled_config();
    config.mode = GatekeeperMode::Long;
    config.tas.enabled = true;
    config.pdd.enabled = true;

    let mut features = ghost_core::checkpoint::MaterializedFeatureSet::default();
    features.tx_intel_features.tx_count = 15;
    features.tx_intel_features.buy_count = 12;
    features.tx_intel_features.unique_signers = 10;
    features.tx_intel_features.buy_ratio = 0.80;
    features.tx_intel_features.total_volume_sol = 8.0;
    features.tx_intel_features.avg_interval_ms = 300.0;
    features.tx_intel_features.interval_cv = 0.5;
    features.tx_intel_features.timing_entropy = 2.0;
    features.account_features.market_cap_sol = 50.0;
    features.account_features.bonding_progress = 0.20;
    features.session_metadata.observation_duration_ms = 7_000;
    // No tx_segment_sequence — key test condition.

    let assessment =
        build_assessment_from_features(features, &config, PolicyEvaluationContext::default());

    let (tas_available, tas_reason) = assessment.tas_availability(&config);
    assert_eq!(tas_available, Some(false));
    assert!(
        tas_reason.is_some(),
        "must provide explicit unavailable reason"
    );
    let reason = tas_reason.unwrap();
    // P1 taxonomy: reason must be from the specific taxonomy, not generic.
    assert_eq!(reason, "missing_sequence");

    let pdd_seq = assessment.pdd_sequence_signals_available(&config);
    assert_eq!(pdd_seq, Some(false));

    // P1: pdd_sequence_signals_unavailable_reason must be logged.
    let buy_log = assessment.to_buy_log(&Pubkey::new_unique(), &config);
    assert_eq!(buy_log.pdd_sequence_signals_available, Some(false));
    assert!(
        buy_log.pdd_sequence_signals_unavailable_reason.is_some(),
        "pdd_sequence_signals_unavailable_reason must be logged when unavailable"
    );
    let pdd_reason = buy_log
        .pdd_sequence_signals_unavailable_reason
        .as_ref()
        .unwrap();
    assert_eq!(pdd_reason, "missing_sequence");
}

/// P1: unavailable reasons are stable, specific taxonomy values.
#[test]
fn p1_availability_reasons_are_specific() {
    use ghost_launcher::components::gatekeeper_policy::{
        build_assessment_from_features, PolicyEvaluationContext,
    };

    let mut config = v25_enabled_config();
    config.tas.enabled = true;
    config.pdd.enabled = true;
    config.tas.tas_min_total_duration_ms = 2_000;
    config.tas.tas_min_tx_per_segment = 3;

    let mut insufficient_duration = ghost_core::checkpoint::MaterializedFeatureSet::default();
    insufficient_duration.tx_intel_features.tx_count = 12;
    insufficient_duration
        .session_metadata
        .observation_duration_ms = 1_000;
    let assessment = build_assessment_from_features(
        insufficient_duration,
        &config,
        PolicyEvaluationContext::default(),
    );
    assert_eq!(
        assessment.tas_availability(&config).1.as_deref(),
        Some("insufficient_duration")
    );
    assert_eq!(
        assessment
            .pdd_sequence_signals_availability(&config)
            .1
            .as_deref(),
        Some("insufficient_duration")
    );

    let mut insufficient_tx = ghost_core::checkpoint::MaterializedFeatureSet::default();
    insufficient_tx.tx_intel_features.tx_count = 5;
    insufficient_tx.session_metadata.observation_duration_ms = 7_000;
    let assessment = build_assessment_from_features(
        insufficient_tx,
        &config,
        PolicyEvaluationContext::default(),
    );
    assert_eq!(
        assessment.tas_availability(&config).1.as_deref(),
        Some("insufficient_tx_per_segment")
    );
    assert_eq!(
        assessment
            .pdd_sequence_signals_availability(&config)
            .1
            .as_deref(),
        Some("insufficient_tx_per_segment")
    );
}

#[test]
fn p1_flash_enabled_marks_path_b_sequence_as_unavailable() {
    use ghost_launcher::components::gatekeeper_policy::{
        build_assessment_from_features, PolicyEvaluationContext,
    };

    let mut config = v25_enabled_config();
    config.tas.enabled = true;
    config.pdd.enabled = true;
    config.pdd.flash_crash_protection_enabled = true;

    let mut features = ghost_core::checkpoint::MaterializedFeatureSet::default();
    features.tx_intel_features.tx_count = 15;
    features.tx_intel_features.buy_count = 12;
    features.tx_intel_features.unique_signers = 10;
    features.tx_intel_features.buy_ratio = 0.80;
    features.tx_intel_features.total_volume_sol = 8.0;
    features.tx_intel_features.avg_interval_ms = 300.0;
    features.tx_intel_features.interval_cv = 0.5;
    features.tx_intel_features.timing_entropy = 2.0;
    features.account_features.market_cap_sol = 50.0;
    features.account_features.bonding_progress = 0.20;
    features.session_metadata.observation_duration_ms = 7_000;
    features.tx_segment_sequence = Some(TxSegmentSequence {
        t0_segment: TrajectorySegmentSnapshot {
            tx_count: 5,
            buy_ratio: 0.80,
            avg_interval_ms: 400.0,
            total_volume_sol: 2.0,
            hhi: 0.25,
            max_single_tx_sol: 0.5,
            same_size_streak: 1,
        },
        t1_segment: TrajectorySegmentSnapshot {
            tx_count: 5,
            buy_ratio: 0.82,
            avg_interval_ms: 350.0,
            total_volume_sol: 2.5,
            hhi: 0.22,
            max_single_tx_sol: 0.6,
            same_size_streak: 2,
        },
        t2_segment: TrajectorySegmentSnapshot {
            tx_count: 5,
            buy_ratio: 0.85,
            avg_interval_ms: 300.0,
            total_volume_sol: 3.5,
            hhi: 0.20,
            max_single_tx_sol: 0.7,
            same_size_streak: 1,
        },
        total_duration_ms: 6000,
        min_tx_per_segment_satisfied: true,
    });

    let assessment =
        build_assessment_from_features(features, &config, PolicyEvaluationContext::default());

    assert_eq!(
        assessment.pdd_sequence_signals_available(&config),
        Some(false)
    );
    assert_eq!(
        assessment
            .pdd_sequence_signals_availability(&config)
            .1
            .as_deref(),
        Some("pdd_flash_crash_unavailable")
    );

    let buy_log = assessment.to_buy_log(&Pubkey::new_unique(), &config);
    assert_eq!(buy_log.pdd_sequence_signals_available, Some(false));
    assert_eq!(
        buy_log.pdd_sequence_signals_unavailable_reason.as_deref(),
        Some("pdd_flash_crash_unavailable")
    );
}

#[test]
fn p1_flash_unavailable_blocks_v25_confidence() {
    use ghost_launcher::components::gatekeeper_policy::{
        build_assessment_from_features, evaluate_policy_from_assessment, PolicyEvaluationContext,
    };

    let mut config = v25_enabled_config();
    config.tas.enabled = true;
    config.pdd.enabled = true;
    config.pdd.flash_crash_protection_enabled = true;
    config.v25.shadow_enabled = true;

    let mut features = ghost_core::checkpoint::MaterializedFeatureSet::default();
    features.tx_intel_features.tx_count = 15;
    features.tx_intel_features.buy_count = 12;
    features.tx_intel_features.unique_signers = 10;
    features.tx_intel_features.buy_ratio = 0.80;
    features.tx_intel_features.total_volume_sol = 8.0;
    features.tx_intel_features.avg_interval_ms = 300.0;
    features.tx_intel_features.interval_cv = 0.5;
    features.tx_intel_features.timing_entropy = 2.0;
    features.account_features.market_cap_sol = 50.0;
    features.account_features.bonding_progress = 0.20;
    features.session_metadata.observation_duration_ms = 7_000;
    features.tx_segment_sequence = Some(TxSegmentSequence {
        t0_segment: TrajectorySegmentSnapshot {
            tx_count: 5,
            buy_ratio: 0.80,
            avg_interval_ms: 400.0,
            total_volume_sol: 2.0,
            hhi: 0.25,
            max_single_tx_sol: 0.5,
            same_size_streak: 1,
        },
        t1_segment: TrajectorySegmentSnapshot {
            tx_count: 5,
            buy_ratio: 0.82,
            avg_interval_ms: 350.0,
            total_volume_sol: 2.5,
            hhi: 0.22,
            max_single_tx_sol: 0.6,
            same_size_streak: 2,
        },
        t2_segment: TrajectorySegmentSnapshot {
            tx_count: 5,
            buy_ratio: 0.85,
            avg_interval_ms: 300.0,
            total_volume_sol: 3.5,
            hhi: 0.20,
            max_single_tx_sol: 0.7,
            same_size_streak: 1,
        },
        total_duration_ms: 6000,
        min_tx_per_segment_satisfied: true,
    });

    let mut assessment =
        build_assessment_from_features(features, &config, PolicyEvaluationContext::default());
    assessment.decision = Some(evaluate_policy_from_assessment(&assessment, &config));
    assessment.cache_v25_confidence(&config);

    assert_eq!(assessment.v25_confidence, None);
    assert_eq!(
        assessment.v25_confidence_availability(&config),
        (Some(false), Some("pdd_flash_crash_unavailable".to_string()))
    );

    let buy_log = assessment.to_buy_log(&Pubkey::new_unique(), &config);
    assert_eq!(buy_log.v25_confidence, None);
    assert_eq!(buy_log.v25_confidence_available, Some(false));
    assert_eq!(
        buy_log.v25_confidence_unavailable_reason.as_deref(),
        Some("pdd_flash_crash_unavailable")
    );
}

#[test]
fn p1_flash_disabled_keeps_complete_sequence_available() {
    use ghost_launcher::components::gatekeeper_policy::{
        build_assessment_from_features, PolicyEvaluationContext,
    };

    let mut config = v25_enabled_config();
    config.tas.enabled = true;
    config.pdd.enabled = true;
    config.pdd.flash_crash_protection_enabled = false;

    let mut features = ghost_core::checkpoint::MaterializedFeatureSet::default();
    features.tx_intel_features.tx_count = 15;
    features.tx_intel_features.buy_count = 12;
    features.tx_intel_features.unique_signers = 10;
    features.tx_intel_features.buy_ratio = 0.80;
    features.tx_intel_features.total_volume_sol = 8.0;
    features.tx_intel_features.avg_interval_ms = 300.0;
    features.tx_intel_features.interval_cv = 0.5;
    features.tx_intel_features.timing_entropy = 2.0;
    features.account_features.market_cap_sol = 50.0;
    features.account_features.bonding_progress = 0.20;
    features.session_metadata.observation_duration_ms = 7_000;
    features.tx_segment_sequence = Some(TxSegmentSequence {
        t0_segment: TrajectorySegmentSnapshot {
            tx_count: 5,
            buy_ratio: 0.80,
            avg_interval_ms: 400.0,
            total_volume_sol: 2.0,
            hhi: 0.25,
            max_single_tx_sol: 0.5,
            same_size_streak: 1,
        },
        t1_segment: TrajectorySegmentSnapshot {
            tx_count: 5,
            buy_ratio: 0.82,
            avg_interval_ms: 350.0,
            total_volume_sol: 2.5,
            hhi: 0.22,
            max_single_tx_sol: 0.6,
            same_size_streak: 2,
        },
        t2_segment: TrajectorySegmentSnapshot {
            tx_count: 5,
            buy_ratio: 0.85,
            avg_interval_ms: 300.0,
            total_volume_sol: 3.5,
            hhi: 0.20,
            max_single_tx_sol: 0.7,
            same_size_streak: 1,
        },
        total_duration_ms: 6000,
        min_tx_per_segment_satisfied: true,
    });

    let assessment =
        build_assessment_from_features(features, &config, PolicyEvaluationContext::default());

    assert_eq!(
        assessment.pdd_sequence_signals_available(&config),
        Some(true)
    );
    assert_eq!(
        assessment.pdd_sequence_signals_availability(&config),
        (Some(true), None)
    );
}

/// P1: Path A and Path B compute the same TAS when segment_sequence present.
#[test]
fn p1_path_a_and_path_b_compute_same_tas_when_segment_sequence_present() {
    use ghost_brain::config::GatekeeperMode;
    use ghost_launcher::components::gatekeeper_policy::{
        build_assessment_from_features, PolicyEvaluationContext,
    };

    let mut config = v25_enabled_config();
    config.mode = GatekeeperMode::Long;
    config.tas.enabled = true;
    config.pdd.enabled = true;

    let mut features = ghost_core::checkpoint::MaterializedFeatureSet::default();
    features.tx_intel_features.tx_count = 15;
    features.tx_intel_features.buy_count = 12;
    features.tx_intel_features.unique_signers = 10;
    features.tx_intel_features.buy_ratio = 0.80;
    features.tx_intel_features.total_volume_sol = 8.0;
    features.tx_intel_features.avg_interval_ms = 300.0;
    features.tx_intel_features.interval_cv = 0.5;
    features.tx_intel_features.timing_entropy = 2.0;
    features.account_features.market_cap_sol = 50.0;
    features.account_features.bonding_progress = 0.20;
    features.session_metadata.observation_duration_ms = 7_000;

    features.tx_segment_sequence = Some(TxSegmentSequence {
        t0_segment: TrajectorySegmentSnapshot {
            tx_count: 5,
            buy_ratio: 0.80,
            avg_interval_ms: 400.0,
            total_volume_sol: 2.0,
            hhi: 0.25,
            max_single_tx_sol: 0.5,
            same_size_streak: 1,
        },
        t1_segment: TrajectorySegmentSnapshot {
            tx_count: 5,
            buy_ratio: 0.82,
            avg_interval_ms: 350.0,
            total_volume_sol: 2.5,
            hhi: 0.22,
            max_single_tx_sol: 0.6,
            same_size_streak: 2,
        },
        t2_segment: TrajectorySegmentSnapshot {
            tx_count: 5,
            buy_ratio: 0.85,
            avg_interval_ms: 300.0,
            total_volume_sol: 3.5,
            hhi: 0.20,
            max_single_tx_sol: 0.7,
            same_size_streak: 1,
        },
        total_duration_ms: 6000,
        min_tx_per_segment_satisfied: true,
    });

    let assessment =
        build_assessment_from_features(features, &config, PolicyEvaluationContext::default());

    let (tas_available, _) = assessment.tas_availability(&config);
    assert_eq!(tas_available, Some(true));

    assert!(assessment.trajectory.is_some());
    let traj = assessment.trajectory.unwrap();
    // Path B TAS from segment_sequence: accelerating volume (2.0→2.5→3.5 SOL),
    // improving HHI (0.25→0.22→0.20), shortening intervals (400→350→300ms).
    // The weighted score must reflect a positive trajectory.
    assert!(
        traj.overall_tas_score > 0.50,
        "accelerating trajectory must score >0.50"
    );
    assert!(
        traj.momentum_score > 0.0,
        "momentum must be positive (T2 > T0)"
    );
    assert!(
        traj.hhi_score > 0.0,
        "HHI must improve (declining concentration)"
    );
    assert_eq!(traj.t0_tx_count, 5);
    assert_eq!(traj.t1_tx_count, 5);
    assert_eq!(traj.t2_tx_count, 5);
}

/// P1: Hard parity proof — Path A (buffer) vs Path B (segment_sequence)
/// produce the same TAS score for identical underlying data.
#[test]
fn p1_hard_parity_path_a_vs_path_b_same_tas_score() {
    use ghost_launcher::components::gatekeeper_trajectory::score_trajectory;

    let mut cfg = v25_enabled_config();
    cfg.tas.enabled = true;
    cfg.tas.tas_min_tx_per_segment = 3;
    cfg.tas.tas_min_total_duration_ms = 2000;
    cfg.max_wait_time_ms = 10_000;
    cfg.mode = GatekeeperMode::Long;

    let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf.set_registered_wall_t0(1000);

    // Build TXs that produce a clear 3-segment trajectory.
    // T0 (1000-2333ms): 4 small TXs
    for i in 0..4 {
        let t = tx(
            &format!("t0_{}", i),
            1200 + i as u64 * 300,
            true,
            0.3 + i as f64 * 0.1,
        );
        let _ = buf.ingest_transaction_tracking_only(std::sync::Arc::new(t));
    }
    // T1 (2333-3666ms): 4 medium TXs
    for i in 0..4 {
        let t = tx(
            &format!("t1_{}", i),
            2500 + i as u64 * 280,
            true,
            0.5 + i as f64 * 0.15,
        );
        let _ = buf.ingest_transaction_tracking_only(std::sync::Arc::new(t));
    }
    // T2 (3666-5000ms): 4 large TXs
    for i in 0..4 {
        let t = tx(
            &format!("t2_{}", i),
            3800 + i as u64 * 280,
            true,
            0.8 + i as f64 * 0.2,
        );
        let _ = buf.ingest_transaction_tracking_only(std::sync::Arc::new(t));
    }

    // Path A: buffer's native materialize_trajectory
    let traj_a = buf
        .materialize_trajectory(&cfg.tas)
        .expect("Path A trajectory");

    // Path B: current_segment_sequence → reconstruct segments → score_trajectory
    let seq = buf
        .current_segment_sequence(&cfg.tas)
        .expect("segment sequence");
    assert!(seq.min_tx_per_segment_satisfied);

    let t0 = ghost_launcher::components::gatekeeper_trajectory::TrajectorySegment {
        tx_count: seq.t0_segment.tx_count as usize,
        buy_ratio: seq.t0_segment.buy_ratio,
        avg_interval_ms: seq.t0_segment.avg_interval_ms,
        total_volume_sol: seq.t0_segment.total_volume_sol,
        hhi: seq.t0_segment.hhi,
    };
    let t1 = ghost_launcher::components::gatekeeper_trajectory::TrajectorySegment {
        tx_count: seq.t1_segment.tx_count as usize,
        buy_ratio: seq.t1_segment.buy_ratio,
        avg_interval_ms: seq.t1_segment.avg_interval_ms,
        total_volume_sol: seq.t1_segment.total_volume_sol,
        hhi: seq.t1_segment.hhi,
    };
    let t2 = ghost_launcher::components::gatekeeper_trajectory::TrajectorySegment {
        tx_count: seq.t2_segment.tx_count as usize,
        buy_ratio: seq.t2_segment.buy_ratio,
        avg_interval_ms: seq.t2_segment.avg_interval_ms,
        total_volume_sol: seq.t2_segment.total_volume_sol,
        hhi: seq.t2_segment.hhi,
    };
    let traj_b = score_trajectory(&t0, &t1, &t2, &cfg.tas);

    // Hard parity: Path A and Path B must produce the same scores.
    let eps = 1e-6;
    assert!(
        (traj_a.overall_tas_score - traj_b.overall_tas_score).abs() < eps,
        "Path A TAS={:.6} Path B TAS={:.6} — must be identical",
        traj_a.overall_tas_score,
        traj_b.overall_tas_score
    );
    assert!((traj_a.momentum_score - traj_b.momentum_score).abs() < eps);
    assert!((traj_a.hhi_score - traj_b.hhi_score).abs() < eps);
    assert!((traj_a.volume_score - traj_b.volume_score).abs() < eps);
    assert!((traj_a.interval_score - traj_b.interval_score).abs() < eps);
    assert_eq!(traj_a.t0_tx_count, traj_b.t0_tx_count);
    assert_eq!(traj_a.t1_tx_count, traj_b.t1_tx_count);
    assert_eq!(traj_a.t2_tx_count, traj_b.t2_tx_count);
}

// ══════════════════════════════════════════════════════════════════════════════
// P2 — APS w decision plane tests
// ══════════════════════════════════════════════════════════════════════════════

/// P2: APS runs in Path B (build_assessment_from_features).
///
/// Verifies that `aps_diagnostics` is populated when APS is enabled and
/// `build_assessment_from_features` is called.
#[test]
fn p2_aps_runs_in_path_b_when_enabled() {
    use ghost_brain::config::GatekeeperMode;
    use ghost_launcher::components::gatekeeper_policy::{
        build_assessment_from_features, PolicyEvaluationContext,
    };

    let mut config = v25_enabled_config();
    config.mode = GatekeeperMode::Long;
    config.aps.enabled = true;
    config.aps.shadow_suggestions_enabled = true;
    config.aps.regime_detection_enabled = true;

    let mut features = ghost_core::checkpoint::MaterializedFeatureSet::default();
    features.tx_intel_features.tx_count = 15;
    features.tx_intel_features.buy_count = 12;
    features.tx_intel_features.unique_signers = 10;
    features.tx_intel_features.buy_ratio = 0.80;
    features.tx_intel_features.total_volume_sol = 8.0;
    features.tx_intel_features.avg_interval_ms = 300.0;
    features.tx_intel_features.interval_cv = 0.5;
    features.tx_intel_features.timing_entropy = 2.0;
    features.account_features.market_cap_sol = 50.0;
    features.account_features.bonding_progress = 0.20;
    features.session_metadata.observation_duration_ms = 7_000;

    let assessment =
        build_assessment_from_features(features, &config, PolicyEvaluationContext::default());

    assert!(
        assessment.aps_diagnostics.is_some(),
        "APS must run in Path B"
    );
    let aps = assessment.aps_diagnostics.unwrap();
    assert!(aps.enabled, "APS must be enabled");
    // Default regime_local_heuristic_enabled = false → regime always Normal
    assert_eq!(
        aps.regime,
        ghost_launcher::components::gatekeeper_adaptive_prosperity::MarketRegime::Normal,
    );
    assert!(!aps.adaptive_thresholds_applied);
}

/// P2: strong HighVol input below min_calibration_samples must still be Normal.
#[test]
fn p2_aps_high_vol_signal_below_min_samples_stays_normal() {
    use ghost_launcher::components::gatekeeper_adaptive_prosperity::MarketRegime;
    use ghost_launcher::components::gatekeeper_policy::{
        build_assessment_from_features, PolicyEvaluationContext,
    };

    let mut config = v25_enabled_config();
    config.aps.enabled = true;
    config.aps.shadow_suggestions_enabled = true;
    config.aps.regime_detection_enabled = true;
    config.aps.regime_local_heuristic_enabled = true;
    config.aps.adaptive_enabled = true;
    config.aps.min_calibration_samples = 30;
    config.v25.live_execution_enabled = false;

    let mut features = ghost_core::checkpoint::MaterializedFeatureSet::default();
    features.tx_intel_features.tx_count = 29;
    features.tx_intel_features.buy_count = 24;
    features.tx_intel_features.unique_signers = 12;
    features.tx_intel_features.buy_ratio = 0.82;
    features.tx_intel_features.total_volume_sol = 12.0;
    features.tx_intel_features.avg_interval_ms = 250.0;
    features.tx_intel_features.interval_cv = 0.4;
    features.tx_intel_features.timing_entropy = 2.0;
    features.account_features.market_cap_sol = 55.0;
    features.account_features.bonding_progress = 0.25;
    features.account_features.price_sol = 5.0;
    features.account_features.current_reserves = (50_000_000_000, 900_000_000);
    features.session_metadata.observation_duration_ms = 7_000;
    features
        .checkpoint_features
        .price_change_from_first_checkpoint_pct = 400.0;
    features.checkpoint_features.price_trajectory = vec![1.0, 3.0, 5.0];
    features.checkpoint_features.trajectory_checkpoint_count = 3;
    features.curve_readiness.curve_data_known = true;
    features.curve_readiness.price_sample_count = 3;

    let assessment =
        build_assessment_from_features(features, &config, PolicyEvaluationContext::default());
    let aps = assessment
        .aps_diagnostics
        .as_ref()
        .expect("APS diagnostics must be present");

    assert_eq!(aps.regime, MarketRegime::Normal);
    assert!(
        !assessment.adaptive_thresholds_applied,
        "adaptive thresholds must not apply when sample_count < min_calibration_samples"
    );
}

/// P2: APS drift override only applies in shadow plane (not live).
///
/// When `live_execution_enabled = false` and `adaptive_enabled = true` and
/// regime is HighVolatility, the drift cap is tightened to 3%.
/// When `live_execution_enabled = true`, the override must NOT apply.
#[test]
fn p2_aps_drift_override_only_in_shadow_plane() {
    use ghost_brain::config::GatekeeperMode;
    use ghost_core::checkpoint::{TrajectorySegmentSnapshot, TxSegmentSequence};
    use ghost_launcher::components::gatekeeper_policy::{
        build_assessment_from_features, PolicyEvaluationContext,
    };

    let mut base_config = v25_enabled_config();
    base_config.mode = GatekeeperMode::Long;
    base_config.aps.enabled = true;
    base_config.aps.shadow_suggestions_enabled = true;
    base_config.aps.regime_detection_enabled = true;
    base_config.aps.regime_local_heuristic_enabled = true;
    base_config.pdd.spike_hard_veto = false;
    base_config.v25.live_execution_enabled = false;

    // Set up a MaterializedFeatureSet with entry drift in the 3%-5% window.
    // HighVol is forced via PDD spike materialized in tx_segment_sequence,
    // not via the drift itself, so the test proves the adaptive override path.
    let mut features = ghost_core::checkpoint::MaterializedFeatureSet::default();
    features.tx_intel_features.tx_count = 30;
    features.tx_intel_features.buy_count = 25;
    features.tx_intel_features.unique_signers = 10;
    features.tx_intel_features.buy_ratio = 0.80;
    features.tx_intel_features.total_volume_sol = 12.0;
    features.tx_intel_features.avg_interval_ms = 300.0;
    features.tx_intel_features.interval_cv = 0.5;
    features.tx_intel_features.timing_entropy = 2.0;
    features.tx_intel_features.top3_volume_pct = 0.30;
    features.tx_intel_features.volume_gini = 0.40;
    features.tx_intel_features.hhi = 0.08;
    features.tx_intel_features.same_ms_tx_ratio = 0.1;
    features.tx_intel_features.max_tx_per_signer = 5;
    features.account_features.market_cap_sol = 50.0;
    features.account_features.bonding_progress = 0.20;
    features.account_features.price_sol = 1.0;
    features.account_features.current_reserves = (50_000_000_000, 900_000_000);
    features.session_metadata.observation_duration_ms = 7_000;
    features
        .checkpoint_features
        .price_change_from_first_checkpoint_pct = 4.0;
    features.checkpoint_features.price_trajectory = vec![0.97, 1.0, 1.04];
    features.checkpoint_features.trajectory_checkpoint_count = 3;
    features.curve_readiness.curve_data_known = true;
    features.curve_readiness.price_sample_count = 3;
    features.curve_readiness.wait_elapsed_ms = Some(7_000);
    features.curve_readiness.freshness = ghost_core::CurveFreshnessState::Fresh;
    features.tx_segment_sequence = Some(TxSegmentSequence {
        t0_segment: TrajectorySegmentSnapshot {
            tx_count: 5,
            buy_ratio: 0.80,
            avg_interval_ms: 400.0,
            total_volume_sol: 1.0,
            hhi: 0.08,
            max_single_tx_sol: 0.4,
            same_size_streak: 1,
        },
        t1_segment: TrajectorySegmentSnapshot {
            tx_count: 5,
            buy_ratio: 0.82,
            avg_interval_ms: 350.0,
            total_volume_sol: 1.0,
            hhi: 0.08,
            max_single_tx_sol: 0.5,
            same_size_streak: 2,
        },
        t2_segment: TrajectorySegmentSnapshot {
            tx_count: 5,
            buy_ratio: 0.85,
            avg_interval_ms: 300.0,
            total_volume_sol: 5.0,
            hhi: 0.08,
            max_single_tx_sol: 1.4,
            same_size_streak: 1,
        },
        total_duration_ms: 6_000,
        min_tx_per_segment_satisfied: true,
    });

    let mut shadow_config = base_config.clone();
    shadow_config.aps.adaptive_enabled = true;
    let shadow_assessment = build_assessment_from_features(
        features.clone(),
        &shadow_config,
        PolicyEvaluationContext::default(),
    );
    let shadow_aps = shadow_assessment
        .aps_diagnostics
        .as_ref()
        .expect("shadow aps diagnostics");
    let shadow_pdd = shadow_assessment
        .pdd_assessment
        .as_ref()
        .expect("shadow pdd diagnostics");

    assert_eq!(shadow_assessment.entry_drift_pct, Some(4.0));
    assert_eq!(
        shadow_aps.regime,
        ghost_launcher::components::gatekeeper_adaptive_prosperity::MarketRegime::HighVolatility,
        "spike-driven sequence must trigger HighVol regime for 4% drift case"
    );
    assert!(shadow_assessment.adaptive_thresholds_applied);
    assert_eq!(
        shadow_pdd.hard_fail,
        Some(ghost_launcher::components::gatekeeper_pdd::PddHardFail::EntryDrift)
    );

    let mut shadow_no_adaptive_config = base_config.clone();
    shadow_no_adaptive_config.aps.adaptive_enabled = false;
    let shadow_no_adaptive_assessment = build_assessment_from_features(
        features.clone(),
        &shadow_no_adaptive_config,
        PolicyEvaluationContext::default(),
    );
    let shadow_no_adaptive_pdd = shadow_no_adaptive_assessment
        .pdd_assessment
        .as_ref()
        .expect("shadow no adaptive pdd diagnostics");

    assert!(!shadow_no_adaptive_assessment.adaptive_thresholds_applied);
    assert_eq!(shadow_no_adaptive_pdd.hard_fail, None);

    let mut live_config = shadow_config.clone();
    live_config.v25.live_execution_enabled = true;
    let live_assessment =
        build_assessment_from_features(features, &live_config, PolicyEvaluationContext::default());
    let live_pdd = live_assessment
        .pdd_assessment
        .as_ref()
        .expect("live pdd diagnostics");

    assert!(!live_assessment.adaptive_thresholds_applied);
    assert_eq!(live_pdd.hard_fail, None);
}

// ══════════════════════════════════════════════════════════════════════════════
// P3 — Legacy drift cap test
// ══════════════════════════════════════════════════════════════════════════════

/// P3: Legacy drift cap blocks extreme pump (HF-4).
///
/// `max_price_change_ratio = 1.50` must reject pools where the cumulative
/// price increase exceeds +50% from the initial anchor.
/// Uses `build_assessment_from_features` for precise control over phase6_curve.
#[test]
fn p3_legacy_drift_cap_blocks_extreme_pump() {
    use ghost_brain::config::GatekeeperMode;
    use ghost_launcher::components::gatekeeper_policy::{
        build_assessment_from_features, PolicyEvaluationContext,
    };

    let mut config = v25_enabled_config();
    config.mode = GatekeeperMode::Long;
    // Override the Rust default (4.0) with the P3 safety cap (1.50).
    config.max_price_change_ratio = 1.50;

    // Build features with price_change_ratio=2.5 (> 1.50 cap) and enough
    // price_data_points for HF-4 to evaluate (>=2).
    let mut features = ghost_core::checkpoint::MaterializedFeatureSet::default();
    features.tx_intel_features.tx_count = 15;
    features.tx_intel_features.buy_count = 12;
    features.tx_intel_features.unique_signers = 10;
    features.tx_intel_features.buy_ratio = 0.80;
    features.tx_intel_features.total_volume_sol = 8.0;
    features.tx_intel_features.avg_interval_ms = 300.0;
    features.tx_intel_features.interval_cv = 0.5;
    features.tx_intel_features.timing_entropy = 2.0;
    features.tx_intel_features.top3_volume_pct = 0.30;
    features.account_features.market_cap_sol = 50.0;
    features.account_features.price_sol = 2.0; // current price
    features.account_features.bonding_progress = 0.20;
    features.account_features.current_reserves = (50_000_000_000, 900_000_000);
    features.session_metadata.observation_duration_ms = 7_000;

    // Key: checkpoint features drive phase6_curve.price_change_ratio via
    // bonding_curve_from_features → price_change_from_first_checkpoint_pct.
    // With price_change_from_first_checkpoint_pct = 100.0% and initial/current
    // prices set, price_change_ratio will exceed 1.50.
    features
        .checkpoint_features
        .price_change_from_first_checkpoint_pct = 150.0;
    features.checkpoint_features.price_trajectory = vec![1.0, 1.5, 2.0];
    // Must have enough price_data_points for HF-4 to fire.
    features.checkpoint_features.trajectory_checkpoint_count = 3;
    features.checkpoint_features.single_tx_max_price_impact_pct = 10.0; // below 50% guard
    features.curve_readiness.curve_data_known = true;
    features.curve_readiness.price_sample_count = 3;

    let assessment =
        build_assessment_from_features(features, &config, PolicyEvaluationContext::default());

    // HF-4 (price_change_ratio > max_price_change_ratio) should fire.
    assert!(
        assessment.hard_reject_reason.is_some(),
        "HF-4 must trigger hard reject for price_change_ratio > 1.50. \
         phase6_curve={:?}, hard_reject_reason={:?}",
        assessment.phase6_curve,
        assessment.hard_reject_reason,
    );
    let reason = assessment.hard_reject_reason.as_ref().unwrap();
    assert!(
        reason.contains("price_change_ratio") || reason.contains("HARD_FAIL"),
        "HF-4 reason must mention price_change_ratio, got: {}",
        reason,
    );
}

/// P3: Verify `max_price_change_ratio = 1.50` is set in the config.
#[test]
fn p3_config_has_legacy_drift_cap_1_50() {
    // Verify the TOML file has max_price_change_ratio = 1.50 (not 9999.0).
    let contents = include_str!("../../ghost-brain/ghost_brain_config.toml");
    let doc: toml::Value = toml::from_str(contents).expect("valid TOML");
    let gk = doc.get("gatekeeper_v2").expect("has gatekeeper_v2 section");
    let max_pcr = gk
        .get("max_price_change_ratio")
        .expect("has max_price_change_ratio field")
        .as_float()
        .expect("is float");
    assert!(
        (max_pcr - 1.50).abs() < f64::EPSILON,
        "max_price_change_ratio must be 1.50 (not 9999.0), got {}",
        max_pcr,
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// P4 — Reason code taxonomy tests
// ══════════════════════════════════════════════════════════════════════════════

use ghost_brain::oracle::reason_code::GatekeeperReasonCode;

fn p4_base_assessment() -> GatekeeperAssessment {
    GatekeeperAssessment {
        phase1_passed: true,
        phase2_velocity: None,
        phase2_passed: true,
        phase3_diversity: None,
        phase3_passed: true,
        phase4_volume: None,
        phase4_passed: true,
        phase5_dev: None,
        phase5_passed: true,
        phase6_curve: None,
        phase6_passed: true,
        phases_passed: 6,
        hard_reject_reason: None,
        total_tx_evaluated: 30,
        unique_tx_evaluated: 30,
        unique_signers_evaluated: 20,
        observation_duration_ms: 7_000,
        finalize_lag_ms: 0,
        dust_filtered_count: 0,
        eval_count: 1,
        buy_count: 20,
        decision: None,
        terminal_reason_code: None,
        early_fingerprint: None,
        curve_t0_event_ts_ms: None,
        curve_t0_clock_source: None,
        curve_wait_elapsed_ms: None,
        feature_snapshot: ghost_core::checkpoint::MaterializedFeatureSet::default(),
        checkpoint_count: 0,
        trajectory_available: false,
        v25_shadow_decisions: Vec::new(),
        trajectory: None,
        decision_eval_snapshots: Vec::new(),
        pdd_assessment: None,
        aps_diagnostics: None,
        observation_stage: None,
        entry_drift_pct: None,
        entry_drift_anchor_quality: None,
        adaptive_thresholds_applied: false,
        v25_confidence: None,
    }
}

/// P4b: `to_buy_log()` must read the typed cause from `decision.reason_code`,
/// not reconstruct it from `reason_chain` text.
#[test]
fn p4_to_buy_log_reads_decision_reason_code_not_text() {
    let config = v25_enabled_config();
    let mut assessment = p4_base_assessment();
    assessment.decision = Some(GatekeeperDecision {
        hard_fail_reason: Some("HARD_FAIL: price_change_ratio=9.9".to_string()),
        core1_passed: false,
        core2_passed: false,
        core3_passed: false,
        soft_signals: SoftSignals::default(),
        soft_points: 0,
        max_soft_points_possible: 0,
        effective_max_soft_points: 0,
        dev_unknown: false,
        sybil_policy: SybilPolicyDiagnostics::default(),
        alpha_gate: AlphaGateDiagnostics::not_run(false),
        prosperity_filter: ProsperityFilterDiagnostics::not_run(false),
        total_soft_points: 0,
        verdict_type: GatekeeperVerdictType::RejectHardFail,
        verdict_buy: false,
        reason_chain: "text without price_change marker".to_string(),
        reason_code: Some(GatekeeperReasonCode::HardFailPriceChange),
        gatekeeper_strength: None,
    });

    let buy_log = assessment.to_buy_log(&Pubkey::new_unique(), &config);
    assert_eq!(
        buy_log.reason_code.as_deref(),
        Some("HARD_FAIL_PRICE_CHANGE")
    );
}

#[test]
fn p4_assessment_only_timeout_uses_terminal_reason_code() {
    let config = v25_enabled_config();
    let mut assessment = p4_base_assessment();
    assessment.decision = None;
    assessment.phase1_passed = false;
    assessment.phases_passed = 0;
    assessment.total_tx_evaluated = 0;
    assessment.buy_count = 0;
    assessment.unique_tx_evaluated = 0;
    assessment.unique_signers_evaluated = 0;
    assessment.terminal_reason_code = Some(GatekeeperReasonCode::TimeoutPhase1NoData);

    let buy_log = assessment.to_buy_log(&Pubkey::new_unique(), &config);
    assert_eq!(
        buy_log.reason_code.as_deref(),
        Some("TIMEOUT_PHASE1_NO_DATA")
    );
}

#[test]
fn p4_assessment_only_timeout_without_terminal_reason_code_uses_invariant() {
    let config = v25_enabled_config();
    let mut assessment = p4_base_assessment();
    assessment.decision = None;
    assessment.phase1_passed = false;
    assessment.phases_passed = 0;
    assessment.total_tx_evaluated = 0;
    assessment.buy_count = 0;
    assessment.unique_tx_evaluated = 0;
    assessment.unique_signers_evaluated = 0;
    assessment.terminal_reason_code = None;

    let buy_log = assessment.to_buy_log(&Pubkey::new_unique(), &config);
    assert_eq!(
        buy_log.reason_code.as_deref(),
        Some("INVARIANT_TIMEOUT_NO_VERDICT")
    );
}

#[test]
fn p4_decision_without_reason_code_does_not_get_timeout_fallback() {
    let config = v25_enabled_config();
    let mut assessment = p4_base_assessment();
    assessment.decision = Some(GatekeeperDecision {
        hard_fail_reason: None,
        core1_passed: true,
        core2_passed: true,
        core3_passed: true,
        soft_signals: SoftSignals::default(),
        soft_points: 0,
        max_soft_points_possible: 0,
        effective_max_soft_points: 0,
        dev_unknown: false,
        sybil_policy: SybilPolicyDiagnostics::default(),
        alpha_gate: AlphaGateDiagnostics::not_run(false),
        prosperity_filter: ProsperityFilterDiagnostics::not_run(false),
        total_soft_points: 0,
        verdict_type: GatekeeperVerdictType::RejectCoreFail,
        verdict_buy: false,
        reason_chain: "CORE_FAIL".to_string(),
        reason_code: None,
        gatekeeper_strength: None,
    });

    let buy_log = assessment.to_buy_log(&Pubkey::new_unique(), &config);
    assert_eq!(buy_log.reason_code, None);
}

/// P4: REJECT_SYBIL_INTERFERENCE maps deterministically in runtime buy logs.
#[test]
fn p4_reject_sybil_interference_has_reason_code() {
    let config = v25_enabled_config();
    let mut assessment = p4_base_assessment();
    assessment.decision = Some(GatekeeperDecision {
        hard_fail_reason: None,
        core1_passed: true,
        core2_passed: true,
        core3_passed: true,
        soft_signals: SoftSignals::default(),
        soft_points: 0,
        max_soft_points_possible: 0,
        effective_max_soft_points: 0,
        dev_unknown: false,
        sybil_policy: SybilPolicyDiagnostics::default(),
        alpha_gate: AlphaGateDiagnostics::not_run(false),
        prosperity_filter: ProsperityFilterDiagnostics::not_run(false),
        total_soft_points: 0,
        verdict_type: GatekeeperVerdictType::RejectSybilInterference,
        verdict_buy: false,
        reason_chain: "SYBIL_COMBO: test".to_string(),
        reason_code: Some(GatekeeperReasonCode::RejectSybilInterference),
        gatekeeper_strength: None,
    });

    let log = assessment.to_buy_log(&Pubkey::new_unique(), &config);
    assert_eq!(
        log.verdict_type.as_deref(),
        Some("REJECT_SYBIL_INTERFERENCE")
    );
    assert_eq!(
        log.reason_code.as_deref(),
        Some("REJECT_SYBIL_INTERFERENCE")
    );
}

#[test]
fn p4_iwim_buy_to_reject_mutates_reason_code() {
    let mut assessment = p4_base_assessment();
    assessment.decision = Some(GatekeeperDecision {
        hard_fail_reason: None,
        core1_passed: true,
        core2_passed: true,
        core3_passed: true,
        soft_signals: SoftSignals::default(),
        soft_points: 0,
        max_soft_points_possible: 0,
        effective_max_soft_points: 0,
        dev_unknown: false,
        sybil_policy: SybilPolicyDiagnostics::default(),
        alpha_gate: AlphaGateDiagnostics::not_run(false),
        prosperity_filter: ProsperityFilterDiagnostics::not_run(false),
        total_soft_points: 0,
        verdict_type: GatekeeperVerdictType::Buy,
        verdict_buy: true,
        reason_chain: "BUY: clean".to_string(),
        reason_code: Some(GatekeeperReasonCode::BuyNormal),
        gatekeeper_strength: None,
    });

    let iwim_verdict_type = GatekeeperVerdictType::RejectIwimLowConf;
    if let Some(ref mut decision) = assessment.decision {
        decision.verdict_buy = false;
        decision.verdict_type = iwim_verdict_type;
        decision.reason_chain = format!("{} → IWIM_REJECT: low_conf", decision.reason_chain);
        decision.reason_code = Some(match iwim_verdict_type {
            GatekeeperVerdictType::RejectIwimVeto => GatekeeperReasonCode::RejectIwimVeto,
            GatekeeperVerdictType::RejectIwimLowConf => GatekeeperReasonCode::RejectIwimLowConf,
            _ => GatekeeperReasonCode::RejectIwimUnknownStrict,
        });
    }

    let decision = assessment.decision.expect("decision should remain present");
    assert_eq!(
        decision.verdict_type,
        GatekeeperVerdictType::RejectIwimLowConf
    );
    assert_eq!(
        decision.reason_code,
        Some(GatekeeperReasonCode::RejectIwimLowConf)
    );
}

/// P4: feature-driven timeout builder uses the concrete low-phases timeout subtype.
#[test]
fn p4_timeout_builder_phase1_passed_uses_deadline_low_phases() {
    use ghost_launcher::components::gatekeeper_policy::{
        build_assessment_from_features, build_timeout_decision_from_assessment,
        PolicyEvaluationContext,
    };

    let mut config = v25_enabled_config();
    config.min_tx_count = 3;
    config.min_unique_signers = 3;
    config.min_buy_count = 3;
    config.min_phases_to_pass = 6;

    let mut features = ghost_core::checkpoint::MaterializedFeatureSet::default();
    features.tx_intel_features.tx_count = 5;
    features.tx_intel_features.buy_count = 5;
    features.tx_intel_features.unique_signers = 5;
    features.tx_intel_features.buy_ratio = 1.0;
    features.tx_intel_features.total_volume_sol = 2.0;
    features.session_metadata.observation_duration_ms = 7_000;

    let mut assessment =
        build_assessment_from_features(features, &config, PolicyEvaluationContext::default());
    assert!(assessment.phase1_passed);

    let decision = build_timeout_decision_from_assessment(&assessment, &config);
    assert_eq!(
        decision.verdict_type,
        GatekeeperVerdictType::TimeoutDeadlineLowPhases
    );
    assert!(decision
        .reason_chain
        .starts_with("TIMEOUT_DEADLINE_LOW_PHASES:"));

    assessment.decision = Some(decision);
    let log = assessment.to_buy_log(&Pubkey::new_unique(), &config);
    assert_eq!(
        log.verdict_type.as_deref(),
        Some("TIMEOUT_DEADLINE_LOW_PHASES")
    );
    assert_eq!(
        log.reason_code.as_deref(),
        Some("TIMEOUT_DEADLINE_LOW_PHASES")
    );
}

/// P4: TIMEOUT decision_reason is never null.
#[test]
fn p4_timeout_decision_reason_is_never_null() {
    let mut cfg = v25_enabled_config();
    cfg.max_wait_time_ms = 10_000;
    cfg.min_tx_count = 12;
    cfg.min_unique_signers = 8;
    cfg.min_buy_count = 6;

    let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf.set_registered_wall_t0(1000);

    // No TXs → check_long_deadline produces Timeout with Phase1 never met.
    let verdict = buf.force_check_deadline(11_001);

    match verdict {
        GatekeeperVerdict::Timeout { assessment } => {
            let log = assessment.to_buy_log(&Pubkey::new_unique(), &cfg);
            // P4: TIMEOUT must have both reason_code AND decision_reason populated.
            assert!(
                log.reason_code.is_some(),
                "TIMEOUT must have typed reason_code"
            );
            assert!(
                log.decision_reason.is_some(),
                "TIMEOUT must have decision_reason (not null)"
            );
            assert_eq!(log.reason_code_version, 2);
            let code = log.reason_code.as_ref().unwrap();
            assert!(
                code.contains("TIMEOUT") || code.contains("PHASE1"),
                "TIMEOUT reason_code must indicate timeout phase, got: {}",
                code
            );
            // verdict_type must be a specific TIMEOUT subtype, not generic.
            let vtype = log
                .verdict_type
                .as_ref()
                .expect("verdict_type must be populated");
            assert!(
                vtype.contains("TIMEOUT_PHASE1_NO_DATA"),
                "expected TIMEOUT_PHASE1_NO_DATA, got: {}",
                vtype
            );
            assert!(
                log.decision_reason.as_ref().unwrap().contains("Phase 1"),
                "decision_reason must explain timeout cause"
            );
        }
        other => panic!(
            "expected Timeout verdict, got {:?}",
            std::mem::discriminant(&other)
        ),
    }

    // Add some TXs but not enough for Phase 1 → TimeoutPhase1Insufficient.
    let mut buf2 = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf2.set_registered_wall_t0(1000);
    for i in 0..5 {
        let t = tx(
            &format!("timeout_signer_{}", i),
            1100 + i as u64 * 200,
            true,
            0.5,
        );
        let _ = buf2.ingest_transaction_tracking_only(std::sync::Arc::new(t));
    }
    let verdict2 = buf2.force_check_deadline(11_001);
    match verdict2 {
        GatekeeperVerdict::Timeout { assessment } => {
            let log = assessment.to_buy_log(&Pubkey::new_unique(), &cfg);
            assert!(log.reason_code.is_some());
            assert!(
                log.decision_reason.is_some(),
                "TIMEOUT with insufficient TX must have decision_reason"
            );
            assert_eq!(log.reason_code_version, 2);
            let vtype = log
                .verdict_type
                .as_ref()
                .expect("verdict_type must be populated");
            assert!(
                vtype.contains("TIMEOUT_PHASE1_INSUFFICIENT"),
                "expected TIMEOUT_PHASE1_INSUFFICIENT, got: {}",
                vtype
            );
        }
        other => panic!(
            "expected Timeout verdict, got {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

/// P4: Third TIMEOUT subtype — Phase 1 passed but not enough phases.
/// Uses legacy decision path (use_three_layer_decision = false).
#[test]
fn p4_timeout_deadline_low_phases_subtype() {
    let mut cfg = v25_enabled_config();
    cfg.max_wait_time_ms = 10_000;
    cfg.use_three_layer_decision = false; // legacy path — produces Timeout for low phases
    cfg.min_tx_count = 3;
    cfg.min_unique_signers = 2;
    cfg.min_buy_count = 2;
    cfg.min_phases_to_pass = 6; // require all 6 → will fail

    let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf.set_registered_wall_t0(1000);

    for i in 0..6 {
        let t = tx(&format!("p1_ok_{}", i), 1100 + i as u64 * 200, true, 0.3);
        let _ = buf.ingest_transaction_tracking_only(std::sync::Arc::new(t));
    }

    let verdict = buf.force_check_deadline(11_001);
    match verdict {
        GatekeeperVerdict::Timeout { assessment } => {
            let log = assessment.to_buy_log(&Pubkey::new_unique(), &cfg);
            assert!(
                log.decision_reason.is_some(),
                "Timeout must have decision_reason"
            );
            assert!(log.reason_code.is_some(), "Timeout must have reason_code");
            let vtype = log.verdict_type.as_ref().unwrap();
            assert!(
                vtype.contains("TIMEOUT_DEADLINE_LOW_PHASES"),
                "expected TIMEOUT_DEADLINE_LOW_PHASES verdict_type, got: {}",
                vtype
            );
            // P4 contract: reason_code must match verdict_type for TIMEOUT taxonomy.
            let rcode = log.reason_code.as_ref().unwrap();
            assert_eq!(
                rcode, "TIMEOUT_DEADLINE_LOW_PHASES",
                "reason_code must be TIMEOUT_DEADLINE_LOW_PHASES for legacy low-phases timeout, got: {}",
                rcode
            );
        }
        other => panic!("expected Timeout, got {:?}", std::mem::discriminant(&other)),
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// P5 — Shadow execution lifecycle tests
// ══════════════════════════════════════════════════════════════════════════════

/// P5: ShadowPayerStrategy::Ephemeral exists and is distinct from Configured.
#[test]
fn p5_shadow_payer_strategy_ephemeral_exists() {
    use ghost_launcher::components::trigger::shadow_run::ShadowPayerStrategy;

    let configured = ShadowPayerStrategy::Configured;
    let ephemeral = ShadowPayerStrategy::Ephemeral;
    assert_ne!(configured, ephemeral);
    assert_eq!(
        ShadowPayerStrategy::default(),
        ShadowPayerStrategy::Configured
    );
}

/// P5: Idempotency key generation is deterministic.
#[test]
fn p5_shadow_idempotency_key_deterministic() {
    use ghost_launcher::components::trigger::shadow_run::make_shadow_idempotency_key;

    let key1 = make_shadow_idempotency_key("pool_abc", "join_xyz", "shadow-burnin-v25-repair");
    let key2 = make_shadow_idempotency_key("pool_abc", "join_xyz", "shadow-burnin-v25-repair");
    assert_eq!(key1, key2, "same inputs → same key");

    let key3 = make_shadow_idempotency_key("pool_def", "join_xyz", "shadow-burnin-v25-repair");
    assert_ne!(key1, key3, "different pool → different key");

    // Key must be non-empty hex.
    assert!(!key1.is_empty());
    assert!(key1.chars().all(|c| c.is_ascii_hexdigit()));
}

/// P5: ShadowBuySimulationRecord carries optional idempotency_key.
#[test]
fn p5_shadow_record_has_idempotency_key_field() {
    use ghost_launcher::components::trigger::shadow_run::make_shadow_idempotency_key;
    use ghost_launcher::components::trigger::shadow_run::ShadowBuySimulationRecord;
    use ghost_launcher::events::ExecutionJoinMetadata;

    // Verify the field exists and can be set.
    let record = ShadowBuySimulationRecord {
        join_metadata: ExecutionJoinMetadata::default(),
        account_diagnostics: Default::default(),
        candidate_id: "test".to_string(),
        pool_amm_id: "pool1".to_string(),
        base_mint: "mint1".to_string(),
        entry_mode: "shadow".to_string(),
        decision_ts_ms: 1000,
        sim_started_ts_ms: 1010,
        sim_finished_ts_ms: 1020,
        decision_to_sim_start_ms: 10,
        shadow_duration_ms: 10,
        amount_lamports: 1000,
        tip_lamports: 0,
        entry_token_amount_raw: None,
        payer_provenance: "ephemeral".to_string(),
        payer_pubkey: None,
        err: None,
        error_class: None,
        error_code: None,
        error_detail_class: None,
        units_consumed: None,
        rpc_slot: None,
        retry_count: 0,
        live_signature: None,
        logs_digest: "".to_string(),
        logs_excerpt: vec![],
        idempotency_key: Some(make_shadow_idempotency_key("p", "j", "r")),
    };
    assert!(record.idempotency_key.is_some());
}

// ══════════════════════════════════════════════════════════════════════════════
// WS2: P1 regression — sequence materializes for PDD even when TAS is off
// ══════════════════════════════════════════════════════════════════════════════

/// P1/WS2: `tx_segment_sequence` must materialize for PDD even when TAS is disabled.
/// Before the fix, `current_segment_sequence_from_config()` was gated behind
/// `tas.enabled`, so disabling TAS would also disable PDD sequence signals.
#[test]
fn p1_sequence_materializes_for_pdd_when_tas_disabled() {
    let mut cfg = v25_enabled_config();
    cfg.tas.enabled = false; // TAS scoring off
    cfg.pdd.enabled = true;
    cfg.pdd.spike_detection_enabled = true;
    cfg.pdd.ramping_detection_enabled = true;
    cfg.max_wait_time_ms = 10_000;

    let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
    buf.set_registered_wall_t0(1000);

    // Feed enough TXs for segment division.
    for i in 0..12 {
        let t = tx(
            &format!("s{}", i),
            1200 + i as u64 * 300,
            true,
            0.3 + i as f64 * 0.1,
        );
        let _ = buf.ingest_transaction_tracking_only(std::sync::Arc::new(t));
    }

    // Sequence must be materialized even with TAS disabled.
    let seq = buf.current_segment_sequence_from_config();
    assert!(
        seq.is_some(),
        "tx_segment_sequence must materialize for PDD even when tas.enabled=false"
    );
    let seq = seq.unwrap();
    assert!(seq.min_tx_per_segment_satisfied);
    assert!(seq.t0_segment.tx_count > 0);
    assert!(seq.t1_segment.tx_count > 0);
    assert!(seq.t2_segment.tx_count > 0);
}
