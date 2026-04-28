//! QuoteEngine — executability quotes for configured trade sizes.
//!
//! Given a [`PoolSnapshot`] and a trade size, produces an executable [`Quote`]
//! with expected_out, effective_price, min_out, and slippage estimate.
//!
//! ## Pricing formulas
//!
//! **Bonding Curve** (constant-product with virtual reserves):
//! - `mark_price = v_sol / v_tokens`
//! - `quote_buy(sol_in)`: `tokens_out = (v_tokens * sol_in_eff) / (v_sol + sol_in_eff)`
//! - `quote_sell(token_in)`: `sol_out = (v_sol * token_in_eff) / (v_tokens + token_in_eff)`
//!
//! **AMM** (constant-product with real reserves):
//! - `mark_price = reserve_sol / reserve_token`
//! - Same constant-product formula with real reserves + fee.

use serde::{Deserialize, Serialize};

use super::config::SsotConfig;
use super::phase::PoolPhase;
use super::snapshot::PoolSnapshot;

/// Side of the trade for quoting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuoteSide {
    /// Buy tokens with SOL.
    Buy,
    /// Sell tokens for SOL.
    Sell,
}

/// Executable quote result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quote {
    /// Expected output amount (tokens for Buy, lamports for Sell).
    pub expected_out: f64,
    /// Effective price (SOL per token) after fee/impact.
    pub effective_price: f64,
    /// Minimum output after applying slippage tolerance.
    pub min_out: f64,
    /// Estimated slippage in basis points.
    pub slippage_est_bps: f64,
}

/// Stateless quote engine — takes a snapshot + trade params, returns a quote.
pub struct QuoteEngine;

impl QuoteEngine {
    /// Compute an executable quote from a pool snapshot.
    ///
    /// # Arguments
    /// - `snapshot`: current pool state
    /// - `side`: Buy or Sell
    /// - `amount_in`: input amount in native units (lamports for Buy, tokens for Sell)
    /// - `config`: SSOT config for fee/slippage defaults
    ///
    /// # Returns
    /// `Some(Quote)` if the snapshot has valid reserves, `None` otherwise.
    pub fn quote(
        snapshot: &PoolSnapshot,
        side: QuoteSide,
        amount_in: u64,
        config: &SsotConfig,
    ) -> Option<Quote> {
        if amount_in == 0 {
            return None;
        }
        match snapshot.phase {
            PoolPhase::BondingCurve => Self::quote_bonding(snapshot, side, amount_in, config),
            PoolPhase::Amm => Self::quote_amm(snapshot, side, amount_in, config),
        }
    }

    /// Mark price from the snapshot (SOL per token).
    #[inline]
    pub fn mark_price(snapshot: &PoolSnapshot) -> f64 {
        snapshot.price_mark_sol_per_token
    }

    // ── Bonding Curve ────────────────────────────────────────────────────

    fn quote_bonding(
        snapshot: &PoolSnapshot,
        side: QuoteSide,
        amount_in: u64,
        config: &SsotConfig,
    ) -> Option<Quote> {
        let v_sol = snapshot.v_sol? as f64;
        let v_tokens = snapshot.v_tokens? as f64;

        if v_sol <= 0.0 || v_tokens <= 0.0 {
            return None;
        }

        let fee_bps = config.bonding_fee_bps_default as f64;
        let fee_mult = 1.0 - fee_bps / 10_000.0;
        let mark_price = v_sol / v_tokens;

        match side {
            QuoteSide::Buy => {
                let sol_in = amount_in as f64;
                let sol_in_eff = sol_in * fee_mult;
                let tokens_out = (v_tokens * sol_in_eff) / (v_sol + sol_in_eff);
                if tokens_out <= 0.0 {
                    return None;
                }
                let eff_price = sol_in / tokens_out;
                let slippage_bps = ((eff_price - mark_price) / mark_price) * 10_000.0;
                let slippage_tolerance = config.slippage_bps_default as f64;
                let min_out = tokens_out * (1.0 - slippage_tolerance / 10_000.0);

                Some(Quote {
                    expected_out: tokens_out,
                    effective_price: eff_price,
                    min_out: min_out.max(0.0),
                    slippage_est_bps: slippage_bps.max(0.0),
                })
            }
            QuoteSide::Sell => {
                let token_in = amount_in as f64;
                let token_in_eff = token_in * fee_mult;
                let sol_out = (v_sol * token_in_eff) / (v_tokens + token_in_eff);
                if sol_out <= 0.0 {
                    return None;
                }
                let eff_price = sol_out / token_in;
                let slippage_bps = ((mark_price - eff_price) / mark_price) * 10_000.0;
                let slippage_tolerance = config.slippage_bps_default as f64;
                let min_out = sol_out * (1.0 - slippage_tolerance / 10_000.0);

                Some(Quote {
                    expected_out: sol_out,
                    effective_price: eff_price,
                    min_out: min_out.max(0.0),
                    slippage_est_bps: slippage_bps.max(0.0),
                })
            }
        }
    }

