//! InitializePool instruction parser for Pump.fun and Bonk.fun AMMs
//!
//! This module provides binary parsing functionality for InitializePool instructions
//! from both Pump.fun and Bonk.fun AMM programs.
//!
//! ## Important: Pump.fun Discriminators
//!
//! Pump.fun does NOT use standard Anchor-style discriminators for pool creation.
//! The actual on-chain discriminator for `create` is `[0xd6, 0x90, 0x4c, 0xec, 0x5f, 0x8b, 0x31, 0xb4]`,
//! which does NOT match SHA256("global:create"). This is a custom/hardcoded discriminator.

use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use solana_sdk::pubkey::Pubkey;
use std::convert::TryInto;
use tracing::{debug, trace, warn};

// =============================================================================
// System Address Validation
// =============================================================================

/// System Program ID - MUST NEVER be extracted as a pool or mint
const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";

/// Token Program ID - MUST NEVER be extracted as a pool or mint
const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

/// Token-2022 Program ID - MUST NEVER be extracted as a creator or mint
const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

/// Pump.fun Global State - MUST NEVER be extracted as a mint
const PUMPFUN_GLOBAL_STATE: &str = "TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM";

/// Validate that a Pubkey is not a system program or invalid address
/// Returns (is_valid, error_description) for better error reporting
fn is_valid_account(pubkey: &Pubkey, field_name: &str) -> (bool, Option<String>) {
    let pubkey_str = pubkey.to_string();

    if pubkey_str == SYSTEM_PROGRAM_ID {
        let msg = format!("System Program ID ({})", SYSTEM_PROGRAM_ID);
        warn!("🚨 REJECTED: {} is {}", field_name, msg);
        return (false, Some(msg));
    }

    if pubkey_str == TOKEN_PROGRAM_ID {
        let msg = format!("Token Program ID ({})", TOKEN_PROGRAM_ID);
        warn!("🚨 REJECTED: {} is {}", field_name, msg);
        return (false, Some(msg));
    }

    if pubkey_str == TOKEN_2022_PROGRAM_ID {
        let msg = format!("Token-2022 Program ID ({})", TOKEN_2022_PROGRAM_ID);
        warn!("🚨 REJECTED: {} is {}", field_name, msg);
        return (false, Some(msg));
    }

    if pubkey_str == PUMPFUN_GLOBAL_STATE {
        let msg = format!("Pump.fun Global State ({})", PUMPFUN_GLOBAL_STATE);
        warn!("🚨 REJECTED: {} is {}", field_name, msg);
        return (false, Some(msg));
    }

    // Check for sysvar accounts
    if pubkey_str.starts_with("Sysvar") {
        let msg = format!("Sysvar account ({})", pubkey_str);
        warn!("🚨 REJECTED: {} is {}", field_name, msg);
        return (false, Some(msg));
    }

    (true, None)
}

/// ACTUAL Pump.fun "create" discriminator observed on mainnet
/// This is NOT computed from SHA256("global:create") - Pump.fun uses custom discriminators
/// Observed value: d6904cec5f8b31b4 (hex)
pub const PUMPFUN_CREATE_DISCRIMINATOR: [u8; 8] = [0xd6, 0x90, 0x4c, 0xec, 0x5f, 0x8b, 0x31, 0xb4];

/// Pump.fun "buy" discriminator (for reference, matches SHA256("global:buy"))
pub const PUMPFUN_BUY_DISCRIMINATOR: [u8; 8] = [0x66, 0x06, 0x3d, 0x12, 0x01, 0xda, 0xeb, 0xea];

