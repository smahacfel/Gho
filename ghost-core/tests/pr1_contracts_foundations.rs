use ghost_core::account_state_core::monotonic_guard::MonotonicUpdateGuard;
use ghost_core::account_state_core::types::{
    AccountStateFeatures, AccountStateUpdate, CanonicalPoolState, StatePhase, UpdateSource,
};
use ghost_core::checkpoint::types::{
    CheckpointDerivedFeatures, CheckpointTrigger, EvidenceStatus, MaterializedFeatureSet,
    SessionCheckpoint, TrendDirection,
};
use ghost_core::session::types::{
    SessionDiagnostics, SessionId, SessionMetadata, SessionStatus, VerdictOutcome,
};
use ghost_core::tx_intelligence::types::{
    BurstWindow, RiskFlag, RiskSeverity, SybilResistanceFeatures, TxIntelFeatures,
    TxIntelligenceState, DBIA_INSUFFICIENT_BUYERS_REASON, DBIA_NO_DEV_BUY_REASON,
    DBIA_PARTIAL_FINGERPRINT_COVERAGE, DBIA_RAW_FINGERPRINT_UNAVAILABLE_REASON,
    CPV_COVERAGE_WINDOW_UNAVAILABLE, DES_NO_COMPARABLE_PAIRS,
    DES_PARTIAL_SEQUENCE_COVERAGE, FTDI_PARTIAL_FEE_TOPOLOGY_COVERAGE,
    FSC_COVERAGE_WINDOW_UNAVAILABLE, FSC_V2_STATUS_NOT_CLEAN,
    SFD_BUY_AMOUNT_UNAVAILABLE, SFD_NEGATIVE_BALANCE_DELTA_SKIPPED,
};
use ghost_core::CurveFinality;
use solana_sdk::pubkey::Pubkey;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

fn pk(seed: u8) -> Pubkey {
    Pubkey::new_from_array([seed; 32])
}

#[test]
fn monotonic_update_guard_enforces_same_slot_tie_breaking() {
    let mut guard = MonotonicUpdateGuard::default();

    assert!(guard.accept(120, None, 1));
    assert!(!guard.accept(119, None, 99));
    assert!(!guard.accept(120, None, 1));
    assert!(guard.accept(120, None, 2));
    assert!(!guard.accept(120, None, 0));
    assert!(guard.accept(121, None, 0));
}

#[test]
fn state_phase_transitions_preserve_bootstrap_vs_canonical_separation() {
    assert!(StatePhase::Bootstrap.can_transition_to(StatePhase::PendingConfirmation));
    assert!(StatePhase::Bootstrap.can_transition_to(StatePhase::Canonical));
    assert!(StatePhase::PendingConfirmation.can_transition_to(StatePhase::Canonical));
    assert!(StatePhase::Canonical.can_transition_to(StatePhase::Migrated));

    assert!(!StatePhase::Canonical.can_transition_to(StatePhase::Bootstrap));
    assert!(!StatePhase::Migrated.can_transition_to(StatePhase::Canonical));
    assert!(!StatePhase::PendingConfirmation.can_transition_to(StatePhase::Bootstrap));
}

#[test]
fn foundational_types_serialize_and_deserialize_roundtrip() {
    let account_update = AccountStateUpdate {
        pool_amm_id: pk(1),
        base_mint: pk(2),
        bonding_curve: pk(3),
        sol_reserves: 11,
        token_reserves: 22,
        is_complete: 0,
        slot: 77,
        write_version: Some(3),
        receive_ts_ms: 1_700_000_000_000,
        receive_seq: 5,
        curve_finality: CurveFinality::Provisional,
        source: UpdateSource::GeyserAccountUpdate,
    };

    let encoded = bincode::serialize(&account_update).expect("serialize account update");
    let decoded: AccountStateUpdate =
        bincode::deserialize(&encoded).expect("deserialize account update");
    assert_eq!(decoded, account_update);

    let state = CanonicalPoolState {
        pool_amm_id: pk(4),
        base_mint: pk(5),
        bonding_curve: pk(6),
        virtual_sol_reserves: 10,
        virtual_token_reserves: 20,
        real_sol_reserves: 7,
        real_token_reserves: 13,
        bonding_curve_progress: 0.42,
        price_sol: 0.123,
        market_cap_sol: 777.0,
        token_total_supply: 1_000_000,
        is_complete: false,
        last_update_slot: 99,
        last_update_ts_ms: 1_700_000_000_123,
        curve_finality: CurveFinality::Finalized,
        state_phase: StatePhase::Canonical,
        update_count: 9,
        initial_price_sol: 0.123,
        price_change_since_t0_pct: 0.0,
        reserve_velocity_sol_per_sec: 0.0,
    };

    let state_encoded = bincode::serialize(&state).expect("serialize canonical state");
    let state_decoded: CanonicalPoolState =
        bincode::deserialize(&state_encoded).expect("deserialize canonical state");
    assert_eq!(state_decoded, state);

    let status = SessionStatus::Decided(VerdictOutcome::Pass {
        reason: "policy_pass".to_string(),
    });
    let status_encoded = bincode::serialize(&status).expect("serialize session status");
    let status_decoded: SessionStatus =
        bincode::deserialize(&status_encoded).expect("deserialize session status");
    assert_eq!(status_decoded, status);

    let mut unique_signers = HashSet::new();
    unique_signers.insert(pk(7));
    unique_signers.insert(pk(8));

    let mut signer_volume_map = HashMap::new();
    signer_volume_map.insert(pk(7), 1.5);
    signer_volume_map.insert(pk(8), 2.5);

    let tx_state = TxIntelligenceState {
        total_buys: 4,
        total_sells: 1,
        total_tx: 5,
        unique_signers,
        buy_volume_sol: 4.5,
        sell_volume_sol: 1.0,
        dev_buy_lamports: 100,
        dev_has_sold: false,
        dev_tx_count: 1,
        signer_volume_map,
        tx_intervals_ms: vec![10, 20, 30],
        burst_windows: vec![BurstWindow {
            start_ts_ms: 100,
            end_ts_ms: 200,
            tx_count: 3,
        }],
        bundle_suspicion_count: 1,
        same_ms_tx_count: 2,
        dust_tx_count: 0,
        failed_tx_count: 0,
    };

    let tx_state_encoded = bincode::serialize(&tx_state).expect("serialize tx state");
    let tx_state_decoded: TxIntelligenceState =
        bincode::deserialize(&tx_state_encoded).expect("deserialize tx state");
    assert_eq!(tx_state_decoded, tx_state);
}

