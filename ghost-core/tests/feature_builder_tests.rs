use ghost_core::account_state_core::types::{AccountStateFeatures, StatePhase};
use ghost_core::checkpoint::{
    CheckpointDerivedFeatures, CheckpointTrigger, FeatureMaterializer, ObservationFeatureBuilder,
    SessionCheckpoint, TrendDirection,
};
use ghost_core::session::types::{SessionId, SessionMetadata};
use ghost_core::tx_intelligence::types::{RiskFlag, RiskSeverity, TxIntelFeatures};
use ghost_core::CurveFinality;
use solana_sdk::pubkey::Pubkey;
use std::borrow::Cow;

fn account_features(price_sol: f64, reserves: (u64, u64), progress: f64) -> AccountStateFeatures {
    AccountStateFeatures {
        current_reserves: reserves,
        price_sol,
        market_cap_sol: price_sol * 1_000.0,
        bonding_progress: progress,
        price_change_since_t0_pct: 0.0,
        reserve_velocity_sol_per_sec: 0.5,
        is_bootstrap: false,
        curve_finality: CurveFinality::Finalized,
        state_phase: StatePhase::Canonical,
        update_count: 4,
    }
}

fn tx_features(tx_count: u64, buy_ratio: f64, unique_ratio: f64) -> TxIntelFeatures {
    TxIntelFeatures {
        tx_count,
        buy_count: (tx_count as f64 * buy_ratio) as u64,
        sell_count: tx_count.saturating_sub((tx_count as f64 * buy_ratio) as u64),
        unique_signers: (tx_count as f64 * unique_ratio).ceil() as u64,
        buy_ratio,
        sol_buy_ratio: buy_ratio,
        avg_tx_sol: 1.0,
        volume_cv: 0.2,
        hhi: 0.15,
        volume_gini: 0.12,
        unique_signer_ratio: unique_ratio,
        avg_tx_per_signer: 1.3,
        same_ms_tx_ratio: 0.05,
        bundle_suspicion_ratio: 0.01,
        top3_volume_pct: 0.4,
        dev_buy_sol: 0.0,
        dev_volume_ratio: 0.0,
        dev_tx_ratio: 0.0,
        dev_has_sold: false,
        interval_cv: 0.35,
        timing_entropy: 1.8,
        avg_interval_ms: 110.0,
        burst_ratio: 0.08,
        dust_ratio: 0.0,
        max_tx_per_signer: 0,
        total_volume_sol: tx_count as f64,
        min_tx_sol: 1.0,
        max_tx_sol: 1.0,
        max_consecutive_buys: tx_count,
        dev_wallet_known: false,
        dev_initial_buy_tokens: None,
        dev_tx_count: 0,
        dev_is_first_buyer: false,
        dust_tx_count: 0,
        failed_tx_count: 0,
    }
}

fn checkpoint(
    id: u32,
    timestamp_ms: u64,
    account: AccountStateFeatures,
    tx: TxIntelFeatures,
    risk_flags: Vec<RiskFlag>,
) -> SessionCheckpoint {
    SessionCheckpoint {
        checkpoint_id: id,
        timestamp_ms,
        trigger: CheckpointTrigger::TimeBased(timestamp_ms),
        account_state_snapshot: account,
        tx_intel_snapshot: tx,
        risk_flags,
    }
}

fn risk_flag(flag_id: &'static str, detected_at_ms: u64) -> RiskFlag {
    RiskFlag {
        flag_id: Cow::Borrowed(flag_id),
        severity: RiskSeverity::Soft(2),
        detected_at_ms,
        detail: flag_id.to_string(),
    }
}

