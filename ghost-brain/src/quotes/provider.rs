//! ExecutableQuoteProvider — Single Source of Truth for price references
//!
//! Both PaperBroker (fill price) and LiveBackend (audit trail) reference
//! quotes from this provider. Quotes are stored in a per-pool ring buffer
//! with automatic stale detection and eviction.

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, VecDeque};
use tracing::{debug, warn};

use crate::execution::backend::QuoteId;

// ─── Config ─────────────────────────────────────────────────────────────────

/// Configuration for the quote provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteProviderConfig {
    /// Maximum age before a quote is considered stale (ms).
    pub max_quote_age_ms: u64,
    /// Ring buffer size per pool.
    pub ring_buffer_size: usize,
    /// How often to generate quotes (ms). Informational — caller drives cadence.
    pub generation_interval_ms: u64,
    /// Age at which to emit a warning (ms), before full stale threshold.
    pub stale_warning_threshold_ms: u64,
}

impl Default for QuoteProviderConfig {
    fn default() -> Self {
        Self {
            max_quote_age_ms: 1500,
            ring_buffer_size: 256,
            generation_interval_ms: 500,
            stale_warning_threshold_ms: 1000,
        }
    }
}

// ─── Quote types ────────────────────────────────────────────────────────────

/// Source of a quote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuoteSource {
    BondingCurve,
    RaydiumAMM,
    External,
}

/// A single price quote with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutableQuote {
    pub quote_id: QuoteId,
    pub pool_amm_id: Pubkey,
    pub base_mint: Pubkey,
    pub timestamp_ms: u64,
    pub slot: Option<u64>,
    pub price_sol_per_token: f64,
    pub price_mcap_usd: Option<f64>,
    pub sol_reserves: u64,
    pub token_reserves: u64,
    /// Price impact percent for slippage model.
    pub price_impact_pct: f64,
    pub source: QuoteSource,
    pub is_stale: bool,
}

// ─── Provider ───────────────────────────────────────────────────────────────

/// SSOT price reference provider.
///
/// Stores quotes per pool in a ring buffer with FIFO eviction.
/// Both Paper and Live backends use this for value resolution.
pub struct ExecutableQuoteProvider {
    config: QuoteProviderConfig,
    /// Per-pool ring buffers, keyed by pool_amm_id.
    buffers: HashMap<Pubkey, VecDeque<ExecutableQuote>>,
    /// Flat index: quote_id → (pool_amm_id, index_in_buffer).
    /// Rebuilt on eviction — OK for current scale.
    quote_index: HashMap<QuoteId, Pubkey>,
    /// Monotonic counter for unique quote IDs.
    next_seq: u64,
}

impl ExecutableQuoteProvider {
    pub fn new(config: QuoteProviderConfig) -> Self {
        Self {
            config,
            buffers: HashMap::new(),
            quote_index: HashMap::new(),
            next_seq: 0,
        }
    }

    /// Generate a new quote from raw market data and store it.
    ///
    /// Returns the quote_id for reference.
    pub fn generate_quote(
        &mut self,
        pool_amm_id: &Pubkey,
        base_mint: &Pubkey,
        now_ms: u64,
        slot: Option<u64>,
        price_sol_per_token: f64,
        sol_reserves: u64,
        token_reserves: u64,
        price_impact_pct: f64,
        source: QuoteSource,
    ) -> QuoteId {
        let seq = self.next_seq;
        self.next_seq += 1;
        let quote_id = format!("{}_{}_{}", slot.unwrap_or(0), now_ms, seq);

        let quote = ExecutableQuote {
            quote_id: quote_id.clone(),
            pool_amm_id: *pool_amm_id,
            base_mint: *base_mint,
            timestamp_ms: now_ms,
            slot,
            price_sol_per_token,
            price_mcap_usd: None,
            sol_reserves,
            token_reserves,
            price_impact_pct,
            source,
            is_stale: false,
        };

        // Insert into ring buffer
        let buffer = self
            .buffers
            .entry(*pool_amm_id)
            .or_insert_with(VecDeque::new);

        // Evict oldest if full
        if buffer.len() >= self.config.ring_buffer_size {
            if let Some(evicted) = buffer.pop_front() {
                self.quote_index.remove(&evicted.quote_id);
            }
        }

        self.quote_index.insert(quote_id.clone(), *pool_amm_id);
        buffer.push_back(quote);

        debug!(
            quote_id = %quote_id,
            pool_amm_id = %pool_amm_id,
            price = price_sol_per_token,
            "QuoteProvider: generated new quote"
        );

        quote_id
    }

