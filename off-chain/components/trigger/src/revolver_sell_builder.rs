//! SELL Transaction Builder for Revolver (Pump.fun & Bonk.fun)
//!
//! This module provides functionality to build SELL transactions with proper
//! min_output calculation and direct AMM integration for both Pump.fun and Bonk.fun.
//!
//! It constructs raw Solana instructions manually to avoid heavy Anchor dependencies
//! in the hot-path and ensure maximum control over the byte layout.

use crate::errors::{Result, TriggerError};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::{v0, VersionedMessage},
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
    system_instruction, system_program,
    transaction::VersionedTransaction,
};
use std::str::FromStr;
use tracing::{debug, info};

// --- Constants for Pump.fun ---
const PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
// Fee recipient read from global_state PDA at offset 290 (confirmed on-chain 2025-04).
// Update if pump.fun governance changes the fee recipient stored in global state.
const PUMP_FEE_RECIPIENT: &str = "CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM";
const PUMP_GLOBAL_SEED: &[u8] = b"global";
const PUMP_BONDING_CURVE_SEED: &[u8] = b"bonding-curve";
const PUMP_BONDING_CURVE_V2_SEED: &[u8] = b"bonding-curve-v2";
const PUMP_USER_VOLUME_ACCUMULATOR_SEED: &[u8] = b"user_volume_accumulator";
const PUMP_EVENT_AUTHORITY_SEED: &[u8] = b"__event_authority";
const PUMP_CREATOR_VAULT_SEED: &[u8] = b"creator-vault";
const PUMP_FEE_CONFIG_SEED: &[u8] = b"fee_config";
const PUMP_FEE_PROGRAM_ID: &str = "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ";
const PUMP_FEE_SEED_CONST: [u8; 32] = [
    1, 86, 224, 246, 147, 102, 90, 207, 68, 219, 21, 104, 191, 23, 91, 170, 81, 137, 203, 151, 245,
    210, 255, 59, 101, 93, 43, 182, 253, 109, 24, 176,
];

// --- Constants for Bonk.fun ---
const BONK_PROGRAM_ID: &str = "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj";
const BONK_FEE_RECIPIENT: &str = "C8Qf4o5ZwJbSz7Y6srR4gvfXx4Z4qyhW5AsYLSRQA8nc";
// $BONK Token Mint (Quote Token for Bonk.fun)
const BONK_TOKEN_MINT: &str = "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263";

// --- Shared Constants ---
const TOKEN_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const TOKEN_2022_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
const ASSOCIATED_TOKEN_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
const SELL_PRICE_SCALE_DENOMINATOR: u128 = 1_000_000_000;

/// Compute unit limit for SELL transactions.
/// Token-2022 PumpFun sells with CPI can exceed the default 200k CU budget,
/// causing Jito simulation failures (bundle REJECTED). 400k provides safe headroom.
const SELL_COMPUTE_UNIT_LIMIT: u32 = 400_000;

/// Compute unit price in micro-lamports for priority fee.
/// Ensures the SELL transaction gets sufficient priority during simulation
/// and on-chain execution. 1000 µL ≈ minimal cost at 400k CU.
const SELL_COMPUTE_UNIT_PRICE_MICRO_LAMPORTS: u64 = 50_000;

/// Instruction discriminator for Pump.fun 'sell' instruction
/// This is the first 8 bytes of sha256("global:sell") as per Anchor convention
/// Value: 33e685a4017f83ad (hex) = [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad]
const SELL_DISCRIMINATOR: [u8; 8] = [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad];

/// Protocol Identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmmProtocol {
    PumpFun,
    BonkFun,
}

/// Configuration for SELL transaction building
#[derive(Debug, Clone)]
pub struct SellTxConfig {
    pub pump_program_id: Pubkey,
    pub pump_fee_recipient: Pubkey,

    pub bonk_program_id: Pubkey,
    pub bonk_fee_recipient: Pubkey,
    pub bonk_quote_mint: Pubkey,

    pub timeout_secs: i64,
}