#[test]
fn materialized_feature_set_contains_complete_inputs() {
    let feature_set = MaterializedFeatureSet {
        account_features: AccountStateFeatures {
            current_reserves: (123, 456),
            price_sol: 0.00123,
            market_cap_sol: 321.0,
            bonding_progress: 0.64,
            price_change_since_t0_pct: 12.5,
            reserve_velocity_sol_per_sec: 2.0,
            is_bootstrap: false,
            curve_finality: CurveFinality::Finalized,
            state_phase: StatePhase::Canonical,
            update_count: 7,
        },
        tx_intel_features: TxIntelFeatures {
            tx_count: 9,
            buy_count: 8,
            sell_count: 1,
            unique_signers: 5,
            buy_ratio: 0.88,
            sol_buy_ratio: 0.91,
            avg_tx_sol: 1.3,
            volume_cv: 0.3,
            hhi: 0.22,
            volume_gini: 0.19,
            unique_signer_ratio: 0.55,
            avg_tx_per_signer: 1.8,
            same_ms_tx_ratio: 0.1,
            bundle_suspicion_ratio: 0.05,
            top3_volume_pct: 0.67,
            dev_buy_sol: 0.75,
            dev_volume_ratio: 0.2,
            dev_tx_ratio: 0.11,
            dev_has_sold: false,
            interval_cv: 0.4,
            timing_entropy: 1.7,
            avg_interval_ms: 85.0,
            burst_ratio: 0.33,
            dust_ratio: 0.02,
            max_tx_per_signer: 3,
            total_volume_sol: 11.7,
            min_tx_sol: 0.1,
            max_tx_sol: 3.4,
            max_consecutive_buys: 4,
            dev_wallet_known: true,
            dev_initial_buy_tokens: Some(12_345.0),
            dev_tx_count: 1,
            dev_is_first_buyer: true,
            dust_tx_count: 0,
            failed_tx_count: 0,
        },
        checkpoint_features: CheckpointDerivedFeatures {
            price_trajectory: vec![0.001, 0.0011, 0.0013],
            reserve_trajectory: vec![(100, 900), (150, 850), (200, 800)],
            buy_pressure_trend: TrendDirection::Rising,
            signer_diversity_trend: TrendDirection::Stable,
            risk_flag_count_trend: TrendDirection::Falling,
            trajectory_checkpoint_count: 3,
            price_change_from_first_checkpoint_pct: 30.0,
            single_tx_max_price_impact_pct: 4.5,
            max_single_sell_impact_pct: 2.0,
            bonding_progress: 0.64,
            trajectory_assessment: None,
        },
        risk_flags: vec![RiskFlag {
            flag_id: Cow::Borrowed("bundle_suspected"),
            severity: RiskSeverity::Soft(2),
            detected_at_ms: 1_700_000_000_321,
            detail: "same-ms burst detected".to_string(),
        }],
        session_metadata: SessionMetadata {
            session_id: SessionId(44),
            pool_amm_id: pk(9),
            base_mint: pk(10),
            observation_duration_ms: 3_000,
            is_dev_known: true,
        },
        curve_readiness: Default::default(),
        sybil_resistance: SybilResistanceFeatures {
            fee_topology_diversity_index: Some(0.42),
            dev_buyer_infrastructure_affinity: Some(0.18),
            spend_fraction_divergence: None,
            demand_elasticity_score: Some(0.55),
            signer_cross_pool_velocity: None,
            funding_source_concentration: Some(0.27),
            funding_source_diagnostics: None,
            funding_source_v2: None,
            cpv_distinct_other_pools_mean: None,
            cpv_other_pool_activity_count_p95: None,
            toolchain_fingerprint_coverage: None,
            des_valid_sequence_coverage: None,
            degraded_reasons: vec!["DBIA_NO_DEV_BUY".to_string()],
            buy_sample_count: 9,
            signer_sample_count: 5,
        },
        alpha_fingerprint: Default::default(),
        ..Default::default()
    };

    let checkpoint = SessionCheckpoint {
        checkpoint_id: 1,
        timestamp_ms: 1_700_000_000_222,
        trigger: CheckpointTrigger::EventBased("milestone".to_string()),
        account_state_snapshot: feature_set.account_features.clone(),
        tx_intel_snapshot: feature_set.tx_intel_features.clone(),
        risk_flags: feature_set.risk_flags.clone(),
    };

    assert_eq!(checkpoint.account_state_snapshot.update_count, 7);
    assert_eq!(feature_set.account_features.current_reserves, (123, 456));
    assert_eq!(feature_set.tx_intel_features.unique_signers, 5);
    assert_eq!(
        feature_set.checkpoint_features.trajectory_checkpoint_count,
        3
    );
    assert_eq!(feature_set.risk_flags.len(), 1);
    assert!(feature_set.session_metadata.is_dev_known);
    assert_eq!(
        feature_set.sybil_resistance.fee_topology_diversity_index,
        Some(0.42)
    );
    assert_eq!(
        feature_set.sybil_resistance.degraded_reasons,
        vec!["DBIA_NO_DEV_BUY".to_string()]
    );
    assert_eq!(
        feature_set.evidence_status.account_state.status,
        EvidenceStatus::Unavailable
    );
    assert!(!feature_set.organic_broadening.sequence_available);
    assert!(
        !feature_set
            .manipulation_contradictions
            .sybil_evidence_degraded
    );

    let diagnostics = SessionDiagnostics {
        total_tx_seen: feature_set.tx_intel_features.tx_count,
        total_account_updates: feature_set.account_features.update_count,
        checkpoint_count: feature_set.checkpoint_features.trajectory_checkpoint_count,
        first_tx_ts_ms: Some(1_700_000_000_001),
        last_tx_ts_ms: Some(1_700_000_000_321),
        reject_reasons: vec![],
    };

    assert_eq!(diagnostics.total_tx_seen, 9);
    assert_eq!(diagnostics.total_account_updates, 7);
    assert_eq!(diagnostics.checkpoint_count, 3);
}

