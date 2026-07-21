//! Typed Pump route decoding and V2 instruction construction.
//!
//! This is intentionally separate from the historical `DirectBuyBuilder` and
//! `DirectSellBuilder` compatibility APIs.  A boolean side flag is not enough
//! to describe Pump route semantics: `buy_v2` is exact-base-out, while
//! `buy_exact_quote_in_v2` is exact-quote-in and `sell_v2` is exact-base-in.
//!
//! Account orders and discriminators are pinned to Pump's public IDL commit
//! `9c82f61cb711b044a17f770ab8ce9f9bdf78f333` (IDL SHA-256
//! `b90bc471327f671449271d5d1d42354d1fae6f5a06502f5834459a3108138e49).

use crate::direct_buy_builder::{DirectBuyBuilder, BREAKING_FEE_RECIPIENTS};
use ghost_core::PumpRouteVariant;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};
use thiserror::Error;

pub const PUMP_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P");
pub const PUMP_FEE_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ");
/// Current canonical Pump fee-config PDA.  It is account data owned by the
/// Pump Fees program, not itself an executable program account.
pub const PUMP_FEE_CONFIG_PDA: Pubkey =
    solana_sdk::pubkey!("8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt");
pub const SPL_TOKEN_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
pub const TOKEN_2022_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
pub const ASSOCIATED_TOKEN_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
pub const WRAPPED_SOL_MINT: Pubkey =
    solana_sdk::pubkey!("So11111111111111111111111111111111111111112");

const BONDING_CURVE_SEED: &[u8] = b"bonding-curve";
const BONDING_CURVE_V2_SEED: &[u8] = b"bonding-curve-v2";
const CREATOR_VAULT_SEED: &[u8] = b"creator-vault";
const EVENT_AUTHORITY_SEED: &[u8] = b"__event_authority";
const GLOBAL_SEED: &[u8] = b"global";
const GLOBAL_VOLUME_ACCUMULATOR_SEED: &[u8] = b"global_volume_accumulator";
const USER_VOLUME_ACCUMULATOR_SEED: &[u8] = b"user_volume_accumulator";
const SHARING_CONFIG_SEED: &[u8] = b"sharing-config";
const FEE_CONFIG_SEED: &[u8] = b"fee_config";
const FEE_SEED_CONST: [u8; 32] = [
    1, 86, 224, 246, 147, 102, 90, 207, 68, 219, 21, 104, 191, 23, 91, 170, 81, 137, 203, 151, 245,
    210, 255, 59, 101, 93, 43, 182, 253, 109, 24, 176,
];

pub const BUY_V2_DISCRIMINATOR: [u8; 8] = [0xb8, 0x17, 0xee, 0x61, 0x67, 0xc5, 0xd3, 0x3d];
pub const BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR: [u8; 8] =
    [0xc2, 0xab, 0x1c, 0x46, 0x68, 0x4d, 0x5b, 0x2f];
pub const SELL_V2_DISCRIMINATOR: [u8; 8] = [0x5d, 0xf6, 0x82, 0x3c, 0xe7, 0xe9, 0x40, 0xb2];
pub const LEGACY_BUY_DISCRIMINATOR: [u8; 8] = [0x66, 0x06, 0x3d, 0x12, 0x01, 0xda, 0xeb, 0xea];
pub const LEGACY_SELL_DISCRIMINATOR: [u8; 8] = [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad];

pub const BUY_V2_ACCOUNT_COUNT: usize = 27;
pub const BUY_EXACT_QUOTE_IN_V2_ACCOUNT_COUNT: usize = 27;
pub const SELL_V2_ACCOUNT_COUNT: usize = 26;
// The pinned Anchor IDL has 16/14 core accounts.  These are the exact current
// compatibility layouts after the mandatory BCV2/fee tail observed in the
// retained legacy chain fixtures; arbitrary remaining accounts are rejected.
pub const LEGACY_BUY_ACCOUNT_COUNT: usize = 18;
pub const LEGACY_SELL_ACCOUNT_COUNT: usize = 17;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodedPumpRouteInstruction {
    LegacyBuy {
        amount: u64,
        max_sol_cost: u64,
        track_volume: bool,
    },
    BuyV2 {
        amount: u64,
        max_sol_cost: u64,
    },
    BuyExactQuoteInV2 {
        spendable_quote_in: u64,
        min_tokens_out: u64,
    },
    LegacySell {
        amount: u64,
        min_sol_output: u64,
    },
    SellV2 {
        amount: u64,
        min_sol_output: u64,
    },
}