impl Default for SellTxConfig {
    fn default() -> Self {
        Self {
            pump_program_id: Pubkey::from_str(PUMP_PROGRAM_ID).unwrap(),
            pump_fee_recipient: Pubkey::from_str(PUMP_FEE_RECIPIENT).unwrap(),

            bonk_program_id: Pubkey::from_str(BONK_PROGRAM_ID).unwrap(),
            bonk_fee_recipient: Pubkey::from_str(BONK_FEE_RECIPIENT).unwrap(),
            bonk_quote_mint: Pubkey::from_str(BONK_TOKEN_MINT).unwrap(),

            timeout_secs: 300,
        }
    }
}

/// SELL Transaction Builder
pub struct SellTxBuilder {
    /// Payer/signer keypair
    payer: Keypair,
    /// Configuration
    config: SellTxConfig,
}

impl SellTxBuilder {
    pub fn new(payer: Keypair, config: SellTxConfig) -> Self {
        Self { payer, config }
    }

    pub fn with_default_config(payer: Keypair) -> Self {
        Self::new(payer, SellTxConfig::default())
    }

    /// Get the Pump.fun program ID
    pub fn pump_program_id() -> Pubkey {
        Pubkey::from_str(PUMP_PROGRAM_ID).expect("Invalid PUMP_PROGRAM_ID")
    }

    /// Verify the discriminator used in sell instructions matches expected value
    ///
    /// This is a static method that can be used to verify the SELL_DISCRIMINATOR
    /// constant against what is expected from SHA256("global:sell")[..8].
    pub fn verify_discriminator() -> bool {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(b"global:sell");
        let hash = hasher.finalize();
        let computed: [u8; 8] = hash[0..8].try_into().unwrap();

        computed == SELL_DISCRIMINATOR
    }

    /// Get the raw discriminator bytes for debugging/logging
    pub fn get_discriminator() -> [u8; 8] {
        SELL_DISCRIMINATOR
    }

    /// Derive the bonding curve PDA for a given mint
    pub fn derive_bonding_curve(mint: &Pubkey) -> (Pubkey, u8) {
        let program_id = Self::pump_program_id();
        Pubkey::find_program_address(&[PUMP_BONDING_CURVE_SEED, mint.as_ref()], &program_id)
    }

    /// Build and sign a SELL transaction (Direct AMM Interaction)
    pub async fn build_signed_sell_tx(
        &self,
        mint: Pubkey,
        creator_pubkey: Option<Pubkey>,
        amount_in: u64,
        min_output: u64, // SOL for Pump, Quote Token for Bonk
        blockhash: Hash,
        protocol: AmmProtocol,
    ) -> Result<Vec<u8>> {
        self.build_signed_sell_tx_with_token_program(
            mint,
            creator_pubkey,
            amount_in,
            min_output,
            blockhash,
            protocol,
            TOKEN_PROGRAM_ID,
            false,
        )
        .await
    }

    pub async fn build_signed_sell_tx_with_token_program(
        &self,
        mint: Pubkey,
        creator_pubkey: Option<Pubkey>,
        amount_in: u64,
        min_output: u64, // SOL for Pump, Quote Token for Bonk
        blockhash: Hash,
        protocol: AmmProtocol,
        token_program: Pubkey,
        cashback_enabled: bool,
    ) -> Result<Vec<u8>> {
        self.build_signed_sell_tx_with_token_program_and_priority_tip(
            mint,
            creator_pubkey,
            amount_in,
            min_output,
            blockhash,
            protocol,
            token_program,
            cashback_enabled,
            SELL_COMPUTE_UNIT_PRICE_MICRO_LAMPORTS,
            None,
        )
        .await
    }

