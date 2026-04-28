//! Integration Test: WHF Part 1 - Flowfield Construction & Extraction
//!
//! This test demonstrates the complete flowfield extraction pipeline from
//! transaction streams to aggregated flow vectors.

use ghost_brain::chaos::{
    flow_transaction_from_pool_event, FlowDirection, FlowTransaction, FlowVector, FlowfieldConfig,
    FlowfieldExtractor, DEFAULT_WINDOW_MS, MAX_WINDOW_MS, MIN_WINDOW_MS,
};
use solana_sdk::pubkey::Pubkey;

#[test]
fn test_flowfield_basic_construction() {
    let mut extractor = FlowfieldExtractor::new();

    let wallet1 = Pubkey::new_unique();
    let wallet2 = Pubkey::new_unique();
    let wallet3 = Pubkey::new_unique();

    // Simulate a sequence of transactions
    let transactions = vec![
        FlowTransaction {
            slot: 1000,
            wallet: wallet1,
            is_buy: true,
            volume_sol: 10.0,
            timestamp_ms: 1000,
        },
        FlowTransaction {
            slot: 1000,
            wallet: wallet2,
            is_buy: true,
            volume_sol: 15.0,
            timestamp_ms: 1050,
        },
        FlowTransaction {
            slot: 1001,
            wallet: wallet3,
            is_buy: false,
            volume_sol: 8.0,
            timestamp_ms: 1500,
        },
        FlowTransaction {
            slot: 1001,
            wallet: wallet1,
            is_buy: false,
            volume_sol: 5.0,
            timestamp_ms: 1600,
        },
    ];

    for tx in transactions {
        assert!(extractor.process_transaction(tx));
    }

    // Verify slot aggregation
    let slot_1000 = extractor.get_slot_flow(1000).unwrap();
    assert_eq!(slot_1000.buy, 25.0);
    assert_eq!(slot_1000.sell, 0.0);
    assert_eq!(slot_1000.wallets, 2);
    assert_eq!(slot_1000.net, 25.0);
    assert_eq!(slot_1000.flow_direction(), FlowDirection::Accumulation);

    let slot_1001 = extractor.get_slot_flow(1001).unwrap();
    assert_eq!(slot_1001.buy, 0.0);
    assert_eq!(slot_1001.sell, 13.0);
    assert_eq!(slot_1001.wallets, 2);
    assert_eq!(slot_1001.net, -13.0);
    assert_eq!(slot_1001.flow_direction(), FlowDirection::Distribution);

    // Verify wallet aggregation
    let wallet1_flow = extractor.get_wallet_flow(&wallet1).unwrap();
    assert_eq!(wallet1_flow.buy, 10.0);
    assert_eq!(wallet1_flow.sell, 5.0);
    assert_eq!(wallet1_flow.net, 5.0);

    // Verify aggregate flow
    let agg = extractor.get_aggregate_flow();
    assert_eq!(agg.buy, 25.0);
    assert_eq!(agg.sell, 13.0);
    assert_eq!(agg.wallets, 3);
    assert_eq!(agg.net, 12.0);
}

#[test]
fn test_flowfield_rolling_window() {
    // Configure 5-second window
    let config = FlowfieldConfig::with_window(5000);
    let mut extractor = FlowfieldExtractor::with_config(config);

    let wallet = Pubkey::new_unique();

    // Add transactions over time
    for i in 0..10 {
        let tx = FlowTransaction {
            slot: 1000 + i,
            wallet,
            is_buy: i % 2 == 0,
            volume_sol: 10.0,
            timestamp_ms: 1000 + (i * 1000), // Every 1 second
        };
        extractor.process_transaction(tx);
    }

    // At t=10000, only transactions from t=5000 onwards should remain
    // That's transactions 5, 6, 7, 8, 9 (5 transactions)
    assert_eq!(extractor.window_transaction_count(), 5);

    // Verify slots in window
    let slot_flows = extractor.get_all_slot_flows();
    assert_eq!(slot_flows.len(), 5);

    // First slot in window should be 1005
    assert_eq!(slot_flows[0].0, 1005);
}

#[test]
fn test_flowfield_accumulation_detection() {
    let mut extractor = FlowfieldExtractor::new();

    let mut time = 1000u64;

    // Simulate strong accumulation: many wallets buying
    for _ in 0..10 {
        let wallet = Pubkey::new_unique();
        extractor.process_transaction(FlowTransaction {
            slot: 1000,
            wallet,
            is_buy: true,
            volume_sol: 10.0,
            timestamp_ms: time,
        });
        time += 100;
    }

    let flow = extractor.get_slot_flow(1000).unwrap();
    assert_eq!(flow.buy, 100.0);
    assert_eq!(flow.sell, 0.0);
    assert_eq!(flow.wallets, 10);
    assert_eq!(flow.flow_direction(), FlowDirection::Accumulation);

    let agg = extractor.get_aggregate_flow();
    assert_eq!(agg.flow_direction(), FlowDirection::Accumulation);
    assert!(agg.buy_sell_ratio().is_none()); // No sells
}

