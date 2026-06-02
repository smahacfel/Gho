//! Integration tests for Ghost Event Bus and Metrics Server
//!
//! Tests the unified memory bus (GhostEvent) and Prometheus metrics server.

use ghost_brain::{
    E2EMetrics, GhostMetrics, MetricsServer, MetricsServerConfig, DEFAULT_METRICS_PORT,
};

mod event_bus_tests {
    use ghost_launcher::events::{
        create_event_bus, create_event_bus_with_capacity, DetectedPool, GhostEvent, TradeResult,
        EVENT_BUS_BUFFER_SIZE,
    };
    use std::sync::Arc;

    #[tokio::test]
    async fn test_event_bus_basic_send_receive() {
        let (tx, mut rx) = create_event_bus();

        let pool = DetectedPool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: "test_pool".to_string(),
            base_mint: "test_mint".to_string(),
            quote_mint: "sol".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            slot: Some(12345),
            tx_index: None,
            timestamp_ms: 1700000000000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: None,
            initial_liquidity_sol: Some(10.0),
            signature: "test_sig".to_string(),
            creator: String::new(),
        };

        // Send event
        tx.send(GhostEvent::new_pool_detected(pool.clone()))
            .unwrap();

        // Receive event
        let event = rx.recv().await.unwrap();
        assert_eq!(event.event_type(), "new_pool_detected");

        if let GhostEvent::NewPoolDetected(received_pool) = event {
            assert_eq!(received_pool.pool_amm_id, "test_pool");
            assert_eq!(received_pool.slot, Some(12345));
        } else {
            panic!("Expected NewPoolDetected event");
        }
    }

    #[tokio::test]
    async fn test_event_bus_multiple_subscribers() {
        let (tx, _rx) = create_event_bus();

        // Create multiple subscribers
        let mut rx1 = tx.subscribe();
        let mut rx2 = tx.subscribe();
        let mut rx3 = tx.subscribe();

        // Send event
        tx.send(GhostEvent::transaction_sent("sig123", Some(100), "buy"))
            .unwrap();

        // All subscribers should receive the event
        let e1 = rx1.recv().await.unwrap();
        let e2 = rx2.recv().await.unwrap();
        let e3 = rx3.recv().await.unwrap();

        assert_eq!(e1.event_type(), "transaction_sent");
        assert_eq!(e2.event_type(), "transaction_sent");
        assert_eq!(e3.event_type(), "transaction_sent");
    }

    #[tokio::test]
    async fn test_event_bus_trade_executed() {
        let (tx, mut rx) = create_event_bus();

        let trade = TradeResult {
            signature: "trade_sig".to_string(),
            mint: "token_mint".to_string(),
            sol_amount: 1.5,
            token_amount: 1_000_000.0,
            entry_price: 0.0000015,
            is_buy: false, // Sell
            slot: Some(99999),
            pnl_sol: Some(0.25), // 0.25 SOL profit
            timestamp: 1700000001,
        };

        tx.send(GhostEvent::trade_executed(trade)).unwrap();

        let event = rx.recv().await.unwrap();
        assert_eq!(event.event_type(), "trade_executed");

        if let GhostEvent::TradeExecuted(result) = event {
            assert!(!result.is_buy);
            assert_eq!(result.pnl_sol, Some(0.25));
        } else {
            panic!("Expected TradeExecuted event");
        }
    }

    #[tokio::test]
    async fn test_event_bus_custom_event() {
        let (tx, mut rx) = create_event_bus();

        let data = serde_json::json!({
            "custom_field": "custom_value",
            "number": 42
        });

        tx.send(GhostEvent::custom("my_event", data.clone()))
            .unwrap();

        let event = rx.recv().await.unwrap();
        assert_eq!(event.event_type(), "custom");

        if let GhostEvent::Custom(event_type, payload) = event {
            assert_eq!(event_type, "my_event");
            assert_eq!(payload["custom_field"], "custom_value");
            assert_eq!(payload["number"], 42);
        } else {
            panic!("Expected Custom event");
        }
    }

    #[tokio::test]
    async fn test_event_bus_zero_copy_arc() {
        let (tx, mut rx1) = create_event_bus();
        let mut rx2 = tx.subscribe();

        let pool = DetectedPool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: "shared_pool".to_string(),
            base_mint: "shared_mint".to_string(),
            quote_mint: "sol".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            slot: Some(54321),
            tx_index: None,
            timestamp_ms: 1700000000000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: None,
            initial_liquidity_sol: Some(5.0),
            signature: "shared_sig".to_string(),
            creator: String::new(),
        };

        tx.send(GhostEvent::new_pool_detected(pool)).unwrap();

        let event1 = rx1.recv().await.unwrap();
        let event2 = rx2.recv().await.unwrap();

        // Both events should share the same Arc<DetectedPool>
        if let (GhostEvent::NewPoolDetected(pool1), GhostEvent::NewPoolDetected(pool2)) =
            (event1, event2)
        {
            // Arc::ptr_eq checks if they point to the same allocation
            assert!(Arc::ptr_eq(&pool1, &pool2));
        } else {
            panic!("Expected NewPoolDetected events");
        }
    }

    #[tokio::test]
    async fn test_event_bus_with_custom_capacity() {
        let (tx, mut rx) = create_event_bus_with_capacity(100);

        // Send multiple events
        for i in 0..50 {
            tx.send(GhostEvent::transaction_sent(
                format!("sig_{}", i),
                Some(i as u64),
                "buy",
            ))
            .unwrap();
        }

        // Receive all events
        for i in 0..50 {
            let event = rx.recv().await.unwrap();
            if let GhostEvent::TransactionSent {
                signature, slot, ..
            } = event
            {
                assert_eq!(signature, format!("sig_{}", i));
                assert_eq!(slot, Some(i as u64));
            }
        }
    }

    #[test]
    fn test_default_buffer_size() {
        assert_eq!(EVENT_BUS_BUFFER_SIZE, 1024);
    }
}