impl DecodedPumpRouteInstruction {
    pub const fn route_variant(self) -> PumpRouteVariant {
        match self {
            Self::LegacyBuy { .. } => PumpRouteVariant::LegacyBuy,
            Self::BuyV2 { .. } => PumpRouteVariant::BuyV2,
            Self::BuyExactQuoteInV2 { .. } => PumpRouteVariant::BuyExactQuoteInV2,
            Self::LegacySell { .. } => PumpRouteVariant::LegacySell,
            Self::SellV2 { .. } => PumpRouteVariant::SellV2,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PumpRouteError {
    #[error("Pump instruction payload is shorter than the eight-byte discriminator")]
    TruncatedDiscriminator,
    #[error("unknown Pump route discriminator {0}")]
    UnknownDiscriminator(String),
    #[error("invalid {route} instruction length: expected {expected}, got {actual}")]
    InvalidInstructionLength {
        route: &'static str,
        expected: usize,
        actual: usize,
    },
    #[error("invalid legacy buy track_volume flag {0}")]
    InvalidTrackVolumeFlag(u8),
    #[error("invalid account count for {route}: expected {expected}, got {actual}")]
    InvalidAccountCount {
        route: &'static str,
        expected: usize,
        actual: usize,
    },
    #[error("unexpected Pump program id {0}")]
    UnexpectedProgramId(Pubkey),
    #[error("invalid V2 route account at index {index}: expected {expected}, got {actual}")]
    AccountMismatch {
        index: usize,
        expected: Pubkey,
        actual: Pubkey,
    },
    #[error("invalid V2 route account flags at index {index}")]
    AccountFlagsMismatch { index: usize },
    #[error("route variant mismatch: expected {expected}, decoded {actual}")]
    RouteVariantMismatch {
        expected: &'static str,
        actual: &'static str,
    },
    #[error("owner evidence count for {route} must be {expected}, got {actual}")]
    InvalidOwnerEvidenceCount {
        route: &'static str,
        expected: usize,
        actual: usize,
    },
    #[error("owner evidence account mismatch at index {index}")]
    OwnerEvidenceAccountMismatch { index: usize },
    #[error("owner mismatch at index {index}: expected {expected}, got {actual}")]
    OwnerMismatch {
        index: usize,
        expected: Pubkey,
        actual: Pubkey,
    },
    #[error("base and quote token programs must be SPL Token or Token-2022")]
    UnsupportedTokenProgram,
    #[error("fee recipient is not present in the pinned Pump global allowlist")]
    UnauthorizedFeeRecipient,
    #[error("buyback fee recipient is not present in the pinned Pump allowlist")]
    UnauthorizedBuybackFeeRecipient,
    #[error("route identity account {0} must not be Pubkey::default()")]
    DefaultIdentityAccount(&'static str),
}

fn u64_at(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().expect("checked length"))
}

fn discriminator_hex(discriminator: &[u8]) -> String {
    discriminator
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Decodes only recognized Pump instructions.  Unknown discriminators have no
/// side inference and are a hard error.
pub fn decode_pump_route_instruction(
    data: &[u8],
) -> Result<DecodedPumpRouteInstruction, PumpRouteError> {
    if data.len() < 8 {
        return Err(PumpRouteError::TruncatedDiscriminator);
    }
    let discriminator: [u8; 8] = data[..8].try_into().expect("checked length");
    let invalid_len = |route, expected| PumpRouteError::InvalidInstructionLength {
        route,
        expected,
        actual: data.len(),
    };
    match discriminator {
        LEGACY_BUY_DISCRIMINATOR => {
            if data.len() != 25 {
                return Err(invalid_len("legacy_buy", 25));
            }
            let track_volume = match data[24] {
                0 => false,
                1 => true,
                value => return Err(PumpRouteError::InvalidTrackVolumeFlag(value)),
            };
            Ok(DecodedPumpRouteInstruction::LegacyBuy {
                amount: u64_at(data, 8),
                max_sol_cost: u64_at(data, 16),
                track_volume,
            })
        }
        BUY_V2_DISCRIMINATOR => {
            if data.len() != 24 {
                return Err(invalid_len("buy_v2", 24));
            }
            Ok(DecodedPumpRouteInstruction::BuyV2 {
                amount: u64_at(data, 8),
                max_sol_cost: u64_at(data, 16),
            })
        }
        BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR => {
            if data.len() != 24 {
                return Err(invalid_len("buy_exact_quote_in_v2", 24));
            }
            Ok(DecodedPumpRouteInstruction::BuyExactQuoteInV2 {
                spendable_quote_in: u64_at(data, 8),
                min_tokens_out: u64_at(data, 16),
            })
        }
        LEGACY_SELL_DISCRIMINATOR => {
            if data.len() != 24 {
                return Err(invalid_len("legacy_sell", 24));
            }
            Ok(DecodedPumpRouteInstruction::LegacySell {
                amount: u64_at(data, 8),
                min_sol_output: u64_at(data, 16),
            })
        }
        SELL_V2_DISCRIMINATOR => {
            if data.len() != 24 {
                return Err(invalid_len("sell_v2", 24));
            }
            Ok(DecodedPumpRouteInstruction::SellV2 {
                amount: u64_at(data, 8),
                min_sol_output: u64_at(data, 16),
            })
        }
        _ => Err(PumpRouteError::UnknownDiscriminator(discriminator_hex(
            &discriminator,
        ))),
    }
}

/// Verifies the exact number of accounts for every supported route before a
/// caller treats instruction data as route evidence.
pub fn validate_route_account_count(
    route: PumpRouteVariant,
    account_count: usize,
) -> Result<(), PumpRouteError> {
    let (route_name, expected) = match route {
        PumpRouteVariant::LegacyBuy => ("legacy_buy", LEGACY_BUY_ACCOUNT_COUNT),
        PumpRouteVariant::BuyV2 => ("buy_v2", BUY_V2_ACCOUNT_COUNT),
        PumpRouteVariant::BuyExactQuoteInV2 => {
            ("buy_exact_quote_in_v2", BUY_EXACT_QUOTE_IN_V2_ACCOUNT_COUNT)
        }
        PumpRouteVariant::LegacySell => ("legacy_sell", LEGACY_SELL_ACCOUNT_COUNT),
        PumpRouteVariant::SellV2 => ("sell_v2", SELL_V2_ACCOUNT_COUNT),
    };
    if account_count == expected {
        Ok(())
    } else {
        Err(PumpRouteError::InvalidAccountCount {
            route: route_name,
            expected,
            actual: account_count,
        })
    }
}

/// Inputs required by the unified V2 account layout.  All other accounts are
/// derived from these values under the pinned IDL seeds.  `creator` is
/// required canonical BondingCurve evidence — no default creator fallback is
/// allowed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PumpV2RouteAccounts {
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_token_program: Pubkey,
    pub quote_token_program: Pubkey,
    pub user: Pubkey,
    pub creator: Pubkey,
    pub fee_recipient: Pubkey,
    pub buyback_fee_recipient: Pubkey,
}

/// Inputs for the exact current legacy-buy compatibility layout.  The legacy
/// instruction is exact-base-out (`amount`, then `max_sol_cost`) and has a
/// distinct 18-account layout.  It is intentionally not represented by the
/// older `PumpfunBuyVariant` boolean-style compatibility surface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PumpLegacyBuyRouteAccounts {
    pub base_mint: Pubkey,
    pub base_token_program: Pubkey,
    pub user: Pubkey,
    pub creator: Pubkey,
    pub fee_recipient: Pubkey,
    pub buyback_fee_recipient: Pubkey,
}

/// Inputs for the exact current legacy-sell compatibility layout.  Legacy sell
/// has a pinned 14-account IDL core plus the mandatory three-account BCV2 fee
/// tail.  The builder does not accept arbitrary remaining accounts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PumpLegacySellRouteAccounts {
    pub base_mint: Pubkey,
    pub base_token_program: Pubkey,
    pub user: Pubkey,
    pub creator: Pubkey,
    pub fee_recipient: Pubkey,
    pub buyback_fee_recipient: Pubkey,
}

impl PumpLegacyBuyRouteAccounts {
    pub fn validate(&self) -> Result<(), PumpRouteError> {
        validate_legacy_route_identities(
            self.base_mint,
            self.base_token_program,
            self.user,
            self.creator,
            self.fee_recipient,
            self.buyback_fee_recipient,
        )
    }
}

impl PumpLegacySellRouteAccounts {
    pub fn validate(&self) -> Result<(), PumpRouteError> {
        validate_legacy_route_identities(
            self.base_mint,
            self.base_token_program,
            self.user,
            self.creator,
            self.fee_recipient,
            self.buyback_fee_recipient,
        )
    }
}

/// Account owner captured from canonical account-state evidence.  Program
/// identity checks alone are insufficient for mutable mint, curve, ATA and
/// fee-config accounts; this input lets ingest reject a route before execution
/// when an account is owned by the wrong program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PumpRouteAccountOwnerEvidence {
    pub pubkey: Pubkey,
    pub owner: Pubkey,
}

