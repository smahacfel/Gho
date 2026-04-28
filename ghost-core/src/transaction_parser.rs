//! Transaction parsing for contextual analysis
//!
//! This module provides utilities to parse raw transaction data and extract
//! contextual information for enhanced candidate scoring, including:
//! - Dev buy detection (atomic swaps in same tx/bundle)
//! - Mint authority analysis (SetAuthority instructions)
//! - Metadata parsing (CreateMetadata instructions)

use anyhow::{anyhow, Result};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::transaction::TransactionError;
use std::collections::HashSet;

/// Known program IDs for parsing
pub struct ProgramIds;

impl ProgramIds {
    /// Token Program ID
    pub const TOKEN_PROGRAM: &'static str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

    /// Token-2022 Program ID
    pub const TOKEN_2022_PROGRAM: &'static str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

    /// Metaplex Token Metadata Program
    pub const METADATA_PROGRAM: &'static str = "metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s";

    /// Pump.fun Program
    pub const PUMP_FUN: &'static str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

    /// Bonk.fun Program
    pub const BONK_FUN: &'static str = "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj";
}

/// Normalized transaction metadata for scoring engines
#[derive(Debug, Clone)]
pub struct TransactionMetadata {
    pub signature: Signature,
    pub signer: Pubkey,
    pub slot: u64,
    /// True when transaction succeeded (meta.err is None)
    pub success: bool,
    /// Parsed error code (e.g., InstructionError, SlippageExceeded)
    pub error_code: Option<String>,
    /// Compute units consumed (if available)
    pub compute_units: u64,
    /// Priority fee in micro-lamports (if available)
    pub priority_fee_micro_lamports: u64,
    /// True if transaction was sent via Jito bundle
    pub is_jito_bundle: bool,
    /// Requested SOL amount (intent), even if failed
    pub requested_sol_amount: f64,
    /// Executed SOL amount (actual), zero on failure
    pub executed_sol_amount: f64,
}

/// Extract an error code string from a Solana transaction error.
pub fn error_code_from_transaction_error(err: &TransactionError) -> String {
    match err {
        TransactionError::InstructionError(_, inner) => {
            format!("InstructionError::{:?}", inner)
        }
        _ => format!("{:?}", err),
    }
}

/// Extract error code from a transaction status Result.
pub fn error_code_from_status(status: &Result<(), TransactionError>) -> Option<String> {
    status.as_ref().err().map(error_code_from_transaction_error)
}

/// Parsed metadata from CreateMetadata instruction
#[derive(Debug, Clone)]
pub struct ParsedMetadata {
    /// Token name
    pub name: String,
    /// Token symbol
    pub symbol: String,
    /// URI (often IPFS link)
    pub uri: String,
}

/// Parse CreateMetadata instruction from Metaplex Token Metadata program
///
/// Expected instruction format (simplified):
/// - Discriminator: varies by version
/// - Data struct with name, symbol, uri fields
///
/// # Arguments
/// * `data` - Raw instruction data
///
/// # Returns
/// Parsed metadata if successful
pub fn parse_create_metadata(data: &[u8]) -> Result<ParsedMetadata> {
    // Metaplex CreateMetadata has various versions, but generally:
    // After discriminator (8 bytes for Anchor), there's:
    // - name: String (4 bytes length prefix + UTF-8 data)
    // - symbol: String (4 bytes length prefix + UTF-8 data)
    // - uri: String (4 bytes length prefix + UTF-8 data)

    if data.len() < 20 {
        return Err(anyhow!("Data too short for CreateMetadata"));
    }

    // Skip discriminator (first 8 bytes for Anchor-style instructions)
    let mut offset = 8;

    // Parse name (length-prefixed string)
    let name = match parse_borsh_string(data, &mut offset) {
        Ok(s) => s,
        Err(_) => {
            // Fallback: try without discriminator skip
            offset = 0;
            parse_borsh_string(data, &mut offset)?
        }
    };

    // Parse symbol
    let symbol = parse_borsh_string(data, &mut offset)?;

    // Parse URI
    let uri = parse_borsh_string(data, &mut offset)?;

    Ok(ParsedMetadata { name, symbol, uri })
}

/// Parse a Borsh-encoded string (4-byte length prefix + UTF-8 data)
fn parse_borsh_string(data: &[u8], offset: &mut usize) -> Result<String> {
    if data.len() < *offset + 4 {
        return Err(anyhow!("Insufficient data for string length"));
    }

    // Read 4-byte little-endian length
    let len_bytes: [u8; 4] = data[*offset..*offset + 4]
        .try_into()
        .map_err(|_| anyhow!("Failed to read string length"))?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    *offset += 4;

    if len > 1024 {
        // Sanity check: strings shouldn't be this long
        return Err(anyhow!("String length too large: {}", len));
    }

    if data.len() < *offset + len {
        return Err(anyhow!("Insufficient data for string content"));
    }

    // Read UTF-8 string data
    let string_bytes = &data[*offset..*offset + len];
    *offset += len;

    String::from_utf8(string_bytes.to_vec()).map_err(|e| anyhow!("Invalid UTF-8 in string: {}", e))
}

