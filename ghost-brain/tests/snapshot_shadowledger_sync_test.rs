//! Integration test for SnapshotEngine → ShadowLedger synchronization (PR-3b contract)
//!
//! PR-3b Pre-commit boundary contract:
//! - SnapshotEngine maintains LOCAL soft-truth state only (ring buffer)
//! - ShadowLedger is written EXCLUSIVELY by:
//!   (a) Gatekeeper commit loop (canonical history)
//!   (b) LivePipeline.flush_mint() (post-commit live events)
//! - SnapshotEngine MUST NOT write pre-commit snapshots to ShadowLedger
//!
//! The old behavior (SnapshotEngine writing snapshots via push_snapshot_with_source
//! during handle_tx_event) was removed in PR-3b to eliminate the competing pre-commit
//! write path.

use ghost_brain::oracle::{
    snapshot_engine::{PoolLifecycle, PoolMetrics},
    DataSource, InitPoolEvent, SnapshotEngine, TxEvent,
};
use ghost_core::shadow_ledger::types::{MarketSnapshot as GhostCoreMarketSnapshot, PriceState};
use ghost_core::shadow_ledger::ShadowLedger;
use seer::types::RawBytesMissingReason;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Helper to get current timestamp in milliseconds
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_millis() as u64
}

/// PR-3b contract: SnapshotEngine does NOT write pre-commit snapshots to ShadowLedger.
/// After handle_initialize_pool_event and handle_tx_event calls:
///   - LOCAL ring buffer accumulates snapshots (accessible via last_n)
///   - ShadowLedger retains ONLY the externally-committed snapshot (no extra writes)
#[test]
fn test_snapshot_engine_does_not_write_shadow_ledger_precommit() {
    // ===== SETUP =====

    let shadow_ledger = Arc::new(ShadowLedger::new());

    let mut engine = SnapshotEngine::new(128, 200);
    engine.set_shadow_ledger(Arc::clone(&shadow_ledger));
    let engine = Arc::new(engine);

    let pool_amm_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let timestamp_ms = now_ms();

    // Seed ShadowLedger with exactly 1 externally-committed snapshot
    let seed_snapshot = GhostCoreMarketSnapshot {
        slot: Some(1000),
        timestamp_ms,
        price_sol_per_token: 0.00001,
        price_state: PriceState::Valid,
        reserve_base: 1_000_000.0,
        reserve_quote: 10.0,
        market_cap_sol: 0.00001 * 1_000_000.0,
        ..Default::default()
    };
    shadow_ledger.commit_history(base_mint, vec![seed_snapshot], None);
    engine.mark_pool_active(pool_amm_id);

    // Verify initial state: 1 committed snapshot in ShadowLedger
    let initial = shadow_ledger
        .get_snapshots(&base_mint)
        .expect("initial snapshot");
    assert_eq!(
        initial.len(),
        1,
        "ShadowLedger should start with 1 committed snapshot"
    );

    // ===== PHASE 1: Bootstrap (handle_initialize_pool_event) =====

    let init_event = InitPoolEvent {
        pool_amm_id,
        base_mint,
        quote_mint: Pubkey::new_unique(),
        slot: Some(1000),
        timestamp_ms,
        initial_liquidity_sol: 10.0,
        initial_reserve_base: 1_000_000.0,
        initial_reserve_quote: 10.0,
        initial_price_quote: 0.00001,
    };
    engine.handle_initialize_pool_event(&init_event);

    // PR-3b contract: ShadowLedger still has ONLY the 1 externally-committed snapshot.
    // Bootstrap snapshots (G0/G1/G2) go into LOCAL ring buffer ONLY.
    let after_bootstrap_ledger = shadow_ledger
        .get_snapshots(&base_mint)
        .expect("ledger after bootstrap");
    assert_eq!(
        after_bootstrap_ledger.len(),
        1,
        "ShadowLedger must NOT receive bootstrap snapshots (PR-3b pre-commit boundary)"
    );

    // LOCAL ring buffer should have the 3 bootstrap snapshots
    let after_bootstrap_local = engine.last_n(&pool_amm_id, 10);
    assert!(
        !after_bootstrap_local.is_empty(),
        "LOCAL ring buffer should have bootstrap snapshots (G0/G1/G2)"
    );

    println!(
        "✅ Phase 1: Bootstrap — local ring buffer has {} snapshots, ShadowLedger unchanged ({})",
        after_bootstrap_local.len(),
        after_bootstrap_ledger.len()
    );

    // ===== PHASE 2: Transaction events =====

    let mut current_time = timestamp_ms.saturating_add(1000);
    let mut cumulative_volume = 0.0;
    let mut cumulative_buy_volume = 0.0;
    let mut cumulative_sell_volume = 0.0;

    for i in 1..=10 {
        let volume_sol = 0.5 + (i as f64 * 0.1);
        cumulative_volume += volume_sol;
        if i % 3 != 0 {
            cumulative_buy_volume += volume_sol;
        } else {
            cumulative_sell_volume += volume_sol;
        }

        let tx_event = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id,
            base_mint,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: i as u64,
                unique_addrs: i as u64,
                volume_sol: cumulative_volume,
                buy_volume_sol: cumulative_buy_volume,
                sell_volume_sol: cumulative_sell_volume,
                dev_buy_lamports: 0,
                ..Default::default()
            },
            slot: Some(1000u64.saturating_add(i)),
            timestamp_ms: current_time,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: Pubkey::new_unique(),
            is_buy: i % 3 != 0,
            volume_sol,
            reserve_base: Some(1_000_000.0 - (i as f64 * 1000.0)),
            reserve_quote: Some(10.0 + (i as f64 * 0.5)),
            price_quote: Some(0.00001 * (1.0 + i as f64 * 0.01)),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some(format!("tx_sig_{}", i)),
            event_ordinal: None,
            block_time: Some((current_time / 1000) as i64),
            arrival_time_ms: Some(current_time.saturating_add(50)),
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
        };

        engine.handle_tx_event(&tx_event);

        // Advance time past snapshot interval (200ms)
        current_time = current_time.saturating_add(250);
    }

    // PR-3b contract: ShadowLedger still has ONLY the 1 externally-committed snapshot.
    // handle_tx_event must NOT write to ShadowLedger.
    let after_txs_ledger = shadow_ledger
        .get_snapshots(&base_mint)
        .expect("ledger after txs");
    assert_eq!(
        after_txs_ledger.len(),
        1,
        "ShadowLedger must NOT receive tx snapshots (PR-3b: only {} allowed, got {})",
        1,
        after_txs_ledger.len()
    );

    // LOCAL ring buffer should have grown
    let after_txs_local = engine.last_n(&pool_amm_id, 50);
    assert!(
        after_txs_local.len() > after_bootstrap_local.len(),
        "LOCAL ring buffer should grow with tx snapshots (bootstrap={}, after_txs={})",
        after_bootstrap_local.len(),
        after_txs_local.len()
    );

    println!(
        "✅ Phase 2: Tx events — local ring buffer has {} snapshots, ShadowLedger unchanged ({})",
        after_txs_local.len(),
        after_txs_ledger.len()
    );

    // ===== PHASE 3: Verify snapshot evolution in LOCAL ring buffer =====

    let first_snapshot = &after_txs_local[after_txs_local.len() - 1]; // oldest
    let last_snapshot = &after_txs_local[0]; // newest (last_n returns newest-first)

    assert!(
        last_snapshot.tx_count >= first_snapshot.tx_count,
        "Transaction count should not decrease (first={}, last={})",
        first_snapshot.tx_count,
        last_snapshot.tx_count
    );

    println!(
        "✅ Phase 3: Snapshot evolution — tx_count: {} → {}, volume: {:.2} → {:.2}",
        first_snapshot.tx_count,
        last_snapshot.tx_count,
        first_snapshot.cum_volume_sol,
        last_snapshot.cum_volume_sol
    );

    println!("\n🎉 TEST PASSED: PR-3b pre-commit boundary contract enforced correctly!");
}