impl PumpV2RouteAccounts {
    pub fn validate(&self) -> Result<(), PumpRouteError> {
        for (name, key) in [
            ("base_mint", self.base_mint),
            ("quote_mint", self.quote_mint),
            ("user", self.user),
            ("creator", self.creator),
            ("fee_recipient", self.fee_recipient),
            ("buyback_fee_recipient", self.buyback_fee_recipient),
        ] {
            if key == Pubkey::default() {
                return Err(PumpRouteError::DefaultIdentityAccount(name));
            }
        }
        if !is_supported_token_program(&self.base_token_program)
            || !is_supported_token_program(&self.quote_token_program)
        {
            return Err(PumpRouteError::UnsupportedTokenProgram);
        }
        if !DirectBuyBuilder::is_authorized_fee_recipient(&self.fee_recipient) {
            return Err(PumpRouteError::UnauthorizedFeeRecipient);
        }
        if !BREAKING_FEE_RECIPIENTS.contains(&self.buyback_fee_recipient) {
            return Err(PumpRouteError::UnauthorizedBuybackFeeRecipient);
        }
        Ok(())
    }
}

fn is_supported_token_program(program: &Pubkey) -> bool {
    *program == SPL_TOKEN_PROGRAM_ID || *program == TOKEN_2022_PROGRAM_ID
}

fn validate_legacy_route_identities(
    base_mint: Pubkey,
    base_token_program: Pubkey,
    user: Pubkey,
    creator: Pubkey,
    fee_recipient: Pubkey,
    buyback_fee_recipient: Pubkey,
) -> Result<(), PumpRouteError> {
    for (name, key) in [
        ("base_mint", base_mint),
        ("user", user),
        ("creator", creator),
        ("fee_recipient", fee_recipient),
        ("buyback_fee_recipient", buyback_fee_recipient),
    ] {
        if key == Pubkey::default() {
            return Err(PumpRouteError::DefaultIdentityAccount(name));
        }
    }
    if !is_supported_token_program(&base_token_program) {
        return Err(PumpRouteError::UnsupportedTokenProgram);
    }
    if !DirectBuyBuilder::is_authorized_fee_recipient(&fee_recipient) {
        return Err(PumpRouteError::UnauthorizedFeeRecipient);
    }
    if !BREAKING_FEE_RECIPIENTS.contains(&buyback_fee_recipient) {
        return Err(PumpRouteError::UnauthorizedBuybackFeeRecipient);
    }
    Ok(())
}

fn ata(owner: &Pubkey, token_program: &Pubkey, mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[owner.as_ref(), token_program.as_ref(), mint.as_ref()],
        &ASSOCIATED_TOKEN_PROGRAM_ID,
    )
    .0
}

fn pda(seeds: &[&[u8]], program: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(seeds, program).0
}

/// Raw V2 builders with no implicit legacy fallback.
pub struct PumpV2RouteBuilder;

impl PumpV2RouteBuilder {
    pub fn build_buy_v2(
        accounts: &PumpV2RouteAccounts,
        amount: u64,
        max_sol_cost: u64,
    ) -> Result<Instruction, PumpRouteError> {
        accounts.validate()?;
        Ok(Self::build(
            PumpRouteVariant::BuyV2,
            accounts,
            amount,
            max_sol_cost,
        ))
    }

    pub fn build_buy_exact_quote_in_v2(
        accounts: &PumpV2RouteAccounts,
        spendable_quote_in: u64,
        min_tokens_out: u64,
    ) -> Result<Instruction, PumpRouteError> {
        accounts.validate()?;
        Ok(Self::build(
            PumpRouteVariant::BuyExactQuoteInV2,
            accounts,
            spendable_quote_in,
            min_tokens_out,
        ))
    }

    pub fn build_sell_v2(
        accounts: &PumpV2RouteAccounts,
        amount: u64,
        min_sol_output: u64,
    ) -> Result<Instruction, PumpRouteError> {
        accounts.validate()?;
        Ok(Self::build(
            PumpRouteVariant::SellV2,
            accounts,
            amount,
            min_sol_output,
        ))
    }

    fn build(
        route: PumpRouteVariant,
        route_accounts: &PumpV2RouteAccounts,
        first_arg: u64,
        second_arg: u64,
    ) -> Instruction {
        let discriminator = match route {
            PumpRouteVariant::BuyV2 => BUY_V2_DISCRIMINATOR,
            PumpRouteVariant::BuyExactQuoteInV2 => BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR,
            PumpRouteVariant::SellV2 => SELL_V2_DISCRIMINATOR,
            PumpRouteVariant::LegacyBuy | PumpRouteVariant::LegacySell => {
                unreachable!("V2 builder must not construct legacy routes")
            }
        };
        let mut data = Vec::with_capacity(24);
        data.extend_from_slice(&discriminator);
        data.extend_from_slice(&first_arg.to_le_bytes());
        data.extend_from_slice(&second_arg.to_le_bytes());
        Instruction {
            program_id: PUMP_PROGRAM_ID,
            accounts: Self::expected_accounts(route, route_accounts),
            data,
        }
    }

