//! Direct Buy Builder - Direct AMM Interaction for Pump.fun
//!
//! This module provides zero-cost direct instruction building for Pump.fun AMM.
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
const GLOBAL_CONFIG: &str = "4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf";
const BONDING_CURVE_SEED: &[u8] = b"bonding-curve";
const BONDING_CURVE_V2_SEED: &[u8] = b"bonding-curve-v2";
const GLOBAL_SEED: &[u8] = b"global";
const EVENT_AUTHORITY_SEED: &[u8] = b"__event_authority";
const GLOBAL_VOLUME_ACCUMULATOR_SEED: &[u8] = b"global_volume_accumulator";
const USER_VOLUME_ACCUMULATOR_SEED: &[u8] = b"user_volume_accumulator";
const CREATOR_VAULT_SEED: &[u8] = b"creator-vault";
const FEE_CONFIG_SEED: &[u8] = b"fee_config";
// Current on-chain Pump.fun global-state recipients fetched from
// global PDA 4wTV1... on 2026-05-06. The primary `fee_recipient`
// is 62qc..., while the remaining entries are allowlisted fallbacks
// from `fee_recipients[]` / `reserved_fee_recipient` / `reserved_fee_recipients[]`.
const FEE_RECIPIENT: Pubkey = solana_sdk::pubkey!("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV");
const AUTHORIZED_FEE_RECIPIENTS: [Pubkey; 15] = [
    solana_sdk::pubkey!("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV"),
    solana_sdk::pubkey!("7VtfL8fvgNfhz17qKRMjzQEXgbdpnHHHQRh54R9jP2RJ"),
    solana_sdk::pubkey!("7hTckgnGnLQR6sdH7YkqFTAA7VwTfYFaZ6EhEsU3saCX"),
    solana_sdk::pubkey!("9rPYyANsfQZw3DnDmKE3YCQF5E8oD89UXoHn9JFEhJUz"),
    solana_sdk::pubkey!("AVmoTthdrX6tKt4nDjco2D775W2YK3sDhxPcMmzUAmTY"),
    solana_sdk::pubkey!("CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM"),
    solana_sdk::pubkey!("FWsW1xNtWscwNmKv6wVsU1iTzRN6wmmk3MjxRP5tT7hz"),
    solana_sdk::pubkey!("G5UZAVbAf46s7cKWoyKu8kYTip9DGTpbLZ2qa9Aq69dP"),
    solana_sdk::pubkey!("GesfTA3X2arioaHp8bbKdjG9vJtskViWACZoYvxp4twS"),
    solana_sdk::pubkey!("4budycTjhs9fD6xw62VBducVTNgMgJJ5BgtKq7mAZwn6"),
    solana_sdk::pubkey!("8SBKzEQU4nLSzcwF4a74F2iaUDQyTfjGndn6qUWBnrpR"),
    solana_sdk::pubkey!("4UQeTP1T39KZ9Sfxzo3WR5skgsaP6NZa87BAkuazLEKH"),
    solana_sdk::pubkey!("8sNeir4QsLsJdYpc9RZacohhK1Y5FLU3nC5LXgYB4aa6"),
    solana_sdk::pubkey!("Fh9HmeLNUMVCvejxCtCL2DbYaRyBFVJ5xrWkLnMH6fdk"),
    solana_sdk::pubkey!("463MEnMeGyJekNZFQSTUABBEbLnvMTALbT6ZmsxAbAdq"),
];
const FEE_PROGRAM_ID: &str = "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ";
pub const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
const ASSOC_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
const BUYBACK_FEE_RECIPIENTS: [&str; 8] = [
    "5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD",
    "9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7",
    "GXPFM2caqTtQYC2cJ5yJRi9VDkpsYZXzYdwYpGnLmtDL",
    "3BpXnfJaUTiwXnJNe7Ej1rcbzqTTQUvLShZaWazebsVR",
    "5cjcW9wExnJJiqgLjq7DEG75Pm6JBgE1hNv4B2vHXUW6",
    "EHAAiTxcdDwQ3U4bU6YcMsQGaekdzLS3B5SmYo46kJtL",
    "5eHhjP8JaYkz83CWwvGU2uMUXefd3AazWGx4gpcuEEYD",
    "A7hAgCzFw14fejgCp387JUJRMNyz4j89JKnhtKU8piqW",
];
const BUY_EXACT_SOL_IN_DISCRIMINATOR: [u8; 8] = [0x38, 0xfc, 0x74, 0x08, 0x9e, 0xdf, 0xcd, 0x5f];
const LEGACY_TRACK_VOLUME_ENABLED: u8 = 1;
const FEE_SEED_CONST: [u8; 32] = [
    1, 86, 224, 246, 147, 102, 90, 207, 68, 219, 21, 104, 191, 23, 91, 170, 81, 137, 203, 151, 245,
    210, 255, 59, 101, 93, 43, 182, 253, 109, 24, 176,
];

