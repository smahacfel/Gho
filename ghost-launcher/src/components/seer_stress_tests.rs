#[cfg(test)]
mod stress_tests {
    use crate::components::seer::{SessionPoolTradeBridge, SessionTradeDecision};
    use seer::types::{CandidatePool, RawBytesMissingReason, TradeEvent};
    use solana_sdk::{pubkey::Pubkey, signature::Signature};
    use std::time::{Duration, Instant};

    fn make_heavy_candidate(pool: Pubkey, mint: Pubkey) -> CandidatePool {
        CandidatePool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(999),
            event_ts_ms: Some(1700000000000),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: Signature::new_unique().to_string(),
            amm_program_id: "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
                .parse()
                .unwrap(),
            pool_amm_id: pool,
            base_mint: mint,
            quote_mint: "So11111111111111111111111111111111111111112"
                .parse()
                .unwrap(),
            bonding_curve: pool, // Simplified for pump.fun
            creator: Pubkey::new_unique(),
            timestamp: 1700000000,
            bonding_curve_progress: Some(0.0),
            initial_liquidity_sol: Some(30.0),
            token_total_supply: Some(1_000_000_000_000_000),
            block_time: Some(1700000000),
        }
    }

    fn make_atomic_dev_buy(pool: Pubkey, mint: Pubkey) -> TradeEvent {
        TradeEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(999),
            signature: Signature::new_unique(),
            event_ordinal: Some(1),
            provenance: None,
            timestamp_ms: 1700000000000,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 1700000000005,
            pool_amm_id: pool,
            mint,
            signer: Pubkey::new_unique(),
            is_buy: true,
            is_dev_buy: true,
            amount: 100_000_000_000,
            max_sol_cost: 5_000_000_000,
            min_sol_output: 0,
            success: true,
            error_code: None,
            compute_units_consumed: Some(250_000),
            owner_token_deltas: vec![],
            mpcf_payload: vec![1, 2, 3],
            mpcf_payload_missing_reason: RawBytesMissingReason::NotMissing,
            v_tokens_in_bonding_curve: Some(1073000000.0),
            v_sol_in_bonding_curve: Some(30.0),
            market_cap_sol: Some(30.0),
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: Some("legacy_buy".into()),
            associated_bonding_curve: None,
            bonding_curve_v2: None,
            bonding_curve_v2_provenance: None,
            buy_remaining_accounts: vec![],
            is_mayhem_mode: None,
            cu_price_micro_lamports: Some(100),
            compute_unit_limit: Some(200_000),
            inner_ix_count: Some(5),
            cpi_depth: Some(1),
            ata_create_count: Some(1),
            signer_pre_balance_lamports: Some(10_000_000_000),
            signer_post_balance_lamports: None,
            jito_tip_detected: Some(true),
            toolchain_fingerprint: seer::types::ToolchainFingerprintInput::default(),
            curve_data_known: true,
            curve_finality: ghost_core::CurveFinality::Provisional,
            is_pumpswap: false,
        }
    }

    #[tokio::test]
    async fn test_canonical_pipeline_pool_detected_before_trade() {
        // Canonical production flow: seer FIFO ordering_gate guarantees PoolDetected
        // arrives on the IPC channel BEFORE any Trade for newly created pools.
        //
        // This test verifies the bridge handles that ordering correctly regardless of
        // simulated processing times, since the ordering is enforced by the channel.
        let ttl = Duration::from_millis(10);
        let mut bridge = SessionPoolTradeBridge::new(ttl, 100, 1000, Duration::from_secs(60), 1000);

        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        let trade_cu = 100_000u64;
        let pool_cu = 160_000u64;
        let ms_per_10k_cu = 1.0;

        let trade_delay = Duration::from_millis((trade_cu as f64 / 10000.0 * ms_per_10k_cu) as u64);
        let pool_delay = Duration::from_millis((pool_cu as f64 / 10000.0 * ms_per_10k_cu) as u64);

        println!("\n--- CANONICAL PIPELINE STRESS TEST ---");
        println!("  Verifying PoolDetected→Trade ordering (seer FIFO guarantee):");
        println!(
            "  - Pool processing:  {} CU -> {}ms",
            pool_cu,
            pool_delay.as_millis()
        );
        println!(
            "  - Trade processing: {} CU -> {}ms",
            trade_cu,
            trade_delay.as_millis()
        );

        let mut trade = make_atomic_dev_buy(pool, mint);
        trade.compute_units_consumed = Some(trade_cu);

        let atomic_arrival = Instant::now();

        // Simulate parallel processing: pool and trade arrive near-simultaneously.
        let pool_handle = tokio::spawn(async move {
            tokio::time::sleep(pool_delay).await;
            Instant::now()
        });
        let ingest_handle = {
            let trade_clone = trade.clone();
            tokio::spawn(async move {
                tokio::time::sleep(trade_delay).await;
                (trade_clone, Instant::now())
            })
        };

        // Step 1: PoolDetected arrives first (seer FIFO guarantee).
        let register_time = pool_handle.await.unwrap();
        let flush = bridge.register_detected_pool(pool, register_time);
        println!(
            "  [T+{}ms] PoolDetected processed → pool registered",
            register_time.duration_since(atomic_arrival).as_millis()
        );
        assert!(
            flush.replay_ready.is_empty(),
            "Nothing buffered before registration"
        );

        // Step 2: Trade arrives — pool is already registered.
        let (finished_trade, ingest_time) = ingest_handle.await.unwrap();
        let ingress = bridge.ingest_trade(&finished_trade, ingest_time);
        println!(
            "  [T+{}ms] Dev Buy processed → ForwardNow (pool was pre-registered)",
            ingest_time.duration_since(atomic_arrival).as_millis()
        );
        assert_eq!(
            ingress.decision,
            SessionTradeDecision::ForwardNow,
            "Trade must be forwarded — pool was registered before trade arrived"
        );
        println!("  Outcome: ✅ SUCCESS - Canonical FIFO path works correctly");
        println!("------------------------------------------\n");
    }

    #[tokio::test]
    async fn test_pre_session_pool_trade_silently_dropped() {
        // Pre-session pools (existing pools streaming via grpc_global_stream) never emit
        // PoolDetected in this session. Their trades must be silently discarded — no
        // buffering, no expiry overhead, no log spam.
        //
        // This test verifies that even under extreme timing (e.g. congested pipeline where
        // trades arrive long before any registration attempt), the decision is SilentDrop
        // and the system carries zero buffering overhead.
        let ttl = Duration::from_millis(10);
        let mut bridge = SessionPoolTradeBridge::new(ttl, 100, 1000, Duration::from_secs(60), 1000);
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        let trade_cu = 100_000u64;
        let pool_cu = 160_000u64;
        let ms_per_2k_cu = 1.0;

        let trade_delay = Duration::from_millis((trade_cu as f64 / 2000.0 * ms_per_2k_cu) as u64);
        let pool_delay = Duration::from_millis((pool_cu as f64 / 2000.0 * ms_per_2k_cu) as u64);

        println!("\n--- PRE-SESSION POOL SILENT DROP TEST ---");
        println!("  Simulating High Load (1ms / 2k CU):");
        println!("  - Trade Delay: {}ms", trade_delay.as_millis());
        println!(
            "  - Pool (not in session) Delay: {}ms",
            pool_delay.as_millis()
        );

        let trade = make_atomic_dev_buy(pool, mint);
        let t_start = Instant::now();

        let t_ingest = t_start + trade_delay;
        let t_register = t_start + pool_delay;

        // Trade for unregistered pool → SilentDrop (no buffering).
        let ingress = bridge.ingest_trade(&trade, t_ingest);
        assert_eq!(
            ingress.decision,
            SessionTradeDecision::SilentDrop,
            "Pre-session pool trade must be silently dropped — no buffering"
        );

        // Even if register_detected_pool is called later (e.g. snapshot bootstrap),
        // there is nothing to replay because nothing was buffered.
        let flush = bridge.register_detected_pool(pool, t_register);
        let gap = t_register.duration_since(t_ingest).as_millis();
        println!("  Gap: {}ms — irrelevant, no buffer was created", gap);
        assert!(
            flush.replay_ready.is_empty(),
            "Nothing buffered → nothing to replay"
        );
        assert_eq!(
            flush.expired_count, 0,
            "Nothing expired → nothing was buffered"
        );
        println!("  Outcome: ✅ SUCCESS - Silent drop, zero buffering overhead");
        println!("-----------------------------------------\n");
    }
}