    /// Lookup the quote nearest to `target_time_ms` for a given pool.
    ///
    /// Uses binary search on time-ordered ring buffers. If the buffer contains
    /// out-of-order timestamps (should not happen in normal flow), falls back
    /// to linear scan for correctness.
    pub fn lookup_nearest(
        &self,
        pool_amm_id: &Pubkey,
        target_time_ms: u64,
    ) -> Option<&ExecutableQuote> {
        let buffer = self.buffers.get(pool_amm_id)?;
        if buffer.is_empty() {
            return None;
        }

        let ordered = buffer
            .iter()
            .zip(buffer.iter().skip(1))
            .all(|(a, b)| a.timestamp_ms <= b.timestamp_ms);

        if !ordered {
            let mut best: Option<&ExecutableQuote> = None;
            let mut best_delta = u64::MAX;
            for quote in buffer {
                let delta = quote.timestamp_ms.abs_diff(target_time_ms);
                if delta < best_delta {
                    best_delta = delta;
                    best = Some(quote);
                }
            }
            return best;
        }

        let mut lo = 0usize;
        let mut hi = buffer.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let ts = buffer[mid].timestamp_ms;
            if ts < target_time_ms {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }

        if lo == 0 {
            return buffer.front();
        }
        if lo >= buffer.len() {
            return buffer.back();
        }

        let prev = &buffer[lo - 1];
        let next = &buffer[lo];
        let prev_delta = prev.timestamp_ms.abs_diff(target_time_ms);
        let next_delta = next.timestamp_ms.abs_diff(target_time_ms);

        if prev_delta <= next_delta {
            Some(prev)
        } else {
            Some(next)
        }
    }

    /// Lookup quote by ID.
    pub fn get_by_id(&self, quote_id: &QuoteId) -> Option<&ExecutableQuote> {
        let pool = self.quote_index.get(quote_id)?;
        let buffer = self.buffers.get(pool)?;
        buffer.iter().find(|q| &q.quote_id == quote_id)
    }

    /// Check if a quote is stale relative to `now_ms`.
    pub fn is_stale(&self, quote_id: &QuoteId, now_ms: u64) -> bool {
        if let Some(quote) = self.get_by_id(quote_id) {
            let age = now_ms.saturating_sub(quote.timestamp_ms);
            if age > self.config.max_quote_age_ms {
                return true;
            }
            // Emit warning if close to stale
            if age > self.config.stale_warning_threshold_ms {
                warn!(
                    quote_id = %quote_id,
                    age_ms = age,
                    threshold_ms = self.config.max_quote_age_ms,
                    "Quote approaching stale threshold"
                );
            }
            false
        } else {
            // Unknown quote → treat as stale
            true
        }
    }

    /// Evict all quotes older than `max_quote_age_ms` from `now_ms`.
    pub fn evict_expired(&mut self, now_ms: u64) {
        let max_age = self.config.max_quote_age_ms;
        let mut evicted_count = 0usize;

        for buffer in self.buffers.values_mut() {
            while let Some(front) = buffer.front() {
                if now_ms.saturating_sub(front.timestamp_ms) > max_age * 2 {
                    // Evict quotes older than 2x max age (keep some history)
                    if let Some(evicted) = buffer.pop_front() {
                        self.quote_index.remove(&evicted.quote_id);
                        evicted_count += 1;
                    }
                } else {
                    break; // Buffer is time-ordered, so stop
                }
            }
        }

        if evicted_count > 0 {
            debug!(evicted_count, "QuoteProvider: evicted expired quotes");
        }
    }

    /// Get the latest quote for a pool.
    pub fn latest_quote(&self, pool_amm_id: &Pubkey) -> Option<&ExecutableQuote> {
        self.buffers.get(pool_amm_id)?.back()
    }

    /// Total number of quotes across all pools.
    pub fn total_quotes(&self) -> usize {
        self.buffers.values().map(|b| b.len()).sum()
    }

    /// Returns how stale the most recent quote is, in milliseconds.
    /// If no quotes exist, returns `u64::MAX` (extremely stale).
    pub fn stale_age_ms(&self) -> u64 {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let most_recent = self
            .buffers
            .values()
            .filter_map(|b| b.back())
            .map(|q| q.timestamp_ms)
            .max();

        match most_recent {
            Some(ts) => now_ms.saturating_sub(ts),
            None => u64::MAX,
        }
    }