/// Legacy Pump.fun `buy` discriminator (`global:buy`).
///
/// Kept for backwards-compatible validation helpers and older tests. The active
/// builder path below emits routed `buy_exact_sol_in`, not this legacy variant.
const LEGACY_BUY_DISCRIMINATOR: [u8; 8] = [0x66, 0x06, 0x3d, 0x12, 0x01, 0xda, 0xeb, 0xea];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PumpfunBuyVariant {
    LegacyBuy,
    RoutedExactSolIn,
}

impl PumpfunBuyVariant {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LegacyBuy => "legacy_buy",
            Self::RoutedExactSolIn => "routed_exact_sol_in",
        }
    }
}

/// Default slippage tolerance for token estimation (20%)
/// Used when estimating tokens out from SOL input
pub const DEFAULT_SLIPPAGE_TOLERANCE: f64 = 0.20;

/// Estimated tokens per SOL at launch (rough approximation)
/// Based on typical Pump.fun bonding curve initial state
/// This is used for initial slippage estimation only - actual price comes from Shadow Ledger
const TOKENS_PER_SOL_ESTIMATE: f64 = 30_000_000.0;

/// Fallback entry price in micro-units (used when min_amount_out is zero)
/// Represents 0.001 SOL per token as a reasonable fallback
pub const FALLBACK_ENTRY_PRICE: u64 = 1000;

/// DirectBuyBuilder - Constructs raw Pump.fun buy instructions
///
/// This builder creates instructions for direct interaction with Pump.fun AMM.
pub struct DirectBuyBuilder;

impl DirectBuyBuilder {
    /// Get the Pump.fun program ID
    pub fn pump_program_id() -> Pubkey {
        Pubkey::from_str(PUMP_PROGRAM_ID).expect("Invalid PUMP_PROGRAM_ID")
    }

    pub fn canonical_fee_recipient() -> Pubkey {
        FEE_RECIPIENT
    }

    pub fn canonical_global_config() -> Pubkey {
        Self::derive_global().0
    }

    pub fn is_authorized_fee_recipient(candidate: &Pubkey) -> bool {
        AUTHORIZED_FEE_RECIPIENTS.contains(candidate)
    }

    pub fn routed_buyback_fee_recipient(payer: &Pubkey, mint: &Pubkey) -> Pubkey {
        let payer_bytes = payer.to_bytes();
        let mint_bytes = mint.to_bytes();
        let index = usize::from(payer_bytes[0] ^ mint_bytes[0]) % BUYBACK_FEE_RECIPIENTS.len();
        Pubkey::from_str(BUYBACK_FEE_RECIPIENTS[index]).expect("Invalid BUYBACK_FEE_RECIPIENT")
    }

    /// Builds the routed Pump.fun `buy_exact_sol_in` instruction used by live
    /// mainnet buys.
    ///
    /// # Arguments
    /// * `payer` - The wallet paying for the transaction (signer)
    /// * `mint` - The token mint address being purchased
    /// * `amount_sol_in` - Maximum SOL to spend (in lamports)
    /// * `min_tokens_out` - Minimum tokens to receive (slippage protection)
    ///
    /// # Returns
    /// A Solana instruction ready to be included in a transaction
    ///
    /// # Note
    /// The routed instruction uses the following data layout:
    /// - Discriminator: 8 bytes (`BUY_EXACT_SOL_IN_DISCRIMINATOR`)
    /// - spendable SOL in: u64 (8 bytes, little-endian)
    /// - min tokens out: u64 (8 bytes, little-endian)
    /// - track_volume flag: u8
    pub fn build_buy_ix(
        payer: &Pubkey,
        mint: &Pubkey,
        amount_sol_in: u64,
        min_tokens_out: u64,
    ) -> Instruction {
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("Invalid TOKEN_PROGRAM_ID");
        Self::build_buy_ix_with_accounts(
            payer,
            mint,
            &token_program,
            None,
            None,
            None,
            None,
            None,
            amount_sol_in,
            min_tokens_out,
        )
    }

    pub fn build_buy_ix_with_token_program(
        payer: &Pubkey,
        mint: &Pubkey,
        token_program: &Pubkey,
        amount_sol_in: u64,
        min_tokens_out: u64,
    ) -> Instruction {
        Self::build_buy_ix_with_accounts(
            payer,
            mint,
            token_program,
            None,
            None,
            None,
            None,
            None,
            amount_sol_in,
            min_tokens_out,
        )
    }

