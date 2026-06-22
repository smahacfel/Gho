use ghost_brain::config::GatekeeperV2Config;
use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_core::checkpoint::{
    MetricEvidenceQuality, TemporalAnchorReachedBy, TemporalMetricSource,
};
use ghost_core::{CurveFinality, EventSemanticEnvelope};
use ghost_launcher::events::{PoolTransaction, RawBytesMissingReason};
use ghost_launcher::session::{OpenSessionRequest, SessionConfig, SessionManager};
use ghost_launcher::tx_intelligence::FundingSourceConfig;
use seer::early_fingerprint::EarlyFingerprintConfig;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn test_candidate(pool_id: Pubkey, base_mint: Pubkey, bonding_curve: Pubkey) -> EnhancedCandidate {
    let mut candidate = EnhancedCandidate::default();
    candidate.pool_amm_id = pool_id;
    candidate.base_mint = base_mint;
    candidate.bonding_curve = bonding_curve;
    candidate.timestamp = 10_000;
    candidate
}

fn temporal_config() -> GatekeeperV2Config {
    let mut config = GatekeeperV2Config::default();
    config.max_wait_time_ms = 5_000;
    config.temporal_carry_forward_enabled = true;
    config.temporal_carry_forward_event_counters_enabled = true;
    config.temporal_carry_forward_state_metrics_enabled = false;
    config.temporal_carry_forward_ratio_metrics_enabled = false;
    config.temporal_carry_forward_max_staleness_ms = 1_000;
    config
}

fn open_session(
    manager: &SessionManager,
    config: GatekeeperV2Config,
) -> (ghost_launcher::session::SharedSession, Pubkey) {
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    manager
        .open_session(OpenSessionRequest {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            dev_wallet: Some(Pubkey::new_unique()),
            candidate_snapshot: test_candidate(pool_id, base_mint, bonding_curve),
            created_at_wall_ms: 10_000,
            deadline_wall_ms: Some(15_000),
            gatekeeper_config: config.clone(),
            funding_source_config: FundingSourceConfig::from_gatekeeper_config(&config),
            fingerprint_config: EarlyFingerprintConfig::default(),
        })
        .expect("session should open");
    (
        manager
            .get_session(&pool_id)
            .expect("session must be retrievable"),
        pool_id,
    )
}

fn temporal_tx(
    pool_id: Pubkey,
    signer: Pubkey,
    signature: &str,
    timestamp_ms: u64,
    is_buy: bool,
    volume_sol: f64,
    market_cap_sol: Option<f64>,
    price_quote: Option<f64>,
    jito_tip_detected: Option<bool>,
) -> Arc<PoolTransaction> {
    Arc::new(PoolTransaction {
        semantic: EventSemanticEnvelope::default(),
        pool_amm_id: pool_id.to_string(),
        slot: Some(timestamp_ms / 400),
        event_ordinal: Some(0),
        tx_index: None,
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
        sol_amount_lamports: Some((volume_sol.abs() * 1_000_000_000.0) as u64),
        token_amount_units: Some(1_000_000),
        reserve_base: None,
        reserve_quote: None,
        price_quote,
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
        market_cap_sol,
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
        jito_tip_detected,
        toolchain_fingerprint: seer::types::ToolchainFingerprintInput::default(),
        curve_data_known: market_cap_sol.is_some() || price_quote.is_some(),
        curve_finality: CurveFinality::Speculative,
    })
}