#[test]
fn pr1_public_quality_reason_codes_contract_is_complete() {
    let mut expected = [
        FTDI_PARTIAL_FEE_TOPOLOGY_COVERAGE,
        DBIA_PARTIAL_FINGERPRINT_COVERAGE,
        DES_PARTIAL_SEQUENCE_COVERAGE,
        DES_NO_COMPARABLE_PAIRS,
        SFD_NEGATIVE_BALANCE_DELTA_SKIPPED,
        SFD_BUY_AMOUNT_UNAVAILABLE,
        CPV_COVERAGE_WINDOW_UNAVAILABLE,
        FSC_V2_STATUS_NOT_CLEAN,
        FSC_COVERAGE_WINDOW_UNAVAILABLE,
    ];
    expected.sort_unstable();

    let mut observed = [
        DBIA_NO_DEV_BUY_REASON,
        DBIA_INSUFFICIENT_BUYERS_REASON,
        DBIA_RAW_FINGERPRINT_UNAVAILABLE_REASON,
        FTDI_PARTIAL_FEE_TOPOLOGY_COVERAGE,
        DBIA_PARTIAL_FINGERPRINT_COVERAGE,
        DES_PARTIAL_SEQUENCE_COVERAGE,
        DES_NO_COMPARABLE_PAIRS,
        SFD_NEGATIVE_BALANCE_DELTA_SKIPPED,
        SFD_BUY_AMOUNT_UNAVAILABLE,
        CPV_COVERAGE_WINDOW_UNAVAILABLE,
        FSC_V2_STATUS_NOT_CLEAN,
        FSC_COVERAGE_WINDOW_UNAVAILABLE,
    ];
    observed.sort_unstable();

    assert_eq!(expected.len(), 9);
    assert!(expected.iter().all(|code| observed.binary_search(code).is_ok()));
    assert_eq!(observed.len(), observed.iter().filter(|code| code.is_empty() == false).count());

    for code in observed.iter() {
        assert!(!code.is_empty(), "reason code string must be non-empty");
    }
}
