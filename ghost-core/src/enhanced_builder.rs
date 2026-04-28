//! Enhanced candidate builder from transaction data
//!
//! This module provides utilities to build EnhancedCandidate instances
//! by analyzing raw transaction data for contextual signals.

use crate::context_analysis::{
    calculate_vanity_score, compute_metadata_len_score, TransactionContext,
};
use crate::init_pool_parser::AmmType;
use crate::transaction_parser::{
    extract_signers, is_set_authority, parse_create_metadata, parse_set_authority,
    parse_swap_instruction, ProgramIds,
};
use anyhow::Result;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashSet;

/// Transaction data needed for enhanced analysis
pub struct TransactionData<'a> {
    /// All account keys in the transaction
    pub accounts: &'a [Pubkey],

    /// Number of required signatures (signers are first N accounts)
    pub num_required_signatures: usize,

    /// All instructions in the transaction
    pub instructions: &'a [InstructionData<'a>],
}

/// Single instruction data
pub struct InstructionData<'a> {
    /// Program ID executing this instruction
    pub program_id: Pubkey,

    /// Account indices used by this instruction
    pub account_indices: &'a [u8],

    /// Raw instruction data
    pub data: &'a [u8],
}

/// Result of enhanced analysis
#[derive(Debug, Clone, Default)]
pub struct EnhancedAnalysis {
    /// Vanity score for the mint (0-100)
    pub vanity_score: u8,

    /// Whether dev performs atomic buy
    pub has_dev_buy: bool,

    /// Total SOL spent by dev on buys
    pub dev_buy_sol: f64,

    /// Whether mint authority has been disabled
    pub mint_auth_disabled: bool,

    /// Metadata quality score (0-100)
    pub metadata_len_score: u8,
}

/// Analyze a transaction to extract enhanced candidate information
///
/// This function performs zero-RPC analysis of transaction data to populate
/// EnhancedCandidate fields.
///
/// # Arguments
/// * `tx_data` - Transaction data to analyze
/// * `base_mint` - The base token mint (to calculate vanity score)
/// * `amm_type` - Type of AMM (PumpFun, BonkFun, etc.)
///
/// # Returns
/// Enhanced analysis results
pub fn analyze_transaction(
    tx_data: &TransactionData,
    base_mint: &Pubkey,
    amm_type: AmmType,
) -> Result<EnhancedAnalysis> {
    let mut analysis = EnhancedAnalysis::default();

    // === 1. VANITY SCORE ===
    analysis.vanity_score = calculate_vanity_score(base_mint);

    // === 2. BUILD DEV ACCOUNT CONTEXT ===
    let mut context = TransactionContext::new();

    // Extract signers (first N accounts based on num_required_signatures)
    let signers = extract_signers(tx_data.accounts, tx_data.num_required_signatures);
    for signer in signers {
        context.add_signer(signer);
    }

    // Set payer (first account is typically the payer)
    if let Some(&payer) = tx_data.accounts.first() {
        context.set_payer(payer);
    }

    // === 3. SCAN INSTRUCTIONS ===
    let dev_accounts = context.get_dev_accounts();
    let mut total_dev_buy_lamports: u64 = 0;
    let mut found_metadata = false;

    for instruction in tx_data.instructions {
        // --- Check for SetAuthority (mint authority analysis) ---
        if is_set_authority(&instruction.program_id, instruction.data) {
            if let Ok(is_disabled) = parse_set_authority(instruction.data) {
                if is_disabled {
                    analysis.mint_auth_disabled = true;
                }
                // Note: We mark as disabled only if set to None.
                // Transferring to bonding curve PDA would require checking the target,
                // which we'll handle at the scoring level.
            }
        }

        // --- Check for CreateMetadata (metadata analysis) ---
        let metadata_program: Pubkey = ProgramIds::METADATA_PROGRAM
            .parse()
            .unwrap_or_else(|_| Pubkey::new_unique());

        if instruction.program_id == metadata_program && !found_metadata {
            if let Ok(metadata) = parse_create_metadata(instruction.data) {
                analysis.metadata_len_score =
                    compute_metadata_len_score(&metadata.name, &metadata.symbol);
                found_metadata = true;
            }
        }

        // --- Check for Swap/Buy instructions (atomic dev buy detection) ---
        if let Ok(Some(swap_info)) = parse_swap_instruction(
            instruction.data,
            tx_data.accounts,
            instruction.account_indices,
            amm_type,
        ) {
            // Check if user is a dev account
            if dev_accounts.contains(&swap_info.user) && swap_info.is_buy {
                analysis.has_dev_buy = true;
                total_dev_buy_lamports += swap_info.amount_in_lamports;
            }
        }
    }

    // Convert lamports to SOL
    analysis.dev_buy_sol = total_dev_buy_lamports as f64 / 1_000_000_000.0;

    Ok(analysis)
}