    pub async fn build_signed_sell_tx_with_token_program_and_priority_tip(
        &self,
        mint: Pubkey,
        creator_pubkey: Option<Pubkey>,
        amount_in: u64,
        min_output: u64, // SOL for Pump, Quote Token for Bonk
        blockhash: Hash,
        protocol: AmmProtocol,
        token_program: Pubkey,
        cashback_enabled: bool,
        compute_unit_price_micro_lamports: u64,
        inline_tip: Option<(Pubkey, u64)>,
    ) -> Result<Vec<u8>> {
        debug!(
            "Building {:?} SELL tx: mint={}, amount_in={}, min_output={}, token_program={}, cashback_enabled={}",
            protocol, mint, amount_in, min_output, token_program, cashback_enabled
        );

        if min_output == 0 {
            return Err(TriggerError::ConfigError(
                "min_output cannot be 0 (Unsafe slippage)".to_string(),
            ));
        }

        // Select Instruction Builder Strategy
        let instruction = match protocol {
            AmmProtocol::PumpFun => self.build_pump_sell_instruction_with_token_program(
                mint,
                creator_pubkey,
                amount_in,
                min_output,
                token_program,
                cashback_enabled,
            )?,
            AmmProtocol::BonkFun => {
                self.build_bonk_sell_instruction(mint, amount_in, min_output)?
            }
        };

        // Compile Versioned Message with ComputeBudget + SELL instruction.
        // ComputeBudget instructions MUST precede the AMM instruction to avoid
        // Jito simulation failures caused by exceeding the default 200k CU limit
        // (Token-2022 PumpFun sells with CPI routinely need 300k+).
        let cu_limit_ix = ComputeBudgetInstruction::set_compute_unit_limit(SELL_COMPUTE_UNIT_LIMIT);
        let cu_price_ix =
            ComputeBudgetInstruction::set_compute_unit_price(compute_unit_price_micro_lamports);
        let mut instructions = vec![cu_limit_ix, cu_price_ix, instruction];
        if let Some((tip_account, tip_lamports)) = inline_tip {
            if tip_lamports > 0 {
                instructions.push(system_instruction::transfer(
                    &self.payer.pubkey(),
                    &tip_account,
                    tip_lamports,
                ));
            }
        }
        let message = v0::Message::try_compile(
            &self.payer.pubkey(),
            &instructions,
            &[], // ALTs could be added here
            blockhash,
        )
        .map_err(|e| TriggerError::TransactionBuildFailed(e.to_string()))?;

        let versioned_message = VersionedMessage::V0(message);

        // Sign
        let mut tx = VersionedTransaction {
            signatures: vec![],
            message: versioned_message,
        };

        let signature = self.payer.sign_message(tx.message.serialize().as_ref());
        tx.signatures.push(signature);

        // Serialize
        let tx_bytes =
            bincode::serialize(&tx).map_err(|e| TriggerError::SerializationError(e.to_string()))?;

        info!(
            "Built valid {:?} SELL tx: mint={}, size={} bytes, priority_fee_micro_lamports={}",
            protocol,
            mint,
            tx_bytes.len(),
            compute_unit_price_micro_lamports
        );

        Ok(tx_bytes)
    }

    /// Calculate minimum Output (SOL or Quote Token)
    pub fn calculate_min_output(
        token_amount: u64,
        target_price: u64, // Price in Output Token units per Base Token
        slippage_margin_bps: u16,
    ) -> Result<u64> {
        if slippage_margin_bps > 10000 {
            return Err(TriggerError::ConfigError(
                "Invalid slippage > 100%".to_string(),
            ));
        }

        let expected_output = (token_amount as u128)
            .checked_mul(target_price as u128)
            .ok_or_else(|| {
                TriggerError::ConfigError(
                    "Output calculation overflow (token_amount * target_price)".to_string(),
                )
            })?
            .checked_div(SELL_PRICE_SCALE_DENOMINATOR)
            .ok_or_else(|| {
                TriggerError::ConfigError(
                    "Output calculation underflow after price scale division".to_string(),
                )
            })?;

        let slippage_factor = 10000u128 - slippage_margin_bps as u128;
        let min_output = expected_output
            .checked_mul(slippage_factor)
            .ok_or_else(|| {
                TriggerError::ConfigError(
                    "Output calculation overflow after slippage factor".to_string(),
                )
            })?
            .checked_div(10000)
            .ok_or_else(|| {
                TriggerError::ConfigError(
                    "Output calculation underflow after slippage division".to_string(),
                )
            })?;

        u64::try_from(min_output).map_err(|_| {
            TriggerError::ConfigError(
                "Output calculation overflow after slippage division".to_string(),
            )
        })
    }

    // --- Pump.fun Implementation ---
    fn build_pump_sell_instruction(
        &self,
        mint: Pubkey,
        creator_pubkey: Option<Pubkey>,
        amount: u64,
        min_sol_output: u64,
    ) -> Result<Instruction> {
        self.build_pump_sell_instruction_with_token_program(
            mint,
            creator_pubkey,
            amount,
            min_sol_output,
            TOKEN_PROGRAM_ID,
            false,
        )
    }

