//! Direct Sell Builder - Direct AMM Interaction for Pump.fun
//!
//! This module provides zero-cost direct instruction building for Pump.fun AMM sell operations.
//!
//! ## Features
//! - Raw instruction building without IDL (byte-level)
//! - PDA derivation for bonding curves
//! - Associated token account derivation
//! - Minimal overhead for maximum speed
//!
//! ## Security Note
//! The Pump.fun program ID and other constants are mainnet values.
//! For devnet/localnet testing, these should be configured via environment.
//!
//! ## Data Layout
//! The Pump.fun 'sell' instruction uses the following data layout:
//! - Discriminator: 8 bytes (SELL_DISCRIMINATOR constant)
//! - amount (tokens to sell): u64 (8 bytes, little-endian)
//! - min_sol_output (minimum SOL to receive): u64 (8 bytes, little-endian)

use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};
use std::str::FromStr;

// Pump.fun Program Constants (Mainnet)
// These are well-known public addresses for the Pump.fun protocol
// Source: https://pump.fun documentation and on-chain verification
const PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const GLOBAL_SEED: &[u8] = b"global";
const BONDING_CURVE_SEED: &[u8] = b"bonding-curve";
const FEE_RECIPIENT: &str = "CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM";
const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const ASSOC_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";

/// Instruction discriminator for Pump.fun 'sell' instruction
/// This is the first 8 bytes of sha256("global:sell") as per Anchor convention
/// Value: 33e685a4017f83ad (hex) = [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad]
const SELL_DISCRIMINATOR: [u8; 8] = [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad];

/// Default slippage tolerance for sell transactions (5%)
/// Used when estimating minimum SOL output
pub const DEFAULT_SELL_SLIPPAGE_BPS: u16 = 500;

/// DirectSellBuilder - Constructs raw Pump.fun sell instructions
///
/// This builder creates instructions for direct interaction with Pump.fun AMM.
/// It mirrors the pattern of DirectBuyBuilder for consistency.
pub struct DirectSellBuilder;

impl DirectSellBuilder {
    /// Get the Pump.fun program ID
    pub fn pump_program_id() -> Pubkey {
        Pubkey::from_str(PUMP_PROGRAM_ID).expect("Invalid PUMP_PROGRAM_ID")
    }

    /// Builds a raw 'sell' instruction for Pump.fun AMM
    ///
    /// # Arguments
    /// * `payer` - The wallet executing the sell (signer)
    /// * `mint` - The token mint address being sold
    /// * `amount_tokens` - Amount of tokens to sell
    /// * `min_sol_output` - Minimum SOL to receive (slippage protection)
    ///
    /// # Returns
    /// A Solana instruction ready to be included in a transaction
    ///
    /// # Note
    /// The Pump.fun 'sell' instruction uses the following data layout:
    /// - Discriminator: 8 bytes (SELL_DISCRIMINATOR constant)
    /// - amount (tokens to sell): u64 (8 bytes, little-endian)
    /// - min_sol_output (minimum SOL to receive): u64 (8 bytes, little-endian)
    pub fn build_sell_ix(
        payer: &Pubkey,
        mint: &Pubkey,
        amount_tokens: u64,
        min_sol_output: u64,
    ) -> Instruction {
        let program_id = Self::pump_program_id();
        let fee_recipient = Pubkey::from_str(FEE_RECIPIENT).expect("Invalid FEE_RECIPIENT");

        // Derive PDAs
        let (global, _) = Pubkey::find_program_address(&[GLOBAL_SEED], &program_id);
        let (bonding_curve, _) =
            Pubkey::find_program_address(&[BONDING_CURVE_SEED, mint.as_ref()], &program_id);
        let associated_bonding_curve = Self::get_associated_token_address(&bonding_curve, mint);
        let associated_user = Self::get_associated_token_address(payer, mint);

        // Build instruction data
        // Layout: [Discriminator (8), amount/tokens (8), min_sol_output (8)] = 24 bytes
        let mut data = Vec::with_capacity(24);
        data.extend_from_slice(&SELL_DISCRIMINATOR);
        data.extend_from_slice(&amount_tokens.to_le_bytes()); // tokens we're selling
        data.extend_from_slice(&min_sol_output.to_le_bytes()); // minimum SOL we want to receive

        // Build account metas
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("Invalid TOKEN_PROGRAM_ID");
        let assoc_token_program =
            Pubkey::from_str(ASSOC_TOKEN_PROGRAM_ID).expect("Invalid ASSOC_TOKEN_PROGRAM_ID");

        // Pump.fun sell instruction accounts (12 accounts)
        let accounts = vec![
            AccountMeta::new_readonly(global, false), // 0: global state
            AccountMeta::new(fee_recipient, false),   // 1: fee recipient
            AccountMeta::new_readonly(*mint, false),  // 2: token mint
            AccountMeta::new(bonding_curve, false),   // 3: bonding curve
            AccountMeta::new(associated_bonding_curve, false), // 4: bonding curve token account
            AccountMeta::new(associated_user, false), // 5: user token account
            AccountMeta::new(*payer, true),           // 6: payer (signer)
            AccountMeta::new_readonly(system_program::id(), false), // 7: system program
            AccountMeta::new_readonly(assoc_token_program, false), // 8: associated token program
            AccountMeta::new_readonly(token_program, false), // 9: token program
            AccountMeta::new_readonly(program_id, false), // 10: event authority
            AccountMeta::new_readonly(program_id, false), // 11: pump program
        ];

        Instruction {
            program_id,
            accounts,
            data,
        }
    }