mod metrics_server_tests {
    use super::*;
    use prometheus::Registry;

    #[test]
    fn test_metrics_server_config_default() {
        let config = MetricsServerConfig::default();
        assert_eq!(config.port, DEFAULT_METRICS_PORT);
        assert_eq!(config.bind_address, "0.0.0.0");
    }

    #[test]
    fn test_ghost_metrics_creation() {
        let registry = Registry::new();
        let ghost_metrics = GhostMetrics::new(&registry);

        // Record some metrics
        ghost_metrics.observe_rpc_latency(50.0);
        ghost_metrics.observe_rpc_latency(100.0);
        ghost_metrics.set_tpu_leaders_resolved(true);
        ghost_metrics.inc_validation_rejects();
        ghost_metrics.inc_validation_rejects();

        // Verify values
        assert_eq!(ghost_metrics.tpu_leaders_resolved.get(), 1.0);
        assert_eq!(ghost_metrics.validation_rejects.get(), 2.0);

        // Verify metrics are in registry
        let families = registry.gather();
        let names: Vec<&str> = families.iter().map(|f| f.get_name()).collect();
        assert!(names.contains(&"ghost_rpc_latency_ms"));
        assert!(names.contains(&"ghost_tpu_leaders_resolved"));
        assert!(names.contains(&"ghost_validation_rejects_total"));
    }

    #[test]
    fn test_ghost_metrics_tpu_leaders_toggle() {
        let registry = Registry::new();
        let ghost_metrics = GhostMetrics::new(&registry);

        ghost_metrics.set_tpu_leaders_resolved(false);
        assert_eq!(ghost_metrics.tpu_leaders_resolved.get(), 0.0);

        ghost_metrics.set_tpu_leaders_resolved(true);
        assert_eq!(ghost_metrics.tpu_leaders_resolved.get(), 1.0);

        ghost_metrics.set_tpu_leaders_resolved(false);
        assert_eq!(ghost_metrics.tpu_leaders_resolved.get(), 0.0);
    }

    #[test]
    fn test_e2e_metrics_with_ghost_metrics() {
        let metrics = E2EMetrics::new();
        let ghost_metrics = GhostMetrics::new(&metrics.registry);

        // E2E metrics
        metrics
            .seer_pools_detected
            .with_label_values(&["pumpfun"])
            .inc_by(100.0);
        metrics
            .seer_pools_parsed
            .with_label_values(&["pumpfun"])
            .inc_by(98.0);
        metrics.trigger_txs_sent.inc_by(50.0);
        metrics.trigger_txs_confirmed.inc_by(46.0);

        // Ghost metrics
        ghost_metrics.observe_rpc_latency(25.0);
        ghost_metrics.set_tpu_leaders_resolved(true);
        ghost_metrics.inc_validation_rejects();

        // Verify all metrics are in the same registry
        let families = metrics.registry.gather();
        let names: Vec<&str> = families.iter().map(|f| f.get_name()).collect();

        // E2E metrics
        assert!(names.contains(&"ghost_seer_pools_detected_total"));
        assert!(names.contains(&"ghost_trigger_txs_sent_total"));

        // Ghost-specific metrics
        assert!(names.contains(&"ghost_rpc_latency_ms"));
        assert!(names.contains(&"ghost_tpu_leaders_resolved"));
        assert!(names.contains(&"ghost_validation_rejects_total"));
    }

    #[tokio::test]
    async fn test_metrics_server_creation() {
        let metrics = E2EMetrics::new();
        let config = MetricsServerConfig::with_port(19091);
        let server = MetricsServer::new(metrics, config);

        assert!(!server.is_running().await);
    }
}

mod pnl_calculation_tests {
    //! Tests for PnL calculation logic

    use ghost_launcher::events::TradeResult;