#[test]
fn feature_builder_materializes_complete_feature_set() {
    let builder = ObservationFeatureBuilder;
    let checkpoints = vec![
        checkpoint(
            1,
            1_000,
            account_features(1.0, (100, 900), 0.10),
            tx_features(2, 0.50, 0.40),
            vec![],
        ),
        checkpoint(
            2,
            2_000,
            account_features(1.2, (150, 850), 0.20),
            tx_features(4, 0.70, 0.50),
            vec![risk_flag("burst_watch", 2_000)],
        ),
    ];
    let account = account_features(1.5, (200, 800), 0.30);
    let tx = tx_features(6, 0.85, 0.65);
    let feature_set = builder.materialize(
        account.clone(),
        tx.clone(),
        &checkpoints,
        vec![
            risk_flag("dev_watch", 2_500),
            risk_flag("bundle_watch", 2_600),
        ],
        SessionMetadata {
            session_id: SessionId(7),
            pool_amm_id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            observation_duration_ms: 3_000,
            is_dev_known: true,
        },
    );

    assert_eq!(feature_set.account_features, account);
    assert_eq!(feature_set.tx_intel_features, tx);
    assert_eq!(
        feature_set.checkpoint_features.trajectory_checkpoint_count,
        2
    );
    assert_eq!(
        feature_set.checkpoint_features.price_trajectory,
        vec![1.0, 1.2]
    );
    assert_eq!(
        feature_set.checkpoint_features.reserve_trajectory,
        vec![(100, 900), (150, 850)]
    );
    assert_eq!(
        feature_set.checkpoint_features.buy_pressure_trend,
        TrendDirection::Rising
    );
    assert_eq!(
        feature_set.checkpoint_features.signer_diversity_trend,
        TrendDirection::Rising
    );
    assert_eq!(
        feature_set.checkpoint_features.risk_flag_count_trend,
        TrendDirection::Rising
    );
    assert!(
        (feature_set
            .checkpoint_features
            .price_change_from_first_checkpoint_pct
            - 50.0)
            .abs()
            < 1e-9
    );
    assert!(
        (feature_set
            .checkpoint_features
            .single_tx_max_price_impact_pct
            - 25.0)
            .abs()
            < 1e-9
    );
    assert_eq!(
        feature_set.checkpoint_features.max_single_sell_impact_pct,
        0.0
    );
    assert!((feature_set.checkpoint_features.bonding_progress - 0.30).abs() < 1e-9);
}

#[test]
fn feature_builder_materializes_sell_impact_from_reserve_trajectory() {
    let builder = ObservationFeatureBuilder;
    let checkpoints = vec![
        checkpoint(
            1,
            1_000,
            account_features(1.0, (100, 900), 0.10),
            tx_features(2, 0.50, 0.40),
            vec![],
        ),
        checkpoint(
            2,
            2_000,
            account_features(1.2, (120, 880), 0.20),
            tx_features(4, 0.70, 0.50),
            vec![],
        ),
        checkpoint(
            3,
            3_000,
            account_features(0.9, (110, 890), 0.25),
            tx_features(5, 0.60, 0.55),
            vec![],
        ),
    ];
    let feature_set = builder.materialize(
        account_features(0.8, (100, 900), 0.30),
        tx_features(6, 0.55, 0.60),
        &checkpoints,
        vec![],
        SessionMetadata {
            session_id: SessionId(8),
            pool_amm_id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            observation_duration_ms: 4_000,
            is_dev_known: false,
        },
    );

    assert!((feature_set.checkpoint_features.max_single_sell_impact_pct - 25.0).abs() < 1e-9);
}

#[test]
fn feature_builder_returns_insufficient_trends_without_checkpoint_history() {
    let builder = ObservationFeatureBuilder;
    let feature_set = builder.materialize(
        account_features(1.0, (100, 900), 0.15),
        tx_features(1, 1.0, 1.0),
        &[],
        vec![],
        SessionMetadata {
            session_id: SessionId(1),
            pool_amm_id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            observation_duration_ms: 0,
            is_dev_known: false,
        },
    );

    assert_eq!(
        feature_set.checkpoint_features,
        CheckpointDerivedFeatures {
            price_trajectory: vec![],
            reserve_trajectory: vec![],
            buy_pressure_trend: TrendDirection::Insufficient,
            signer_diversity_trend: TrendDirection::Insufficient,
            risk_flag_count_trend: TrendDirection::Insufficient,
            trajectory_checkpoint_count: 0,
            price_change_from_first_checkpoint_pct: 0.0,
            single_tx_max_price_impact_pct: 0.0,
            max_single_sell_impact_pct: 0.0,
            bonding_progress: 0.15,
        }
    );
}