    /// Derive the bonding curve PDA for a given mint
    pub fn derive_bonding_curve(mint: &Pubkey) -> (Pubkey, u8) {
        let program_id = Self::pump_program_id();
        Pubkey::find_program_address(&[BONDING_CURVE_SEED, mint.as_ref()], &program_id)
    }

    /// Derive the global state PDA
    pub fn derive_global() -> (Pubkey, u8) {
        let program_id = Self::pump_program_id();
        Pubkey::find_program_address(&[GLOBAL_SEED], &program_id)
    }

    /// Calculate associated token address for a wallet and mint
    fn get_associated_token_address(wallet: &Pubkey, mint: &Pubkey) -> Pubkey {
        let token_pid = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("Invalid TOKEN_PROGRAM_ID");
        let assoc_pid =
            Pubkey::from_str(ASSOC_TOKEN_PROGRAM_ID).expect("Invalid ASSOC_TOKEN_PROGRAM_ID");
        Pubkey::find_program_address(
            &[&wallet.to_bytes(), &token_pid.to_bytes(), &mint.to_bytes()],
            &assoc_pid,
        )
        .0
    }

    /// Calculate minimum SOL output with slippage protection
    ///
    /// # Arguments
    /// * `token_amount` - Amount of tokens to sell
    /// * `price_per_token` - Expected price per token in lamports (scaled by 1e9)
    /// * `slippage_bps` - Slippage tolerance in basis points (e.g., 500 = 5%)
    ///
    /// # Returns
    /// Minimum SOL output with slippage applied
    ///
    /// # Example
    /// ```ignore
    /// let min_sol = DirectSellBuilder::calculate_min_sol_output(
    ///     1_000_000,  // 1M tokens
    ///     30_000,     // 0.00003 SOL per token
    ///     500,        // 5% slippage
    /// );
    /// ```
    pub fn calculate_min_sol_output(
        token_amount: u64,
        price_per_token: u64,
        slippage_bps: u16,
    ) -> u64 {
        if slippage_bps > 10000 {
            return 0; // Invalid slippage, return 0
        }

        let expected_output =
            (token_amount as u128).saturating_mul(price_per_token as u128) / 1_000_000_000u128; // Descale from 1e9

        let slippage_factor = 10000u128.saturating_sub(slippage_bps as u128);
        let min_output = expected_output.saturating_mul(slippage_factor) / 10000u128;

        min_output.min(u64::MAX as u128) as u64
    }

    /// Build a validated sell instruction with pool integrity verification
    ///
    /// This method performs full pool validation before building the instruction,
    /// ensuring the pool is legitimate and safe to trade.
    ///
    /// # Arguments
    /// * `payer` - The wallet executing the sell (signer)
    /// * `mint` - The token mint address being sold
    /// * `amount_tokens` - Amount of tokens to sell
    /// * `min_sol_output` - Minimum SOL to receive (slippage protection)
    /// * `validator` - Pool validator instance for integrity checks
    /// * `rpc_client` - RPC client for blockchain queries
    ///
    /// # Returns
    /// * `Ok(Instruction)` if validation passes and instruction is built
    /// * `Err(TriggerError::InvalidPool)` if pool validation fails
    ///
    /// # Security
    /// This is the recommended method for building sell instructions.
    /// It ensures pool integrity before any transaction is built.
    pub async fn build_validated_sell_ix(
        payer: &Pubkey,
        mint: &Pubkey,
        amount_tokens: u64,
        min_sol_output: u64,
        validator: &crate::validation::PoolValidator,
        rpc_client: &solana_client::nonblocking::rpc_client::RpcClient,
    ) -> crate::errors::Result<Instruction> {
        // Derive the bonding curve PDA for this mint
        let (bonding_curve, _) = Self::derive_bonding_curve(mint);

        // Validate pool integrity
        let is_valid = validator
            .verify_pool_integrity(&bonding_curve, mint, rpc_client)
            .await?;

        if !is_valid {
            return Err(crate::errors::TriggerError::InvalidPool(format!(
                "Pool validation failed for mint {}: bonding curve {} is invalid or potentially malicious",
                mint, bonding_curve
            )));
        }

        // Pool is valid, build the instruction
        Ok(Self::build_sell_ix(
            payer,
            mint,
            amount_tokens,
            min_sol_output,
        ))
    }