    pub fn build_buy_ix_with_accounts(
        payer: &Pubkey,
        mint: &Pubkey,
        token_program: &Pubkey,
        global_config: Option<Pubkey>,
        fee_recipient: Option<Pubkey>,
        creator_pubkey: Option<Pubkey>,
        buy_variant: Option<PumpfunBuyVariant>,
        associated_bonding_curve: Option<Pubkey>,
        amount_sol_in: u64,
        min_tokens_out: u64,
    ) -> Instruction {
        let program_id = Self::pump_program_id();
        let buy_variant = buy_variant.unwrap_or(PumpfunBuyVariant::RoutedExactSolIn);
        let fee_program =
            Pubkey::from_str(FEE_PROGRAM_ID).expect("Invalid pump fee program identifier");
        let fee_recipient = fee_recipient
            .filter(Self::is_authorized_fee_recipient)
            .unwrap_or_else(Self::canonical_fee_recipient);
        let creator_pubkey = creator_pubkey.unwrap_or_default();

        // Derive PDAs
        let global = global_config
            .filter(|candidate| *candidate == Self::canonical_global_config())
            .unwrap_or_else(Self::canonical_global_config);
        let (bonding_curve, _) =
            Pubkey::find_program_address(&[BONDING_CURVE_SEED, mint.as_ref()], &program_id);
        let (bonding_curve_v2, _) =
            Pubkey::find_program_address(&[BONDING_CURVE_V2_SEED, mint.as_ref()], &program_id);
        let (event_authority, _) =
            Pubkey::find_program_address(&[EVENT_AUTHORITY_SEED], &program_id);
        let (global_volume_accumulator, _) =
            Pubkey::find_program_address(&[GLOBAL_VOLUME_ACCUMULATOR_SEED], &program_id);
        let (user_volume_accumulator, _) = Pubkey::find_program_address(
            &[USER_VOLUME_ACCUMULATOR_SEED, payer.as_ref()],
            &program_id,
        );
        let (creator_vault, _) = Pubkey::find_program_address(
            &[CREATOR_VAULT_SEED, creator_pubkey.as_ref()],
            &program_id,
        );
        let (fee_config, _) =
            Pubkey::find_program_address(&[FEE_CONFIG_SEED, &FEE_SEED_CONST], &fee_program);
        let associated_bonding_curve = associated_bonding_curve
            .filter(|candidate| {
                Self::validate_associated_bonding_curve(mint, token_program, candidate)
            })
            .unwrap_or_else(|| Self::canonical_associated_bonding_curve(mint, token_program));
        let associated_user = Self::get_associated_token_address(payer, mint, token_program);

        // Build instruction data for the selected on-chain buy variant.
        let data = match buy_variant {
            PumpfunBuyVariant::LegacyBuy => {
                let mut data = Vec::with_capacity(25);
                data.extend_from_slice(&LEGACY_BUY_DISCRIMINATOR);
                data.extend_from_slice(&min_tokens_out.to_le_bytes());
                data.extend_from_slice(&amount_sol_in.to_le_bytes());
                data.push(LEGACY_TRACK_VOLUME_ENABLED);
                data
            }
            PumpfunBuyVariant::RoutedExactSolIn => {
                let mut data = Vec::with_capacity(25);
                data.extend_from_slice(&BUY_EXACT_SOL_IN_DISCRIMINATOR);
                data.extend_from_slice(&amount_sol_in.to_le_bytes());
                data.extend_from_slice(&min_tokens_out.to_le_bytes());
                data.push(LEGACY_TRACK_VOLUME_ENABLED);
                data
            }
        };

        // Build account metas
        let mut accounts = vec![
            AccountMeta::new_readonly(global, false), // 0: global state
            AccountMeta::new(fee_recipient, false),   // 1: fee recipient
            AccountMeta::new_readonly(*mint, false),  // 2: token mint
            AccountMeta::new(bonding_curve, false),   // 3: bonding curve
            AccountMeta::new(associated_bonding_curve, false), // 4: bonding curve token account
            AccountMeta::new(associated_user, false), // 5: user token account
            AccountMeta::new(*payer, true),           // 6: payer (signer)
            AccountMeta::new_readonly(system_program::id(), false), // 7: system program
            AccountMeta::new_readonly(*token_program, false), // 8: token program
            AccountMeta::new(creator_vault, false),   // 9: creator vault PDA
            AccountMeta::new_readonly(event_authority, false), // 10: event authority
            AccountMeta::new_readonly(program_id, false), // 11: pump program
            AccountMeta::new_readonly(global_volume_accumulator, false), // 12: global volume accumulator
            AccountMeta::new(user_volume_accumulator, false), // 13: user volume accumulator
            AccountMeta::new_readonly(fee_config, false),     // 14: fee config PDA
            AccountMeta::new_readonly(fee_program, false),    // 15: fee program
            AccountMeta::new_readonly(bonding_curve_v2, false), // 16: bonding curve V2
        ];

        if buy_variant == PumpfunBuyVariant::RoutedExactSolIn {
            accounts.push(AccountMeta::new(
                Self::routed_buyback_fee_recipient(payer, mint),
                false,
            ));
        }

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
        let derived = Pubkey::find_program_address(&[GLOBAL_SEED], &program_id);
        let fallback = Pubkey::from_str(GLOBAL_CONFIG).expect("Invalid GLOBAL_CONFIG");
        if derived.0 == fallback {
            derived
        } else {
            (fallback, 0)
        }
    }