/// Analyze multiple transactions in a bundle
///
/// This extends the single-transaction analysis to handle Jito bundles where
/// the dev might perform actions across multiple transactions.
///
/// # Arguments
/// * `transactions` - Multiple transaction data to analyze
/// * `base_mint` - The base token mint
/// * `amm_type` - Type of AMM
///
/// # Returns
/// Combined enhanced analysis from all transactions
pub fn analyze_bundle(
    transactions: &[TransactionData],
    base_mint: &Pubkey,
    amm_type: AmmType,
) -> Result<EnhancedAnalysis> {
    let mut combined = EnhancedAnalysis::default();

    // Vanity score is computed once for the mint
    combined.vanity_score = calculate_vanity_score(base_mint);

    // Collect all dev accounts from all transactions
    let mut all_dev_accounts = HashSet::new();

    for tx_data in transactions {
        let signers = extract_signers(tx_data.accounts, tx_data.num_required_signatures);
        all_dev_accounts.extend(signers);

        if let Some(&payer) = tx_data.accounts.first() {
            all_dev_accounts.insert(payer);
        }
    }

    // Scan all instructions across all transactions
    let mut total_dev_buy_lamports: u64 = 0;
    let mut found_metadata = false;

    for tx_data in transactions {
        for instruction in tx_data.instructions {
            // SetAuthority check
            if is_set_authority(&instruction.program_id, instruction.data) {
                if let Ok(is_disabled) = parse_set_authority(instruction.data) {
                    if is_disabled {
                        combined.mint_auth_disabled = true;
                    }
                }
            }

            // Metadata check
            let metadata_program: Pubkey = ProgramIds::METADATA_PROGRAM
                .parse()
                .unwrap_or_else(|_| Pubkey::new_unique());

            if instruction.program_id == metadata_program && !found_metadata {
                if let Ok(metadata) = parse_create_metadata(instruction.data) {
                    combined.metadata_len_score =
                        compute_metadata_len_score(&metadata.name, &metadata.symbol);
                    found_metadata = true;
                }
            }

            // Swap check
            if let Ok(Some(swap_info)) = parse_swap_instruction(
                instruction.data,
                tx_data.accounts,
                instruction.account_indices,
                amm_type,
            ) {
                if all_dev_accounts.contains(&swap_info.user) && swap_info.is_buy {
                    combined.has_dev_buy = true;
                    total_dev_buy_lamports += swap_info.amount_in_lamports;
                }
            }
        }
    }

    combined.dev_buy_sol = total_dev_buy_lamports as f64 / 1_000_000_000.0;

    Ok(combined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_transaction_vanity_only() {
        let accounts = vec![Pubkey::new_unique(), Pubkey::new_unique()];

        let base_mint = Pubkey::new_unique();

        let tx_data = TransactionData {
            accounts: &accounts,
            num_required_signatures: 1,
            instructions: &[],
        };

        let result = analyze_transaction(&tx_data, &base_mint, AmmType::PumpFun);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        // Should have some vanity score (even if low for random address)
        assert!(analysis.vanity_score <= 100);
    }

    #[test]
    fn test_analyze_transaction_with_set_authority() {
        let accounts = vec![
            Pubkey::new_unique(), // Payer/signer
            Pubkey::new_unique(), // Mint
        ];

        let token_program: Pubkey = ProgramIds::TOKEN_PROGRAM.parse().unwrap();

        // Create SetAuthority instruction data (authority disabled)
        let set_auth_data = vec![6, 0, 0]; // SetAuthority, MintTokens, None

        let instruction = InstructionData {
            program_id: token_program,
            account_indices: &[0, 1],
            data: &set_auth_data,
        };

        let base_mint = Pubkey::new_unique();

        let tx_data = TransactionData {
            accounts: &accounts,
            num_required_signatures: 1,
            instructions: &[instruction],
        };

        let result = analyze_transaction(&tx_data, &base_mint, AmmType::PumpFun);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert!(analysis.mint_auth_disabled);
    }

    #[test]
    fn test_analyze_bundle_multiple_transactions() {
        let accounts1 = vec![Pubkey::new_unique(), Pubkey::new_unique()];

        let accounts2 = vec![Pubkey::new_unique(), Pubkey::new_unique()];

        let base_mint = Pubkey::new_unique();

        let tx_data1 = TransactionData {
            accounts: &accounts1,
            num_required_signatures: 1,
            instructions: &[],
        };

        let tx_data2 = TransactionData {
            accounts: &accounts2,
            num_required_signatures: 1,
            instructions: &[],
        };

        let result = analyze_bundle(&[tx_data1, tx_data2], &base_mint, AmmType::PumpFun);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert!(analysis.vanity_score <= 100);
    }
}