    /// Reconstructs the pinned IDL account sequence and flags.  This function
    /// has a single output for a given typed account contract; it accepts no
    /// free-form remaining-account tail.
    pub fn expected_accounts(
        route: PumpRouteVariant,
        route_accounts: &PumpV2RouteAccounts,
    ) -> Vec<AccountMeta> {
        debug_assert!(matches!(
            route,
            PumpRouteVariant::BuyV2
                | PumpRouteVariant::BuyExactQuoteInV2
                | PumpRouteVariant::SellV2
        ));
        let global = pda(&[GLOBAL_SEED], &PUMP_PROGRAM_ID);
        let bonding_curve = pda(
            &[BONDING_CURVE_SEED, route_accounts.base_mint.as_ref()],
            &PUMP_PROGRAM_ID,
        );
        let creator_vault = pda(
            &[CREATOR_VAULT_SEED, route_accounts.creator.as_ref()],
            &PUMP_PROGRAM_ID,
        );
        let sharing_config = pda(
            &[SHARING_CONFIG_SEED, route_accounts.base_mint.as_ref()],
            &PUMP_FEE_PROGRAM_ID,
        );
        let global_volume_accumulator = pda(&[GLOBAL_VOLUME_ACCUMULATOR_SEED], &PUMP_PROGRAM_ID);
        let user_volume_accumulator = pda(
            &[USER_VOLUME_ACCUMULATOR_SEED, route_accounts.user.as_ref()],
            &PUMP_PROGRAM_ID,
        );
        let fee_config = pda(&[FEE_CONFIG_SEED, &FEE_SEED_CONST], &PUMP_FEE_PROGRAM_ID);
        let event_authority = pda(&[EVENT_AUTHORITY_SEED], &PUMP_PROGRAM_ID);

        let mut metas = vec![
            AccountMeta::new_readonly(global, false),
            AccountMeta::new_readonly(route_accounts.base_mint, false),
            AccountMeta::new_readonly(route_accounts.quote_mint, false),
            AccountMeta::new_readonly(route_accounts.base_token_program, false),
            AccountMeta::new_readonly(route_accounts.quote_token_program, false),
            AccountMeta::new_readonly(ASSOCIATED_TOKEN_PROGRAM_ID, false),
            AccountMeta::new(route_accounts.fee_recipient, false),
            AccountMeta::new(
                ata(
                    &route_accounts.fee_recipient,
                    &route_accounts.quote_token_program,
                    &route_accounts.quote_mint,
                ),
                false,
            ),
            AccountMeta::new(route_accounts.buyback_fee_recipient, false),
            AccountMeta::new(
                ata(
                    &route_accounts.buyback_fee_recipient,
                    &route_accounts.quote_token_program,
                    &route_accounts.quote_mint,
                ),
                false,
            ),
            AccountMeta::new(bonding_curve, false),
            AccountMeta::new(
                ata(
                    &bonding_curve,
                    &route_accounts.base_token_program,
                    &route_accounts.base_mint,
                ),
                false,
            ),
            AccountMeta::new(
                ata(
                    &bonding_curve,
                    &route_accounts.quote_token_program,
                    &route_accounts.quote_mint,
                ),
                false,
            ),
            AccountMeta::new(route_accounts.user, true),
            AccountMeta::new(
                ata(
                    &route_accounts.user,
                    &route_accounts.base_token_program,
                    &route_accounts.base_mint,
                ),
                false,
            ),
            AccountMeta::new(
                ata(
                    &route_accounts.user,
                    &route_accounts.quote_token_program,
                    &route_accounts.quote_mint,
                ),
                false,
            ),
            AccountMeta::new(creator_vault, false),
            AccountMeta::new(
                ata(
                    &creator_vault,
                    &route_accounts.quote_token_program,
                    &route_accounts.quote_mint,
                ),
                false,
            ),
            AccountMeta::new_readonly(sharing_config, false),
        ];
        if matches!(
            route,
            PumpRouteVariant::BuyV2 | PumpRouteVariant::BuyExactQuoteInV2
        ) {
            metas.push(AccountMeta::new_readonly(global_volume_accumulator, false));
        }
        metas.extend([
            AccountMeta::new(user_volume_accumulator, false),
            AccountMeta::new(
                ata(
                    &user_volume_accumulator,
                    &route_accounts.quote_token_program,
                    &route_accounts.quote_mint,
                ),
                false,
            ),
            AccountMeta::new_readonly(fee_config, false),
            AccountMeta::new_readonly(PUMP_FEE_PROGRAM_ID, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(event_authority, false),
            AccountMeta::new_readonly(PUMP_PROGRAM_ID, false),
        ]);
        metas
    }
}

/// Strict builder for the current `buy` compatibility route.  Legacy buy is
/// exact-base-out: `amount` is the requested token output and
/// `max_sol_cost` is only a wallet-debit ceiling.
pub struct PumpLegacyBuyRouteBuilder;

impl PumpLegacyBuyRouteBuilder {
    pub fn build_buy(
        accounts: &PumpLegacyBuyRouteAccounts,
        amount: u64,
        max_sol_cost: u64,
    ) -> Result<Instruction, PumpRouteError> {
        accounts.validate()?;
        let mut data = Vec::with_capacity(25);
        data.extend_from_slice(&LEGACY_BUY_DISCRIMINATOR);
        data.extend_from_slice(&amount.to_le_bytes());
        data.extend_from_slice(&max_sol_cost.to_le_bytes());
        data.push(1); // `track_volume`; fixed by the current pinned route.
        Ok(Instruction {
            program_id: PUMP_PROGRAM_ID,
            accounts: Self::expected_accounts(accounts),
            data,
        })
    }

    pub fn expected_accounts(accounts: &PumpLegacyBuyRouteAccounts) -> Vec<AccountMeta> {
        let global = pda(&[GLOBAL_SEED], &PUMP_PROGRAM_ID);
        let bonding_curve = pda(
            &[BONDING_CURVE_SEED, accounts.base_mint.as_ref()],
            &PUMP_PROGRAM_ID,
        );
        let bonding_curve_v2 = pda(
            &[BONDING_CURVE_V2_SEED, accounts.base_mint.as_ref()],
            &PUMP_PROGRAM_ID,
        );
        let creator_vault = pda(
            &[CREATOR_VAULT_SEED, accounts.creator.as_ref()],
            &PUMP_PROGRAM_ID,
        );
        let event_authority = pda(&[EVENT_AUTHORITY_SEED], &PUMP_PROGRAM_ID);
        let global_volume_accumulator = pda(&[GLOBAL_VOLUME_ACCUMULATOR_SEED], &PUMP_PROGRAM_ID);
        let user_volume_accumulator = pda(
            &[USER_VOLUME_ACCUMULATOR_SEED, accounts.user.as_ref()],
            &PUMP_PROGRAM_ID,
        );
        let fee_config = pda(&[FEE_CONFIG_SEED, &FEE_SEED_CONST], &PUMP_FEE_PROGRAM_ID);
        vec![
            AccountMeta::new_readonly(global, false),
            AccountMeta::new(accounts.fee_recipient, false),
            AccountMeta::new_readonly(accounts.base_mint, false),
            AccountMeta::new(bonding_curve, false),
            AccountMeta::new(
                ata(
                    &bonding_curve,
                    &accounts.base_token_program,
                    &accounts.base_mint,
                ),
                false,
            ),
            AccountMeta::new(
                ata(
                    &accounts.user,
                    &accounts.base_token_program,
                    &accounts.base_mint,
                ),
                false,
            ),
            AccountMeta::new(accounts.user, true),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(accounts.base_token_program, false),
            AccountMeta::new(creator_vault, false),
            AccountMeta::new_readonly(event_authority, false),
            AccountMeta::new_readonly(PUMP_PROGRAM_ID, false),
            AccountMeta::new_readonly(global_volume_accumulator, false),
            AccountMeta::new(user_volume_accumulator, false),
            AccountMeta::new_readonly(fee_config, false),
            AccountMeta::new_readonly(PUMP_FEE_PROGRAM_ID, false),
            AccountMeta::new_readonly(bonding_curve_v2, false),
            AccountMeta::new(accounts.buyback_fee_recipient, false),
        ]
    }
}

/// Strict builder for the current `sell` compatibility route.  It is distinct
/// from [`PumpV2RouteBuilder`] because legacy sell has a different instruction
/// discriminator and account sequence.
pub struct PumpLegacySellRouteBuilder;

impl PumpLegacySellRouteBuilder {
    pub fn build_sell(
        accounts: &PumpLegacySellRouteAccounts,
        amount: u64,
        min_sol_output: u64,
    ) -> Result<Instruction, PumpRouteError> {
        accounts.validate()?;
        let mut data = Vec::with_capacity(24);
        data.extend_from_slice(&LEGACY_SELL_DISCRIMINATOR);
        data.extend_from_slice(&amount.to_le_bytes());
        data.extend_from_slice(&min_sol_output.to_le_bytes());
        Ok(Instruction {
            program_id: PUMP_PROGRAM_ID,
            accounts: Self::expected_accounts(accounts),
            data,
        })
    }

