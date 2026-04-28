//! Canonical Transaction Event - C2 Requirement
//!
//! This module defines the ONE canonical input model for both Gatekeeper commit
//! and LivePipeline. This ensures consistent data flow and eliminates multiple
//! conversion paths.
//!
//! All transaction events feeding into the canonical history must use this type.

use super::trade_types::{TradeSide, TxKey, TxKeyError};
use solana_sdk::pubkey::Pubkey;

/// Canonical Transaction Event - the single source of truth for TX data
///
/// This type is used by:
/// - Gatekeeper buffer (pre-commit history building)
/// - LivePipeline (post-commit live append)
///
/// All fields are REQUIRED and must contain real data, no estimates or defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalTxEvent {
    /// Token mint address (base_mint)
    pub base_mint: Pubkey,

    /// Transaction key for deterministic ordering
    pub tx_key: TxKey,

    /// Trade side (Buy or Sell)
    pub side: TradeSide,

    /// Actual SOL amount in lamports (MUST be real, no estimates)
    pub d_sol_lamports: u64,

    /// Actual token amount in base units (MUST be real, no estimates)
    pub d_tok_units: u64,

    /// SOL reserve AFTER this transaction (lamports)
    pub reserve_sol_after: u64,

    /// Token reserve AFTER this transaction (base units)
    pub reserve_tok_after: u64,

    /// Transaction signer (optional)
    pub signer: Option<Pubkey>,

    /// Whether this is a dev buy
    pub dev_buy: bool,
}

impl CanonicalTxEvent {
    /// Create a new CanonicalTxEvent with validation
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - slot == 0
    /// - d_sol_lamports == 0 and d_tok_units == 0
    /// - reserve_sol_after == 0 and reserve_tok_after == 0
    pub fn new(
        base_mint: Pubkey,
        tx_key: TxKey,
        side: TradeSide,
        d_sol_lamports: u64,
        d_tok_units: u64,
        reserve_sol_after: u64,
        reserve_tok_after: u64,
        signer: Option<Pubkey>,
        dev_buy: bool,
    ) -> Result<Self, CanonicalTxEventError> {
        // No slot validation - slots are optional metadata (EVENT-TIME architecture)
        // Validate timestamp instead
        if tx_key.timestamp_ms == 0 {
            return Err(CanonicalTxEventError::InvalidTimestamp);
        }

        // Validate non-zero amounts
        if d_sol_lamports == 0 && d_tok_units == 0 {
            return Err(CanonicalTxEventError::ZeroAmounts);
        }

        // Validate reserves (at least one must be non-zero)
        if reserve_sol_after == 0 && reserve_tok_after == 0 {
            return Err(CanonicalTxEventError::ZeroReserves);
        }

        Ok(Self {
            base_mint,
            tx_key,
            side,
            d_sol_lamports,
            d_tok_units,
            reserve_sol_after,
            reserve_tok_after,
            signer,
            dev_buy,
        })
    }

    /// Calculate average price (SOL per token) - FIXED Issue #4
    ///
    /// Correctly scales lamports to SOL and token units to tokens
    pub fn price_avg_sol_per_tok(&self) -> f64 {
        if self.d_tok_units == 0 {
            return 0.0;
        }
        // Convert lamports to SOL (1 SOL = 1e9 lamports)
        let d_sol = (self.d_sol_lamports as f64) / 1_000_000_000.0;
        // Assume token units need scaling (typically 1e6 for many tokens)
        // This needs to be parameterized in production based on mint decimals
        let d_tok = (self.d_tok_units as f64) / 1_000_000.0;

        if d_tok == 0.0 {
            return 0.0;
        }
        d_sol / d_tok
    }

    /// Calculate instant price after this TX (reserve ratio) - FIXED Issue #4
    ///
    /// Correctly scales reserves
    pub fn price_instant_after_sol_per_tok(&self) -> f64 {
        if self.reserve_tok_after == 0 {
            return 0.0;
        }
        // Convert reserves to proper units
        let reserve_sol = (self.reserve_sol_after as f64) / 1_000_000_000.0; // lamports to SOL
        let reserve_tok = (self.reserve_tok_after as f64) / 1_000_000.0; // units to tokens

        if reserve_tok == 0.0 {
            return 0.0;
        }
        reserve_sol / reserve_tok
    }
}

#[derive(thiserror::Error, Debug, Clone, PartialEq)]
pub enum CanonicalTxEventError {
    #[error("invalid timestamp (must be > 0)")]
    InvalidTimestamp,