    fn build_pump_sell_instruction_with_token_program(
        &self,
        mint: Pubkey,
        creator_pubkey: Option<Pubkey>,
        amount: u64,
        min_sol_output: u64,
        token_program: Pubkey,
        cashback_enabled: bool,
    ) -> Result<Instruction> {
        let program_id = self.config.pump_program_id;
        let user_pubkey = self.payer.pubkey();
        let creator_pubkey = creator_pubkey.ok_or_else(|| {
            TriggerError::ConfigError("missing canonical creator_pubkey for pump sell".to_string())
        })?;
        let fee_program =
            Pubkey::from_str(PUMP_FEE_PROGRAM_ID).expect("Invalid PUMP_FEE_PROGRAM_ID");
        validate_supported_pump_token_program(&token_program)?;

        // Derivations
        let (global_state, _) = Pubkey::find_program_address(&[PUMP_GLOBAL_SEED], &program_id);
        let (bonding_curve, _) =
            Pubkey::find_program_address(&[PUMP_BONDING_CURVE_SEED, mint.as_ref()], &program_id);
        let (bonding_curve_v2, _) =
            Pubkey::find_program_address(&[PUMP_BONDING_CURVE_V2_SEED, mint.as_ref()], &program_id);
        let (user_volume_accumulator, _) = Pubkey::find_program_address(
            &[PUMP_USER_VOLUME_ACCUMULATOR_SEED, user_pubkey.as_ref()],
            &program_id,
        );
        let (event_authority, _) =
            Pubkey::find_program_address(&[PUMP_EVENT_AUTHORITY_SEED], &program_id);
        let (creator_vault, _) = Pubkey::find_program_address(
            &[PUMP_CREATOR_VAULT_SEED, creator_pubkey.as_ref()],
            &program_id,
        );
        let (fee_config, _) = Pubkey::find_program_address(
            &[PUMP_FEE_CONFIG_SEED, &PUMP_FEE_SEED_CONST],
            &fee_program,
        );
        let associated_bonding_curve =
            get_associated_token_address(&bonding_curve, &mint, &token_program);
        let associated_user = get_associated_token_address(&user_pubkey, &mint, &token_program);

        // Data: global:sell discriminator + amount + min_sol_output
        let mut data = Vec::with_capacity(24);
        data.extend_from_slice(&SELL_DISCRIMINATOR);
        data.extend_from_slice(&amount.to_le_bytes());
        data.extend_from_slice(&min_sol_output.to_le_bytes());

        let mut accounts = vec![
            AccountMeta::new(global_state, false), // 0: global state (WRITABLE)
            AccountMeta::new(self.config.pump_fee_recipient, false), // 1: fee recipient (WRITABLE)
            AccountMeta::new(mint, false),         // 2: token mint (WRITABLE)
            AccountMeta::new(bonding_curve, false), // 3: bonding curve (WRITABLE)
            AccountMeta::new(associated_bonding_curve, false), // 4: bonding curve ATA (WRITABLE)
            AccountMeta::new(associated_user, false), // 5: user ATA (WRITABLE)
            AccountMeta::new(user_pubkey, true),   // 6: user (SIGNER+WRITABLE)
            AccountMeta::new_readonly(system_program::id(), false), // 7: system program
            AccountMeta::new(creator_vault, false), // 8: creator vault (WRITABLE)
            AccountMeta::new_readonly(token_program, false), // 9: token program
            AccountMeta::new_readonly(event_authority, false), // 10: event authority
            AccountMeta::new_readonly(program_id, false), // 11: pump program
            AccountMeta::new(fee_config, false),   // 12: fee config (WRITABLE)
            AccountMeta::new_readonly(fee_program, false), // 13: fee program
        ];
        if cashback_enabled {
            accounts.push(AccountMeta::new(user_volume_accumulator, false)); // 14: user vol accumulator (WRITABLE)
        }
        accounts.push(AccountMeta::new_readonly(bonding_curve_v2, false)); // LAST: bonding curve v2 (READONLY)

        Ok(Instruction {
            program_id,
            accounts,
            data,
        })
    }

