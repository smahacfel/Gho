#[cfg(test)]
mod tests {
    use ghost_brain::fast_pipeline::EnhancedCandidate;
    use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
    use ghost_brain::oracle::SnapshotEngine;
    use ghost_core::shadow_ledger::ShadowLedger;
    use ghost_launcher::oracle_runtime::OracleRuntime;
    use solana_sdk::pubkey::Pubkey;
    use std::sync::Arc;

    #[test]
    fn test_score_pool_respects_rpc_state() {
        // Setup
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let initial_sol = 100_000_000_000; // 100 SOL (Simulated RPC State)

        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            virtual_sol_reserves: Some(initial_sol), // RPC says 100 SOL
            ..Default::default()
        };

        // Register pool
        runtime.register_new_pool(pool_id, base_mint, candidate, None);

        // Verify Initial State
        let reserves_before = runtime.inspect_candidate_reserves(pool_id);
        assert_eq!(
            reserves_before,
            Some(initial_sol),
            "Initial state should be 100 SOL"
        );

        // Trigger Score Pool
        // This triggers the logic we fixed.
        let snapshot_engine = SnapshotEngine::new(100, 100);
        let _ = runtime.score_pool(pool_id, &snapshot_engine, None, false);

        // Verify After Score State
        let reserves_after = runtime.inspect_candidate_reserves(pool_id);

        // ASSERTION: It should stay at 100 SOL because RPC > 30.5 SOL
        assert_eq!(
            reserves_after,
            Some(initial_sol),
            "SUCCESS: State was preserved at 100 SOL, respecting RPC data"
        );
    }
}
