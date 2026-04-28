use ghost_core::account_state_core::types::{AccountStateFeatures, StatePhase};
use ghost_core::checkpoint::{
    CheckpointConfig, CheckpointEngine, CheckpointProducer, CheckpointTrigger,
    EventCheckpointTrigger,
};
use ghost_core::tx_intelligence::types::{RiskFlag, RiskSeverity, TxIntelFeatures};
use ghost_core::CurveFinality;
use std::borrow::Cow;

fn account_features(price_sol: f64) -> AccountStateFeatures {
    AccountStateFeatures {
        current_reserves: (100, 900),
        price_sol,
        market_cap_sol: 42.0,
        bonding_progress: 0.12,
        price_change_since_t0_pct: 5.0,
        reserve_velocity_sol_per_sec: 1.0,
        is_bootstrap: false,
        curve_finality: CurveFinality::Finalized,
        state_phase: StatePhase::Canonical,
        update_count: 3,
    }
}

fn tx_features(tx_count: u64, unique_signers: u64) -> TxIntelFeatures {
    TxIntelFeatures {
        tx_count,
        buy_count: tx_count.saturating_sub(1),
        sell_count: 1,
        unique_signers,
        buy_ratio: 0.8,
        sol_buy_ratio: 0.85,
        avg_tx_sol: 1.2,
        volume_cv: 0.2,
        hhi: 0.18,
        volume_gini: 0.11,
        unique_signer_ratio: 0.6,
        avg_tx_per_signer: 1.5,
        same_ms_tx_ratio: 0.05,
        bundle_suspicion_ratio: 0.0,
        top3_volume_pct: 0.45,
        dev_buy_sol: 0.0,
        dev_volume_ratio: 0.0,
        dev_tx_ratio: 0.0,
        dev_has_sold: false,
        interval_cv: 0.4,
        timing_entropy: 1.5,
        avg_interval_ms: 120.0,
        burst_ratio: 0.1,
        dust_ratio: 0.0,
        max_tx_per_signer: 0,
        total_volume_sol: tx_count as f64 * 1.2,
        min_tx_sol: 1.2,
        max_tx_sol: 1.2,
        max_consecutive_buys: tx_count.saturating_sub(1),
        dev_wallet_known: false,
        dev_initial_buy_tokens: None,
        dev_tx_count: 0,
        dev_is_first_buyer: false,
        dust_tx_count: 0,
        failed_tx_count: 0,
    }
}

fn dev_sell_flag() -> RiskFlag {
    RiskFlag {
        flag_id: Cow::Borrowed("dev_has_sold"),
        severity: RiskSeverity::Hard,
        detected_at_ms: 3_000,
        detail: "developer sold".to_string(),
    }
}

#[test]
fn checkpoint_engine_emits_time_based_checkpoint_after_interval_and_tx_minimum() {
    let mut engine = CheckpointEngine::default();
    let features = tx_features(5, 3);

    let trigger = engine.evaluate_trigger(2_000, None, &features, &[], None);
    assert_eq!(trigger, Some(CheckpointTrigger::TimeBased(2_000)));

    let checkpoint = engine.create_checkpoint(&account_features(1.0), &features, &[]);
    assert_eq!(checkpoint.checkpoint_id, 1);
    assert_eq!(checkpoint.timestamp_ms, 2_000);
    assert_eq!(checkpoint.tx_intel_snapshot.tx_count, 5);
}

#[test]
fn checkpoint_engine_emits_dev_sell_event_once() {
    let mut engine = CheckpointEngine::new(CheckpointConfig {
        interval_ms: 10_000,
        min_tx_between_checkpoints: 1,
        enable_event_checkpoints: true,
        event_triggers: vec![EventCheckpointTrigger::DevSell],
    });
    let mut features = tx_features(2, 2);
    features.dev_has_sold = true;

    let trigger = engine.evaluate_trigger(1_500, None, &features, &[dev_sell_flag()], None);
    assert_eq!(
        trigger,
        Some(CheckpointTrigger::EventBased("dev_sell".to_string()))
    );
    let checkpoint =
        engine.create_checkpoint(&account_features(1.0), &features, &[dev_sell_flag()]);

    let repeated = engine.evaluate_trigger(
        1_900,
        Some(&checkpoint),
        &features,
        &[dev_sell_flag()],
        None,
    );
    assert_eq!(repeated, None);
}

#[test]
fn checkpoint_engine_emits_large_trade_impact_event() {
    let mut engine = CheckpointEngine::new(CheckpointConfig {
        interval_ms: 10_000,
        min_tx_between_checkpoints: 1,
        enable_event_checkpoints: true,
        event_triggers: vec![EventCheckpointTrigger::LargeTradeImpact(12.5)],
    });

    let trigger = engine.evaluate_trigger(1_250, None, &tx_features(1, 1), &[], Some(18.0));
    match trigger {
        Some(CheckpointTrigger::EventBased(label)) => {
            assert!(label.starts_with("large_trade_impact:"));
        }
        other => panic!("unexpected trigger: {other:?}"),
    }
}

#[test]
fn checkpoint_engine_emits_signer_milestone_once_per_threshold() {
    let mut engine = CheckpointEngine::new(CheckpointConfig {
        interval_ms: 10_000,
        min_tx_between_checkpoints: 1,
        enable_event_checkpoints: true,
        event_triggers: vec![EventCheckpointTrigger::SignerCountMilestone(3)],
    });

    let trigger = engine.evaluate_trigger(900, None, &tx_features(3, 3), &[], None);
    assert_eq!(
        trigger,
        Some(CheckpointTrigger::EventBased(
            "signer_count_milestone:3".to_string()
        ))
    );
    let checkpoint = engine.create_checkpoint(&account_features(1.0), &tx_features(3, 3), &[]);

    let repeated = engine.evaluate_trigger(1_400, Some(&checkpoint), &tx_features(4, 3), &[], None);
    assert_eq!(repeated, None);
}