    /// Returns `true` if the most recent quote across all pools
    /// is older than `max_quote_age_ms`.
    pub fn is_globally_stale(&self) -> bool {
        self.stale_age_ms() > self.config.max_quote_age_ms
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_provider(buffer_size: usize) -> ExecutableQuoteProvider {
        ExecutableQuoteProvider::new(QuoteProviderConfig {
            max_quote_age_ms: 1500,
            ring_buffer_size: buffer_size,
            generation_interval_ms: 500,
            stale_warning_threshold_ms: 1000,
        })
    }

    fn pool() -> Pubkey {
        Pubkey::new_unique()
    }

    fn mint() -> Pubkey {
        Pubkey::new_unique()
    }

    #[test]
    fn test_generate_and_lookup_by_id() {
        let mut p = make_provider(16);
        let pool = pool();
        let m = mint();
        let qid = p.generate_quote(
            &pool,
            &m,
            1000,
            Some(100),
            0.005,
            1_000_000,
            200_000_000,
            0.01,
            QuoteSource::BondingCurve,
        );
        let q = p.get_by_id(&qid).expect("should find quote");
        assert_eq!(q.price_sol_per_token, 0.005);
        assert_eq!(q.pool_amm_id, pool);
    }

    #[test]
    fn test_lookup_nearest() {
        let mut p = make_provider(16);
        let pool = pool();
        let m = mint();

        let _q1 = p.generate_quote(
            &pool,
            &m,
            1000,
            None,
            0.001,
            100,
            100,
            0.0,
            QuoteSource::BondingCurve,
        );
        let q2 = p.generate_quote(
            &pool,
            &m,
            2000,
            None,
            0.002,
            100,
            100,
            0.0,
            QuoteSource::BondingCurve,
        );
        let _q3 = p.generate_quote(
            &pool,
            &m,
            3000,
            None,
            0.003,
            100,
            100,
            0.0,
            QuoteSource::BondingCurve,
        );

        let nearest = p.lookup_nearest(&pool, 1900).expect("should find nearest");
        assert_eq!(nearest.quote_id, q2);
    }

    #[test]
    fn test_stale_detection() {
        let mut p = make_provider(16);
        let pool = pool();
        let m = mint();
        let qid = p.generate_quote(
            &pool,
            &m,
            1000,
            None,
            0.001,
            100,
            100,
            0.0,
            QuoteSource::BondingCurve,
        );

        // Not stale at 2000 (age=1000, max=1500)
        assert!(!p.is_stale(&qid, 2000));

        // Stale at 3000 (age=2000, max=1500)
        assert!(p.is_stale(&qid, 3000));

        // Unknown quote → stale
        assert!(p.is_stale(&"nonexistent".to_string(), 1000));
    }

    #[test]
    fn test_ring_buffer_eviction() {
        let mut p = make_provider(3);
        let pool = pool();
        let m = mint();

        let q1 = p.generate_quote(
            &pool,
            &m,
            1000,
            None,
            0.001,
            100,
            100,
            0.0,
            QuoteSource::BondingCurve,
        );
        let _q2 = p.generate_quote(
            &pool,
            &m,
            2000,
            None,
            0.002,
            100,
            100,
            0.0,
            QuoteSource::BondingCurve,
        );
        let _q3 = p.generate_quote(
            &pool,
            &m,
            3000,
            None,
            0.003,
            100,
            100,
            0.0,
            QuoteSource::BondingCurve,
        );

        assert_eq!(p.total_quotes(), 3);
        assert!(p.get_by_id(&q1).is_some());

        // Adding 4th should evict q1
        let _q4 = p.generate_quote(
            &pool,
            &m,
            4000,
            None,
            0.004,
            100,
            100,
            0.0,
            QuoteSource::BondingCurve,
        );
        assert_eq!(p.total_quotes(), 3);
        assert!(p.get_by_id(&q1).is_none()); // Evicted
    }

    #[test]
    fn test_evict_expired() {
        let mut p = make_provider(16);
        let pool = pool();
        let m = mint();

        let _q1 = p.generate_quote(
            &pool,
            &m,
            1000,
            None,
            0.001,
            100,
            100,
            0.0,
            QuoteSource::BondingCurve,
        );
        let q2 = p.generate_quote(
            &pool,
            &m,
            5000,
            None,
            0.002,
            100,
            100,
            0.0,
            QuoteSource::BondingCurve,
        );

        // Evict at t=5000: q1 age=4000 > 2*1500=3000 → evicted; q2 age=0 → kept
        p.evict_expired(5000);
        assert_eq!(p.total_quotes(), 1);
        assert!(p.get_by_id(&q2).is_some());
    }

    #[test]
    fn test_latest_quote() {
        let mut p = make_provider(16);
        let pool = pool();
        let m = mint();

        assert!(p.latest_quote(&pool).is_none());

        let _ = p.generate_quote(
            &pool,
            &m,
            1000,
            None,
            0.001,
            100,
            100,
            0.0,
            QuoteSource::BondingCurve,
        );
        let q2 = p.generate_quote(
            &pool,
            &m,
            2000,
            None,
            0.002,
            100,
            100,
            0.0,
            QuoteSource::BondingCurve,
        );

        let latest = p.latest_quote(&pool).expect("should have latest");
        assert_eq!(latest.quote_id, q2);
    }
}