#[test]
fn event_counter_no_event_2s_to_3s_emits_carried_delta_with_status() {
    let manager = SessionManager::new(SessionConfig {
        default_observation_duration_ms: 5_000,
        max_sessions: 4,
        ..SessionConfig::default()
    });
    let (session, pool_id) = open_session(&manager, temporal_config());

    {
        let mut guard = session.write();
        let _ = guard.ingest_transaction(temporal_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-pr3-t0-buy",
            10_000,
            true,
            2.0,
            Some(10.0),
            Some(0.000010),
            Some(false),
        ));
        let _ = guard.ingest_transaction(temporal_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-pr3-1s-sell",
            11_000,
            false,
            4.0,
            Some(12.0),
            Some(0.000012),
            Some(false),
        ));
        let _ = guard.ingest_transaction(temporal_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-pr3-2s-buy",
            12_000,
            true,
            1.0,
            Some(13.0),
            Some(0.000013),
            Some(true),
        ));
        guard.created_at_instant = Instant::now() - Duration::from_millis(3_500);
        guard.begin_evaluation();
    }

    let features = session.read().materialize_features();
    let temporal = features.temporal_deltas;

    assert!(temporal.anchor_3s.reached);
    assert_eq!(
        temporal.anchor_3s.reached_by,
        TemporalAnchorReachedBy::ObservationElapsed
    );
    assert_eq!(
        temporal.anchor_3s.event_counters_evidence.quality,
        MetricEvidenceQuality::CarriedForward
    );
    assert_eq!(
        temporal.anchor_3s.event_counters_evidence.source,
        TemporalMetricSource::CarriedForwardNoEvent
    );
    assert_eq!(
        temporal
            .anchor_3s
            .event_counters_evidence
            .carried_from_anchor_ms,
        Some(2_000)
    );
    assert_eq!(
        temporal.anchor_3s.event_counters_evidence.staleness_ms,
        Some(1_000)
    );

    assert_eq!(temporal.delta_net_quote_sol_1s_to_2s, Some(1.0));
    assert_eq!(temporal.delta_net_quote_sol_1s_to_3s, Some(1.0));
    assert_eq!(temporal.delta_net_quote_sol_2s_to_3s, Some(0.0));
    assert_eq!(temporal.delta_tx_count_2s_to_3s, Some(0));
    assert_eq!(temporal.rate_net_quote_sol_per_s_1s_to_3s, Some(0.5));

    let delta_evidence = temporal
        .delta_evidence
        .get("delta_net_quote_sol_1s_to_3s")
        .expect("delta evidence should exist");
    assert_eq!(
        delta_evidence.quality,
        MetricEvidenceQuality::CarriedForward
    );
    assert_eq!(
        delta_evidence.source,
        TemporalMetricSource::CarriedForwardNoEvent
    );
    assert_eq!(delta_evidence.carried_from_anchor_ms, Some(2_000));
    assert_eq!(delta_evidence.staleness_ms, Some(1_000));

    let rate_evidence = temporal
        .delta_evidence
        .get("rate_net_quote_sol_per_s_1s_to_3s")
        .expect("rate evidence should inherit delta evidence");
    assert_eq!(
        rate_evidence.source,
        TemporalMetricSource::CarriedForwardNoEvent
    );
}

#[test]
fn state_and_ratio_no_event_default_off_stays_unavailable_not_clean() {
    let manager = SessionManager::new(SessionConfig {
        default_observation_duration_ms: 5_000,
        max_sessions: 4,
        ..SessionConfig::default()
    });
    let (session, pool_id) = open_session(&manager, temporal_config());

    {
        let mut guard = session.write();
        let _ = guard.ingest_transaction(temporal_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-pr3-state-t0",
            10_000,
            true,
            1.0,
            Some(10.0),
            Some(0.000010),
            Some(false),
        ));
        let _ = guard.ingest_transaction(temporal_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-pr3-state-2s",
            12_000,
            true,
            1.0,
            Some(12.0),
            Some(0.000012),
            Some(false),
        ));
        guard.created_at_instant = Instant::now() - Duration::from_millis(3_500);
        guard.begin_evaluation();
    }

    let temporal = session.read().materialize_features().temporal_deltas;

    assert_eq!(temporal.anchor_3s.market_cap_sol, None);
    assert_eq!(temporal.anchor_3s.burst_ratio, None);
    assert_eq!(
        temporal.anchor_3s.state_metrics_evidence.quality,
        MetricEvidenceQuality::NotAllowed
    );
    assert_eq!(
        temporal.anchor_3s.ratio_metrics_evidence.quality,
        MetricEvidenceQuality::NotAllowed
    );
    assert_eq!(temporal.delta_mcap_2s_to_3s, None);
    assert_eq!(temporal.delta_burstratio_2s_to_3s, None);
    assert_eq!(
        temporal
            .delta_evidence
            .get("delta_mcap_2s_to_3s")
            .expect("mcap evidence")
            .reason
            .as_deref(),
        Some("state_carry_forward_not_allowed")
    );
    assert_eq!(
        temporal
            .delta_evidence
            .get("delta_burstratio_2s_to_3s")
            .expect("burst evidence")
            .reason
            .as_deref(),
        Some("ratio_carry_forward_not_allowed")
    );
}

