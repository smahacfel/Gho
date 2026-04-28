//! Trade snapshot types used for canonical TX-level history.
//!
//! These types represent transactional snapshots with deterministic ordering and
//! serve as the source of truth for projecting `MarketSnapshot` data used by scoring.

use solana_sdk::{pubkey::Pubkey, signature::Signature};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use metrics::increment_counter;
use tracing::warn;

use super::types::{MarketSnapshot, PriceState, LAMPORTS_PER_SOL};

const BASELINE_UNIQUE_ADDRS: u64 = 1;

/// Deterministic transaction key used to order trades.
///
/// ## Ordering (EVENT-TIME PRIMARY)
///
/// Keys are ordered by:
/// 1. `timestamp_ms` (event time - PRIMARY)
/// 2. `tx_index` (if available)
/// 3. `signature` (lexicographic)
/// 4. `fallback_counter` (last resort)
///
/// Events without slot are first-class inputs and order correctly by timestamp.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TxKey {
    /// Event timestamp in milliseconds since UNIX_EPOCH (PRIMARY ordering key).
    pub timestamp_ms: u64,
    /// Optional Solana slot - diagnostic metadata only, NOT used for logic.
    pub slot: Option<u64>,
    /// Optional transaction log/index order when available.
    pub tx_index: Option<u32>,
    /// Optional signature (lexicographic tie-breaker).
    pub signature: Option<Signature>,
    /// Fallback counter used only when other tie-breakers are missing.
    pub fallback_counter: u64,
}

/// Errors for TxKey validation.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum TxKeyError {
    #[error("timestamp_ms must be greater than zero")]
    TimestampZero,
}

impl TxKey {
    /// Create a TxKey with validation.
    ///
    /// # Arguments
    /// * `timestamp_ms` - Event timestamp (REQUIRED, must be > 0)
    /// * `slot` - Optional Solana slot (diagnostic metadata only)
    /// * `tx_index` - Optional transaction index
    /// * `signature` - Optional transaction signature
    /// * `fallback_counter` - Tie-breaker counter
    pub fn new(
        timestamp_ms: u64,
        slot: Option<u64>,
        tx_index: Option<u32>,
        signature: Option<Signature>,
        fallback_counter: u64,
    ) -> Result<Self, TxKeyError> {
        if timestamp_ms == 0 {
            return Err(TxKeyError::TimestampZero);
        }

        Ok(Self {
            timestamp_ms,
            slot,
            tx_index,
            signature,
            fallback_counter,
        })
    }

    /// Helper to compare slots as optional tie-breaker.
    #[inline]
    fn cmp_signature(&self, other: &Self) -> Ordering {
        match (&self.signature, &other.signature) {
            (Some(left), Some(right)) => left.cmp(right),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }

    /// Helper to compare tx_index as tie-breaker.
    #[inline]
    fn cmp_tx_index(&self, other: &Self) -> Ordering {
        match (self.tx_index, other.tx_index) {
            (Some(left), Some(right)) => left.cmp(&right),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }
}

impl PartialEq for TxKey {
    fn eq(&self, other: &Self) -> bool {
        self.timestamp_ms == other.timestamp_ms
            && self.tx_index == other.tx_index
            && self.signature == other.signature
            && self.fallback_counter == other.fallback_counter
    }
}

impl Eq for TxKey {}

impl Hash for TxKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.timestamp_ms.hash(state);
        self.tx_index.hash(state);
        self.signature.hash(state);
        self.fallback_counter.hash(state);
    }
}

impl Ord for TxKey {
    /// Ordering based on EVENT-TIME (timestamp_ms) as primary key.
    ///
    /// Order: timestamp_ms → tx_index → signature → fallback_counter
    fn cmp(&self, other: &Self) -> Ordering {
        // PRIMARY: Event timestamp (event-time is the only source of truth)
        match self.timestamp_ms.cmp(&other.timestamp_ms) {
            Ordering::Equal => {
                // TIE-BREAKER 1: Transaction index
                match self.cmp_tx_index(other) {
                    Ordering::Equal => {
                        // TIE-BREAKER 2: Signature
                        match self.cmp_signature(other) {
                            Ordering::Equal => {
                                // TIE-BREAKER 3: Fallback counter
                                self.fallback_counter.cmp(&other.fallback_counter)
                            }
                            ord => ord,
                        }
                    }
                    ord => ord,
                }
            }
            ord => ord,
        }
    }
}

impl PartialOrd for TxKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Direction of the trade relative to the pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TradeSide {
    Buy,
    Sell,
}

/// Origin of a trade snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TradeSource {
    GatekeeperBuffered,
    Live,
}