/// PR-3b backward compatibility: SnapshotEngine works without ShadowLedger.
#[test]
fn test_snapshot_sync_without_shadowledger() {
    let engine = Arc::new(SnapshotEngine::new(128, 200));
    let pool_amm_id = Pubkey::new_unique();
    let timestamp_ms = now_ms();
    engine.mark_pool_active(pool_amm_id);

    let base_mint = Pubkey::new_unique();
    let init_event = InitPoolEvent {
        pool_amm_id,
        base_mint,
        quote_mint: Pubkey::new_unique(),
        slot: Some(1000),
        timestamp_ms,
        initial_liquidity_sol: 10.0,
        initial_reserve_base: 1_000_000.0,
        initial_reserve_quote: 10.0,
        initial_price_quote: 0.00001,
    };
    engine.handle_initialize_pool_event(&init_event);

    let tx_event = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id,
        base_mint,
        pool_state: PoolLifecycle::Active,
        metrics: PoolMetrics {
            tx_count: 1,
            unique_addrs: 1,
            volume_sol: 1.0,
            buy_volume_sol: 1.0,
            sell_volume_sol: 0.0,
            dev_buy_lamports: 0,
            ..Default::default()
        },
        slot: Some(1001),
        timestamp_ms: timestamp_ms.saturating_add(250),
        event_time: ghost_core::EventTimeMetadata::default(),
        signer: Pubkey::new_unique(),
        is_buy: true,
        volume_sol: 1.0,
        reserve_base: Some(999_000.0),
        reserve_quote: Some(11.0),
        price_quote: Some(0.000011),
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some("test_sig".to_string()),
        event_ordinal: None,
        block_time: Some(((timestamp_ms.saturating_add(250)) / 1000) as i64),
        arrival_time_ms: Some(timestamp_ms.saturating_add(300)),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
    };
    engine.handle_tx_event(&tx_event);

    let snapshots = engine.last_n(&pool_amm_id, 10);
    assert!(
        !snapshots.is_empty(),
        "SnapshotEngine should still work without ShadowLedger"
    );

    println!("✅ Backward compatibility: SnapshotEngine works without ShadowLedger");
}