#[test]
fn future_state_value_after_anchor_does_not_backfill_earlier_anchors() {
    let manager = SessionManager::new(SessionConfig {
        default_observation_duration_ms: 5_000,
        max_sessions: 4,
        ..SessionConfig::default()
    });
    let mut config = temporal_config();
    config.temporal_carry_forward_state_metrics_enabled = true;
    let (session, pool_id) = open_session(&manager, config);

    {
        let mut guard = session.write();
        let _ = guard.ingest_transaction(temporal_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-pr3-future-t0",
            10_000,
            true,
            1.0,
            None,
            None,
            None,
        ));
        let _ = guard.ingest_transaction(temporal_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-pr3-future-after-3s",
            13_200,
            true,
            1.0,
            Some(99.0),
            Some(0.000099),
            None,
        ));
        guard.created_at_instant = Instant::now() - Duration::from_millis(3_500);
        guard.begin_evaluation();
    }

    let temporal = session.read().materialize_features().temporal_deltas;

    assert_eq!(temporal.anchor_1s.market_cap_sol, None);
    assert_eq!(temporal.anchor_2s.market_cap_sol, None);
    assert_eq!(temporal.anchor_3s.market_cap_sol, None);
    assert_eq!(temporal.delta_mcap_1s_to_3s, None);
    assert_eq!(
        temporal
            .delta_evidence
            .get("delta_mcap_1s_to_3s")
            .expect("future-fill guard evidence")
            .reason
            .as_deref(),
        Some("anchor_value_unavailable")
    );
}

#[test]
fn max_staleness_blocks_event_counter_carry_forward() {
    let manager = SessionManager::new(SessionConfig {
        default_observation_duration_ms: 5_000,
        max_sessions: 4,
        ..SessionConfig::default()
    });
    let mut config = temporal_config();
    config.temporal_carry_forward_max_staleness_ms = 500;
    let (session, pool_id) = open_session(&manager, config);

    {
        let mut guard = session.write();
        let _ = guard.ingest_transaction(temporal_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-pr3-stale-t0",
            10_000,
            true,
            1.0,
            Some(10.0),
            Some(0.000010),
            None,
        ));
        let _ = guard.ingest_transaction(temporal_tx(
            pool_id,
            Pubkey::new_unique(),
            "sig-pr3-stale-1s",
            11_000,
            true,
            1.0,
            Some(11.0),
            Some(0.000011),
            None,
        ));
        guard.created_at_instant = Instant::now() - Duration::from_millis(3_500);
        guard.begin_evaluation();
    }

    let temporal = session.read().materialize_features().temporal_deltas;

    assert_eq!(temporal.anchor_3s.tx_count, None);
    assert_eq!(
        temporal.anchor_3s.event_counters_evidence.quality,
        MetricEvidenceQuality::Stale
    );
    assert_eq!(
        temporal.anchor_3s.event_counters_evidence.source,
        TemporalMetricSource::Stale
    );
    assert_eq!(
        temporal.anchor_3s.event_counters_evidence.reason.as_deref(),
        Some("stale")
    );
    assert_eq!(temporal.delta_tx_count_1s_to_3s, None);
    let evidence = temporal
        .delta_evidence
        .get("delta_tx_count_1s_to_3s")
        .expect("stale delta evidence");
    assert_eq!(evidence.source, TemporalMetricSource::Stale);
    assert_eq!(evidence.reason.as_deref(), Some("stale"));
}