/// Information about a swap instruction (Buy/Sell)
#[derive(Debug, Clone)]
pub struct SwapInfo {
    /// User performing the swap
    pub user: Pubkey,
    /// Amount of SOL (in lamports) being swapped
    pub amount_in_lamports: u64,
    /// Whether this is a buy (SOL → Token) or sell (Token → SOL)
    pub is_buy: bool,
}

/// Parse a swap instruction from Pump.fun or Bonk.fun
///
/// This is a simplified parser that extracts the user and amount.
/// In production, you'd need the exact instruction layout for each AMM.
///
/// # Arguments
/// * `data` - Raw instruction data
/// * `accounts` - Transaction account keys
/// * `account_indices` - Indices used by this instruction
/// * `amm_type` - Type of AMM (PumpFun or BonkFun)
///
/// # Returns
/// Swap information if this is a Buy instruction
pub fn parse_swap_instruction(
    data: &[u8],
    accounts: &[Pubkey],
    account_indices: &[u8],
    amm_type: crate::init_pool_parser::AmmType,
) -> Result<Option<SwapInfo>> {
    if data.len() < 16 {
        return Ok(None);
    }

    let discriminator = &data[0..8];
    let params_data = &data[8..];

    // Get user from expected index (varies by AMM)
    // Pump.fun: index 6
    // PumpSwap: index 1
    let user_idx = match amm_type {
        crate::init_pool_parser::AmmType::PumpFun => 6,
        crate::init_pool_parser::AmmType::PumpSwap => 1,
        crate::init_pool_parser::AmmType::BonkFun => 0, // Fallback
    };

    let user = if let Some(pubkey) = get_account_at_index(accounts, account_indices, user_idx) {
        pubkey
    } else {
        // Heuristic fallback: try first account
        if let Some(&idx) = account_indices.first() {
            accounts[idx as usize]
        } else {
            return Ok(None);
        }
    };

    // Correctly identify trade side and lamport amount based on program
    match amm_type {
        crate::init_pool_parser::AmmType::PumpFun => {
            if discriminator == crate::init_pool_parser::PUMPFUN_BUY_DISCRIMINATOR {
                // buy(amount: u64, max_sol_cost: u64)
                // SOL cost is the second u64 parameter (offset 8)
                let amount_bytes: [u8; 8] = params_data[8..16]
                    .try_into()
                    .map_err(|_| anyhow!("Failed to parse max_sol_cost"))?;
                let lamports = u64::from_le_bytes(amount_bytes);
                Ok(Some(SwapInfo {
                    user,
                    amount_in_lamports: lamports,
                    is_buy: true,
                }))
            } else if discriminator == crate::init_pool_parser::PUMPFUN_SELL_DISCRIMINATOR {
                // sell(amount: u64, min_sol_output: u64)
                // SOL output is the second u64 parameter (offset 8)
                let amount_bytes: [u8; 8] = params_data[8..16]
                    .try_into()
                    .map_err(|_| anyhow!("Failed to parse min_sol_output"))?;
                let lamports = u64::from_le_bytes(amount_bytes);
                Ok(Some(SwapInfo {
                    user,
                    amount_in_lamports: lamports,
                    is_buy: false,
                }))
            } else {
                Ok(None)
            }
        }
        _ => {
            // Heuristic fallback for other AMMs
            let amount_bytes: [u8; 8] = params_data[0..8].try_into().unwrap_or([0; 8]);
            let amount = u64::from_le_bytes(amount_bytes);
            if amount > 0 {
                Ok(Some(SwapInfo {
                    user,
                    amount_in_lamports: amount,
                    is_buy: true,
                }))
            } else {
                Ok(None)
            }
        }
    }
}

/// Helper to get account at specific index (copied from init_pool_parser for local use if needed)
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

/// Check if instruction is a SetAuthority instruction
///
/// # Arguments
/// * `program_id` - The program executing the instruction
/// * `data` - Raw instruction data
///
/// # Returns
/// true if this is a SetAuthority instruction
pub fn is_set_authority(program_id: &Pubkey, data: &[u8]) -> bool {
    // Check if this is Token Program or Token-2022
    let token_program: Pubkey = ProgramIds::TOKEN_PROGRAM.parse().unwrap();
    let token_2022: Pubkey = ProgramIds::TOKEN_2022_PROGRAM.parse().unwrap();

    if program_id != &token_program && program_id != &token_2022 {
        return false;
    }

    // SetAuthority instruction discriminator for SPL Token is 6
    // Format: [6, authority_type, new_authority (optional 32 bytes)]
    data.first() == Some(&6)
}

