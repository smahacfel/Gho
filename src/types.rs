//! Core types and data structures for the H-5N1P3R trading system.

use serde::{Deserialize, Serialize};

/// A simple public key representation (using string for now to avoid Solana dependencies)
pub type Pubkey = String;

/// A premint candidate token discovered on-chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PremintCandidate {
    /// The mint address of the token
    pub mint: Pubkey,
    /// The creator/authority of the token
    pub creator: Pubkey,
    /// The program that created this token (e.g., "pump.fun")
    pub program: String,
    /// The slot number when this was discovered
    pub slot: u64,
    /// Unix timestamp when discovered
    pub timestamp: u64,
    /// Summary of the instruction that created this token
    pub instruction_summary: Option<String>,
    /// Whether this was found in a Jito bundle
    pub is_jito_bundle: Option<bool>,
}

/// GUI candidate information for display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantumCandidateGui {
    /// The candidate information
    pub candidate: PremintCandidate,
    /// Computed score
    pub score: u8,
    /// Human-readable reason for the score
    pub reason: String,
    /// Feature breakdown
    pub features: std::collections::HashMap<String, f64>,
}