/// Canonical TX-level trade snapshot.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TradeSnapshot {
    pub base_mint: Pubkey,
    pub tx_key: TxKey,
    pub side: TradeSide,
    pub dev_buy: bool,
    /// BUY: SOL in. SELL: SOL out (after fees).
    pub d_sol_lamports: u64,
    /// BUY: tokens out. SELL: tokens in.
    pub d_tok_units: u64,
    pub price_avg_sol_per_tok: f64,
    pub price_instant_after_sol_per_tok: f64,
    pub reserve_sol_after_lamports: u64,
    pub reserve_tok_after_units: u64,
    pub fee_lamports: Option<u64>,
    pub trader: Option<Pubkey>,
    pub source: TradeSource,
}

/// Errors for trade snapshot projection/validation.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum TradeSnapshotError {
    #[error("timestamp_ms must be greater than zero")]
    TimestampZero,
    #[error("duplicate transaction key detected: {0:?}")]
    DuplicateTxKey(TxKey),
}

impl TradeSnapshot {
    /// Create a TradeSnapshot.
    ///
    /// # Validation
    /// * `tx_key.timestamp_ms` must be > 0 (TxKey validates this)
    /// * `slot` is optional diagnostic metadata
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_mint: Pubkey,
        tx_key: TxKey,
        side: TradeSide,
        dev_buy: bool,
        d_sol_lamports: u64,
        d_tok_units: u64,
        price_avg_sol_per_tok: f64,
        price_instant_after_sol_per_tok: f64,
        reserve_sol_after_lamports: u64,
        reserve_tok_after_units: u64,
        fee_lamports: Option<u64>,
        trader: Option<Pubkey>,
        source: TradeSource,
    ) -> Result<Self, TradeSnapshotError> {
        // No slot validation - slots are optional metadata
        // TxKey validates timestamp_ms > 0
        if tx_key.timestamp_ms == 0 {
            return Err(TradeSnapshotError::TimestampZero);
        }

        Ok(Self {
            base_mint,
            tx_key,
            side,
            dev_buy,
            d_sol_lamports,
            d_tok_units,
            price_avg_sol_per_tok,
            price_instant_after_sol_per_tok,
            reserve_sol_after_lamports,
            reserve_tok_after_units,
            fee_lamports,
            trader,
            source,
        })
    }

    #[inline]
    fn volume_delta_sol(&self) -> f64 {
        // cum_volume_sol counts BUY-side SOL only (SELL volume is not accumulated).
        match self.side {
            TradeSide::Buy => self.d_sol_lamports as f64 / LAMPORTS_PER_SOL,
            TradeSide::Sell => 0.0,
        }
    }

    /// Project this TradeSnapshot into a MarketSnapshot view for scoring.
    /// `unique_addrs` should include the baseline owner/pool creator (1) + seen traders.
    /// `cum_volume_sol` counts BUY-side SOL only.
    pub fn to_market_snapshot(
        &self,
        prev: Option<&MarketSnapshot>,
        unique_addrs: u64,
    ) -> MarketSnapshot {
        let tx_count = prev.map_or(1, |snap| snap.tx_count.saturating_add(1));
        let cum_volume_sol = prev.map_or(0.0, |snap| snap.cum_volume_sol) + self.volume_delta_sol();
        let price = self.price_instant_after_sol_per_tok;
        let (price_state, price_reason) = PriceState::from_price(price);
        let reserve_base = self.reserve_tok_after_units as f64;
        let reserve_quote = self.reserve_sol_after_lamports as f64 / LAMPORTS_PER_SOL;
        // Market cap is not derived from reserves here (supply unknown), so keep it explicit
        // to avoid feeding scoring with a proxy value.
        let market_cap_sol = 0.0;

        MarketSnapshot {
            // Slot is now Option<u64> - pass through directly
            slot: self.tx_key.slot,
            tx_key: Some(self.tx_key.clone()),
            timestamp_ms: self.tx_key.timestamp_ms,
            cum_volume_sol,
            tx_count,
            unique_addrs,
            price_sol_per_token: price,
            price_state,
            price_reason,
            market_cap_sol,
            reserve_base,
            reserve_quote,
            // TODO: Provide bonding_progress_pct from curve projection when available.
            bonding_progress_pct: 0.0,
            d_price_d_volume: 0.0,
            d_price_d_liquidity: 0.0,
            d_price_d_slippage: 0.0,
        }
    }
}