/// Pump.fun "sell" discriminator
pub const PUMPFUN_SELL_DISCRIMINATOR: [u8; 8] = [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad];
/// PumpSwap CPI event wrapper discriminator.
pub const PUMPSWAP_OUTER_WRAPPER_DISCRIMINATOR: [u8; 8] =
    [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
/// PumpSwap BuyEvent discriminator.
pub const PUMPSWAP_EVENT_BUY_DISCRIMINATOR: [u8; 8] =
    [0x67, 0xf4, 0x52, 0x1f, 0x2c, 0xf5, 0x77, 0x77];
/// PumpSwap SellEvent discriminator.
pub const PUMPSWAP_EVENT_SELL_DISCRIMINATOR: [u8; 8] =
    [0x3e, 0x2f, 0x37, 0x0a, 0xa5, 0x03, 0xdc, 0x2a];
/// PumpSwap buy_exact_quote_in discriminator.
pub const PUMPSWAP_BUY_EXACT_QUOTE_IN_DISCRIMINATOR: [u8; 8] =
    [0xc6, 0x2e, 0x15, 0x52, 0xb4, 0xd9, 0xe8, 0x70];

/// Discriminator for Anchor `create` instruction (theoretical - NOT used by Pump.fun)
///
/// Computed as the first 8 bytes of SHA256("global:create")
/// NOTE: Pump.fun does NOT use this! Kept for reference only.
#[allow(dead_code)]
pub fn compute_create_discriminator() -> [u8; 8] {
    let mut hasher = Sha256::new();
    hasher.update(b"global:create");
    let result = hasher.finalize();
    result[0..8].try_into().unwrap()
}

/// Discriminator for Anchor `initialize_pool` instruction (legacy/Bonk.fun)
///
/// Computed as the first 8 bytes of SHA256("global:initialize_pool")
pub fn compute_initialize_pool_discriminator() -> [u8; 8] {
    let mut hasher = Sha256::new();
    hasher.update(b"global:initialize_pool");
    let result = hasher.finalize();
    result[0..8].try_into().unwrap()
}

/// Known discriminators for pool creation instructions
pub struct Discriminators;

impl Discriminators {
    /// Standard Anchor discriminator for initialize_pool
    pub fn anchor_initialize_pool() -> [u8; 8] {
        compute_initialize_pool_discriminator()
    }

    /// Pump.fun specific discriminator - ACTUAL on-chain value
    /// This is a custom discriminator, NOT computed from Anchor convention
    /// Value: [0xd6, 0x90, 0x4c, 0xec, 0x5f, 0x8b, 0x31, 0xb4]
    pub fn pumpfun_initialize_pool() -> [u8; 8] {
        PUMPFUN_CREATE_DISCRIMINATOR
    }

    /// Bonk.fun specific discriminator
    /// Uses standard Anchor "initialize_pool" discriminator
    pub fn bonkfun_initialize_pool() -> [u8; 8] {
        Self::anchor_initialize_pool()
    }
}

/// Parsed trade (Buy/Sell) instruction data
#[derive(Debug, Clone)]
pub struct TradeData {
    /// True if Buy, false if Sell
    pub is_buy: bool,
    /// Amount of tokens/lamports to trade
    pub amount: u64,
    /// For Buy: maximum SOL cost user is willing to pay
    pub max_sol_cost: u64,
    /// For Sell: minimum SOL output user expects to receive
    pub min_sol_output: u64,
}

/// Parsed InitializePool instruction data
#[derive(Debug, Clone, Default)]
pub struct InitializePoolData {
    /// Virtual token reserves (for bonding curve calculations)
    pub virtual_token_reserves: Option<u64>,

    /// Virtual SOL reserves (for bonding curve calculations)
    pub virtual_sol_reserves: Option<u64>,

    /// Real token reserves (actual tokens in pool)
    pub real_token_reserves: Option<u64>,

    /// Real SOL reserves (actual SOL in pool)
    pub real_sol_reserves: Option<u64>,

    /// Total token supply
    pub token_supply: Option<u64>,

    /// Raw instruction data for debugging
    pub raw_data: Vec<u8>,
}

/// AMM program type for parsing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmmType {
    PumpFun,
    BonkFun,
    PumpSwap,
}

impl AmmType {
    /// Get the expected discriminator for this AMM type
    pub fn discriminator(&self) -> [u8; 8] {
        match self {
            AmmType::PumpFun => Discriminators::pumpfun_initialize_pool(),
            AmmType::BonkFun => Discriminators::bonkfun_initialize_pool(),
            AmmType::PumpSwap => Discriminators::pumpfun_initialize_pool(), // PumpSwap uses same disc for now
        }
    }

    /// Get the program ID for this AMM type
    pub fn program_id(&self) -> Pubkey {
        match self {
            AmmType::PumpFun => "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
                .parse()
                .unwrap(),
            AmmType::BonkFun => "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj"
                .parse()
                .unwrap(),
            AmmType::PumpSwap => "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
                .parse()
                .unwrap(),
        }
    }

    /// Identify AMM type from program ID
    pub fn from_program_id(program_id: &Pubkey) -> Option<Self> {
        if program_id == &AmmType::PumpFun.program_id() {
            Some(AmmType::PumpFun)
        } else if program_id == &AmmType::BonkFun.program_id() {
            Some(AmmType::BonkFun)
        } else if program_id == &AmmType::PumpSwap.program_id() {
            Some(AmmType::PumpSwap)
        } else {
            None
        }
    }
}

/// Check if instruction data contains an InitializePool discriminator
pub fn is_initialize_pool(data: &[u8], amm_type: AmmType, verbose: bool) -> bool {
    if data.len() < 8 {
        if verbose {
            trace!(
                "Data too short for discriminator check: {} bytes",
                data.len()
            );
        }
        return false;
    }

    let discriminator = &data[0..8];
    let expected = amm_type.discriminator();

    let matches = discriminator == expected;

    if verbose {
        if matches {
            debug!(
                "Discriminator match for {:?}: {:02x?}",
                amm_type, discriminator
            );
        } else {
            trace!(
                "Discriminator mismatch for {:?}: expected {:02x?}, got {:02x?}",
                amm_type,
                expected,
                discriminator
            );
        }
    }

    matches
}

/// Parse InitializePool instruction data
///
/// # Arguments
/// * `data` - Raw instruction data
/// * `amm_type` - Type of AMM (PumpFun or BonkFun)
/// * `verbose` - Enable verbose logging
///
/// # Returns
/// Parsed InitializePoolData if successful
pub fn parse_initialize_pool(
    data: &[u8],
    amm_type: AmmType,
    verbose: bool,
) -> Result<InitializePoolData> {
    // Verify discriminator
    if !is_initialize_pool(data, amm_type, verbose) {
        return Err(anyhow!("Invalid discriminator for InitializePool"));
    }

    if verbose {
        debug!(
            "Parsing InitializePool for {:?}, data length: {} bytes",
            amm_type,
            data.len()
        );
        trace!("Raw data: {:02x?}", data);
    }

    // Skip discriminator (first 8 bytes)
    let params_data = &data[8..];

    let mut parsed = InitializePoolData {
        raw_data: data.to_vec(),
        ..Default::default()
    };

    // Parse parameters based on AMM type
    match amm_type {
        AmmType::PumpFun => parse_pumpfun_params(params_data, &mut parsed, verbose)?,
        AmmType::BonkFun => parse_bonkfun_params(params_data, &mut parsed, verbose)?,
        AmmType::PumpSwap => parse_pumpfun_params(params_data, &mut parsed, verbose)?, // PumpSwap uses pump-fun layout for now
    }

    if verbose {
        debug!("Successfully parsed InitializePool: {:?}", parsed);
    }

    Ok(parsed)
}