    // ── AMM (constant product) ───────────────────────────────────────────

    fn quote_amm(
        snapshot: &PoolSnapshot,
        side: QuoteSide,
        amount_in: u64,
        config: &SsotConfig,
    ) -> Option<Quote> {
        let rsol = snapshot.reserve_sol? as f64;
        let rtok = snapshot.reserve_token? as f64;

        if rsol <= 0.0 || rtok <= 0.0 {
            return None;
        }

        let fee_bps = snapshot.fee_bps.unwrap_or(config.amm_fee_bps_default) as f64;
        let fee_mult = 1.0 - fee_bps / 10_000.0;
        let mark_price = rsol / rtok;

        match side {
            QuoteSide::Buy => {
                let sol_in = amount_in as f64;
                let sol_in_eff = sol_in * fee_mult;
                let tokens_out = (rtok * sol_in_eff) / (rsol + sol_in_eff);
                if tokens_out <= 0.0 {
                    return None;
                }
                let eff_price = sol_in / tokens_out;
                let slippage_bps = ((eff_price - mark_price) / mark_price) * 10_000.0;
                let slippage_tolerance = config.slippage_bps_default as f64;
                let min_out = tokens_out * (1.0 - slippage_tolerance / 10_000.0);

                Some(Quote {
                    expected_out: tokens_out,
                    effective_price: eff_price,
                    min_out: min_out.max(0.0),
                    slippage_est_bps: slippage_bps.max(0.0),
                })
            }
            QuoteSide::Sell => {
                let token_in = amount_in as f64;
                let token_in_eff = token_in * fee_mult;
                let sol_out = (rsol * token_in_eff) / (rtok + token_in_eff);
                if sol_out <= 0.0 {
                    return None;
                }
                let eff_price = sol_out / token_in;
                let slippage_bps = ((mark_price - eff_price) / mark_price) * 10_000.0;
                let slippage_tolerance = config.slippage_bps_default as f64;
                let min_out = sol_out * (1.0 - slippage_tolerance / 10_000.0);

                Some(Quote {
                    expected_out: sol_out,
                    effective_price: eff_price,
                    min_out: min_out.max(0.0),
                    slippage_est_bps: slippage_bps.max(0.0),
                })
            }
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::super::snapshot::SnapshotSource;
    use super::*;
    use solana_sdk::pubkey::Pubkey;

    fn dummy() -> Pubkey {
        Pubkey::new_unique()
    }

    fn cfg() -> SsotConfig {
        SsotConfig {
            bonding_fee_bps_default: 100, // 1%
            amm_fee_bps_default: 25,      // 0.25%
            slippage_bps_default: 100,    // 1%
            ..SsotConfig::default()
        }
    }

    // ── Bonding curve buy ────────────────────────────────────────────────

    #[test]
    fn test_bonding_buy_basic() {
        let snap = PoolSnapshot::new_bonding(
            dummy(),
            dummy(),
            30_000_000_000,        // 30 SOL
            1_073_000_000_000_000, // ~1.073T tokens
            None,
            SnapshotSource::Yellowstone,
            1000,
        );
        let c = cfg();
        let q = QuoteEngine::quote(&snap, QuoteSide::Buy, 1_000_000_000, &c).unwrap(); // 1 SOL

        // tokens_out = (v_tok * sol_in_eff) / (v_sol + sol_in_eff)
        // sol_in_eff = 1e9 * 0.99 = 0.99e9
        // tokens_out = (1.073e15 * 0.99e9) / (30e9 + 0.99e9)
        let sol_eff = 1_000_000_000.0 * 0.99;
        let expected_tokens = (1_073_000_000_000_000.0 * sol_eff) / (30_000_000_000.0 + sol_eff);
        assert!((q.expected_out - expected_tokens).abs() < 1.0);
        assert!(q.effective_price > 0.0);
        assert!(q.min_out > 0.0);
        assert!(q.min_out < q.expected_out);
        assert!(q.slippage_est_bps >= 0.0);
    }

    // ── Bonding curve sell ───────────────────────────────────────────────

    #[test]
    fn test_bonding_sell_basic() {
        let snap = PoolSnapshot::new_bonding(
            dummy(),
            dummy(),
            30_000_000_000,
            1_073_000_000_000_000,
            None,
            SnapshotSource::Yellowstone,
            1000,
        );
        let c = cfg();
        // Sell 1 billion tokens
        let q = QuoteEngine::quote(&snap, QuoteSide::Sell, 1_000_000_000, &c).unwrap();

        let tok_eff = 1_000_000_000.0 * 0.99;
        let expected_sol = (30_000_000_000.0 * tok_eff) / (1_073_000_000_000_000.0 + tok_eff);
        assert!((q.expected_out - expected_sol).abs() < 1.0);
        assert!(q.effective_price > 0.0);
        assert!(q.min_out > 0.0);
    }

    // ── AMM buy ──────────────────────────────────────────────────────────

    #[test]
    fn test_amm_buy_basic() {
        let snap = PoolSnapshot::new_amm(
            dummy(),
            dummy(),
            50_000_000_000,  // 50 SOL
            200_000_000_000, // 200B tokens
            Some(25),        // 0.25% fee
            SnapshotSource::Yellowstone,
            1000,
        );
        let c = cfg();
        let q = QuoteEngine::quote(&snap, QuoteSide::Buy, 1_000_000_000, &c).unwrap(); // 1 SOL

        let sol_eff = 1_000_000_000.0 * (1.0 - 25.0 / 10_000.0);
        let expected_tokens = (200_000_000_000.0 * sol_eff) / (50_000_000_000.0 + sol_eff);
        assert!((q.expected_out - expected_tokens).abs() < 1.0);
        assert!(q.effective_price > 0.0);
    }

    // ── AMM sell ─────────────────────────────────────────────────────────

    #[test]
    fn test_amm_sell_basic() {
        let snap = PoolSnapshot::new_amm(
            dummy(),
            dummy(),
            50_000_000_000,
            200_000_000_000,
            Some(25),
            SnapshotSource::Yellowstone,
            1000,
        );
        let c = cfg();
        let q = QuoteEngine::quote(&snap, QuoteSide::Sell, 1_000_000_000, &c).unwrap();

        let tok_eff = 1_000_000_000.0 * (1.0 - 25.0 / 10_000.0);
        let expected_sol = (50_000_000_000.0 * tok_eff) / (200_000_000_000.0 + tok_eff);
        assert!((q.expected_out - expected_sol).abs() < 1.0);
    }

    // ── Edge cases ───────────────────────────────────────────────────────

    #[test]
    fn test_zero_amount_returns_none() {
        let snap = PoolSnapshot::new_bonding(
            dummy(),
            dummy(),
            1_000_000_000,
            1_000_000_000_000,
            None,
            SnapshotSource::Yellowstone,
            1000,
        );
        assert!(QuoteEngine::quote(&snap, QuoteSide::Buy, 0, &cfg()).is_none());
    }

    #[test]
    fn test_zero_reserves_returns_none() {
        let snap = PoolSnapshot::new_bonding(
            dummy(),
            dummy(),
            0,
            0,
            None,
            SnapshotSource::Yellowstone,
            1000,
        );
        assert!(QuoteEngine::quote(&snap, QuoteSide::Buy, 1_000_000_000, &cfg()).is_none());
    }

    #[test]
    fn test_amm_uses_snapshot_fee_over_default() {
        let snap_with_fee = PoolSnapshot::new_amm(
            dummy(),
            dummy(),
            50_000_000_000,
            200_000_000_000,
            Some(300), // 3% fee override
            SnapshotSource::Yellowstone,
            1000,
        );
        let snap_default_fee = PoolSnapshot::new_amm(
            dummy(),
            dummy(),
            50_000_000_000,
            200_000_000_000,
            None, // will use config default (25bps)
            SnapshotSource::Yellowstone,
            1000,
        );
        let c = cfg();
        let q_high = QuoteEngine::quote(&snap_with_fee, QuoteSide::Buy, 1_000_000_000, &c).unwrap();
        let q_low =
            QuoteEngine::quote(&snap_default_fee, QuoteSide::Buy, 1_000_000_000, &c).unwrap();

        // Higher fee → fewer tokens out
        assert!(q_high.expected_out < q_low.expected_out);
    }

    #[test]
    fn test_mark_price_bonding() {
        let snap = PoolSnapshot::new_bonding(
            dummy(),
            dummy(),
            30_000_000_000,
            1_073_000_000_000_000,
            None,
            SnapshotSource::Yellowstone,
            1000,
        );
        let mp = QuoteEngine::mark_price(&snap);
        let expected = 30_000_000_000.0 / 1_073_000_000_000_000.0;
        assert!((mp - expected).abs() < 1e-18);
    }

    #[test]
    fn test_mark_price_amm() {
        let snap = PoolSnapshot::new_amm(
            dummy(),
            dummy(),
            50_000_000_000,
            200_000_000_000,
            Some(25),
            SnapshotSource::Yellowstone,
            1000,
        );
        let mp = QuoteEngine::mark_price(&snap);
        let expected = 50_000_000_000.0 / 200_000_000_000.0;
        assert!((mp - expected).abs() < 1e-12);
    }

    // ── Min out / slippage ───────────────────────────────────────────────

    #[test]
    fn test_min_out_applies_slippage() {
        let snap = PoolSnapshot::new_bonding(
            dummy(),
            dummy(),
            30_000_000_000,
            1_073_000_000_000_000,
            None,
            SnapshotSource::Yellowstone,
            1000,
        );
        let c = SsotConfig {
            slippage_bps_default: 200, // 2%
            bonding_fee_bps_default: 100,
            ..SsotConfig::default()
        };
        let q = QuoteEngine::quote(&snap, QuoteSide::Buy, 1_000_000_000, &c).unwrap();
        let expected_min = q.expected_out * (1.0 - 200.0 / 10_000.0);
        assert!((q.min_out - expected_min).abs() < 1.0);
    }

    // ── Integration: synthetic updates → snapshot → quote changes ────────

    #[test]
    fn test_integration_snapshot_updates_change_quotes() {
        let pool = dummy();
        let mint = dummy();
        let c = cfg();

        // Snapshot 1: low sol reserves
        let snap1 = PoolSnapshot::new_bonding(
            pool,
            mint,
            30_000_000_000,
            1_073_000_000_000_000,
            None,
            SnapshotSource::Yellowstone,
            1000,
        );
        let q1 = QuoteEngine::quote(&snap1, QuoteSide::Buy, 1_000_000_000, &c).unwrap();

        // Snapshot 2: higher sol reserves (someone bought)
        let snap2 = PoolSnapshot::new_bonding(
            pool,
            mint,
            35_000_000_000,
            900_000_000_000_000,
            None,
            SnapshotSource::Yellowstone,
            2000,
        );
        let q2 = QuoteEngine::quote(&snap2, QuoteSide::Buy, 1_000_000_000, &c).unwrap();

        // Prices MUST differ (reserves changed)
        assert!(
            (q1.effective_price - q2.effective_price).abs() > 1e-15,
            "Quotes must change when reserves change: q1={}, q2={}",
            q1.effective_price,
            q2.effective_price,
        );
    }

    #[test]
    fn test_integration_amm_migration_quotes_continue() {
        let pool = dummy();
        let mint = dummy();
        let c = cfg();

        // Before migration: bonding
        let snap_bc = PoolSnapshot::new_bonding(
            pool,
            mint,
            80_000_000_000,
            200_000_000_000_000,
            None,
            SnapshotSource::Yellowstone,
            1000,
        );
        let q_bc = QuoteEngine::quote(&snap_bc, QuoteSide::Sell, 1_000_000_000, &c).unwrap();

        // After migration: AMM
        let snap_amm = PoolSnapshot::new_amm(
            pool,
            mint,
            80_000_000_000,
            200_000_000_000,
            Some(25),
            SnapshotSource::Yellowstone,
            2000,
        );
        let q_amm = QuoteEngine::quote(&snap_amm, QuoteSide::Sell, 1_000_000_000, &c).unwrap();

        // Both produce valid quotes (system continues post-migration)
        assert!(q_bc.expected_out > 0.0);
        assert!(q_amm.expected_out > 0.0);
        // Prices differ due to different reserve scales
        assert!((q_bc.effective_price - q_amm.effective_price).abs() > 1e-15);
    }
}