    // --- Bonk.fun Implementation ---
    fn build_bonk_sell_instruction(
        &self,
        mint: Pubkey,
        amount: u64,
        min_quote_output: u64,
    ) -> Result<Instruction> {
        let program_id = self.config.bonk_program_id;
        let user_pubkey = self.payer.pubkey();
        let quote_mint = self.config.bonk_quote_mint; // e.g. $BONK

        // Derivations
        // Note: Bonk.fun seeds might differ slightly, usually "global" and "bonding-curve" are standard.
        // Checking standard SVM forks pattern:
        let (global_state, _) = Pubkey::find_program_address(&[b"global"], &program_id);

        let (bonding_curve, _) =
            Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], &program_id);

        // Token Accounts for the TOKEN being sold
        let associated_bonding_curve =
            get_associated_token_address(&bonding_curve, &mint, &TOKEN_PROGRAM_ID);
        let associated_user = get_associated_token_address(&user_pubkey, &mint, &TOKEN_PROGRAM_ID);

        // Token Accounts for the QUOTE TOKEN (e.g. BONK) being received
        // Crucial difference: We need ATA for the Quote Token!
        let associated_bonding_curve_quote =
            get_associated_token_address(&bonding_curve, &quote_mint, &TOKEN_PROGRAM_ID);
        let associated_user_quote =
            get_associated_token_address(&user_pubkey, &quote_mint, &TOKEN_PROGRAM_ID);

        // Data: global:sell discriminator + amount + min_quote_output
        // Using standard Anchor "sell" discriminator: SHA256("global:sell")[..8]
        let mut data = Vec::with_capacity(24);
        data.extend_from_slice(&SELL_DISCRIMINATOR);
        data.extend_from_slice(&amount.to_le_bytes());
        data.extend_from_slice(&min_quote_output.to_le_bytes());

        // Bonk.fun Account Structure (Quote-based Bonding Curve)
        // 1. Global
        // 2. Fee Recipient
        // 3. Mint (Token A)
        // 4. Bonding Curve
        // 5. Assoc Bond Curve (Token A)
        // 6. Assoc User (Token A)
        // 7. Quote Mint (Token B - e.g. BONK)
        // 8. Assoc Bond Curve (Quote)
        // 9. Assoc User (Quote)
        // 10. User
        // 11. System
        // 12. Token Program
        // 13. Assoc Token Program
        // 14. Program

        let accounts = vec![
            AccountMeta::new_readonly(global_state, false),
            AccountMeta::new(self.config.bonk_fee_recipient, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(bonding_curve, false),
            AccountMeta::new(associated_bonding_curve, false),
            AccountMeta::new(associated_user, false),
            // Quote Token Specifics:
            AccountMeta::new_readonly(quote_mint, false),
            AccountMeta::new(associated_bonding_curve_quote, false),
            AccountMeta::new(associated_user_quote, false),
            // Signer & Programs:
            AccountMeta::new(user_pubkey, true),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
            AccountMeta::new_readonly(ASSOCIATED_TOKEN_PROGRAM_ID, false),
            AccountMeta::new_readonly(program_id, false), // Event Auth / Program
        ];

        Ok(Instruction {
            program_id,
            accounts,
            data,
        })
    }
}

/// Helper to derive Associated Token Address
fn get_associated_token_address(
    wallet_address: &Pubkey,
    token_mint_address: &Pubkey,
    token_program: &Pubkey,
) -> Pubkey {
    Pubkey::find_program_address(
        &[
            &wallet_address.to_bytes(),
            &token_program.to_bytes(),
            &token_mint_address.to_bytes(),
        ],
        &ASSOCIATED_TOKEN_PROGRAM_ID,
    )
    .0
}