#[test]
fn test_flowfield_distribution_detection() {
    let mut extractor = FlowfieldExtractor::new();

    let mut time = 1000u64;

    // Simulate distribution: many wallets selling
    for _ in 0..10 {
        let wallet = Pubkey::new_unique();
        extractor.process_transaction(FlowTransaction {
            slot: 2000,
            wallet,
            is_buy: false,
            volume_sol: 5.0,
            timestamp_ms: time,
        });
        time += 100;
    }

    let flow = extractor.get_slot_flow(2000).unwrap();
    assert_eq!(flow.buy, 0.0);
    assert_eq!(flow.sell, 50.0);
    assert_eq!(flow.wallets, 10);
    assert_eq!(flow.flow_direction(), FlowDirection::Distribution);

    let agg = extractor.get_aggregate_flow();
    assert_eq!(agg.flow_direction(), FlowDirection::Distribution);
}

#[test]
fn test_flowfield_balanced_flow() {
    let mut extractor = FlowfieldExtractor::new();

    let mut time = 1000u64;

    // Simulate balanced flow: equal buy/sell
    for i in 0..10 {
        let wallet = Pubkey::new_unique();
        extractor.process_transaction(FlowTransaction {
            slot: 3000,
            wallet,
            is_buy: i % 2 == 0,
            volume_sol: 10.0,
            timestamp_ms: time,
        });
        time += 100;
    }

    let flow = extractor.get_slot_flow(3000).unwrap();
    assert_eq!(flow.buy, 50.0);
    assert_eq!(flow.sell, 50.0);
    assert_eq!(flow.net, 0.0);
    assert_eq!(flow.flow_direction(), FlowDirection::Neutral);

    assert_eq!(flow.buy_sell_ratio(), Some(1.0));
}

#[test]
fn test_flowfield_multi_slot_aggregation() {
    let mut extractor = FlowfieldExtractor::new();

    let wallet = Pubkey::new_unique();
    let mut time = 1000u64;

    // Spread transactions across multiple slots
    for slot in 1000..1010 {
        extractor.process_transaction(FlowTransaction {
            slot,
            wallet,
            is_buy: true,
            volume_sol: 5.0,
            timestamp_ms: time,
        });
        time += 500;
    }

    let all_slots = extractor.get_all_slot_flows();
    assert_eq!(all_slots.len(), 10);

    // Each slot should have 5.0 SOL buy
    for (_, flow) in &all_slots {
        assert_eq!(flow.buy, 5.0);
        assert_eq!(flow.sell, 0.0);
    }

    // Aggregate should sum all
    let agg = extractor.get_aggregate_flow();
    assert_eq!(agg.buy, 50.0);
}

#[test]
fn test_flowfield_wallet_tracking() {
    let mut extractor = FlowfieldExtractor::new();

    let wallet1 = Pubkey::new_unique();
    let wallet2 = Pubkey::new_unique();

    // Wallet 1 buys multiple times
    for slot in 1000..1003 {
        extractor.process_transaction(FlowTransaction {
            slot,
            wallet: wallet1,
            is_buy: true,
            volume_sol: 10.0,
            timestamp_ms: 1000 + (slot - 1000) * 100,
        });
    }

    // Wallet 2 sells multiple times
    for slot in 1000..1003 {
        extractor.process_transaction(FlowTransaction {
            slot,
            wallet: wallet2,
            is_buy: false,
            volume_sol: 5.0,
            timestamp_ms: 1000 + (slot - 1000) * 100,
        });
    }

    let w1_flow = extractor.get_wallet_flow(&wallet1).unwrap();
    assert_eq!(w1_flow.buy, 30.0);
    assert_eq!(w1_flow.sell, 0.0);

    let w2_flow = extractor.get_wallet_flow(&wallet2).unwrap();
    assert_eq!(w2_flow.buy, 0.0);
    assert_eq!(w2_flow.sell, 15.0);

    assert_eq!(extractor.window_wallet_count(), 2);
}