/// PR-3b: ShadowLedger external writes (commit_history) are independent of SnapshotEngine.
/// Verifying that set_shadow_ledger does not cause unsolicited writes.
#[test]
fn test_shadow_ledger_not_polluted_by_snapshot_engine() {
    let shadow_ledger = Arc::new(ShadowLedger::new());
    let mut engine = SnapshotEngine::new(128, 200);
    engine.set_shadow_ledger(Arc::clone(&shadow_ledger));
    let engine = Arc::new(engine);

    let pool_amm_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let timestamp_ms = now_ms();

    // ShadowLedger starts empty for this mint
    assert!(
        shadow_ledger.get_snapshots(&base_mint).is_none(),
        "ShadowLedger should be empty before any commits"
    );

    engine.mark_pool_active(pool_amm_id);

    let init_event = InitPoolEvent {
        pool_amm_id,
        base_mint,
        quote_mint: Pubkey::new_unique(),
        slot: Some(1000),
        timestamp_ms,
        initial_liquidity_sol: 10.0,
        initial_reserve_base: 1_000_000.0,
        initial_reserve_quote: 10.0,
        initial_price_quote: 0.00001,
    };
    engine.handle_initialize_pool_event(&init_event);

    // Even after bootstrap, ShadowLedger must remain empty for this mint
    assert!(
        shadow_ledger.get_snapshots(&base_mint).is_none(),
        "SnapshotEngine bootstrap must NOT write to ShadowLedger"
    );

    // Process a transaction
    let tx_event = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id,
        base_mint,
        pool_state: PoolLifecycle::Active,
        metrics: PoolMetrics {
            tx_count: 1,
            unique_addrs: 1,
            volume_sol: 1.0,
            buy_volume_sol: 1.0,
            sell_volume_sol: 0.0,
            dev_buy_lamports: 0,
            ..Default::default()
        },
        slot: Some(1001),
        timestamp_ms: timestamp_ms.saturating_add(250),
        event_time: ghost_core::EventTimeMetadata::default(),
        signer: Pubkey::new_unique(),
        is_buy: true,
        volume_sol: 1.0,
        reserve_base: Some(999_000.0),
        reserve_quote: Some(11.0),
        price_quote: Some(0.000011),
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some("tx_only_sig".to_string()),
        event_ordinal: None,
        block_time: Some(((timestamp_ms.saturating_add(250)) / 1000) as i64),
        arrival_time_ms: Some(timestamp_ms.saturating_add(300)),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
    };
    engine.handle_tx_event(&tx_event);

    // ShadowLedger still empty — no write from SnapshotEngine
    assert!(
        shadow_ledger.get_snapshots(&base_mint).is_none(),
        "SnapshotEngine handle_tx_event must NOT write to ShadowLedger (PR-3b contract)"
    );

    // LOCAL ring buffer has data
    let local = engine.last_n(&pool_amm_id, 10);
    assert!(!local.is_empty(), "LOCAL ring buffer should have snapshots");

    println!(
        "✅ PR-3b: ShadowLedger not polluted by SnapshotEngine (pre-commit boundary enforced)"
    );
}