    #[test]
    fn test_trade_result_pnl_buy() {
        let trade = TradeResult {
            signature: "buy_sig".to_string(),
            mint: "token".to_string(),
            sol_amount: 1.0,
            token_amount: 1_000_000.0,
            entry_price: 0.000001,
            is_buy: true,
            slot: Some(1),
            pnl_sol: None, // No PnL on buy
            timestamp: 1,
        };

        assert!(trade.is_buy);
        assert!(trade.pnl_sol.is_none());
    }

    #[test]
    fn test_trade_result_pnl_sell_profit() {
        let trade = TradeResult {
            signature: "sell_sig".to_string(),
            mint: "token".to_string(),
            sol_amount: 1.2, // Sold for more than bought
            token_amount: 1_000_000.0,
            entry_price: 0.0000012, // Higher than buy price
            is_buy: false,
            slot: Some(100),
            pnl_sol: Some(0.2), // 0.2 SOL profit
            timestamp: 2,
        };

        assert!(!trade.is_buy);
        assert_eq!(trade.pnl_sol, Some(0.2));
    }

    #[test]
    fn test_trade_result_pnl_sell_loss() {
        let trade = TradeResult {
            signature: "sell_sig".to_string(),
            mint: "token".to_string(),
            sol_amount: 0.8, // Sold for less than bought
            token_amount: 1_000_000.0,
            entry_price: 0.0000008, // Lower than buy price
            is_buy: false,
            slot: Some(100),
            pnl_sol: Some(-0.2), // 0.2 SOL loss
            timestamp: 2,
        };

        assert!(!trade.is_buy);
        assert_eq!(trade.pnl_sol, Some(-0.2));
    }

    #[test]
    fn test_trade_result_serialize() {
        let trade = TradeResult {
            signature: "sig".to_string(),
            mint: "mint".to_string(),
            sol_amount: 2.0,
            token_amount: 500_000.0,
            entry_price: 0.000004,
            is_buy: false,
            slot: Some(200),
            pnl_sol: Some(0.5),
            timestamp: 12345,
        };

        let json = serde_json::to_string(&trade).unwrap();
        assert!(json.contains("\"signature\":\"sig\""));
        assert!(json.contains("\"pnl_sol\":0.5"));

        // Deserialize and verify
        let deserialized: TradeResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.signature, "sig");
        assert_eq!(deserialized.pnl_sol, Some(0.5));
    }
}

mod integration_smoke_tests {
    use super::*;

    #[tokio::test]
    async fn test_full_pipeline_smoke() {
        // This test verifies that all components can be instantiated together

        // 1. Create E2E metrics
        let metrics = E2EMetrics::new();

        // 2. Create Ghost metrics on the same registry
        let ghost_metrics = GhostMetrics::new(&metrics.registry);

        // 3. Simulate some activity
        metrics
            .seer_pools_detected
            .with_label_values(&["pumpfun"])
            .inc();
        metrics
            .seer_pools_parsed
            .with_label_values(&["pumpfun"])
            .inc();
        ghost_metrics.observe_rpc_latency(10.0);
        ghost_metrics.set_tpu_leaders_resolved(true);

        // 4. Verify land rate calculation
        let land_rate = metrics.update_land_rate("pumpfun");
        assert_eq!(land_rate, 100.0); // 1/1 = 100%

        // 5. Verify metrics are available
        let families = metrics.registry.gather();
        assert!(!families.is_empty());
    }

    #[tokio::test]
    async fn test_event_bus_and_metrics_together() {
        use ghost_launcher::events::{create_event_bus, DetectedPool, GhostEvent};

        // Create event bus
        let (tx, mut rx) = create_event_bus();

        // Create metrics
        let metrics = E2EMetrics::new();
        let ghost_metrics = GhostMetrics::new(&metrics.registry);

        // Simulate Seer detecting a pool and emitting event
        let pool = DetectedPool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: "integration_pool".to_string(),
            base_mint: "integration_mint".to_string(),
            quote_mint: "sol".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            slot: Some(11111),
            tx_index: None,
            timestamp_ms: 1700000000000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: None,
            initial_liquidity_sol: Some(15.0),
            signature: "integration_sig".to_string(),
            creator: String::new(),
        };

        // Record detection metric
        metrics
            .seer_pools_detected
            .with_label_values(&["pumpfun"])
            .inc();

        // Send event
        tx.send(GhostEvent::new_pool_detected(pool)).unwrap();

        // Receive and process
        let event = rx.recv().await.unwrap();
        if let GhostEvent::NewPoolDetected(_) = event {
            // Record parsed metric
            metrics
                .seer_pools_parsed
                .with_label_values(&["pumpfun"])
                .inc();
            // Record RPC latency
            ghost_metrics.observe_rpc_latency(15.0);
        }

        // Verify metrics
        assert_eq!(
            metrics
                .seer_pools_detected
                .with_label_values(&["pumpfun"])
                .get(),
            1.0
        );
        assert_eq!(
            metrics
                .seer_pools_parsed
                .with_label_values(&["pumpfun"])
                .get(),
            1.0
        );

        let land_rate = metrics.update_land_rate("pumpfun");
        assert_eq!(land_rate, 100.0);
    }
}