#[test]
fn test_pool_event_conversion() {
    let wallet = Pubkey::new_unique();
    let wallet_str = wallet.to_string();

    let flow_tx = flow_transaction_from_pool_event(12345, &wallet_str, true, 25.5, 5000).unwrap();

    assert_eq!(flow_tx.slot, 12345);
    assert_eq!(flow_tx.wallet, wallet);
    assert_eq!(flow_tx.is_buy, true);
    assert_eq!(flow_tx.volume_sol, 25.5);
    assert_eq!(flow_tx.timestamp_ms, 5000);
}

#[test]
fn test_flowfield_config_validation() {
    // Test minimum window (input below min should clamp to MIN_WINDOW_MS)
    let config1 = FlowfieldConfig::with_window(MIN_WINDOW_MS - 1000);
    assert_eq!(config1.window_ms, MIN_WINDOW_MS);

    // Test maximum window (input above max should clamp to MAX_WINDOW_MS)
    let config2 = FlowfieldConfig::with_window(MAX_WINDOW_MS + 1000);
    assert_eq!(config2.window_ms, MAX_WINDOW_MS);

    // Test valid window (within range should be preserved)
    let config3 = FlowfieldConfig::with_window(DEFAULT_WINDOW_MS);
    assert_eq!(config3.window_ms, DEFAULT_WINDOW_MS);

    // Test default
    let config4 = FlowfieldConfig::default();
    assert_eq!(config4.window_ms, DEFAULT_WINDOW_MS);
    assert!(config4.enable_slot_aggregation);
    assert!(config4.enable_wallet_aggregation);
}

#[test]
fn test_flowfield_clear() {
    let mut extractor = FlowfieldExtractor::new();

    let wallet = Pubkey::new_unique();
    extractor.process_transaction(FlowTransaction {
        slot: 1000,
        wallet,
        is_buy: true,
        volume_sol: 10.0,
        timestamp_ms: 1000,
    });

    assert_eq!(extractor.window_transaction_count(), 1);

    extractor.clear();

    assert_eq!(extractor.window_transaction_count(), 0);
    assert_eq!(extractor.window_slot_count(), 0);
    assert_eq!(extractor.window_wallet_count(), 0);
}

#[test]
fn test_flowfield_time_series_output() {
    let mut extractor = FlowfieldExtractor::new();

    let wallet = Pubkey::new_unique();
    let mut time = 1000u64;

    // Create a time series of flow data
    for i in 0..5 {
        let slot = 1000 + i;
        extractor.process_transaction(FlowTransaction {
            slot,
            wallet,
            is_buy: i % 2 == 0,
            volume_sol: 10.0 + (i as f32),
            timestamp_ms: time,
        });
        time += 1000;
    }

    // Get time-ordered slot flows
    let slot_flows = extractor.get_all_slot_flows();
    assert_eq!(slot_flows.len(), 5);

    // Verify slots are ordered
    for i in 0..5 {
        assert_eq!(slot_flows[i].0, 1000 + i as u64);
    }

    // Verify flow vectors form a time series F(t)
    assert_eq!(slot_flows[0].1.buy, 10.0); // t=0, buy
    assert_eq!(slot_flows[1].1.sell, 11.0); // t=1, sell
    assert_eq!(slot_flows[2].1.buy, 12.0); // t=2, buy
    assert_eq!(slot_flows[3].1.sell, 13.0); // t=3, sell
    assert_eq!(slot_flows[4].1.buy, 14.0); // t=4, buy
}

#[test]
fn test_flowfield_high_activity_scenario() {
    let mut extractor = FlowfieldExtractor::new();

    let mut time = 1000u64;

    // Simulate high-activity scenario: 100 wallets, 500 transactions
    for slot_offset in 0..50 {
        let slot = 10000 + slot_offset;

        for tx_idx in 0..10 {
            let wallet = Pubkey::new_unique();
            // Alternate buy/sell deterministically
            let is_buy = (slot_offset + tx_idx) % 2 == 0;
            // Vary volume based on index for diversity
            let volume = 1.0 + ((slot_offset * 10 + tx_idx) as f32 % 10.0);

            extractor.process_transaction(FlowTransaction {
                slot,
                wallet,
                is_buy,
                volume_sol: volume,
                timestamp_ms: time,
            });

            time += 10; // 10ms between transactions
        }
    }

    // Verify we tracked all the data
    assert_eq!(extractor.window_transaction_count(), 500);
    assert_eq!(extractor.window_slot_count(), 50);

    // Aggregate should have all wallets
    let agg = extractor.get_aggregate_flow();
    assert_eq!(agg.wallets, 500); // All unique wallets

    // Total volume should be sum of all transactions
    assert!(agg.buy + agg.sell > 0.0);
}