/// Build ordered MarketSnapshots from TradeSnapshot history.
/// `cum_volume_sol` is accumulated from BUY-side SOL only.
/// `unique_addrs` uses a baseline of 1 (owner/creator) plus observed traders.
pub fn build_market_snapshots_from_trades(
    trades: &[TradeSnapshot],
) -> Result<Vec<MarketSnapshot>, TradeSnapshotError> {
    let mut ordered = trades.to_vec();
    ordered.sort_by(|left, right| left.tx_key.cmp(&right.tx_key));

    let mut snapshots = Vec::with_capacity(ordered.len());
    let mut seen_keys = HashSet::new();
    let mut seen_traders = HashSet::new();

    for trade in ordered {
        // No slot validation - slots are optional metadata
        // timestamp_ms validation is done on TxKey construction
        if seen_keys.contains(&trade.tx_key) {
            return Err(TradeSnapshotError::DuplicateTxKey(trade.tx_key.clone()));
        }
        seen_keys.insert(trade.tx_key.clone());

        if let Some(trader) = trade.trader {
            seen_traders.insert(trader);
        }
        // Emit degraded ordering telemetry at the canonical history boundary to avoid
        // logging for transient TradeSnapshots that never reach projection.
        if trade.tx_key.tx_index.is_none() && trade.tx_key.signature.is_none() {
            increment_counter!("shadowledger_trade_snapshot_degraded_ordering_total");
            warn!(
                slot = ?trade.tx_key.slot,
                timestamp_ms = trade.tx_key.timestamp_ms,
                "TradeSnapshot: degraded ordering (missing tx_index and signature)"
            );
        }

        let unique_addrs = BASELINE_UNIQUE_ADDRS + seen_traders.len() as u64;
        let prev = snapshots.last();
        let snapshot = trade.to_market_snapshot(prev, unique_addrs);
        snapshots.push(snapshot);
    }

    Ok(snapshots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::pubkey::Pubkey;

    fn sig() -> Signature {
        Signature::new_unique()
    }

    // =========================================================================
    // TxKey Tests
    // =========================================================================

    #[test]
    fn test_tx_key_rejects_timestamp_zero() {
        // timestamp_ms = 0 is invalid
        let err = TxKey::new(0, Some(100), Some(1), None, 0).unwrap_err();
        assert_eq!(err, TxKeyError::TimestampZero);
    }

    #[test]
    fn test_tx_key_accepts_none_slot() {
        // Events without slot are first-class inputs
        let key = TxKey::new(1000, None, Some(1), None, 0).unwrap();
        assert_eq!(key.timestamp_ms, 1000);
        assert!(key.slot.is_none());
    }

    #[test]
    fn test_tx_key_sorting_by_timestamp() {
        // PRIMARY: timestamp_ms ordering
        let key_early = TxKey::new(1000, Some(100), None, None, 0).unwrap();
        let key_late = TxKey::new(2000, Some(100), None, None, 0).unwrap();

        assert!(key_early < key_late, "Earlier timestamp should sort first");
    }

    #[test]
    fn test_tx_key_sorting_slot_ignored() {
        // Same timestamp, different slots -> slot is ignored for ordering/equality
        let key_slot_low = TxKey::new(1000, Some(100), None, None, 0).unwrap();
        let key_slot_high = TxKey::new(1000, Some(200), None, None, 0).unwrap();

        assert_eq!(key_slot_low, key_slot_high);
        assert_eq!(key_slot_low.cmp(&key_slot_high), Ordering::Equal);
    }

    #[test]
    fn test_tx_key_sorting_without_slot() {
        // Compare keys: one with slot, one without (slot ignored)
        let key_with_slot = TxKey::new(1000, Some(100), None, None, 0).unwrap();
        let key_without_slot = TxKey::new(1000, None, None, None, 0).unwrap();

        assert_eq!(key_with_slot, key_without_slot);
        assert_eq!(key_with_slot.cmp(&key_without_slot), Ordering::Equal);
    }

    #[test]
    fn test_tx_key_sorting_full_tiebreakers() {
        // Same timestamp, same slot -> use tx_index, signature, fallback
        let key_index_1 = TxKey::new(1000, Some(100), Some(1), None, 0).unwrap();
        let key_index_2 = TxKey::new(1000, Some(100), Some(2), None, 0).unwrap();
        let sig_a = sig();
        let sig_b = sig();
        let (sig_low, sig_high) = if sig_a <= sig_b {
            (sig_a, sig_b)
        } else {
            (sig_b, sig_a)
        };
        let key_sig_a = TxKey::new(1000, Some(100), None, Some(sig_low), 0).unwrap();
        let key_sig_b = TxKey::new(1000, Some(100), None, Some(sig_high), 0).unwrap();
        let key_fallback_1 = TxKey::new(1000, Some(100), None, None, 1).unwrap();
        let key_fallback_2 = TxKey::new(1000, Some(100), None, None, 2).unwrap();

        let mut keys = vec![
            key_fallback_2.clone(),
            key_sig_b.clone(),
            key_index_2.clone(),
            key_fallback_1.clone(),
            key_sig_a.clone(),
            key_index_1.clone(),
        ];
        keys.sort();

        assert_eq!(
            keys,
            vec![
                key_index_1,
                key_index_2,
                key_sig_a,
                key_sig_b,
                key_fallback_1,
                key_fallback_2,
            ]
        );
    }

    // =========================================================================
    // TradeSnapshot Tests
    // =========================================================================

    #[test]
    fn test_trade_snapshot_projection_builds_market_history() {
        let base_mint = Pubkey::new_unique();
        let trader_a = Pubkey::new_unique();
        let trader_b = Pubkey::new_unique();

        // Note: TxKey::new(timestamp_ms, slot, tx_index, signature, fallback)
        let trade1 = TradeSnapshot::new(
            base_mint,
            TxKey::new(1_000, Some(100), Some(1), None, 0).unwrap(),
            TradeSide::Buy,
            false,
            1_000_000_000,
            100,
            0.01,
            0.02,
            2_000_000_000,
            900,
            Some(10_000),
            Some(trader_a),
            TradeSource::GatekeeperBuffered,
        )
        .unwrap();

        let trade2 = TradeSnapshot::new(
            base_mint,
            TxKey::new(2_000, Some(101), Some(2), None, 0).unwrap(),
            TradeSide::Sell,
            false,
            500_000_000,
            50,
            0.012,
            0.015,
            1_500_000_000,
            950,
            None,
            Some(trader_b),
            TradeSource::Live,
        )
        .unwrap();

        // Ordering by timestamp_ms (1000 < 2000)
        let snapshots = build_market_snapshots_from_trades(&[trade2, trade1]).unwrap();
        assert_eq!(snapshots.len(), 2);

        // First snapshot (timestamp 1000, slot 100)
        assert_eq!(snapshots[0].slot, Some(100));
        assert_eq!(snapshots[0].timestamp_ms, 1_000);
        assert_eq!(snapshots[0].tx_count, 1);
        assert!((snapshots[0].cum_volume_sol - 1.0).abs() < f64::EPSILON);
        assert_eq!(snapshots[0].unique_addrs, 2);
        assert_eq!(snapshots[0].price_sol_per_token, 0.02);
        assert_eq!(snapshots[0].reserve_quote, 2.0);

        // Second snapshot (timestamp 2000, slot 101)
        assert_eq!(snapshots[1].slot, Some(101));
        assert_eq!(snapshots[1].timestamp_ms, 2_000);
        assert_eq!(snapshots[1].tx_count, 2);
        assert_eq!(snapshots[1].unique_addrs, 3);
    }

    #[test]
    fn test_trade_snapshot_without_slot() {
        // Events without slot are first-class inputs (Rule #8)
        let base_mint = Pubkey::new_unique();

        let trade = TradeSnapshot::new(
            base_mint,
            TxKey::new(1_000, None, None, None, 0).unwrap(), // No slot!
            TradeSide::Buy,
            false,
            1_000_000_000,
            100,
            0.01,
            0.02,
            2_000_000_000,
            900,
            None,
            None,
            TradeSource::Live,
        )
        .unwrap();

        let snapshots = build_market_snapshots_from_trades(&[trade]).unwrap();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].slot, None); // Slot is None when TxKey.slot is None
        assert_eq!(snapshots[0].timestamp_ms, 1_000);
    }

    #[test]
    fn test_trade_snapshot_duplicate_key_rejected() {
        let base_mint = Pubkey::new_unique();
        let tx_key = TxKey::new(1_000, Some(100), Some(1), None, 0).unwrap();
        let trade1 = TradeSnapshot::new(
            base_mint,
            tx_key.clone(),
            TradeSide::Buy,
            false,
            1_000_000_000,
            100,
            0.01,
            0.02,
            2_000_000_000,
            900,
            None,
            None,
            TradeSource::GatekeeperBuffered,
        )
        .unwrap();
        let trade2 = TradeSnapshot::new(
            base_mint,
            tx_key.clone(),
            TradeSide::Sell,
            false,
            500_000_000,
            50,
            0.012,
            0.015,
            1_500_000_000,
            950,
            None,
            None,
            TradeSource::Live,
        )
        .unwrap();

        let err = build_market_snapshots_from_trades(&[trade1, trade2]).unwrap_err();
        assert!(matches!(err, TradeSnapshotError::DuplicateTxKey(key) if key == tx_key));
    }

    #[test]
    fn test_trade_snapshot_rejects_timestamp_zero() {
        let base_mint = Pubkey::new_unique();
        // Manually construct TxKey with timestamp_ms = 0 (bypassing ::new validation)
        let tx_key = TxKey {
            timestamp_ms: 0,
            slot: None,
            tx_index: None,
            signature: None,
            fallback_counter: 0,
        };
        let err = TradeSnapshot::new(
            base_mint,
            tx_key,
            TradeSide::Buy,
            false,
            0,
            0,
            0.0,
            0.0,
            0,
            0,
            None,
            None,
            TradeSource::Live,
        )
        .unwrap_err();
        assert_eq!(err, TradeSnapshotError::TimestampZero);
    }
}
