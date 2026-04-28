//! Protocol genesis authority for Shadow Ledger bootstrap.
//!
//! Shadow Ledger treats this module as the single source of truth for the
//! Pump.fun bonding-curve state at mint birth. Snapshot 0 / G0 must always be
//! derived from this protocol-defined genesis state so bootstrap stays
//! deterministic even before any AccountUpdate has arrived.

use crate::market_state::BondingCurve;

/// Canonical virtual token reserves for a newly created Pump.fun bonding curve.
pub const PROTOCOL_GENESIS_VIRTUAL_TOKEN_RESERVES: u64 = 1_073_000_000_000_000;
/// Canonical virtual SOL reserves for a newly created Pump.fun bonding curve.
pub const PROTOCOL_GENESIS_VIRTUAL_SOL_RESERVES: u64 = 30_000_000_000;
/// Canonical real token reserves for a newly created Pump.fun bonding curve.
pub const PROTOCOL_GENESIS_REAL_TOKEN_RESERVES: u64 = 793_100_000_000_000;
/// Canonical real SOL reserves for a newly created Pump.fun bonding curve.
pub const PROTOCOL_GENESIS_REAL_SOL_RESERVES: u64 = 30_000_000_000;
/// Canonical token supply for a newly created Pump.fun bonding curve.
pub const PROTOCOL_GENESIS_TOKEN_TOTAL_SUPPLY: u64 = 1_000_000_000_000_000;

/// Return the authoritative protocol genesis bonding-curve state.
///
/// This is intentionally centralized in one module so every Shadow Ledger
/// bootstrap path shares the same mint-birth state instead of relying on later
/// account updates to establish the initial curve shape.
#[inline]
pub const fn protocol_genesis_curve() -> BondingCurve {
    BondingCurve {
        discriminator: 0,
        virtual_token_reserves: PROTOCOL_GENESIS_VIRTUAL_TOKEN_RESERVES,
        virtual_sol_reserves: PROTOCOL_GENESIS_VIRTUAL_SOL_RESERVES,
        real_token_reserves: PROTOCOL_GENESIS_REAL_TOKEN_RESERVES,
        real_sol_reserves: PROTOCOL_GENESIS_REAL_SOL_RESERVES,
        token_total_supply: PROTOCOL_GENESIS_TOKEN_TOTAL_SUPPLY,
        complete: 0,
        _padding: [0; 7],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_genesis_curve_returns_canonical_reserves() {
        let curve = protocol_genesis_curve();

        assert_eq!(
            curve.virtual_token_reserves,
            PROTOCOL_GENESIS_VIRTUAL_TOKEN_RESERVES
        );
        assert_eq!(
            curve.virtual_sol_reserves,
            PROTOCOL_GENESIS_VIRTUAL_SOL_RESERVES
        );
        assert_eq!(
            curve.real_token_reserves,
            PROTOCOL_GENESIS_REAL_TOKEN_RESERVES
        );
        assert_eq!(curve.real_sol_reserves, PROTOCOL_GENESIS_REAL_SOL_RESERVES);
        assert_eq!(
            curve.token_total_supply,
            PROTOCOL_GENESIS_TOKEN_TOTAL_SUPPLY
        );
        assert_eq!(curve.complete, 0);
    }

    #[test]
    fn test_protocol_genesis_curve_is_deterministic() {
        let first = protocol_genesis_curve();
        let second = protocol_genesis_curve();

        assert_eq!(first.virtual_token_reserves, second.virtual_token_reserves);
        assert_eq!(first.virtual_sol_reserves, second.virtual_sol_reserves);
        assert_eq!(first.real_token_reserves, second.real_token_reserves);
        assert_eq!(first.real_sol_reserves, second.real_sol_reserves);
        assert_eq!(first.token_total_supply, second.token_total_supply);
        assert_eq!(first.complete, second.complete);
    }
}