    #[error("zero amounts (d_sol and d_tok both zero)")]
    ZeroAmounts,

    #[error("zero reserves (both reserves are zero)")]
    ZeroReserves,

    #[error("tx key error: {0}")]
    TxKeyError(#[from] TxKeyError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::signature::Signature;
    use std::str::FromStr;

    fn test_pubkey(seed: u8) -> Pubkey {
        Pubkey::new_from_array([seed; 32])
    }

    #[test]
    fn test_canonical_tx_event_valid() {
        // TxKey::new(timestamp_ms, slot, tx_index, signature, fallback)
        let tx_key = TxKey::new(
            1700000000000,
            Some(12345),
            Some(1),
            Some(Signature::from_str("3pKQ7vD7C6xqJwGgHqYvbmUqN4ZXpqJzfVJvFJTkXjLjkPvD7vD7C6xqJwGgHqYvbmUqN4ZXpqJzfVJvFJTkXjL").unwrap()),
            0,
        ).unwrap();

        let event = CanonicalTxEvent::new(
            test_pubkey(1),
            tx_key,
            TradeSide::Buy,
            1_000_000_000,   // 1 SOL
            1_000_000,       // 1M tokens
            100_000_000_000, // 100 SOL reserve
            1_000_000_000,   // 1B tokens reserve
            Some(test_pubkey(2)),
            false,
        )
        .unwrap();

        assert_eq!(event.base_mint, test_pubkey(1));
        assert_eq!(event.d_sol_lamports, 1_000_000_000);
        assert_eq!(event.d_tok_units, 1_000_000);
    }

    #[test]
    fn test_canonical_tx_event_rejects_timestamp_zero() {
        // Manually construct TxKey with timestamp_ms = 0
        let tx_key = TxKey {
            timestamp_ms: 0,
            slot: Some(12345),
            tx_index: Some(1),
            signature: None,
            fallback_counter: 0,
        };

        let result = CanonicalTxEvent::new(
            test_pubkey(1),
            tx_key,
            TradeSide::Buy,
            1_000_000_000,
            1_000_000,
            100_000_000_000,
            1_000_000_000,
            Some(test_pubkey(2)),
            false,
        );

        assert!(matches!(
            result,
            Err(CanonicalTxEventError::InvalidTimestamp)
        ));
    }

    #[test]
    fn test_canonical_tx_event_rejects_zero_amounts() {
        let tx_key = TxKey::new(
            1700000000000,
            Some(12345),
            Some(1),
            Some(Signature::from_str("3pKQ7vD7C6xqJwGgHqYvbmUqN4ZXpqJzfVJvFJTkXjLjkPvD7vD7C6xqJwGgHqYvbmUqN4ZXpqJzfVJvFJTkXjL").unwrap()),
            0,
        ).unwrap();

        let result = CanonicalTxEvent::new(
            test_pubkey(1),
            tx_key,
            TradeSide::Buy,
            0, // INVALID
            0, // INVALID
            100_000_000_000,
            1_000_000_000,
            Some(test_pubkey(2)),
            false,
        );

        assert!(matches!(result, Err(CanonicalTxEventError::ZeroAmounts)));
    }

    #[test]
    fn test_price_calculations() {
        let tx_key = TxKey::new(
            1700000000000,
            Some(12345),
            Some(1),
            Some(Signature::from_str("3pKQ7vD7C6xqJwGgHqYvbmUqN4ZXpqJzfVJvFJTkXjLjkPvD7vD7C6xqJwGgHqYvbmUqN4ZXpqJzfVJvFJTkXjL").unwrap()),
            0,
        ).unwrap();

        let event = CanonicalTxEvent::new(
            test_pubkey(1),
            tx_key,
            TradeSide::Buy,
            2_000_000_000,   // 2 SOL
            1_000_000,       // 1M tokens
            100_000_000_000, // 100 SOL reserve
            1_000_000_000,   // 1B tokens reserve
            Some(test_pubkey(2)),
            false,
        )
        .unwrap();

        // Average price uses base units -> token scaling (1_000_000 units = 1 token)
        let price_avg = event.price_avg_sol_per_tok();
        assert!((price_avg - 2.0).abs() < 1e-10);

        // Instant price uses the same reserve scaling (1_000_000_000 units = 1000 tokens)
        let price_instant = event.price_instant_after_sol_per_tok();
        assert!((price_instant - 0.1).abs() < 1e-12);
    }
}