    /// Verify the discriminator used in sell instructions matches expected value
    ///
    /// This delegates to the validation module, following the same pattern as DirectBuyBuilder.
    pub fn verify_discriminator() -> bool {
        crate::validation::verify_sell_discriminator(&SELL_DISCRIMINATOR)
    }

    /// Get the raw discriminator bytes for debugging/logging
    pub fn get_discriminator() -> [u8; 8] {
        SELL_DISCRIMINATOR
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pump_program_id() {
        let program_id = DirectSellBuilder::pump_program_id();
        assert_eq!(
            program_id.to_string(),
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
        );
    }

    #[test]
    fn test_build_sell_ix() {
        let payer = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let amount_tokens = 1_000_000; // 1M tokens
        let min_sol_output = 500_000; // 0.0005 SOL

        let ix = DirectSellBuilder::build_sell_ix(&payer, &mint, amount_tokens, min_sol_output);

        // Verify instruction structure
        assert_eq!(ix.program_id, DirectSellBuilder::pump_program_id());
        assert_eq!(ix.accounts.len(), 12);
        assert_eq!(ix.data.len(), 24); // 8 discriminator + 8 amount + 8 min_sol_output

        // Verify discriminator
        assert_eq!(&ix.data[0..8], &SELL_DISCRIMINATOR);

        // Verify tokens amount (little-endian)
        let tokens_bytes = u64::from_le_bytes(ix.data[8..16].try_into().unwrap());
        assert_eq!(tokens_bytes, amount_tokens);

        // Verify min_sol_output (little-endian)
        let sol_bytes = u64::from_le_bytes(ix.data[16..24].try_into().unwrap());
        assert_eq!(sol_bytes, min_sol_output);

        // Verify payer is signer
        assert!(ix.accounts[6].is_signer);
        assert_eq!(ix.accounts[6].pubkey, payer);
    }

    #[test]
    fn test_derive_bonding_curve() {
        let mint = Pubkey::new_unique();
        let (bonding_curve, bump) = DirectSellBuilder::derive_bonding_curve(&mint);

        // Verify it's a valid PDA
        assert!(bonding_curve != Pubkey::default());
        assert!(bump <= 255);
    }

    #[test]
    fn test_calculate_min_sol_output() {
        // 1M tokens at 0.00003 SOL per token with 5% slippage
        let token_amount = 1_000_000u64;
        let price_per_token = 30_000_000_000u64; // 0.00003 SOL scaled by 1e9
        let slippage_bps = 500u16; // 5%

        let min_sol = DirectSellBuilder::calculate_min_sol_output(
            token_amount,
            price_per_token,
            slippage_bps,
        );

        // Expected: 1M * 0.00003 SOL * 0.95 = 28.5 SOL * 1e9 lamports
        // But with descaling: 1M * 30e9 / 1e9 * 0.95 = 28,500,000 lamports
        assert_eq!(min_sol, 28_500_000);
    }

    #[test]
    fn test_calculate_min_sol_output_invalid_slippage() {
        // Slippage > 100% should return 0
        let result = DirectSellBuilder::calculate_min_sol_output(1000, 100, 10001);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_account_metas_order() {
        let payer = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        let ix = DirectSellBuilder::build_sell_ix(&payer, &mint, 1_000_000, 1_000);

        // Verify critical account positions
        // Account 6 should be payer (signer, writable)
        assert!(ix.accounts[6].is_signer);
        assert!(ix.accounts[6].is_writable);

        // Account 7 should be system program (read-only)
        assert_eq!(ix.accounts[7].pubkey, system_program::id());
        assert!(!ix.accounts[7].is_signer);
        assert!(!ix.accounts[7].is_writable);
        assert_eq!(
            ix.accounts[1].pubkey.to_string(),
            "CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM"
        );
    }

    #[test]
    fn test_verify_discriminator() {
        // The discriminator should match SHA256("global:sell")[..8]
        assert!(DirectSellBuilder::verify_discriminator());
    }

    #[test]
    fn test_get_discriminator() {
        let discriminator = DirectSellBuilder::get_discriminator();
        assert_eq!(
            discriminator,
            [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad]
        );
    }

    #[test]
    fn test_derive_global() {
        let (global, bump) = DirectSellBuilder::derive_global();

        // Verify it's a valid PDA
        assert!(global != Pubkey::default());
        assert!(bump <= 255);
    }
}