/// Parse Pump.fun specific parameters
///
/// Expected format (after discriminator):
/// - virtual_token_reserves: u64 (8 bytes)
/// - virtual_sol_reserves: u64 (8 bytes)
/// - real_token_reserves: u64 (8 bytes)
/// - real_sol_reserves: u64 (8 bytes)
/// - token_supply: u64 (8 bytes)
fn parse_pumpfun_params(
    params_data: &[u8],
    parsed: &mut InitializePoolData,
    verbose: bool,
) -> Result<()> {
    if verbose {
        debug!(
            "Parsing Pump.fun parameters, {} bytes available",
            params_data.len()
        );
    }

    let mut offset = 0;

    // Parse virtual_token_reserves (offset 0, 8 bytes)
    if params_data.len() >= offset + 8 {
        parsed.virtual_token_reserves = Some(read_u64_le(params_data, offset)?);
        if verbose {
            debug!(
                "virtual_token_reserves: {:?}",
                parsed.virtual_token_reserves
            );
        }
        offset += 8;
    }

    // Parse virtual_sol_reserves (offset 8, 8 bytes)
    if params_data.len() >= offset + 8 {
        parsed.virtual_sol_reserves = Some(read_u64_le(params_data, offset)?);
        if verbose {
            debug!("virtual_sol_reserves: {:?}", parsed.virtual_sol_reserves);
        }
        offset += 8;
    }

    // Parse real_token_reserves (offset 16, 8 bytes)
    if params_data.len() >= offset + 8 {
        parsed.real_token_reserves = Some(read_u64_le(params_data, offset)?);
        if verbose {
            debug!("real_token_reserves: {:?}", parsed.real_token_reserves);
        }
        offset += 8;
    }

    // Parse real_sol_reserves (offset 24, 8 bytes)
    if params_data.len() >= offset + 8 {
        parsed.real_sol_reserves = Some(read_u64_le(params_data, offset)?);
        if verbose {
            debug!("real_sol_reserves: {:?}", parsed.real_sol_reserves);
        }
        offset += 8;
    }

    // Parse token_supply (offset 32, 8 bytes)
    if params_data.len() >= offset + 8 {
        parsed.token_supply = Some(read_u64_le(params_data, offset)?);
        if verbose {
            debug!("token_supply: {:?}", parsed.token_supply);
        }
    }

    Ok(())
}

/// Parse Bonk.fun specific parameters
///
/// Expected format (after discriminator):
/// Similar to Pump.fun but may have different parameter ordering or additional fields
fn parse_bonkfun_params(
    params_data: &[u8],
    parsed: &mut InitializePoolData,
    verbose: bool,
) -> Result<()> {
    if verbose {
        debug!(
            "Parsing Bonk.fun parameters, {} bytes available",
            params_data.len()
        );
    }

    // Bonk.fun uses similar structure to Pump.fun for now
    // If the actual format differs, update this function accordingly
    parse_pumpfun_params(params_data, parsed, verbose)?;

    // Add any Bonk.fun specific parsing here if needed

    Ok(())
}

/// Read a u64 value in little-endian format from buffer at given offset
fn read_u64_le(data: &[u8], offset: usize) -> Result<u64> {
    if data.len() < offset + 8 {
        return Err(anyhow!(
            "Insufficient data to read u64 at offset {}: only {} bytes available",
            offset,
            data.len()
        ));
    }

    let bytes: [u8; 8] = data[offset..offset + 8]
        .try_into()
        .map_err(|_| anyhow!("Failed to convert bytes to u64"))?;

    Ok(u64::from_le_bytes(bytes))
}

/// Helper to get account at specific index
fn get_account_at_index(
    all_accounts: &[Pubkey],
    account_indices: &[u8],
    index: usize,
) -> Option<Pubkey> {
    account_indices
        .get(index)
        .and_then(|&idx| all_accounts.get(idx as usize))
        .copied()
}

/// Parse accounts from instruction for InitializePool
///
/// ## Lenient Parsing Behavior
///
/// This function is designed to be **forward compatible** with extra accounts.
/// It extracts accounts based on the instruction's account_indices, using
/// Option<Pubkey> for all fields. This means:
/// - Extra accounts in the transaction are safely ignored
/// - Missing accounts result in None values (caller decides if that's an error)
/// - Transactions with 24, 26, 30+ accounts work without modification
///
/// ## Common Account Layout for InitializePool
///
/// - 0. Creator/Payer (signer)
/// - 1. Pool account (PDA, to be initialized)
/// - 2. Mint (base token mint)
/// - 3. Bonding curve (PDA)
/// - 4. Quote mint (optional, depending on AMM)
/// - 5+ Additional accounts (safely ignored by this parser)
#[derive(Debug, Clone)]
pub struct InitializePoolAccounts {
    pub creator: Option<Pubkey>,
    pub pool: Option<Pubkey>,
    pub mint: Option<Pubkey>,
    pub bonding_curve: Option<Pubkey>,
    pub quote_mint: Option<Pubkey>,
}

/// Extract InitializePool accounts from transaction accounts
///
/// This function extracts accounts at the expected indices for Pump.fun "Create" instruction.
///
/// ## Pump.fun Account Layout (FIXED - DO NOT CHANGE)
/// - Index 0: Creator/Payer (signer)
/// - Index 1: Pool account (PDA, to be initialized)
/// - Index 2: Mint (base token mint)
/// - Index 3: Bonding curve (PDA)
/// - Index 4: Quote mint (optional, Pump.fun defaults to SOL)
///
/// If the explicit pool account is missing but a bonding curve is present, the parser
/// falls back to using the bonding curve as the pool for backward compatibility.
///
/// This function is lenient with extra accounts - it only extracts the accounts
/// at the expected indices and ignores any additional accounts in the transaction.

