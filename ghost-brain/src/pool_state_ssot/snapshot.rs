//! PoolSnapshot — the SSOT data object for per-pool state.
//!
//! Updated atomically on each incoming update from Yellowstone, PumpPortal,
//! or fallback RPC. All pricing and quoting derives from this snapshot.

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

use super::phase::PoolPhase;

/// Source of the snapshot data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SnapshotSource {
    /// Primary: Yellowstone gRPC account subscription.
    Yellowstone,
    /// Secondary: PumpPortal WebSocket (bonding phase fast hint).
    PumpPortal,
    /// Tertiary: fallback RPC getAccountInfo.
    FallbackRpc,
}

impl std::fmt::Display for SnapshotSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotSource::Yellowstone => write!(f, "Yellowstone"),
            SnapshotSource::PumpPortal => write!(f, "PumpPortal"),
            SnapshotSource::FallbackRpc => write!(f, "FallbackRPC"),
        }
    }
}

/// Per-pool SSOT snapshot.
///
/// Maintained per pool/mint. Contains phase-dependent fields for both
/// bonding curve and AMM phases, plus derived mark price and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolSnapshot {
    /// Pool identifier (bonding curve key or AMM pool key).
    pub pool_id: Pubkey,
    /// Base token mint address.
    pub base_mint: Pubkey,
    /// Current pool lifecycle phase.
    pub phase: PoolPhase,

    // ── Bonding curve fields (valid when phase == BondingCurve) ──────────
    /// Virtual SOL reserves in lamports.
    pub v_sol: Option<u64>,
    /// Virtual token reserves (native units).
    pub v_tokens: Option<u64>,
    /// Market cap in SOL (optional, informational).
    pub market_cap_sol: Option<f64>,

    // ── AMM fields (valid when phase == Amm) ────────────────────────────
    /// SOL reserves in the AMM pool (lamports).
    pub reserve_sol: Option<u64>,
    /// Token reserves in the AMM pool (native units).
    pub reserve_token: Option<u64>,
    /// AMM fee in basis points (if known from pool state).
    pub fee_bps: Option<u16>,

    // ── Meta ─────────────────────────────────────────────────────────────
    /// Unix timestamp (ms) of the last update.
    pub last_update_unix_ms: u64,
    /// Data source that produced this snapshot.
    pub source: SnapshotSource,

    // ── Derived ─────────────────────────────────────────────────────────
    /// Mark price: SOL per token, derived from reserves.
    pub price_mark_sol_per_token: f64,
}

impl PoolSnapshot {
    /// Create a new bonding-curve-phase snapshot.
    pub fn new_bonding(
        pool_id: Pubkey,
        base_mint: Pubkey,
        v_sol: u64,
        v_tokens: u64,
        market_cap_sol: Option<f64>,
        source: SnapshotSource,
        now_ms: u64,
    ) -> Self {
        let price = if v_tokens > 0 {
            v_sol as f64 / v_tokens as f64
        } else {
            0.0
        };
        Self {
            pool_id,
            base_mint,
            phase: PoolPhase::BondingCurve,
            v_sol: Some(v_sol),
            v_tokens: Some(v_tokens),
            market_cap_sol,
            reserve_sol: None,
            reserve_token: None,
            fee_bps: None,
            last_update_unix_ms: now_ms,
            source,
            price_mark_sol_per_token: price,
        }
    }

    /// Create a new AMM-phase snapshot.
    pub fn new_amm(
        pool_id: Pubkey,
        base_mint: Pubkey,
        reserve_sol: u64,
        reserve_token: u64,
        fee_bps: Option<u16>,
        source: SnapshotSource,
        now_ms: u64,
    ) -> Self {
        let price = if reserve_token > 0 {
            reserve_sol as f64 / reserve_token as f64
        } else {
            0.0
        };
        Self {
            pool_id,
            base_mint,
            phase: PoolPhase::Amm,
            v_sol: None,
            v_tokens: None,
            market_cap_sol: None,
            reserve_sol: Some(reserve_sol),
            reserve_token: Some(reserve_token),
            fee_bps,
            last_update_unix_ms: now_ms,
            source,
            price_mark_sol_per_token: price,
        }
    }

    /// Age of this snapshot in milliseconds relative to `now_ms`.
    #[inline]
    pub fn age_ms(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.last_update_unix_ms)
    }

    /// Whether this snapshot is stale (age exceeds threshold).
    #[inline]
    pub fn is_stale(&self, now_ms: u64, stale_threshold_ms: u64) -> bool {
        self.age_ms(now_ms) > stale_threshold_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_key() -> Pubkey {
        Pubkey::new_unique()
    }

    #[test]
    fn test_bonding_snapshot_price() {
        let snap = PoolSnapshot::new_bonding(
            dummy_key(),
            dummy_key(),
            30_000_000_000, // 30 SOL
            1_073_000_000_000_000,
            None,
            SnapshotSource::Yellowstone,
            1000,
        );
        assert_eq!(snap.phase, PoolPhase::BondingCurve);
        let expected = 30_000_000_000.0 / 1_073_000_000_000_000.0;
        assert!((snap.price_mark_sol_per_token - expected).abs() < 1e-18);
    }

    #[test]
    fn test_amm_snapshot_price() {
        let snap = PoolSnapshot::new_amm(
            dummy_key(),
            dummy_key(),
            50_000_000_000,
            200_000_000,
            Some(25),
            SnapshotSource::Yellowstone,
            2000,
        );
        assert_eq!(snap.phase, PoolPhase::Amm);
        let expected = 50_000_000_000.0 / 200_000_000.0;
        assert!((snap.price_mark_sol_per_token - expected).abs() < 1e-6);
    }

    #[test]
    fn test_staleness() {
        let snap = PoolSnapshot::new_bonding(
            dummy_key(),
            dummy_key(),
            1_000_000_000,
            1_000_000_000_000,
            None,
            SnapshotSource::PumpPortal,
            1000,
        );
        assert!(!snap.is_stale(2000, 1500));
        assert!(snap.is_stale(3000, 1500));
    }

    #[test]
    fn test_zero_token_reserves_price() {
        let snap = PoolSnapshot::new_bonding(
            dummy_key(),
            dummy_key(),
            1_000_000_000,
            0,
            None,
            SnapshotSource::Yellowstone,
            1000,
        );
        assert_eq!(snap.price_mark_sol_per_token, 0.0);
    }
}