fn validate_supported_pump_token_program(token_program: &Pubkey) -> Result<()> {
    if *token_program == TOKEN_PROGRAM_ID || *token_program == TOKEN_2022_PROGRAM_ID {
        Ok(())
    } else {
        Err(TriggerError::ConfigError(format!(
            "unsupported pump token program: {}",
            token_program
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_build_bonk_instruction_structure() {
        let payer = Keypair::new();
        let builder = SellTxBuilder::with_default_config(payer);
        let mint = Pubkey::new_unique();

        let instruction_res = builder.build_bonk_sell_instruction(mint, 1_000_000, 500_000);

        assert!(instruction_res.is_ok());
        let instruction = instruction_res.unwrap();

        // Verify Discriminator
        assert_eq!(instruction.data[0..8], SELL_DISCRIMINATOR);

        // Bonk instructions typically have more accounts due to Quote Token ATAs
        // Expecting around 14 accounts
        assert_eq!(instruction.accounts.len(), 14);

        // Verify Quote Mint is present (Index 6)
        let quote_mint_acc = &instruction.accounts[6];
        assert_eq!(quote_mint_acc.pubkey, builder.config.bonk_quote_mint);
    }

    #[test]
    fn test_verify_sell_discriminator() {
        // The discriminator should match SHA256("global:sell")[..8]
        assert!(SellTxBuilder::verify_discriminator());
    }

    #[test]
    fn test_get_sell_discriminator() {
        let discriminator = SellTxBuilder::get_discriminator();
        assert_eq!(
            discriminator,
            [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad]
        );
    }

    #[test]
    fn test_pump_program_id() {
        let program_id = SellTxBuilder::pump_program_id();
        assert_eq!(
            program_id.to_string(),
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
        );
    }

    #[test]
    fn test_derive_bonding_curve() {
        let mint = Pubkey::new_unique();
        let (bonding_curve, bump) = SellTxBuilder::derive_bonding_curve(&mint);

        // Verify it's a valid PDA
        assert!(bonding_curve != Pubkey::default());
        assert!(bump <= 255);
    }

    #[tokio::test]
    async fn test_build_pump_sell_instruction() {
        let payer = Keypair::new();
        let builder = SellTxBuilder::with_default_config(payer);
        let mint = Pubkey::new_unique();
        let creator = Pubkey::new_unique();

        let instruction_res =
            builder.build_pump_sell_instruction(mint, Some(creator), 1_000_000, 500_000);

        assert!(instruction_res.is_ok());
        let instruction = instruction_res.unwrap();

        // Verify Discriminator
        assert_eq!(instruction.data[0..8], SELL_DISCRIMINATOR);

        // Non-cashback Pump.fun sells use the upgraded 15-account layout.
        assert_eq!(instruction.accounts.len(), 15);
        let (creator_vault, _) = Pubkey::find_program_address(
            &[PUMP_CREATOR_VAULT_SEED, creator.as_ref()],
            &builder.config.pump_program_id,
        );
        let fee_program = Pubkey::from_str(PUMP_FEE_PROGRAM_ID).expect("valid pump fee program id");
        let (fee_config, _) = Pubkey::find_program_address(
            &[PUMP_FEE_CONFIG_SEED, &PUMP_FEE_SEED_CONST],
            &fee_program,
        );
        assert_eq!(instruction.accounts[8].pubkey, creator_vault);
        assert_eq!(instruction.accounts[9].pubkey, TOKEN_PROGRAM_ID);
        assert_eq!(instruction.accounts[12].pubkey, fee_config);
        assert_eq!(instruction.accounts[13].pubkey, fee_program);
        let (bonding_curve_v2, _) = Pubkey::find_program_address(
            &[PUMP_BONDING_CURVE_V2_SEED, mint.as_ref()],
            &builder.config.pump_program_id,
        );
        assert_eq!(instruction.accounts[14].pubkey, bonding_curve_v2);

        // Verify instruction data layout (discriminator + amount + min_sol_output)
        assert_eq!(instruction.data.len(), 24);

        // Verify amount (1_000_000 in little-endian)
        let amount_bytes = u64::from_le_bytes(instruction.data[8..16].try_into().unwrap());
        assert_eq!(amount_bytes, 1_000_000);

        // Verify min_sol_output (500_000 in little-endian)
        let min_output_bytes = u64::from_le_bytes(instruction.data[16..24].try_into().unwrap());
        assert_eq!(min_output_bytes, 500_000);
    }

    #[test]
    fn test_calculate_min_output() {
        // 1 whole token (1_000_000 raw units) at 0.03 SOL/token (1e9-scaled) with 5% slippage.
        let result = SellTxBuilder::calculate_min_output(1_000_000, 30_000_000_000, 500);
        assert!(result.is_ok());
        let min_output = result.unwrap();
        assert_eq!(min_output, 28_500_000);
    }

    #[test]
    fn test_calculate_min_output_invalid_slippage() {
        // Slippage > 100% should error
        let result = SellTxBuilder::calculate_min_output(1000, 100, 10001);
        assert!(result.is_err());
    }

    #[test]
    fn test_calculate_min_output_overflow_protection() {
        let result = SellTxBuilder::calculate_min_output(u64::MAX, u64::MAX, 0);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_build_pump_sell_instruction_supports_token2022_accounts() {
        let payer = Keypair::new();
        let builder = SellTxBuilder::with_default_config(payer);
        let mint = Pubkey::new_unique();
        let creator = Pubkey::new_unique();

        let instruction = builder
            .build_pump_sell_instruction_with_token_program(
                mint,
                Some(creator),
                1_000_000,
                500_000,
                TOKEN_2022_PROGRAM_ID,
                false,
            )
            .expect("token-2022 pump sell instruction");

        let (bonding_curve, _) = Pubkey::find_program_address(
            &[PUMP_BONDING_CURVE_SEED, mint.as_ref()],
            &builder.config.pump_program_id,
        );
        let expected_curve_ata =
            get_associated_token_address(&bonding_curve, &mint, &TOKEN_2022_PROGRAM_ID);
        let expected_user_ata =
            get_associated_token_address(&builder.payer.pubkey(), &mint, &TOKEN_2022_PROGRAM_ID);
        let (event_authority, _) = Pubkey::find_program_address(
            &[PUMP_EVENT_AUTHORITY_SEED],
            &builder.config.pump_program_id,
        );
        let (creator_vault, _) = Pubkey::find_program_address(
            &[PUMP_CREATOR_VAULT_SEED, creator.as_ref()],
            &builder.config.pump_program_id,
        );

        assert_eq!(instruction.accounts[4].pubkey, expected_curve_ata);
        assert_eq!(instruction.accounts[5].pubkey, expected_user_ata);
        assert_eq!(instruction.accounts[8].pubkey, creator_vault);
        assert_eq!(instruction.accounts[9].pubkey, TOKEN_2022_PROGRAM_ID);
        assert_eq!(instruction.accounts[10].pubkey, event_authority);
        let (bonding_curve_v2, _) = Pubkey::find_program_address(
            &[PUMP_BONDING_CURVE_V2_SEED, mint.as_ref()],
            &builder.config.pump_program_id,
        );
        assert_eq!(instruction.accounts[14].pubkey, bonding_curve_v2);
    }

    #[tokio::test]
    async fn test_build_pump_sell_instruction_requires_creator_pubkey() {
        let payer = Keypair::new();
        let builder = SellTxBuilder::with_default_config(payer);
        let mint = Pubkey::new_unique();

        let err = builder
            .build_pump_sell_instruction(mint, None, 1_000_000, 500_000)
            .expect_err("missing creator should fail");

        assert!(err.to_string().contains("missing canonical creator_pubkey"));
    }

    #[tokio::test]
    async fn test_build_pump_sell_instruction_cashback_layout_keeps_accumulator_before_v2() {
        let payer = Keypair::new();
        let builder = SellTxBuilder::with_default_config(payer);
        let mint = Pubkey::new_unique();
        let creator = Pubkey::new_unique();

        let instruction = builder
            .build_pump_sell_instruction_with_token_program(
                mint,
                Some(creator),
                1_000_000,
                500_000,
                TOKEN_2022_PROGRAM_ID,
                true,
            )
            .expect("cashback pump sell instruction");

        let (user_volume_accumulator, _) = Pubkey::find_program_address(
            &[
                PUMP_USER_VOLUME_ACCUMULATOR_SEED,
                builder.payer.pubkey().as_ref(),
            ],
            &builder.config.pump_program_id,
        );
        let (bonding_curve_v2, _) = Pubkey::find_program_address(
            &[PUMP_BONDING_CURVE_V2_SEED, mint.as_ref()],
            &builder.config.pump_program_id,
        );

        assert_eq!(instruction.accounts.len(), 16);
        assert_eq!(instruction.accounts[14].pubkey, user_volume_accumulator);
        assert_eq!(instruction.accounts[15].pubkey, bonding_curve_v2);
    }
}