    /// Derive the canonical ATA used by Pump.fun for the bonding curve vault.
    pub fn canonical_associated_bonding_curve(mint: &Pubkey, token_program: &Pubkey) -> Pubkey {
        let (bonding_curve, _) = Self::derive_bonding_curve(mint);
        Self::get_associated_token_address(&bonding_curve, mint, token_program)
    }

    /// Validate an observed associated bonding curve against the canonical ATA derivation.
    pub fn validate_associated_bonding_curve(
        mint: &Pubkey,
        token_program: &Pubkey,
        candidate: &Pubkey,
    ) -> bool {
        *candidate == Self::canonical_associated_bonding_curve(mint, token_program)
    }

    /// Calculate associated token address for a wallet and mint
    fn get_associated_token_address(
        wallet: &Pubkey,
        mint: &Pubkey,
        token_program: &Pubkey,
    ) -> Pubkey {
        let assoc_pid =
            Pubkey::from_str(ASSOC_TOKEN_PROGRAM_ID).expect("Invalid ASSOC_TOKEN_PROGRAM_ID");
        Pubkey::find_program_address(
            &[
                &wallet.to_bytes(),
                &token_program.to_bytes(),
                &mint.to_bytes(),
            ],
            &assoc_pid,
        )
        .0
    }

    /// Estimate tokens out based on SOL input
    ///
    /// This is a rough estimation for initial slippage protection.
    /// For accurate calculations, use Shadow Ledger data which provides
    /// real-time bonding curve state.
    ///
    /// # Arguments
    /// * `amount_sol_lamports` - SOL amount in lamports
    /// * `slippage_percent` - Slippage tolerance (e.g., 0.20 for 20%)
    ///
    /// # Returns
    /// (estimated_tokens, min_tokens_with_slippage)
    ///
    /// # Note
    /// The estimation uses TOKENS_PER_SOL_ESTIMATE constant which represents
    /// typical initial token supply on Pump.fun bonding curves. This is only
    /// for initial slippage estimation - actual prices should come from
    /// Shadow Ledger for accuracy.
    pub fn estimate_tokens_out(amount_sol_lamports: u64, slippage_percent: f64) -> (u64, u64) {
        const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

        let sol_amount = amount_sol_lamports as f64 / LAMPORTS_PER_SOL;
        let estimated_tokens = (sol_amount * TOKENS_PER_SOL_ESTIMATE) as u64;
        let min_tokens = (estimated_tokens as f64 * (1.0 - slippage_percent)) as u64;

        (estimated_tokens, min_tokens)
    }

    /// Build a validated buy instruction with pool integrity verification
    ///
    /// This method performs full pool validation before building the instruction,
    /// ensuring the pool is legitimate and safe to trade.
    ///
    /// # Arguments
    /// * `payer` - The wallet paying for the transaction (signer)
    /// * `mint` - The token mint address being purchased
    /// * `amount_sol_in` - Maximum SOL to spend (in lamports)
    /// * `min_tokens_out` - Minimum tokens to receive (slippage protection)
    /// * `validator` - Pool validator instance for integrity checks
    /// * `rpc_client` - RPC client for blockchain queries
    ///
    /// # Returns
    /// * `Ok(Instruction)` if validation passes and instruction is built
    /// * `Err(TriggerError::InvalidPool)` if pool validation fails
    ///
    /// # Security
    /// This is the recommended method for building buy instructions.
    /// It ensures pool integrity before any transaction is built.
    pub async fn build_validated_buy_ix(
        payer: &Pubkey,
        mint: &Pubkey,
        amount_sol_in: u64,
        min_tokens_out: u64,
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
        Ok(Self::build_buy_ix(
            payer,
            mint,
            amount_sol_in,
            min_tokens_out,
        ))
    }

    /// Verify the active routed discriminator used by this builder.
    pub fn verify_discriminator() -> bool {
        Self::get_discriminator() == BUY_EXACT_SOL_IN_DISCRIMINATOR
    }

    /// Get the active routed discriminator bytes for debugging/logging.
    pub fn get_discriminator() -> [u8; 8] {
        BUY_EXACT_SOL_IN_DISCRIMINATOR
    }

