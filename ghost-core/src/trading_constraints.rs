//! Trading Constraints Module
//!
//! This module provides constants, types, and utilities for validating
//! trading operations on supported AMM programs (Pump.fun, Bonk.fun).

use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

/// Trading constraints for swap validation
#[derive(Debug, Clone)]
pub struct TradingConstraints {
    /// Minimum amount for a swap (to prevent dust attacks)
    pub min_swap_amount: u64,

    /// Maximum timeout duration in seconds (7 days)
    pub max_timeout_duration: i64,

    /// Whitelisted AMM program IDs
    pub whitelisted_amm_programs: Vec<Pubkey>,

    /// Whether to enforce pool whitelist (can be disabled for testing)
    pub enable_pool_whitelist: bool,
}

impl TradingConstraints {
    /// Create constraints with default values
    ///
    pub fn new() -> Self {
        Self {
            min_swap_amount: 1000,
            max_timeout_duration: 7 * 24 * 60 * 60, // 7 days in seconds
            whitelisted_amm_programs: vec![
                // Pump.fun Program ID - well-known public address
                Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P")
                    .expect("Invalid Pump.fun program ID"),
                // Bonk.fun Program ID - well-known public address
                Pubkey::from_str("LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj")
                    .expect("Invalid Bonk.fun program ID"),
                // Raydium AMM program ID (v4)
                Pubkey::from_str("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8")
                    .expect("Invalid Raydium program ID"),
            ],
            // Enabled by default to block unsupported pools early
            enable_pool_whitelist: true,
        }
    }

    /// Check if a pool address is from a whitelisted AMM program
    ///
    pub fn is_whitelisted_pool(&self, amm_program_id: &Pubkey) -> bool {
        if !self.enable_pool_whitelist {
            return true;
        }

        Self::is_authorized_amm_program(amm_program_id)
    }

    /// Get Pump.fun program ID
    pub fn pumpfun_program_id() -> Pubkey {
        Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P")
            .expect("Invalid Pump.fun program ID")
    }

    /// Get Bonk.fun program ID
    pub fn bonkfun_program_id() -> Pubkey {
        Pubkey::from_str("LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj")
            .expect("Invalid Bonk.fun program ID")
    }

    /// Get Raydium program ID
    pub fn raydium_program_id() -> Pubkey {
        Pubkey::from_str("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8")
            .expect("Invalid Raydium program ID")
    }

    /// Check if a program ID is whitelisted
    pub fn is_authorized_amm_program(program_id: &Pubkey) -> bool {
        program_id == &Self::pumpfun_program_id()
            || program_id == &Self::bonkfun_program_id()
            || program_id == &Self::raydium_program_id()
    }
}

impl Default for TradingConstraints {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trading_constraints_defaults() {
        let constraints = TradingConstraints::new();

        assert_eq!(constraints.min_swap_amount, 1000);
        assert_eq!(constraints.max_timeout_duration, 7 * 24 * 60 * 60);
        // Pool whitelist is enabled by default
        assert!(constraints.enable_pool_whitelist);
        assert_eq!(constraints.whitelisted_amm_programs.len(), 3);
    }

    #[test]
    fn test_amm_program_ids() {
        let pumpfun = TradingConstraints::pumpfun_program_id();
        let bonkfun = TradingConstraints::bonkfun_program_id();
        let raydium = TradingConstraints::raydium_program_id();

        assert_ne!(pumpfun, Pubkey::default());
        assert_ne!(bonkfun, Pubkey::default());
        assert_ne!(raydium, Pubkey::default());
        assert_ne!(pumpfun, bonkfun);
        assert_ne!(pumpfun, raydium);

        assert!(TradingConstraints::is_authorized_amm_program(&pumpfun));
        assert!(TradingConstraints::is_authorized_amm_program(&bonkfun));
        assert!(TradingConstraints::is_authorized_amm_program(&raydium));
        assert!(!TradingConstraints::is_authorized_amm_program(
            &Pubkey::new_unique()
        ));
    }

    #[test]
    fn test_pool_whitelist_check() {
        let constraints = TradingConstraints::new();

        assert!(constraints.is_whitelisted_pool(&TradingConstraints::pumpfun_program_id()));
        assert!(constraints.is_whitelisted_pool(&TradingConstraints::raydium_program_id()));
        assert!(!constraints.is_whitelisted_pool(&Pubkey::new_unique()));
    }
}