/// Extract InitializePool accounts from transaction accounts
pub fn extract_accounts(
    all_accounts: &[Pubkey],
    account_indices: &[u8],
    amm_type: AmmType,
    verbose: bool,
) -> Result<InitializePoolAccounts> {
    if verbose {
        debug!(
            "Extracting accounts for {:?}, {} indices provided",
            amm_type,
            account_indices.len()
        );
    }

    let mut accounts = InitializePoolAccounts {
        creator: None,
        pool: None,
        mint: None,
        bonding_curve: None,
        quote_mint: None,
    };

    // DEFINICJA MAPOWANIA INDEKSÓW ZALEŻNIE OD AMM
    let (mint_idx, pool_idx, bc_idx, creator_idx) = match amm_type {
        AmmType::PumpFun => {
            // PUMP.FUN LAYOUT:
            // 0: Mint
            // 1: Mint Authority
            // 2: Bonding Curve (To jest de facto Pool)
            // 3: Associated Bonding Curve
            // 4: Global State
            // ...
            // 7: User (Signer) - czasami może być gdzie indziej, ale 7 to standard
            (0, 2, 2, 7)
        }
        AmmType::BonkFun => {
            // BONK.FUN / STANDARD ANCHOR (do weryfikacji, ale zakładamy standard):
            // 0: Creator
            // 1: Pool
            // 2: Mint
            // 3: Bonding Curve
            (2, 1, 3, 0)
        }
        AmmType::PumpSwap => {
            // PUMPSWAP LAYOUT: similar to PumpFun for now
            (0, 2, 2, 7)
        }
    };

    // 1. EXTRACT CREATOR
    // Dla Pump.fun creator jest daleko (index 7), więc musimy sprawdzić czy tablica jest dość długa
    accounts.creator =
        if let Some(creator) = get_account_at_index(all_accounts, account_indices, creator_idx) {
            // Opcjonalna walidacja, czy to nie system/token program (zostawiamy Twoją logikę)
            let (is_valid, _) = is_valid_account(&creator, "creator");
            if is_valid {
                Some(creator)
            } else {
                None
            }
        } else {
            // Fallback dla Pump.fun: jeśli nie ma indexu 7, spróbujmy Mint Authority (index 1) jako creatora
            if amm_type == AmmType::PumpFun {
                get_account_at_index(all_accounts, account_indices, 1).and_then(|creator| {
                    let (is_valid, _) = is_valid_account(&creator, "creator_fallback");
                    if is_valid {
                        Some(creator)
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        };

    // 2. EXTRACT POOL & BONDING CURVE
    // W Pump.fun Pool == Bonding Curve
    if let Some(bc) = get_account_at_index(all_accounts, account_indices, bc_idx) {
        let (is_valid, error_desc) = is_valid_account(&bc, "bonding_curve");
        if is_valid {
            accounts.bonding_curve = Some(bc);
            // Jeśli layout wskazuje ten sam index dla poola, lub pool nie został znaleziony wcześniej
            accounts.pool = Some(bc);
        } else {
            let msg = error_desc.unwrap_or_else(|| "unknown".to_string());
            warn!(
                "🚨 CRITICAL: Invalid bonding curve at index {}: {} - {}",
                bc_idx, bc, msg
            );
            return Err(anyhow!("Invalid bonding curve: {}", msg));
        }
    }

    // 3. EXTRACT MINT
    if let Some(mint) = get_account_at_index(all_accounts, account_indices, mint_idx) {
        let (is_valid, error_desc) = is_valid_account(&mint, "mint");
        if is_valid {
            accounts.mint = Some(mint);
        } else {
            let msg = error_desc.unwrap_or_else(|| "unknown".to_string());
            warn!(
                "🚨 CRITICAL: Invalid mint at index {}: {} - {}",
                mint_idx, mint, msg
            );
            return Err(anyhow!("Invalid mint: {}", msg));
        }
    }

    // 4. QUOTE MINT (Bez zmian)
    match amm_type {
        AmmType::PumpFun => {
            accounts.quote_mint = Some(
                "So11111111111111111111111111111111111111112"
                    .parse()
                    .unwrap(),
            );
        }
        AmmType::BonkFun => {
            // Tu zakładamy, że Bonk może mieć quote na indexie 4 (jeśli standard)
            accounts.quote_mint =
                get_account_at_index(all_accounts, account_indices, 4).or_else(|| {
                    Some(
                        "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263"
                            .parse()
                            .unwrap(),
                    )
                });
        }
        AmmType::PumpSwap => {
            // PumpSwap uses SOL as quote (same as PumpFun)
            accounts.quote_mint = Some(
                "So11111111111111111111111111111111111111112"
                    .parse()
                    .unwrap(),
            );
        }
    }

    if verbose {
        debug!(
            "Extracted accounts [{:?}]: Mint={:?}, Pool={:?}, Creator={:?}",
            amm_type, accounts.mint, accounts.pool, accounts.creator
        );
    }

    Ok(accounts)
}

// =============================================================================
// Trade (Buy/Sell) Instruction Parsing
// =============================================================================

/// Extracted trade accounts from transaction
#[derive(Debug, Clone)]
pub struct TradeAccounts {
    /// The mint being traded
    pub mint: Option<Pubkey>,
    /// The signer/user executing the trade
    pub signer: Option<Pubkey>,
    /// The bonding curve account (pool)
    pub bonding_curve: Option<Pubkey>,
}

/// Check if instruction data is a Buy or Sell instruction
pub fn is_trade_instruction(data: &[u8], amm_type: AmmType) -> Option<bool> {
    if data.len() < 8 {
        return None;
    }

    let discriminator = &data[0..8];

    match amm_type {
        AmmType::PumpFun => {
            if discriminator == PUMPFUN_BUY_DISCRIMINATOR {
                Some(true) // is_buy = true
            } else if discriminator == PUMPFUN_SELL_DISCRIMINATOR {
                Some(false) // is_buy = false
            } else {
                None
            }
        }
        AmmType::BonkFun => {
            // Bonk.fun may use similar or different discriminators
            // For now, we'll check the same discriminators
            if discriminator == PUMPFUN_BUY_DISCRIMINATOR {
                Some(true)
            } else if discriminator == PUMPFUN_SELL_DISCRIMINATOR {
                Some(false)
            } else {
                None
            }
        }
        AmmType::PumpSwap => {
            if discriminator == PUMPFUN_BUY_DISCRIMINATOR
                || discriminator == PUMPSWAP_OUTER_WRAPPER_DISCRIMINATOR
                || discriminator == PUMPSWAP_EVENT_BUY_DISCRIMINATOR
                || discriminator == PUMPSWAP_BUY_EXACT_QUOTE_IN_DISCRIMINATOR
            {
                Some(true)
            } else if discriminator == PUMPFUN_SELL_DISCRIMINATOR
                || discriminator == PUMPSWAP_EVENT_SELL_DISCRIMINATOR
            {
                Some(false)
            } else {
                None
            }
        }
    }
}

/// Parse Buy/Sell instruction data
///
/// # Arguments
/// * `data` - Raw instruction data
/// * `is_buy` - True if Buy, false if Sell (from discriminator check)
/// * `verbose` - Enable verbose logging
///
/// # Returns
/// Parsed TradeData if successful
pub fn parse_trade_instruction(data: &[u8], is_buy: bool, verbose: bool) -> Result<TradeData> {
    if data.len() < 8 {
        return Err(anyhow!("Data too short for trade instruction"));
    }

    if verbose {
        debug!(
            "Parsing {} instruction, data length: {} bytes",
            if is_buy { "Buy" } else { "Sell" },
            data.len()
        );
    }

    // Skip discriminator (first 8 bytes)
    let params_data = &data[8..];

    // Parse amount (u64 at offset 0)
    let amount = if params_data.len() >= 8 {
        read_u64_le(params_data, 0)?
    } else {
        return Err(anyhow!("Insufficient data for amount field"));
    };

    // Parse max_sol_cost (for Buy) or min_sol_output (for Sell) at offset 8
    let (max_sol_cost, min_sol_output) = if params_data.len() >= 16 {
        let second_param = read_u64_le(params_data, 8)?;
        if is_buy {
            (second_param, 0)
        } else {
            (0, second_param)
        }
    } else {
        (0, 0)
    };

    if verbose {
        if is_buy {
            debug!("Buy: amount={}, max_sol_cost={}", amount, max_sol_cost);
        } else {
            debug!("Sell: amount={}, min_sol_output={}", amount, min_sol_output);
        }
    }

    Ok(TradeData {
        is_buy,
        amount,
        max_sol_cost,
        min_sol_output,
    })
}

/// Extract trade accounts from instruction
///
/// For Pump.fun Buy/Sell instructions, typical account layout:
/// - [0] = Global state
/// - [1] = Fee recipient
/// - [2] = Mint (the token being traded)
/// - [3] = Bonding curve (the pool)
/// - [4] = Associated bonding curve (token account)
/// - [5] = Associated user (user's token account)
/// - [6] = User/Signer (the wallet executing the trade)
/// - [7+] = System accounts (Token Program, etc.)
///
/// This function extracts mint (index 2), bonding_curve (index 3), and signer (index 6).
pub fn extract_trade_accounts(
    all_accounts: &[Pubkey],
    account_indices: &[u8],
    amm_type: AmmType,
    verbose: bool,
) -> Result<TradeAccounts> {
    if verbose {
        debug!(
            "Extracting trade accounts for {:?}, {} indices provided",
            amm_type,
            account_indices.len()
        );
    }

    let mut accounts = TradeAccounts {
        mint: None,
        signer: None,
        bonding_curve: None,
    };

    // Pump.fun trade account layout (FIXED - DO NOT CHANGE)
    // Index 2 = Mint
    // Index 3 = Bonding Curve (pool)
    // Index 6 = User/Signer

    match amm_type {
        AmmType::PumpFun | AmmType::BonkFun | AmmType::PumpSwap => {
            // Mint at index 2
            accounts.mint = get_account_at_index(all_accounts, account_indices, 2);

            // Bonding curve at index 3
            accounts.bonding_curve = get_account_at_index(all_accounts, account_indices, 3);

            // Signer at index 6
            accounts.signer = get_account_at_index(all_accounts, account_indices, 6);
        }
    }

    if verbose {
        debug!(
            "Extracted trade accounts: mint={:?}, signer={:?}, bonding_curve={:?}",
            accounts.mint, accounts.signer, accounts.bonding_curve
        );
    }

    Ok(accounts)
}

/// Log unknown instruction data for debugging
pub fn log_unknown_instruction(program_id: &Pubkey, data: &[u8], accounts: &[Pubkey]) {
    warn!(
        "Unknown instruction for program {}: {} bytes, {} accounts",
        program_id,
        data.len(),
        accounts.len()
    );

    if data.len() >= 8 {
        warn!("Discriminator: {:02x?}", &data[0..8]);
        warn!(
            "Full data (first 64 bytes): {:02x?}",
            &data[..data.len().min(64)]
        );
    } else {
        warn!("Data too short: {:02x?}", data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discriminator_constants() {
        // Verify Pump.fun create discriminator is the correct hardcoded value
        assert_eq!(
            PUMPFUN_CREATE_DISCRIMINATOR,
            [0xd6, 0x90, 0x4c, 0xec, 0x5f, 0x8b, 0x31, 0xb4]
        );

        // Verify buy discriminator
        assert_eq!(
            PUMPFUN_BUY_DISCRIMINATOR,
            [0x66, 0x06, 0x3d, 0x12, 0x01, 0xda, 0xeb, 0xea]
        );

        // Verify Discriminators::pumpfun_initialize_pool returns the correct value
        assert_eq!(
            Discriminators::pumpfun_initialize_pool(),
            PUMPFUN_CREATE_DISCRIMINATOR
        );
    }

    #[test]
    fn test_discriminator_computation() {
        let disc = compute_initialize_pool_discriminator();
        assert_eq!(disc.len(), 8);
        // Verify it's deterministic
        let disc2 = compute_initialize_pool_discriminator();
        assert_eq!(disc, disc2);
    }

    #[test]
    fn test_amm_type_identification() {
        let pump_id = AmmType::PumpFun.program_id();
        assert_eq!(AmmType::from_program_id(&pump_id), Some(AmmType::PumpFun));

        let bonk_id = AmmType::BonkFun.program_id();
        assert_eq!(AmmType::from_program_id(&bonk_id), Some(AmmType::BonkFun));

        let random_id = Pubkey::new_unique();
        assert_eq!(AmmType::from_program_id(&random_id), None);
    }

    #[test]
    fn test_is_initialize_pool() {
        // Test Pump.fun with ACTUAL "create" discriminator (hardcoded, not computed)
        let pumpfun_disc = Discriminators::pumpfun_initialize_pool();
        let mut pumpfun_data = pumpfun_disc.to_vec();
        pumpfun_data.extend_from_slice(&[0u8; 40]); // Add some parameter data

        assert!(is_initialize_pool(&pumpfun_data, AmmType::PumpFun, false));
        // Pump.fun discriminator should NOT match Bonk.fun (they use different discriminators)
        assert!(!is_initialize_pool(&pumpfun_data, AmmType::BonkFun, false));

        // Test Bonk.fun with "initialize_pool" discriminator
        let bonkfun_disc = Discriminators::bonkfun_initialize_pool();
        let mut bonkfun_data = bonkfun_disc.to_vec();
        bonkfun_data.extend_from_slice(&[0u8; 40]);

        assert!(is_initialize_pool(&bonkfun_data, AmmType::BonkFun, false));
        // Bonk.fun discriminator should NOT match Pump.fun
        assert!(!is_initialize_pool(&bonkfun_data, AmmType::PumpFun, false));

        // Test with wrong discriminator
        let wrong_data = vec![0xFF; 48];
        assert!(!is_initialize_pool(&wrong_data, AmmType::PumpFun, false));

        // Test with too short data
        let short_data = vec![0u8; 4];
        assert!(!is_initialize_pool(&short_data, AmmType::PumpFun, false));
    }

    #[test]
    fn test_parse_pumpfun_instruction() {
        let disc = Discriminators::pumpfun_initialize_pool();
        let mut data = disc.to_vec();

        // Add parameter data
        data.extend_from_slice(&1000u64.to_le_bytes()); // virtual_token_reserves
        data.extend_from_slice(&2000u64.to_le_bytes()); // virtual_sol_reserves
        data.extend_from_slice(&500u64.to_le_bytes()); // real_token_reserves
        data.extend_from_slice(&1000u64.to_le_bytes()); // real_sol_reserves
        data.extend_from_slice(&1_000_000u64.to_le_bytes()); // token_supply

        let result = parse_initialize_pool(&data, AmmType::PumpFun, false);
        assert!(result.is_ok());

        let parsed = result.unwrap();
        assert_eq!(parsed.virtual_token_reserves, Some(1000));
        assert_eq!(parsed.virtual_sol_reserves, Some(2000));
        assert_eq!(parsed.real_token_reserves, Some(500));
        assert_eq!(parsed.real_sol_reserves, Some(1000));
        assert_eq!(parsed.token_supply, Some(1_000_000));
    }

    #[test]
    fn test_parse_bonkfun_instruction() {
        let disc = Discriminators::bonkfun_initialize_pool();
        let mut data = disc.to_vec();

        // Add parameter data
        data.extend_from_slice(&5000u64.to_le_bytes()); // virtual_token_reserves
        data.extend_from_slice(&10000u64.to_le_bytes()); // virtual_sol_reserves

        let result = parse_initialize_pool(&data, AmmType::BonkFun, false);
        assert!(result.is_ok());

        let parsed = result.unwrap();
        assert_eq!(parsed.virtual_token_reserves, Some(5000));
        assert_eq!(parsed.virtual_sol_reserves, Some(10000));
    }

    #[test]
    fn test_parse_insufficient_data() {
        let disc = Discriminators::pumpfun_initialize_pool();
        let data = disc.to_vec(); // Only discriminator, no parameters

        let result = parse_initialize_pool(&data, AmmType::PumpFun, false);
        assert!(result.is_ok()); // Should succeed but with None values

        let parsed = result.unwrap();
        assert_eq!(parsed.virtual_token_reserves, None);
        assert_eq!(parsed.virtual_sol_reserves, None);
    }

    #[test]
    fn test_extract_accounts() {
        let accounts = vec![
            Pubkey::new_unique(), // creator
            Pubkey::new_unique(), // pool
            Pubkey::new_unique(), // mint
            Pubkey::new_unique(), // bonding_curve
            Pubkey::new_unique(), // additional
        ];

        let indices = vec![0, 1, 2, 3, 4];

        let result = extract_accounts(&accounts, &indices, AmmType::PumpFun, false);
        assert!(result.is_ok());

        let extracted = result.unwrap();
        assert_eq!(extracted.creator, Some(accounts[1]));
        assert_eq!(extracted.pool, Some(accounts[2]));
        assert_eq!(extracted.mint, Some(accounts[0]));
        assert_eq!(extracted.bonding_curve, Some(accounts[2]));
        assert!(extracted.quote_mint.is_some()); // SOL for PumpFun
    }

    #[test]
    fn test_read_u64_le() {
        let data = 12345u64.to_le_bytes();
        let result = read_u64_le(&data, 0);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 12345);

        // Test insufficient data
        let short_data = vec![0u8; 4];
        let result = read_u64_le(&short_data, 0);
        assert!(result.is_err());
    }

    // ==========================================
    // Lenient account extraction tests
    // ==========================================

    #[test]
    fn test_extract_accounts_with_24_accounts() {
        // Test that extraction works with 24 accounts (more than typical 23)
        // This ensures forward compatibility when protocol versions add accounts
        let mut accounts: Vec<Pubkey> = Vec::new();
        for _ in 0..24 {
            accounts.push(Pubkey::new_unique());
        }
        assert_eq!(accounts.len(), 24);

        // Only first 5 indices used in extraction
        let indices = vec![0, 1, 2, 3, 4];

        let result = extract_accounts(&accounts, &indices, AmmType::PumpFun, false);
        assert!(result.is_ok(), "Should succeed with 24 accounts");

        let extracted = result.unwrap();
        assert_eq!(extracted.creator, Some(accounts[1]));
        assert_eq!(extracted.pool, Some(accounts[2]));
        assert_eq!(extracted.mint, Some(accounts[0]));
        assert_eq!(extracted.bonding_curve, Some(accounts[2]));
    }

    #[test]
    fn test_extract_accounts_with_30_accounts() {
        // Test extreme case with 30 accounts
        let mut accounts: Vec<Pubkey> = Vec::new();
        for _ in 0..30 {
            accounts.push(Pubkey::new_unique());
        }
        assert_eq!(accounts.len(), 30);

        let indices = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]; // 10 indices, but only first 4-5 used

        let result = extract_accounts(&accounts, &indices, AmmType::PumpFun, false);
        assert!(result.is_ok(), "Should succeed with 30 accounts");

        let extracted = result.unwrap();
        assert!(extracted.creator.is_some());
        assert!(extracted.pool.is_some());
        assert!(extracted.mint.is_some());
        assert!(extracted.bonding_curve.is_some());
    }

    #[test]
    fn test_extract_accounts_extra_indices_ignored() {
        // Test that extra indices beyond what we need are safely ignored
        let accounts = vec![
            Pubkey::new_unique(), // creator
            Pubkey::new_unique(), // pool
            Pubkey::new_unique(), // mint
            Pubkey::new_unique(), // bonding_curve
            Pubkey::new_unique(), // additional
        ];

        // 20 indices provided, but we only use first 4-5
        let indices: Vec<u8> = (0..20).collect();

        let result = extract_accounts(&accounts, &indices, AmmType::PumpFun, false);
        assert!(result.is_ok(), "Extra indices should be safely ignored");

        let extracted = result.unwrap();
        assert_eq!(extracted.creator, Some(accounts[1]));
        assert_eq!(extracted.pool, Some(accounts[2]));
        assert_eq!(extracted.mint, Some(accounts[0]));
        assert_eq!(extracted.bonding_curve, Some(accounts[2]));
    }

    #[test]
    fn test_extract_accounts_graceful_on_missing_indices() {
        // Test that missing indices result in None values, not errors
        let accounts = vec![Pubkey::new_unique(), Pubkey::new_unique()];

        // Only 2 indices, so bonding_curve (index 3) will be missing
        let indices = vec![0, 1];

        let result = extract_accounts(&accounts, &indices, AmmType::PumpFun, false);
        assert!(result.is_ok(), "Should succeed with missing indices");

        let extracted = result.unwrap();
        assert_eq!(extracted.creator, Some(accounts[1]));
        assert_eq!(extracted.mint, Some(accounts[0]));
        assert!(
            extracted.pool.is_none(),
            "Pool should be None when index 2 missing"
        );
        assert!(
            extracted.bonding_curve.is_none(),
            "Bonding curve should be None when index 3 missing"
        );
    }

    #[test]
    fn test_extract_accounts_rejects_token_2022_creator_fallback() {
        let mint = Pubkey::new_unique();
        let token_2022 = TOKEN_2022_PROGRAM_ID.parse().unwrap();
        let curve = Pubkey::new_unique();
        let accounts = vec![mint, token_2022, curve];
        let indices = vec![0, 1, 2];

        let extracted = extract_accounts(&accounts, &indices, AmmType::PumpFun, false).unwrap();

        assert_eq!(extracted.mint, Some(mint));
        assert_eq!(extracted.bonding_curve, Some(curve));
        assert!(extracted.creator.is_none());
    }

    // =============================================================================
    // Trade (Buy/Sell) instruction parsing tests
    // =============================================================================

    #[test]
    fn test_is_trade_instruction_buy() {
        let mut buy_data = PUMPFUN_BUY_DISCRIMINATOR.to_vec();
        buy_data.extend_from_slice(&[0u8; 16]); // Add some param data

        let result = is_trade_instruction(&buy_data, AmmType::PumpFun);
        assert_eq!(result, Some(true), "Should detect Buy instruction");
    }

    #[test]
    fn test_is_trade_instruction_sell() {
        let mut sell_data = PUMPFUN_SELL_DISCRIMINATOR.to_vec();
        sell_data.extend_from_slice(&[0u8; 16]); // Add some param data

        let result = is_trade_instruction(&sell_data, AmmType::PumpFun);
        assert_eq!(result, Some(false), "Should detect Sell instruction");
    }

    #[test]
    fn test_is_trade_instruction_not_trade() {
        let create_data = PUMPFUN_CREATE_DISCRIMINATOR.to_vec();

        let result = is_trade_instruction(&create_data, AmmType::PumpFun);
        assert_eq!(result, None, "Should not detect Create as trade");
    }

    #[test]
    fn test_is_trade_instruction_too_short() {
        let short_data = vec![0u8; 4];

        let result = is_trade_instruction(&short_data, AmmType::PumpFun);
        assert_eq!(result, None, "Should return None for too short data");
    }

    #[test]
    fn test_is_trade_instruction_pumpswap_outer_wrapper() {
        let mut data = PUMPSWAP_OUTER_WRAPPER_DISCRIMINATOR.to_vec();
        data.extend_from_slice(&[0u8; 32]);

        let result = is_trade_instruction(&data, AmmType::PumpSwap);
        assert_eq!(result, Some(true));
    }

    #[test]
    fn test_is_trade_instruction_pumpswap_buy_exact_quote_in() {
        let mut data = PUMPSWAP_BUY_EXACT_QUOTE_IN_DISCRIMINATOR.to_vec();
        data.extend_from_slice(&[0u8; 16]);

        let result = is_trade_instruction(&data, AmmType::PumpSwap);
        assert_eq!(result, Some(true));
    }

    #[test]
    fn test_is_trade_instruction_pumpswap_sell_event() {
        let mut data = PUMPSWAP_EVENT_SELL_DISCRIMINATOR.to_vec();
        data.extend_from_slice(&[0u8; 16]);

        let result = is_trade_instruction(&data, AmmType::PumpSwap);
        assert_eq!(result, Some(false));
    }

    #[test]
    fn test_parse_buy_instruction() {
        let mut data = PUMPFUN_BUY_DISCRIMINATOR.to_vec();
        data.extend_from_slice(&1000u64.to_le_bytes()); // amount
        data.extend_from_slice(&5000u64.to_le_bytes()); // max_sol_cost

        let result = parse_trade_instruction(&data, true, false);
        assert!(result.is_ok());

        let trade = result.unwrap();
        assert!(trade.is_buy);
        assert_eq!(trade.amount, 1000);
        assert_eq!(trade.max_sol_cost, 5000);
        assert_eq!(trade.min_sol_output, 0);
    }

    #[test]
    fn test_parse_sell_instruction() {
        let mut data = PUMPFUN_SELL_DISCRIMINATOR.to_vec();
        data.extend_from_slice(&2000u64.to_le_bytes()); // amount
        data.extend_from_slice(&3000u64.to_le_bytes()); // min_sol_output

        let result = parse_trade_instruction(&data, false, false);
        assert!(result.is_ok());

        let trade = result.unwrap();
        assert!(!trade.is_buy);
        assert_eq!(trade.amount, 2000);
        assert_eq!(trade.max_sol_cost, 0);
        assert_eq!(trade.min_sol_output, 3000);
    }

    #[test]
    fn test_parse_trade_instruction_insufficient_data() {
        let data = PUMPFUN_BUY_DISCRIMINATOR.to_vec(); // Only discriminator, no params

        let result = parse_trade_instruction(&data, true, false);
        assert!(result.is_err(), "Should fail with insufficient data");
    }

    #[test]
    fn test_extract_trade_accounts() {
        let accounts = vec![
            Pubkey::new_unique(), // 0: global
            Pubkey::new_unique(), // 1: fee recipient
            Pubkey::new_unique(), // 2: mint
            Pubkey::new_unique(), // 3: bonding curve
            Pubkey::new_unique(), // 4: associated bonding curve
            Pubkey::new_unique(), // 5: associated user
            Pubkey::new_unique(), // 6: signer
            Pubkey::new_unique(), // 7: token program
        ];

        let indices = vec![0, 1, 2, 3, 4, 5, 6, 7];

        let result = extract_trade_accounts(&accounts, &indices, AmmType::PumpFun, false);
        assert!(result.is_ok());

        let trade_accounts = result.unwrap();
        assert_eq!(trade_accounts.mint, Some(accounts[2]));
        assert_eq!(trade_accounts.bonding_curve, Some(accounts[3]));
        assert_eq!(trade_accounts.signer, Some(accounts[6]));
    }

    #[test]
    fn test_extract_trade_accounts_missing_indices() {
        let accounts = vec![Pubkey::new_unique(), Pubkey::new_unique()];

        let indices = vec![0, 1];

        let result = extract_trade_accounts(&accounts, &indices, AmmType::PumpFun, false);
        assert!(result.is_ok());

        let trade_accounts = result.unwrap();
        assert_eq!(
            trade_accounts.mint, None,
            "Mint should be None when index 2 is missing"
        );
        assert_eq!(
            trade_accounts.bonding_curve, None,
            "Bonding curve should be None when index 3 is missing"
        );
        assert_eq!(
            trade_accounts.signer, None,
            "Signer should be None when index 6 is missing"
        );
    }
}