    /// Verify the legacy `global:buy` discriminator retained for backwards
    /// compatibility and older mocks.
    pub fn verify_legacy_buy_discriminator() -> bool {
        crate::validation::verify_buy_discriminator(&LEGACY_BUY_DISCRIMINATOR)
    }

    /// Get the raw legacy `global:buy` discriminator bytes.
    pub fn get_legacy_buy_discriminator() -> [u8; 8] {
        LEGACY_BUY_DISCRIMINATOR
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pump_program_id() {
        let program_id = DirectBuyBuilder::pump_program_id();
        assert_eq!(
            program_id.to_string(),
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
        );
    }

    #[test]
    fn test_build_buy_ix() {
        let payer = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let amount_sol = 1_000_000_000; // 1 SOL
        let min_tokens = 24_000_000; // 24M tokens (with 20% slippage)

        let ix = DirectBuyBuilder::build_buy_ix(&payer, &mint, amount_sol, min_tokens);

        // Verify instruction structure
        assert_eq!(ix.program_id, DirectBuyBuilder::pump_program_id());
        assert_eq!(ix.accounts.len(), 18);
        assert_eq!(ix.data.len(), 25); // routed exact-sol-in + track_volume

        // Verify discriminator
        assert_eq!(
            &ix.data[0..8],
            &[0x38, 0xfc, 0x74, 0x08, 0x9e, 0xdf, 0xcd, 0x5f]
        );
        assert_eq!(
            u64::from_le_bytes(ix.data[8..16].try_into().unwrap()),
            amount_sol
        );
        assert_eq!(
            u64::from_le_bytes(ix.data[16..24].try_into().unwrap()),
            min_tokens
        );
        assert_eq!(ix.data[24], LEGACY_TRACK_VOLUME_ENABLED);

        // Verify SOL amount (little-endian)
        let sol_bytes = u64::from_le_bytes(ix.data[8..16].try_into().unwrap());
        assert_eq!(sol_bytes, amount_sol);

        // Verify min tokens amount (little-endian)
        let tokens_bytes = u64::from_le_bytes(ix.data[16..24].try_into().unwrap());
        assert_eq!(tokens_bytes, min_tokens);

        // Verify payer is signer
        assert!(ix.accounts[6].is_signer);
        assert_eq!(ix.accounts[6].pubkey, payer);
        assert_eq!(
            ix.accounts[17].pubkey,
            DirectBuyBuilder::routed_buyback_fee_recipient(&payer, &mint)
        );
    }

    #[test]
    fn test_build_legacy_buy_ix_with_accounts() {
        let payer = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");

        let ix = DirectBuyBuilder::build_buy_ix_with_accounts(
            &payer,
            &mint,
            &token_program,
            None,
            None,
            None,
            Some(PumpfunBuyVariant::LegacyBuy),
            None,
            47_958_222,
            1_024_500_538_013,
        );

        assert_eq!(&ix.data[0..8], &LEGACY_BUY_DISCRIMINATOR);
        assert_eq!(ix.accounts.len(), 17);
        assert_eq!(
            u64::from_le_bytes(ix.data[8..16].try_into().unwrap()),
            1_024_500_538_013
        );
        assert_eq!(
            u64::from_le_bytes(ix.data[16..24].try_into().unwrap()),
            47_958_222
        );
        assert_eq!(ix.data[24], LEGACY_TRACK_VOLUME_ENABLED);
    }

    #[test]
    fn test_routed_buy_derived_accounts_match_observed_chain_samples() {
        let token_2022 = Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("valid token2022 program");
        let fee_program = Pubkey::from_str(FEE_PROGRAM_ID).expect("valid fee program");

        let cases = [
            (
                "success",
                "GqFDJJhnpjEtAURPzE11X6aDSKpUZDzmBQsiq3KVpump",
                "GRn7Mq2t6qmF15nLQFbmVc6tHJE7zNH2ozXQnmx7LZjj",
                "ChovELwvUxprzAWddf4fronV32ASuUSjKXpUiHbfq2np",
                "5E4RPqeCuohvYrFtt3RVT1CZwsoNpmjxkhpH72gHC2g",
                "EyCtqKHZnY1gUR6M7PK8SwSLLXNkxhfEutU9VhG6CBps",
                "HNq6c4iNBRoA7Tu9GEJRVcsifMkrkYpTHYYAAYtt2A6f",
                "21PNnUZD8knZBmEkRd2JCzkaYzMGdiACaYyFYLZNQazb",
                "7V8C1KRonHNoh8wEcs96TbeiCLAF2N8ddQXJCnJAiJZo",
                true,
            ),
            (
                "fail",
                "6MFsT18HtdShAvLcWH4rJzXiz4MpF14V2z89Bv4ipump",
                "7gEnRjDud56BVmBQMWLTticJoouzrpnyaAnN5A3EEqiy",
                "Hi5yNvPSfagdja5xjYMTYWnnjSE3ze5KsezTLfuD2mqd",
                "CJXnphAndFVyYfcbnwXvz6ZJYbUnGpEAGt2TdsxCqzMx",
                "AZ6DWZJNNaVMuXEfafLejWuBU2Sq1ZoxBP4bmixvnWNW",
                "GZzub4idN12y8TUqRddJjwZSgvJP3bLPsEZJXcnnPCa6",
                "5BRLycydvC574QDrtZuXbXmWHUsjUTTPVL9NayVQbvvY",
                "6biTuL7xRX1b6PA7uK9eUvvtGfigwtXiahuZRWHF5MeN",
                false,
            ),
            (
                "success_latest",
                "GqFDJJhnpjEtAURPzE11X6aDSKpUZDzmBQsiq3KVpump",
                "GRn7Mq2t6qmF15nLQFbmVc6tHJE7zNH2ozXQnmx7LZjj",
                "ChovELwvUxprzAWddf4fronV32ASuUSjKXpUiHbfq2np",
                "5E4RPqeCuohvYrFtt3RVT1CZwsoNpmjxkhpH72gHC2g",
                "EyCtqKHZnY1gUR6M7PK8SwSLLXNkxhfEutU9VhG6CBps",
                "HNq6c4iNBRoA7Tu9GEJRVcsifMkrkYpTHYYAAYtt2A6f",
                "21PNnUZD8knZBmEkRd2JCzkaYzMGdiACaYyFYLZNQazb",
                "7V8C1KRonHNoh8wEcs96TbeiCLAF2N8ddQXJCnJAiJZo",
                true,
            ),
            (
                "fail_latest",
                "6MFsT18HtdShAvLcWH4rJzXiz4MpF14V2z89Bv4ipump",
                "7gEnRjDud56BVmBQMWLTticJoouzrpnyaAnN5A3EEqiy",
                "Hi5yNvPSfagdja5xjYMTYWnnjSE3ze5KsezTLfuD2mqd",
                "CJXnphAndFVyYfcbnwXvz6ZJYbUnGpEAGt2TdsxCqzMx",
                "75UVQ8HtF44xww6CTQJsoqU1FRZke6mmkyJ7vrGcAyP7",
                "GZzub4idN12y8TUqRddJjwZSgvJP3bLPsEZJXcnnPCa6",
                "5BRLycydvC574QDrtZuXbXmWHUsjUTTPVL9NayVQbvvY",
                "6biTuL7xRX1b6PA7uK9eUvvtGfigwtXiahuZRWHF5MeN",
                true,
            ),
        ];

        for (
            label,
            mint_s,
            payer_s,
            creator_s,
            expected_curve_s,
            expected_assoc_curve_s,
            expected_creator_vault_s,
            expected_user_volume_s,
            expected_curve_v2_s,
            expected_assoc_curve_matches,
        ) in cases
        {
            let mint = Pubkey::from_str(mint_s).expect("valid mint");
            let payer = Pubkey::from_str(payer_s).expect("valid payer");
            let creator = Pubkey::from_str(creator_s).expect("valid creator");

            let (curve, _) = Pubkey::find_program_address(
                &[BONDING_CURVE_SEED, mint.as_ref()],
                &DirectBuyBuilder::pump_program_id(),
            );
            let assoc_curve =
                DirectBuyBuilder::get_associated_token_address(&curve, &mint, &token_2022);
            let (creator_vault, _) = Pubkey::find_program_address(
                &[CREATOR_VAULT_SEED, creator.as_ref()],
                &DirectBuyBuilder::pump_program_id(),
            );
            let (user_volume, _) = Pubkey::find_program_address(
                &[USER_VOLUME_ACCUMULATOR_SEED, payer.as_ref()],
                &DirectBuyBuilder::pump_program_id(),
            );
            let (curve_v2, _) = Pubkey::find_program_address(
                &[BONDING_CURVE_V2_SEED, mint.as_ref()],
                &DirectBuyBuilder::pump_program_id(),
            );
            let assoc_curve_v2 =
                DirectBuyBuilder::get_associated_token_address(&curve_v2, &mint, &token_2022);
            let legacy_token =
                Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid legacy token program");
            let assoc_curve_legacy =
                DirectBuyBuilder::get_associated_token_address(&curve, &mint, &legacy_token);
            let (fee_config, _) =
                Pubkey::find_program_address(&[FEE_CONFIG_SEED, &FEE_SEED_CONST], &fee_program);

            assert_eq!(
                curve.to_string(),
                expected_curve_s,
                "{label}: bonding_curve"
            );
            if assoc_curve.to_string() != expected_assoc_curve_s {
                eprintln!(
                    "{label}: assoc_curve={} assoc_curve_v2={} assoc_curve_legacy={}",
                    assoc_curve, assoc_curve_v2, assoc_curve_legacy
                );
            }
            let assoc_curve_matches = assoc_curve.to_string() == expected_assoc_curve_s;
            assert_eq!(
                creator_vault.to_string(),
                expected_creator_vault_s,
                "{label}: creator vault"
            );
            assert_eq!(
                user_volume.to_string(),
                expected_user_volume_s,
                "{label}: user volume accumulator"
            );
            assert_eq!(
                curve_v2.to_string(),
                expected_curve_v2_s,
                "{label}: bonding_curve_v2"
            );
            assert_eq!(
                fee_config.to_string(),
                "8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt",
                "{label}: fee_config"
            );
            assert_eq!(
                assoc_curve_matches, expected_assoc_curve_matches,
                "{label}: associated bonding curve match expectation"
            );
            if !expected_assoc_curve_matches {
                assert_ne!(
                    assoc_curve_v2.to_string(),
                    expected_assoc_curve_s,
                    "{label}: associated bonding curve is not derived from bonding-curve-v2 either"
                );
                assert_ne!(
                    assoc_curve_legacy.to_string(),
                    expected_assoc_curve_s,
                    "{label}: associated bonding curve is not legacy-token ATA either"
                );
            }
        }
    }

    #[test]
    fn test_build_buy_ix_keeps_valid_associated_bonding_curve_override() {
        let payer = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let token_program =
            Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("valid token2022 program");
        let assoc_override =
            DirectBuyBuilder::canonical_associated_bonding_curve(&mint, &token_program);

        let ix = DirectBuyBuilder::build_buy_ix_with_accounts(
            &payer,
            &mint,
            &token_program,
            None,
            None,
            None,
            Some(PumpfunBuyVariant::RoutedExactSolIn),
            Some(assoc_override),
            1_000_000,
            1_000,
        );

        assert_eq!(ix.accounts[4].pubkey, assoc_override);
    }

    #[test]
    fn test_build_buy_ix_ignores_invalid_associated_bonding_curve_override() {
        let payer = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let token_program =
            Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("valid token2022 program");
        let assoc_override = Pubkey::new_unique();

        let ix = DirectBuyBuilder::build_buy_ix_with_accounts(
            &payer,
            &mint,
            &token_program,
            None,
            None,
            None,
            Some(PumpfunBuyVariant::RoutedExactSolIn),
            Some(assoc_override),
            1_000_000,
            1_000,
        );

        assert_eq!(
            ix.accounts[4].pubkey,
            DirectBuyBuilder::canonical_associated_bonding_curve(&mint, &token_program)
        );
        assert_ne!(ix.accounts[4].pubkey, assoc_override);
    }

    #[test]
    fn test_build_buy_ix_ignores_noncanonical_fee_recipient_override() {
        let payer = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let token_program =
            Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("valid token2022 program");
        let observed_override = Pubkey::new_unique();

        let ix = DirectBuyBuilder::build_buy_ix_with_accounts(
            &payer,
            &mint,
            &token_program,
            None,
            Some(observed_override),
            None,
            Some(PumpfunBuyVariant::RoutedExactSolIn),
            None,
            1_000_000,
            1_000,
        );

        assert_eq!(
            ix.accounts[1].pubkey,
            DirectBuyBuilder::canonical_fee_recipient()
        );
        assert_ne!(ix.accounts[1].pubkey, observed_override);
    }

    #[test]
    fn test_build_buy_ix_keeps_authorized_fee_recipient_override() {
        let payer = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let token_program =
            Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("valid token2022 program");
        let observed_override = Pubkey::from_str("GesfTA3X2arioaHp8bbKdjG9vJtskViWACZoYvxp4twS")
            .expect("authorized reserved fee recipient");

        let ix = DirectBuyBuilder::build_buy_ix_with_accounts(
            &payer,
            &mint,
            &token_program,
            None,
            Some(observed_override),
            None,
            Some(PumpfunBuyVariant::RoutedExactSolIn),
            None,
            1_000_000,
            1_000,
        );

        assert_eq!(ix.accounts[1].pubkey, observed_override);
    }

    #[test]
    fn test_build_buy_ix_ignores_noncanonical_global_override() {
        let payer = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let token_program =
            Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("valid token2022 program");
        let observed_override = Pubkey::new_unique();

        let ix = DirectBuyBuilder::build_buy_ix_with_accounts(
            &payer,
            &mint,
            &token_program,
            Some(observed_override),
            None,
            None,
            Some(PumpfunBuyVariant::RoutedExactSolIn),
            None,
            1_000_000,
            1_000,
        );

        assert_eq!(
            ix.accounts[0].pubkey,
            DirectBuyBuilder::canonical_global_config()
        );
        assert_ne!(ix.accounts[0].pubkey, observed_override);
    }

    #[test]
    fn test_routed_buy_ix_uses_deterministic_buyback_fee_recipient() {
        let payer =
            Pubkey::from_str("GRn7Mq2t6qmF15nLQFbmVc6tHJE7zNH2ozXQnmx7LZjj").expect("valid payer");
        let mint =
            Pubkey::from_str("GqFDJJhnpjEtAURPzE11X6aDSKpUZDzmBQsiq3KVpump").expect("valid mint");
        let token_program =
            Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("valid token2022 program");

        let ix = DirectBuyBuilder::build_buy_ix_with_accounts(
            &payer,
            &mint,
            &token_program,
            None,
            None,
            Some(Pubkey::new_unique()),
            Some(PumpfunBuyVariant::RoutedExactSolIn),
            None,
            1_000_000,
            1_000,
        );

        assert_eq!(ix.accounts.len(), 18);
        assert_eq!(
            ix.accounts[17].pubkey.to_string(),
            "5eHhjP8JaYkz83CWwvGU2uMUXefd3AazWGx4gpcuEEYD"
        );
    }

    #[test]
    fn test_derive_bonding_curve() {
        let mint = Pubkey::new_unique();
        let (bonding_curve, bump) = DirectBuyBuilder::derive_bonding_curve(&mint);

        // Verify it's a valid PDA
        assert!(bonding_curve != Pubkey::default());
        assert!(bump <= 255);
    }

    #[test]
    fn test_estimate_tokens_out() {
        let one_sol = 1_000_000_000u64;
        let (estimated, min_with_slippage) = DirectBuyBuilder::estimate_tokens_out(one_sol, 0.20);

        // 1 SOL ≈ 30M tokens
        assert_eq!(estimated, 30_000_000);
        // With 20% slippage: 24M tokens minimum
        assert_eq!(min_with_slippage, 24_000_000);
    }

    #[test]
    fn test_account_metas_order() {
        let payer = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        let ix = DirectBuyBuilder::build_buy_ix(&payer, &mint, 1_000_000, 1_000);

        // Verify critical account positions
        // Account 6 should be payer (signer, writable)
        assert!(ix.accounts[6].is_signer);
        assert!(ix.accounts[6].is_writable);

        // Account 7 should be system program (read-only)
        assert_eq!(ix.accounts[7].pubkey, system_program::id());
        assert!(!ix.accounts[7].is_signer);
        assert!(!ix.accounts[7].is_writable);
        assert_eq!(
            ix.accounts[8].pubkey,
            Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap()
        );
        assert_eq!(
            ix.accounts[15].pubkey,
            Pubkey::from_str(FEE_PROGRAM_ID).unwrap()
        );
        assert_eq!(ix.accounts[11].pubkey, DirectBuyBuilder::pump_program_id());
        assert_eq!(
            ix.accounts[1].pubkey.to_string(),
            "62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV"
        );
    }

    #[test]
    fn test_verify_discriminator() {
        // Active builder now emits routed buy_exact_sol_in.
        assert!(DirectBuyBuilder::verify_discriminator());
    }

    #[test]
    fn test_get_discriminator() {
        let discriminator = DirectBuyBuilder::get_discriminator();
        assert_eq!(
            discriminator,
            [0x38, 0xfc, 0x74, 0x08, 0x9e, 0xdf, 0xcd, 0x5f]
        );
    }

    #[test]
    fn test_legacy_discriminator_still_available() {
        assert!(DirectBuyBuilder::verify_legacy_buy_discriminator());
        assert_eq!(
            DirectBuyBuilder::get_legacy_buy_discriminator(),
            [0x66, 0x06, 0x3d, 0x12, 0x01, 0xda, 0xeb, 0xea]
        );
    }

    #[test]
    fn test_build_buy_ix_with_token2022_uses_token2022_program_and_ata_derivation() {
        let payer = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let token_2022_program =
            Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("valid token2022 program");

        let ix = DirectBuyBuilder::build_buy_ix_with_token_program(
            &payer,
            &mint,
            &token_2022_program,
            1_000_000,
            1_000,
        );

        assert_eq!(ix.accounts[8].pubkey, token_2022_program);
        let expected_user_ata = Pubkey::find_program_address(
            &[
                &payer.to_bytes(),
                &token_2022_program.to_bytes(),
                &mint.to_bytes(),
            ],
            &Pubkey::from_str(ASSOC_TOKEN_PROGRAM_ID).expect("valid ATA program"),
        )
        .0;
        assert_eq!(ix.accounts[5].pubkey, expected_user_ata);
    }
}
