//! Enhanced candidate types for contextual analysis
//!
//! This module defines the EnhancedCandidate structure that extends basic pool
//! candidate information with contextual analysis from transaction data.

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

/// Enhanced candidate with contextual analysis from transaction data
///
/// This structure extends the basic candidate data with heuristics computed
/// at the ingest stage (Shred/Seer) without any RPC calls, enabling fast
/// scam/honeypot detection based on transaction intent and context.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnhancedCandidate {
    // === Base fields (from PremintCandidate/CandidatePool) ===
    /// Slot when detected (if known)
    pub slot: Option<u64>,

    /// Pool AMM account ID
    pub pool_amm_id: Pubkey,

    /// AMM program ID
    pub amm_program_id: Pubkey,

    /// Base token mint
    pub base_mint: Pubkey,

    /// Quote token mint
    pub quote_mint: Pubkey,

    /// Bonding curve account
    pub bonding_curve: Pubkey,

    /// Timestamp when detected (Unix timestamp)
    pub timestamp: u64,

    /// Optional: Bonding curve progress (0.0 - 1.0)
    pub bonding_curve_progress: Option<f64>,

    /// Initial liquidity in SOL
    pub initial_liquidity_sol: f64,

    /// Optional: Token total supply
    pub token_total_supply: Option<u64>,

    /// Transaction signature
    pub signature: String,

    // === NEW FIELDS (Stage 0: Shred/Ingest analysis) ===
    /// Heuristic for vanity/grind address of mint (0-100)
    ///
    /// Higher scores indicate proof-of-work in address generation:
    /// - Meaningful prefixes/suffixes (pump, moon, meta)
    /// - Long character runs (AAAA, 1111)
    /// - Other patterns indicating non-random generation
    pub vanity_score: u8,

    /// Whether dev performs atomic BUY in same transaction/bundle
    ///
    /// Strong positive signal: dev is investing in their own token
    /// atomically at launch, reducing rug risk.
    pub has_dev_buy: bool,

    /// Sum of SOL spent by dev on BUY in same tx/bundle
    ///
    /// Higher amounts indicate stronger dev commitment.
    pub dev_buy_sol: f64,

    /// Whether mint authority has been disabled/transferred
    ///
    /// For Pump.fun: should be transferred to bonding curve PDA
    /// For Raydium/Orca: active mint authority is a red flag
    pub mint_auth_disabled: bool,

    /// Metadata quality heuristic based on name/symbol (0-100)
    ///
    /// Scores based on:
    /// - Trending keywords (PEPE, DOGE, AI, GPT)
    /// - Reasonable name length
    /// - Absence of spam indicators (links, excessive length)
    pub metadata_len_score: u8,

    // === Shadow Ledger Fields (BCV Integration) ===
    /// Expected price per token from Shadow Ledger simulation (in lamports)
    pub expected_price: Option<f64>,

    /// Bonding curve progress from Shadow Ledger (0-100)
    pub shadow_bonding_progress: Option<u64>,

    /// Virtual SOL reserves from Shadow Ledger (in lamports)
    pub virtual_sol_reserves: Option<u64>,

    /// Market cap from Shadow Ledger (in lamports)
    pub shadow_market_cap: Option<u64>,
}
