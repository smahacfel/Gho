//! Enhanced candidate builder for Seer
//!
//! This module provides utilities to build EnhancedCandidate instances from
//! transaction data using zero-RPC contextual analysis.

use crate::types::{AmmProgram, CandidatePool, GeyserEvent, RawBytesMissingReason};
use ghost_core::{
    analyze_transaction, AmmType, EnhancedCandidate, InstructionData, TransactionData,
};
use tracing::{debug, warn};

/// Build an EnhancedCandidate from CandidatePool and transaction data
///
/// This performs contextual analysis on the transaction to populate enhanced fields:
/// - vanity_score: proof-of-work in mint address
/// - has_dev_buy: atomic dev buy detection
/// - dev_buy_sol: SOL invested by dev
/// - mint_auth_disabled: mint authority status
/// - metadata_len_score: metadata quality
///
/// # Arguments
/// * `candidate_pool` - Base candidate from InitializePool parsing
/// * `event` - Original Geyser event with transaction data
/// * `amm_program` - AMM program type (PumpFun/BonkFun)
///
/// # Returns
/// Result containing enhanced candidate or None if analysis fails
pub fn build_enhanced_candidate(
    candidate_pool: &CandidatePool,
    event: &GeyserEvent,
    amm_program: AmmProgram,
) -> Option<EnhancedCandidate> {
    // Extract transaction data from event
    let (accounts, num_required_signatures, instructions): (
        &Vec<solana_sdk::pubkey::Pubkey>,
        usize,
        &Vec<crate::types::RawInstruction>,
    ) = match event {
        GeyserEvent::Transaction {
            accounts,
            instructions,
            ..
        } => {
            // Estimate num_required_signatures from first few accounts (signers are first)
            // In practice, we'd get this from message header, but we approximate here
            let num_sigs = accounts.iter().take(5).count(); // Conservative estimate

            (accounts, num_sigs, instructions)
        }
        _ => {
            warn!("Cannot build enhanced candidate from non-transaction event");
            return None;
        }
    };

    // Convert AMM program to AmmType
    let amm_type = match amm_program {
        AmmProgram::PumpFun => AmmType::PumpFun,
        AmmProgram::PumpSwap => AmmType::PumpSwap,
    };

    // Build TransactionData for analysis
    let instruction_data: Vec<InstructionData> = instructions
        .iter()
        .map(|raw_inst| InstructionData {
            program_id: raw_inst.program_id,
            account_indices: &raw_inst.account_indices,
            data: &raw_inst.data,
        })
        .collect();

    let tx_data = TransactionData {
        accounts,
        num_required_signatures,
        instructions: &instruction_data,
    };

    // Analyze transaction for enhanced signals
    match analyze_transaction(&tx_data, &candidate_pool.base_mint, amm_type) {
        Ok(analysis) => {
            debug!(
                "Enhanced analysis complete: pool={}, vanity={}, dev_buy={}, dev_buy_sol={:.2}, mint_auth_disabled={}, metadata={}",
                candidate_pool.pool_amm_id,
                analysis.vanity_score,
                analysis.has_dev_buy,
                analysis.dev_buy_sol,
                analysis.mint_auth_disabled,
                analysis.metadata_len_score
            );

            // Build EnhancedCandidate from CandidatePool + analysis
            Some(EnhancedCandidate {
                slot: candidate_pool.slot,
                pool_amm_id: candidate_pool.pool_amm_id,
                amm_program_id: candidate_pool.amm_program_id,
                base_mint: candidate_pool.base_mint,
                quote_mint: candidate_pool.quote_mint,
                bonding_curve: candidate_pool.bonding_curve,
                timestamp: candidate_pool.timestamp,
                bonding_curve_progress: candidate_pool.bonding_curve_progress,
                initial_liquidity_sol: candidate_pool.initial_liquidity_sol.unwrap_or(0.0),
                token_total_supply: candidate_pool.token_total_supply,
                signature: candidate_pool.signature.clone(),
                // Enhanced fields from analysis
                vanity_score: analysis.vanity_score,
                has_dev_buy: analysis.has_dev_buy,
                dev_buy_sol: analysis.dev_buy_sol,
                mint_auth_disabled: analysis.mint_auth_disabled,
                metadata_len_score: analysis.metadata_len_score,
                // Shadow Ledger fields not available at this stage
                expected_price: None,
                shadow_bonding_progress: None,
                virtual_sol_reserves: None,
                shadow_market_cap: None,
            })
        }
        Err(e) => {
            warn!(
                "Failed to analyze transaction for enhanced candidate: {}",
                e
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::Signature;

    #[test]
    fn test_build_enhanced_candidate() {
        let candidate_pool = CandidatePool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(12345),
            tx_index: None,
            event_ts_ms: None,
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: "test".to_string(),
            amm_program_id: "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
                .parse()
                .unwrap(),
            pool_amm_id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            quote_mint: Pubkey::new_unique(),
            bonding_curve: Pubkey::new_unique(),
            creator: Pubkey::new_unique(),
            timestamp: 1234567890,
            bonding_curve_progress: Some(0.05),
            initial_liquidity_sol: Some(10.0),
            token_total_supply: Some(1_000_000_000),
            block_time: Some(1234567890),
        };

        let event = GeyserEvent::Transaction {
            slot: Some(12345),
            event_ts_ms: None,
            arrival_ts_ms: Some(crate::types::arrival_time_ms()),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: Signature::default(),
            accounts: vec![Pubkey::new_unique(), Pubkey::new_unique()],
            instructions: vec![],
            logs: vec![],
            block_time: Some(1234567890),
            account_data: std::collections::HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: String::new(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
            inner_instructions: vec![],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        };

        let result = build_enhanced_candidate(&candidate_pool, &event, AmmProgram::PumpFun);

        // Should succeed even with minimal data
        assert!(result.is_some());

        let enhanced = result.unwrap();
        assert_eq!(enhanced.slot, candidate_pool.slot);
        assert_eq!(enhanced.base_mint, candidate_pool.base_mint);
        // Vanity score should be calculated
        assert!(enhanced.vanity_score <= 100);
    }
}