    pub fn expected_accounts(accounts: &PumpLegacySellRouteAccounts) -> Vec<AccountMeta> {
        let global = pda(&[GLOBAL_SEED], &PUMP_PROGRAM_ID);
        let bonding_curve = pda(
            &[BONDING_CURVE_SEED, accounts.base_mint.as_ref()],
            &PUMP_PROGRAM_ID,
        );
        let bonding_curve_v2 = pda(
            &[BONDING_CURVE_V2_SEED, accounts.base_mint.as_ref()],
            &PUMP_PROGRAM_ID,
        );
        let creator_vault = pda(
            &[CREATOR_VAULT_SEED, accounts.creator.as_ref()],
            &PUMP_PROGRAM_ID,
        );
        let event_authority = pda(&[EVENT_AUTHORITY_SEED], &PUMP_PROGRAM_ID);
        let user_volume_accumulator = pda(
            &[USER_VOLUME_ACCUMULATOR_SEED, accounts.user.as_ref()],
            &PUMP_PROGRAM_ID,
        );
        let fee_config = pda(&[FEE_CONFIG_SEED, &FEE_SEED_CONST], &PUMP_FEE_PROGRAM_ID);
        vec![
            AccountMeta::new_readonly(global, false),
            AccountMeta::new(accounts.fee_recipient, false),
            AccountMeta::new_readonly(accounts.base_mint, false),
            AccountMeta::new(bonding_curve, false),
            AccountMeta::new(
                ata(
                    &bonding_curve,
                    &accounts.base_token_program,
                    &accounts.base_mint,
                ),
                false,
            ),
            AccountMeta::new(
                ata(
                    &accounts.user,
                    &accounts.base_token_program,
                    &accounts.base_mint,
                ),
                false,
            ),
            AccountMeta::new(accounts.user, true),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new(creator_vault, false),
            AccountMeta::new_readonly(accounts.base_token_program, false),
            AccountMeta::new_readonly(event_authority, false),
            AccountMeta::new_readonly(PUMP_PROGRAM_ID, false),
            AccountMeta::new_readonly(fee_config, false),
            AccountMeta::new_readonly(PUMP_FEE_PROGRAM_ID, false),
            AccountMeta::new(user_volume_accumulator, false),
            AccountMeta::new_readonly(bonding_curve_v2, false),
            AccountMeta::new(accounts.buyback_fee_recipient, false),
        ]
    }
}

fn validate_instruction_against_expected(
    instruction: &Instruction,
    route: PumpRouteVariant,
    expected: &[AccountMeta],
) -> Result<(), PumpRouteError> {
    if instruction.program_id != PUMP_PROGRAM_ID {
        return Err(PumpRouteError::UnexpectedProgramId(instruction.program_id));
    }
    validate_route_account_count(route, instruction.accounts.len())?;
    for (index, (actual, expected)) in instruction.accounts.iter().zip(expected.iter()).enumerate()
    {
        if actual.pubkey != expected.pubkey {
            return Err(PumpRouteError::AccountMismatch {
                index,
                expected: expected.pubkey,
                actual: actual.pubkey,
            });
        }
        if actual.is_writable != expected.is_writable || actual.is_signer != expected.is_signer {
            return Err(PumpRouteError::AccountFlagsMismatch { index });
        }
    }
    Ok(())
}

/// Validates the complete current legacy-buy layout and its typed payload.
/// No free-form remaining-account tail is accepted.
pub fn validate_legacy_buy_instruction(
    instruction: &Instruction,
    route_accounts: &PumpLegacyBuyRouteAccounts,
) -> Result<DecodedPumpRouteInstruction, PumpRouteError> {
    route_accounts.validate()?;
    let decoded = decode_pump_route_instruction(&instruction.data)?;
    if !matches!(decoded, DecodedPumpRouteInstruction::LegacyBuy { .. }) {
        return Err(PumpRouteError::UnknownDiscriminator(discriminator_hex(
            &instruction.data[..8],
        )));
    }
    validate_instruction_against_expected(
        instruction,
        PumpRouteVariant::LegacyBuy,
        &PumpLegacyBuyRouteBuilder::expected_accounts(route_accounts),
    )?;
    Ok(decoded)
}

/// Validates the complete current legacy-sell layout and its typed payload.
/// No free-form remaining-account tail is accepted.
pub fn validate_legacy_sell_instruction(
    instruction: &Instruction,
    route_accounts: &PumpLegacySellRouteAccounts,
) -> Result<DecodedPumpRouteInstruction, PumpRouteError> {
    route_accounts.validate()?;
    let decoded = decode_pump_route_instruction(&instruction.data)?;
    if !matches!(decoded, DecodedPumpRouteInstruction::LegacySell { .. }) {
        return Err(PumpRouteError::UnknownDiscriminator(discriminator_hex(
            &instruction.data[..8],
        )));
    }
    validate_instruction_against_expected(
        instruction,
        PumpRouteVariant::LegacySell,
        &PumpLegacySellRouteBuilder::expected_accounts(route_accounts),
    )?;
    Ok(decoded)
}

/// Validates both discriminator semantics and every V2 account meta against
/// the IDL-derived layout.  Program identifiers, PDA seeds, account order,
/// writable flags and signer flags are checked before the instruction can be
/// used as route evidence.
pub fn validate_v2_instruction(
    instruction: &Instruction,
    route_accounts: &PumpV2RouteAccounts,
) -> Result<DecodedPumpRouteInstruction, PumpRouteError> {
    route_accounts.validate()?;
    let decoded = decode_pump_route_instruction(&instruction.data)?;
    let route = decoded.route_variant();
    if !matches!(
        route,
        PumpRouteVariant::BuyV2 | PumpRouteVariant::BuyExactQuoteInV2 | PumpRouteVariant::SellV2
    ) {
        return Err(PumpRouteError::UnknownDiscriminator(discriminator_hex(
            &instruction.data[..8],
        )));
    }
    let expected = PumpV2RouteBuilder::expected_accounts(route, route_accounts);
    validate_instruction_against_expected(instruction, route, &expected)?;
    Ok(decoded)
}

fn validate_account_owners(
    route_name: &'static str,
    instruction: &Instruction,
    expected_accounts: &[AccountMeta],
    owner_evidence: &[PumpRouteAccountOwnerEvidence],
    expected_owner: impl Fn(usize) -> Option<Pubkey>,
) -> Result<(), PumpRouteError> {
    if instruction.accounts.len() != expected_accounts.len() {
        return Err(PumpRouteError::InvalidAccountCount {
            route: route_name,
            expected: expected_accounts.len(),
            actual: instruction.accounts.len(),
        });
    }
    if owner_evidence.len() != expected_accounts.len() {
        return Err(PumpRouteError::InvalidOwnerEvidenceCount {
            route: route_name,
            expected: expected_accounts.len(),
            actual: owner_evidence.len(),
        });
    }
    for (index, (meta, evidence)) in instruction
        .accounts
        .iter()
        .zip(owner_evidence.iter())
        .enumerate()
    {
        if meta.pubkey != evidence.pubkey {
            return Err(PumpRouteError::OwnerEvidenceAccountMismatch { index });
        }
        if let Some(expected_owner) = expected_owner(index) {
            if evidence.owner != expected_owner {
                return Err(PumpRouteError::OwnerMismatch {
                    index,
                    expected: expected_owner,
                    actual: evidence.owner,
                });
            }
        }
    }
    Ok(())
}

/// Validates canonical owner evidence for every protocol-owned data account in
/// the exact legacy-buy route.  Caller-provided wallets and native programs
/// remain constrained by the preceding layout validation, not by a guessed
/// account owner.
pub fn validate_legacy_buy_account_owners(
    instruction: &Instruction,
    route_accounts: &PumpLegacyBuyRouteAccounts,
    owner_evidence: &[PumpRouteAccountOwnerEvidence],
) -> Result<(), PumpRouteError> {
    validate_legacy_buy_instruction(instruction, route_accounts)?;
    validate_account_owners(
        "legacy_buy",
        instruction,
        &PumpLegacyBuyRouteBuilder::expected_accounts(route_accounts),
        owner_evidence,
        |index| expected_legacy_buy_owner(index, route_accounts),
    )
}

/// Validates canonical owner evidence for every protocol-owned data account in
/// the exact legacy-sell route.
pub fn validate_legacy_sell_account_owners(
    instruction: &Instruction,
    route_accounts: &PumpLegacySellRouteAccounts,
    owner_evidence: &[PumpRouteAccountOwnerEvidence],
) -> Result<(), PumpRouteError> {
    validate_legacy_sell_instruction(instruction, route_accounts)?;
    validate_account_owners(
        "legacy_sell",
        instruction,
        &PumpLegacySellRouteBuilder::expected_accounts(route_accounts),
        owner_evidence,
        |index| expected_legacy_sell_owner(index, route_accounts),
    )
}

/// Validates canonical owner evidence for all V2 data accounts whose owner is
/// protocol-defined.  Native/executable program accounts and wallet accounts
/// are checked by exact public-key/order validation above; their loader or
/// wallet owner is intentionally not conflated with a Pump state owner.
pub fn validate_v2_account_owners(
    route: PumpRouteVariant,
    instruction: &Instruction,
    route_accounts: &PumpV2RouteAccounts,
    owner_evidence: &[PumpRouteAccountOwnerEvidence],
) -> Result<(), PumpRouteError> {
    let decoded = validate_v2_instruction(instruction, route_accounts)?;
    if decoded.route_variant() != route {
        return Err(PumpRouteError::RouteVariantMismatch {
            expected: route.as_str(),
            actual: decoded.route_variant().as_str(),
        });
    }
    let route_name = match route {
        PumpRouteVariant::BuyV2 => "buy_v2",
        PumpRouteVariant::BuyExactQuoteInV2 => "buy_exact_quote_in_v2",
        PumpRouteVariant::SellV2 => "sell_v2",
        PumpRouteVariant::LegacyBuy | PumpRouteVariant::LegacySell => {
            return Err(PumpRouteError::UnknownDiscriminator(route.as_str().into()));
        }
    };
    let expected_accounts = PumpV2RouteBuilder::expected_accounts(route, route_accounts);
    validate_account_owners(
        route_name,
        instruction,
        &expected_accounts,
        owner_evidence,
        |index| expected_v2_owner(route, index, route_accounts),
    )
}

fn expected_legacy_buy_owner(
    index: usize,
    accounts: &PumpLegacyBuyRouteAccounts,
) -> Option<Pubkey> {
    match index {
        0 | 3 | 9 | 12 | 13 | 16 => Some(PUMP_PROGRAM_ID),
        2 | 4 | 5 => Some(accounts.base_token_program),
        14 => Some(PUMP_FEE_PROGRAM_ID),
        _ => None,
    }
}

fn expected_legacy_sell_owner(
    index: usize,
    accounts: &PumpLegacySellRouteAccounts,
) -> Option<Pubkey> {
    match index {
        0 | 3 | 8 | 14 | 15 => Some(PUMP_PROGRAM_ID),
        2 | 4 | 5 => Some(accounts.base_token_program),
        12 => Some(PUMP_FEE_PROGRAM_ID),
        _ => None,
    }
}

fn expected_v2_owner(
    route: PumpRouteVariant,
    index: usize,
    accounts: &PumpV2RouteAccounts,
) -> Option<Pubkey> {
    // The V2 sell layout omits buy-only global_volume_accumulator at index 19,
    // so the final data-account indices shift by one.
    let buy_route = matches!(
        route,
        PumpRouteVariant::BuyV2 | PumpRouteVariant::BuyExactQuoteInV2
    );
    match index {
        0 | 10 | 16 => Some(PUMP_PROGRAM_ID),
        1 | 11 | 14 => Some(accounts.base_token_program),
        2 | 7 | 9 | 12 | 15 | 17 => Some(accounts.quote_token_program),
        18 => Some(PUMP_FEE_PROGRAM_ID),
        19 if buy_route => Some(PUMP_PROGRAM_ID),
        20 if buy_route => Some(PUMP_PROGRAM_ID),
        21 if buy_route => Some(accounts.quote_token_program),
        22 if buy_route => Some(PUMP_FEE_PROGRAM_ID),
        19 if !buy_route => Some(PUMP_PROGRAM_ID),
        20 if !buy_route => Some(accounts.quote_token_program),
        21 if !buy_route => Some(PUMP_FEE_PROGRAM_ID),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn fixture_accounts() -> PumpV2RouteAccounts {
        PumpV2RouteAccounts {
            base_mint: Pubkey::from_str("6P1EKxhPB1ZBMmduhuHZ9DcMVcgHqzj7frPNSduZpump").unwrap(),
            quote_mint: WRAPPED_SOL_MINT,
            base_token_program: TOKEN_2022_PROGRAM_ID,
            quote_token_program: SPL_TOKEN_PROGRAM_ID,
            user: Pubkey::from_str("56S29mZ3wqvw8hATuUUFqKhGcSGYFASRRFNT38W8q7G3").unwrap(),
            creator: Pubkey::from_str("79iC1835GFEruVVoh1tp6mcmAo3tVdR3V6TCs1BXBXWY").unwrap(),
            fee_recipient: Pubkey::from_str("FWsW1xNtWscwNmKv6wVsU1iTzRN6wmmk3MjxRP5tT7hz")
                .unwrap(),
            buyback_fee_recipient: Pubkey::from_str("9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7")
                .unwrap(),
        }
    }

    #[test]
    fn buy_v2_builder_matches_current_chain_layout() {
        let accounts = fixture_accounts();
        let ix =
            PumpV2RouteBuilder::build_buy_v2(&accounts, 3_488_479_091_111, 467_749_638).unwrap();
        let expected = [
            "4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf",
            "6P1EKxhPB1ZBMmduhuHZ9DcMVcgHqzj7frPNSduZpump",
            "So11111111111111111111111111111111111111112",
            "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
            "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
            "FWsW1xNtWscwNmKv6wVsU1iTzRN6wmmk3MjxRP5tT7hz",
            "7xQYoUjUJF1Kg6WVczoTAkaNhn5syQYcbvjmFrhjWpx",
            "9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7",
            "GAFuhgcd328SkkBYHpfadzmef9hTGAFRCi9QoCnsZQug",
            "9tjWWjthfFY5vJW5XHp1ckmXji8F28pXWwLQtqkBVYDP",
            "HKmHWXZ6mRxVZQe7qCDacyjTtpSCZzhFvYnGGsL7DTSK",
            "9FZq8CagKxN23rjmMXTAbPWozDjkTGPuMYp93pnzs3wk",
            "56S29mZ3wqvw8hATuUUFqKhGcSGYFASRRFNT38W8q7G3",
            "DFpjqhS7KjjbfJ3Z1iT88zDzYNZprotME6V4QTVhGPq1",
            "GVYgQ23wsZrK8Qe6Hv1KGoBXA2gQqQbbXNT31yFbNbk5",
            "3Bhktw9vSBLMLZZh1QiBfgoH4ibtakfHRpCiH7pM5afT",
            "5y5PDbL9Saviioi8oZQUGmftZgfCnZi3L2YrwANbn7zE",
            "41HfcPtkmGqP8CHrNDtP4xdQqeUHH8nLVfD9RpxoqsRQ",
            "Hq2wp8uJ9jCPsYgNHex8RtqdvMPfVGoYwjvF1ATiwn2Y",
            "3keQZW9jqiwCRREZ9vXX7Ngek9myaiSzfdeYQtcwnkZX",
            "GR4WnrUNhcnbiioANtgSj4od7PUTnN26aRi7ET8su9es",
            "8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt",
            "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ",
            "11111111111111111111111111111111",
            "Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1",
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
        ];
        assert_eq!(ix.accounts.len(), BUY_V2_ACCOUNT_COUNT);
        assert_eq!(
            ix.accounts
                .iter()
                .map(|meta| meta.pubkey.to_string())
                .collect::<Vec<_>>(),
            expected,
        );
        assert_eq!(&ix.data[..8], &BUY_V2_DISCRIMINATOR);
        assert_eq!(
            validate_v2_instruction(&ix, &accounts).unwrap(),
            DecodedPumpRouteInstruction::BuyV2 {
                amount: 3_488_479_091_111,
                max_sol_cost: 467_749_638,
            }
        );
    }

    #[test]
    fn all_v2_variants_are_independently_encoded_and_checked() {
        let accounts = fixture_accounts();
        let exact_quote =
            PumpV2RouteBuilder::build_buy_exact_quote_in_v2(&accounts, 22, 11).unwrap();
        assert_eq!(
            exact_quote.accounts.len(),
            BUY_EXACT_QUOTE_IN_V2_ACCOUNT_COUNT
        );
        assert!(matches!(
            validate_v2_instruction(&exact_quote, &accounts),
            Ok(DecodedPumpRouteInstruction::BuyExactQuoteInV2 { .. })
        ));
        let exact_quote_owner_evidence = exact_quote
            .accounts
            .iter()
            .enumerate()
            .map(|(index, meta)| PumpRouteAccountOwnerEvidence {
                pubkey: meta.pubkey,
                owner: expected_v2_owner(PumpRouteVariant::BuyExactQuoteInV2, index, &accounts)
                    .unwrap_or(system_program::id()),
            })
            .collect::<Vec<_>>();
        validate_v2_account_owners(
            PumpRouteVariant::BuyExactQuoteInV2,
            &exact_quote,
            &accounts,
            &exact_quote_owner_evidence,
        )
        .unwrap();
        let sell = PumpV2RouteBuilder::build_sell_v2(&accounts, 22, 11).unwrap();
        assert_eq!(sell.accounts.len(), SELL_V2_ACCOUNT_COUNT);
        assert!(matches!(
            validate_v2_instruction(&sell, &accounts),
            Ok(DecodedPumpRouteInstruction::SellV2 { .. })
        ));
        let sell_owner_evidence = sell
            .accounts
            .iter()
            .enumerate()
            .map(|(index, meta)| PumpRouteAccountOwnerEvidence {
                pubkey: meta.pubkey,
                owner: expected_v2_owner(PumpRouteVariant::SellV2, index, &accounts)
                    .unwrap_or(system_program::id()),
            })
            .collect::<Vec<_>>();
        validate_v2_account_owners(
            PumpRouteVariant::SellV2,
            &sell,
            &accounts,
            &sell_owner_evidence,
        )
        .unwrap();
    }

    #[test]
    fn unknown_discriminator_and_extra_account_fail_closed() {
        assert!(matches!(
            decode_pump_route_instruction(&[7; 24]),
            Err(PumpRouteError::UnknownDiscriminator(_))
        ));
        assert!(matches!(
            validate_route_account_count(PumpRouteVariant::BuyV2, BUY_V2_ACCOUNT_COUNT + 1),
            Err(PumpRouteError::InvalidAccountCount { .. })
        ));
    }

    #[test]
    fn legacy_buy_is_exact_base_out_with_strict_layout_and_owner_checks() {
        let accounts = PumpLegacyBuyRouteAccounts {
            base_mint: Pubkey::from_str("6P1EKxhPB1ZBMmduhuHZ9DcMVcgHqzj7frPNSduZpump").unwrap(),
            base_token_program: TOKEN_2022_PROGRAM_ID,
            user: Pubkey::from_str("56S29mZ3wqvw8hATuUUFqKhGcSGYFASRRFNT38W8q7G3").unwrap(),
            creator: Pubkey::from_str("79iC1835GFEruVVoh1tp6mcmAo3tVdR3V6TCs1BXBXWY").unwrap(),
            fee_recipient: Pubkey::from_str("FWsW1xNtWscwNmKv6wVsU1iTzRN6wmmk3MjxRP5tT7hz")
                .unwrap(),
            buyback_fee_recipient: Pubkey::from_str("9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7")
                .unwrap(),
        };
        let instruction = PumpLegacyBuyRouteBuilder::build_buy(&accounts, 100, 1_000).unwrap();
        assert_eq!(instruction.accounts.len(), LEGACY_BUY_ACCOUNT_COUNT);
        assert_eq!(
            validate_legacy_buy_instruction(&instruction, &accounts).unwrap(),
            DecodedPumpRouteInstruction::LegacyBuy {
                amount: 100,
                max_sol_cost: 1_000,
                track_volume: true,
            }
        );

        let mut evidence = instruction
            .accounts
            .iter()
            .enumerate()
            .map(|(index, meta)| PumpRouteAccountOwnerEvidence {
                pubkey: meta.pubkey,
                owner: expected_legacy_buy_owner(index, &accounts).unwrap_or(system_program::id()),
            })
            .collect::<Vec<_>>();
        validate_legacy_buy_account_owners(&instruction, &accounts, &evidence).unwrap();
        evidence[16].owner = system_program::id();
        assert!(matches!(
            validate_legacy_buy_account_owners(&instruction, &accounts, &evidence),
            Err(PumpRouteError::OwnerMismatch { index: 16, .. })
        ));
    }

    #[test]
    fn owner_evidence_rejects_a_curve_owned_by_another_program() {
        let accounts = fixture_accounts();
        let instruction = PumpV2RouteBuilder::build_buy_v2(&accounts, 100, 1_000).unwrap();
        let mut evidence = instruction
            .accounts
            .iter()
            .enumerate()
            .map(|(index, meta)| PumpRouteAccountOwnerEvidence {
                pubkey: meta.pubkey,
                owner: expected_v2_owner(PumpRouteVariant::BuyV2, index, &accounts)
                    .unwrap_or(system_program::id()),
            })
            .collect::<Vec<_>>();
        validate_v2_account_owners(PumpRouteVariant::BuyV2, &instruction, &accounts, &evidence)
            .unwrap();
        evidence[10].owner = system_program::id();
        assert!(matches!(
            validate_v2_account_owners(PumpRouteVariant::BuyV2, &instruction, &accounts, &evidence),
            Err(PumpRouteError::OwnerMismatch { index: 10, .. })
        ));
    }

    #[test]
    fn legacy_sell_builder_matches_current_chain_layout() {
        let accounts = PumpLegacySellRouteAccounts {
            base_mint: Pubkey::from_str("6P1EKxhPB1ZBMmduhuHZ9DcMVcgHqzj7frPNSduZpump").unwrap(),
            base_token_program: TOKEN_2022_PROGRAM_ID,
            user: Pubkey::from_str("6VCqJ1Jcoy2i2jcfZt15PFQfb6jBb8LQsSExv3kiuEUA").unwrap(),
            creator: Pubkey::from_str("79iC1835GFEruVVoh1tp6mcmAo3tVdR3V6TCs1BXBXWY").unwrap(),
            fee_recipient: Pubkey::from_str("7VtfL8fvgNfhz17qKRMjzQEXgbdpnHHHQRh54R9jP2RJ")
                .unwrap(),
            buyback_fee_recipient: Pubkey::from_str("5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD")
                .unwrap(),
        };
        let ix =
            PumpLegacySellRouteBuilder::build_sell(&accounts, 14_753_444_836_747, 1_487_428_297)
                .unwrap();
        let expected = [
            "4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf",
            "7VtfL8fvgNfhz17qKRMjzQEXgbdpnHHHQRh54R9jP2RJ",
            "6P1EKxhPB1ZBMmduhuHZ9DcMVcgHqzj7frPNSduZpump",
            "9tjWWjthfFY5vJW5XHp1ckmXji8F28pXWwLQtqkBVYDP",
            "HKmHWXZ6mRxVZQe7qCDacyjTtpSCZzhFvYnGGsL7DTSK",
            "D2rnwKJaSCpbViF2fUJJc6GxELyCSHpjr8HjYyfg1ruW",
            "6VCqJ1Jcoy2i2jcfZt15PFQfb6jBb8LQsSExv3kiuEUA",
            "11111111111111111111111111111111",
            "3Bhktw9vSBLMLZZh1QiBfgoH4ibtakfHRpCiH7pM5afT",
            "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
            "Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1",
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
            "8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt",
            "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ",
            "2fQ35ig934RRSK25GxoiC3TSdfT3tw2U9UqonFM2hw56",
            "ETy1VXz4kR6mfqgWSWZTkAWnA3A3qGWNmyuvrUy798J9",
            "5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD",
        ];
        assert_eq!(
            ix.accounts
                .iter()
                .map(|meta| meta.pubkey.to_string())
                .collect::<Vec<_>>(),
            expected,
        );
        assert_eq!(ix.accounts.len(), LEGACY_SELL_ACCOUNT_COUNT);
        assert_eq!(&ix.data[..8], &LEGACY_SELL_DISCRIMINATOR);
        assert!(matches!(
            validate_legacy_sell_instruction(&ix, &accounts),
            Ok(DecodedPumpRouteInstruction::LegacySell { .. })
        ));
        let owner_evidence = ix
            .accounts
            .iter()
            .enumerate()
            .map(|(index, meta)| PumpRouteAccountOwnerEvidence {
                pubkey: meta.pubkey,
                owner: expected_legacy_sell_owner(index, &accounts).unwrap_or(system_program::id()),
            })
            .collect::<Vec<_>>();
        validate_legacy_sell_account_owners(&ix, &accounts, &owner_evidence).unwrap();
    }
}
