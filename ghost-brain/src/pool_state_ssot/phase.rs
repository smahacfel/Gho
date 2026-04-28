//! Pool phase detection and one-way switch logic.
//!
//! Once a pool transitions from `BondingCurve` to `Amm`, it never reverts.

use serde::{Deserialize, Serialize};

/// Pool lifecycle phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PoolPhase {
    /// Pump.fun bonding curve phase — virtual reserves pricing.
    BondingCurve,
    /// Post-migration AMM phase — real reserves constant-product pricing.
    Amm,
}

impl std::fmt::Display for PoolPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PoolPhase::BondingCurve => write!(f, "BondingCurve"),
            PoolPhase::Amm => write!(f, "Amm"),
        }
    }
}

impl Default for PoolPhase {
    fn default() -> Self {
        PoolPhase::BondingCurve
    }
}

/// Determines whether a phase transition from bonding curve to AMM should occur.
///
/// Switch rules (any of these triggers the switch):
/// 1. AMM pool accounts become known (amm accounts resolvable).
/// 2. Migration event observed externally.
/// 3. Bonding progress crosses threshold AND amm accounts are resolvable.
///
/// Once `Amm`, NEVER go back.
pub fn should_switch_to_amm(
    current_phase: PoolPhase,
    amm_accounts_resolved: bool,
    migration_observed: bool,
    _bonding_progress_pct: f64,
    _bonding_progress_threshold_pct: f64,
) -> bool {
    if current_phase == PoolPhase::Amm {
        return false; // already AMM, no switch
    }

    if migration_observed {
        return true;
    }

    // AMM accounts resolved — transition to AMM
    if amm_accounts_resolved {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_phase_is_bonding_curve() {
        assert_eq!(PoolPhase::default(), PoolPhase::BondingCurve);
    }

    #[test]
    fn test_no_switch_when_already_amm() {
        assert!(!should_switch_to_amm(
            PoolPhase::Amm,
            true,
            true,
            100.0,
            95.0
        ));
    }

    #[test]
    fn test_switch_on_migration_observed() {
        assert!(should_switch_to_amm(
            PoolPhase::BondingCurve,
            false,
            true,
            0.0,
            95.0
        ));
    }

    #[test]
    fn test_switch_on_amm_accounts_resolved() {
        assert!(should_switch_to_amm(
            PoolPhase::BondingCurve,
            true,
            false,
            0.0,
            95.0
        ));
    }

    #[test]
    fn test_no_switch_without_signals() {
        assert!(!should_switch_to_amm(
            PoolPhase::BondingCurve,
            false,
            false,
            50.0,
            95.0
        ));
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", PoolPhase::BondingCurve), "BondingCurve");
        assert_eq!(format!("{}", PoolPhase::Amm), "Amm");
    }
}