/// Parse SetAuthority instruction to check if mint authority is being disabled
///
/// # Arguments
/// * `data` - Raw instruction data
///
/// # Returns
/// - `Ok(true)` if authority is being set to None (disabled)
/// - `Ok(false)` if authority is being transferred to another account
/// - `Err` if parsing fails
pub fn parse_set_authority(data: &[u8]) -> Result<bool> {
    if data.is_empty() || data[0] != 6 {
        return Err(anyhow!("Not a SetAuthority instruction"));
    }

    if data.len() < 2 {
        return Err(anyhow!("Data too short for SetAuthority"));
    }

    // authority_type is at offset 1
    // 0 = MintTokens, 1 = FreezeAccount, 2 = AccountOwner, 3 = CloseAccount
    let _authority_type = data[1];

    // Check if new authority is present (option byte at offset 2)
    if data.len() < 3 {
        return Err(anyhow!("Data too short for authority option"));
    }

    let has_new_authority = data[2];

    if has_new_authority == 0 {
        // None - authority is being disabled
        Ok(true)
    } else {
        // Some(Pubkey) - authority is being transferred
        // In the context of scam detection, transferring to bonding curve PDA is OK,
        // but we'll let the caller determine if the target is acceptable
        Ok(false)
    }
}

/// Extract signers from a transaction message
///
/// # Arguments
/// * `accounts` - All account keys in the transaction
/// * `num_required_signatures` - Number of required signatures from message header
///
/// # Returns
/// HashSet of signer pubkeys
pub fn extract_signers(accounts: &[Pubkey], num_required_signatures: usize) -> HashSet<Pubkey> {
    accounts
        .iter()
        .take(num_required_signatures)
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_borsh_string() {
        // Create test data: length (4) + "test"
        let mut data = vec![4, 0, 0, 0]; // length = 4 (little-endian)
        data.extend_from_slice(b"test");

        let mut offset = 0;
        let result = parse_borsh_string(&data, &mut offset);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test");
        assert_eq!(offset, 8); // 4 bytes length + 4 bytes content
    }

    #[test]
    fn test_parse_borsh_string_empty() {
        let data = vec![0, 0, 0, 0]; // length = 0

        let mut offset = 0;
        let result = parse_borsh_string(&data, &mut offset);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "");
        assert_eq!(offset, 4);
    }

    #[test]
    fn test_parse_borsh_string_insufficient_data() {
        let data = vec![10, 0, 0, 0, 1, 2]; // Claims length 10, but only has 2 bytes

        let mut offset = 0;
        let result = parse_borsh_string(&data, &mut offset);

        assert!(result.is_err());
    }

    #[test]
    fn test_parse_create_metadata() {
        // Create minimal test data
        let mut data = vec![0u8; 8]; // Discriminator

        // Add name "TestToken" (9 bytes)
        data.extend_from_slice(&9u32.to_le_bytes());
        data.extend_from_slice(b"TestToken");

        // Add symbol "TEST" (4 bytes)
        data.extend_from_slice(&4u32.to_le_bytes());
        data.extend_from_slice(b"TEST");

        // Add URI "https://test.com" (16 bytes)
        data.extend_from_slice(&16u32.to_le_bytes());
        data.extend_from_slice(b"https://test.com");

        let result = parse_create_metadata(&data);

        if let Err(ref e) = result {
            eprintln!("Parse error: {}", e);
        }

        assert!(result.is_ok());
        let metadata = result.unwrap();
        assert_eq!(metadata.name, "TestToken");
        assert_eq!(metadata.symbol, "TEST");
        assert_eq!(metadata.uri, "https://test.com");
    }

    #[test]
    fn test_is_set_authority() {
        let token_program: Pubkey = ProgramIds::TOKEN_PROGRAM.parse().unwrap();
        let other_program = Pubkey::new_unique();

        let data = vec![6, 0, 0]; // SetAuthority discriminator

        assert!(is_set_authority(&token_program, &data));
        assert!(!is_set_authority(&other_program, &data));

        let wrong_data = vec![5, 0, 0]; // Wrong discriminator
        assert!(!is_set_authority(&token_program, &wrong_data));
    }

    #[test]
    fn test_parse_set_authority_disabled() {
        let data = vec![6, 0, 0]; // SetAuthority, MintTokens, None

        let result = parse_set_authority(&data);
        assert!(result.is_ok());
        assert!(result.unwrap()); // Authority disabled
    }

    #[test]
    fn test_parse_set_authority_transferred() {
        let mut data = vec![6, 0, 1]; // SetAuthority, MintTokens, Some
        data.extend_from_slice(&[0u8; 32]); // New authority pubkey

        let result = parse_set_authority(&data);
        assert!(result.is_ok());
        assert!(!result.unwrap()); // Authority transferred, not disabled
    }

    #[test]
    fn test_extract_signers() {
        let accounts = vec![
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        ];

        let signers = extract_signers(&accounts, 2);

        assert_eq!(signers.len(), 2);
        assert!(signers.contains(&accounts[0]));
        assert!(signers.contains(&accounts[1]));
        assert!(!signers.contains(&accounts[2]));
    }
}
